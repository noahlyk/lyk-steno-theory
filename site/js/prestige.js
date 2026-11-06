// "Learn the keys": the lesson engine behind #/keys. Pure functions, no DOM, so it can be tested
// in node. Ported from the steno-lessons project's Prestige levels: start with a few keys, type
// words made only of the keys you have, and unlock one more key each time a whole lesson (30
// words) is fast and accurate enough. Accuracy is judged per chord (the exact keys held together
// in one hand or the vowels), and the word picker leans hard toward whichever chord you are
// missing or have not met yet, so the weak spot always gets the reps.
//
// Prestige 1: the board shows the keys to press. Prestige 2: the board is hidden (it comes back
// for one word after repeated misses); only the stroke's letters may be shown. Each level
// starts again from six keys, so the second pass proves you know where every sound lives.

import { parseOutline } from './steno.js';

// The order keys are introduced in: vowels and the commonest consonant sounds first, the rest of
// the left hand, the right hand, the star, and last the join key (it only matters for words
// longer than one stroke).
export const KEY_ORDER = [
  'E', 'A', 'T-', 'S-', 'K-', '-T', '-S', 'R-', '-R', 'H-', 'O', 'P-',
  'W-', 'U', '-L', '-D', '-P', '-B', '-G', '-F', '*', '-Z', '+',
];
export const START_KEYS = 6;
export const LESSON_SIZE = 30; // words per lesson: the same window judges speed and accuracy
export const ACCURACY_TARGET = 0.95; // share of words with no miss
export const CHORD_ACCURACY_TARGET = 0.8; // per chord, over its last CHORD_WINDOW tries
export const LEVEL_WPM = { 1: 28, 2: 28 };
export const MAX_LEVEL = 2;
export const LABEL_REVEAL_MISSES = 3; // level 2: the board reappears for a word after this many wrong chords

const MAX_WORD_LENGTH = 9;
const HISTORY_LIMIT = 100;
const LESSON_HISTORY_LIMIT = 10;
const CHORD_WINDOW = 30;
const CHORD_MIN_SAMPLES = 5; // tries before a chord's accuracy can hold back an unlock
const MISS_PENALTY_MS = 800;
const TARGET_MS = 1200;
const SAMPLES_TO_LEARN = 5;
const DRILL_ACCURACY = 0.85;
const DRILL_SHARE = 0.2;
const STEADY_DRILL_SHARE = 0.15;
const DRILL_MAX_LENGTH = 3;
const WEAKNESS_GAIN = 150;
const CHORD_GRACE_SAMPLES = 8;
const UNSEEN_WEAKNESS = 0.35;

const BIT = new Map(KEY_ORDER.map((k, i) => [k, 1 << i]));
export const bitOf = (id) => BIT.get(id);
export const maskOf = (keys) => [...keys].reduce((m, k) => m | (BIT.get(k) ?? 0), 0);
export const keysOf = (mask) => KEY_ORDER.filter((k) => mask & BIT.get(k));
const popcount = (n) => { let c = 0; for (let r = n; r; r &= r - 1) c += 1; return c; };

// ---- chord units: the exact keys held together in one group of one stroke ----
const GROUPS = [
  ['S-', 'T-', 'K-', 'P-', 'W-', 'H-', 'R-'],
  ['A', 'O', '*', 'E', 'U'],
  ['-F', '-R', '-P', '-B', '-L', '-G', '-T', '-S', '-D', '-Z'],
  ['+'],
];
const RANK = new Map();
const GROUP = new Map();
GROUPS.forEach((g, gi) => g.forEach((k) => { RANK.set(k, RANK.size); GROUP.set(k, gi); }));

/** Unit ids of a stroke (a set or array of key ids), in steno order: `T-|P-|H-`. */
export function chordUnits(keys) {
  const groups = GROUPS.map(() => []);
  for (const k of new Set(keys)) if (GROUP.has(k)) groups[GROUP.get(k)].push(k);
  return groups.filter((g) => g.length).map((g) => g.sort((a, b) => RANK.get(a) - RANK.get(b)).join('|'));
}
const unitKeys = (id) => id.split('|');
function isChordId(id) {
  if (typeof id !== 'string') return false;
  const parts = id.split('|');
  const g = GROUP.get(parts[0]);
  return g !== undefined && parts.every((k, i) => GROUP.get(k) === g && (i === 0 || RANK.get(k) > RANK.get(parts[i - 1])));
}
const unitCache = new Map();
function unitsOfMask(mask) {
  let u = unitCache.get(mask);
  if (!u) { u = chordUnits(keysOf(mask)); unitCache.set(mask, u); }
  return u;
}

// ---- progress ----
const LEVEL_FIELDS = ['unlocked', 'chords', 'words', 'lessonHistory'];
const blankStat = () => ({ samples: 0, ewmaMs: null, misses: 0, recent: [] });
const freshLevel = () => ({ unlocked: START_KEYS, chords: {}, words: [], lessonHistory: [] });
const pickLevel = (p) => Object.fromEntries(LEVEL_FIELDS.map((f) => [f, p[f]]));

export function emptyProgress() {
  return {
    ...freshLevel(),
    level: 1,
    totalTyped: 0, // every word ever finished, never reset
    completed: {}, // level -> true
    bests: {}, // level -> fastest whole-lesson wpm, set only with every key unlocked
    stash: {}, // level -> saved fields of a level you are not on
    labels: true, // level 2: show the stroke's letters under the word
    skipped: false, // "I already know steno": lessons stop nagging
  };
}

/** Keep only what the engine understands, so an old or damaged save still loads. */
export function normalize(saved) {
  const p = emptyProgress();
  if (!saved || typeof saved !== 'object') return p;
  Object.assign(p, normalizeLevel(saved));
  if (Number.isFinite(saved.totalTyped)) p.totalTyped = Math.max(0, Math.floor(saved.totalTyped));
  for (const l of Object.keys(LEVEL_WPM)) {
    if (saved.completed?.[l] === true) p.completed[l] = true;
    const best = Number(saved.bests?.[l]);
    if (Number.isFinite(best) && best > 0) p.bests[l] = best;
  }
  if (Number.isInteger(saved.level) && LEVEL_WPM[saved.level]) p.level = saved.level;
  if (typeof saved.labels === 'boolean') p.labels = saved.labels;
  if (typeof saved.skipped === 'boolean') p.skipped = saved.skipped;
  if (saved.stash && typeof saved.stash === 'object') {
    for (const l of Object.keys(LEVEL_WPM)) {
      if (Number(l) !== p.level && saved.stash[l] && typeof saved.stash[l] === 'object') p.stash[l] = normalizeLevel(saved.stash[l]);
    }
  }
  return p;
}

function normalizeLevel(s) {
  const out = freshLevel();
  if (Number.isInteger(s.unlocked)) out.unlocked = Math.min(Math.max(s.unlocked, 1), KEY_ORDER.length);
  if (s.chords && typeof s.chords === 'object') {
    for (const [id, st] of Object.entries(s.chords)) {
      if (!isChordId(id) || !st || typeof st !== 'object') continue;
      out.chords[id] = {
        samples: Math.max(0, Number(st.samples) || 0),
        ewmaMs: typeof st.ewmaMs === 'number' ? st.ewmaMs : null,
        misses: Math.max(0, Number(st.misses) || 0),
        recent: Array.isArray(st.recent) ? st.recent.filter((h) => h === 0 || h === 1).slice(-CHORD_WINDOW) : [],
      };
    }
  }
  if (Array.isArray(s.words)) {
    out.words = s.words.filter((w) => w && Number.isFinite(w.ms) && Number.isFinite(w.strokes) && Number.isFinite(w.misses)).slice(-HISTORY_LIMIT);
  }
  if (Array.isArray(s.lessonHistory)) {
    out.lessonHistory = s.lessonHistory.filter((e) => e && Number.isFinite(e.accuracy) && Number.isFinite(e.wpm)).slice(-LESSON_HISTORY_LIMIT);
  }
  return out;
}

export const unlockedKeys = (p) => KEY_ORDER.slice(0, p.unlocked);
export const unlockedMask = (p) => maskOf(unlockedKeys(p));
export const wpmTarget = (level = 1) => LEVEL_WPM[level] ?? LEVEL_WPM[1];

/** Switch prestige level: the current level's progress is set aside; a new level starts fresh. */
export function switchLevel(p, level) {
  if (!LEVEL_WPM[level] || p.level === level) return false;
  p.stash[p.level] = pickLevel(p);
  Object.assign(p, p.stash[level] ?? freshLevel());
  delete p.stash[level];
  p.level = level;
  return true;
}

/** One row per level for the header: keys unlocked, done, best. */
export function levelSummary(p) {
  return [1, 2].map((level) => {
    const active = level === p.level;
    const slice = active ? p : p.stash[level];
    return {
      level, active, wpm: LEVEL_WPM[level],
      unlocked: slice ? slice.unlocked : START_KEYS, total: KEY_ORDER.length,
      done: Boolean(p.completed[level]), best: p.bests[level] ?? null,
    };
  });
}

// ---- recording ----
function recordChord(p, ids, ms, correct) {
  for (const id of ids) {
    if (!isChordId(id)) continue;
    const st = p.chords[id] ?? blankStat();
    const cost = correct ? ms : ms + MISS_PENALTY_MS; // a miss slows the average rather than counting as a rep
    st.ewmaMs = st.ewmaMs === null ? cost : st.ewmaMs * 0.7 + cost * 0.3;
    st.recent = [...st.recent, correct ? 1 : 0].slice(-CHORD_WINDOW); // a plain rolling window
    if (correct) st.samples += 1; else st.misses += 1;
    p.chords[id] = st;
  }
}

/** A right stroke counts for every chord in it; a wrong one only for the chords that were missing. */
export function recordStroke(p, { target = [], used, ms, correct }) {
  const usedUnits = chordUnits(used);
  if (correct) return recordChord(p, usedUnits, ms, true);
  const targetUnits = chordUnits(target);
  const wrong = targetUnits.filter((id) => !usedUnits.includes(id));
  return recordChord(p, wrong.length ? wrong : targetUnits, ms, false);
}

export function recordWord(p, { ms, strokes, misses }) {
  p.words.push({ ms, strokes, misses });
  if (p.words.length > HISTORY_LIMIT) p.words.splice(0, p.words.length - HISTORY_LIMIT);
  p.totalTyped += 1;
}

/** Speed and accuracy over the newest LESSON_SIZE words since the last unlock. */
export function windowStats(p) {
  const recent = p.words.slice(-LESSON_SIZE);
  const count = recent.length;
  const ms = recent.reduce((s, w) => s + w.ms, 0);
  const strokes = recent.reduce((s, w) => s + w.strokes, 0);
  const misses = recent.reduce((s, w) => s + w.misses, 0);
  return { count, wpm: ms > 0 ? (count * 60000) / ms : 0, accuracy: strokes + misses > 0 ? strokes / (strokes + misses) : 1 };
}

export function chordAccuracy(p, id) {
  const st = p.chords[id];
  if (!st || st.recent.length < CHORD_MIN_SAMPLES) return 1;
  return st.recent.reduce((s, h) => s + h, 0) / st.recent.length;
}
const chordsAccurate = (p) => Object.keys(p.chords).every((id) => chordAccuracy(p, id) >= CHORD_ACCURACY_TARGET);

export function meetsTargets(p) {
  const s = windowStats(p);
  return s.count >= LESSON_SIZE && s.wpm >= wpmTarget(p.level) && s.accuracy >= ACCURACY_TARGET && chordsAccurate(p);
}

/** Unlock the next key if the newest lesson clears the bars. The window restarts. */
export function unlockIfReady(p) {
  if (p.unlocked >= KEY_ORDER.length || !meetsTargets(p)) return false;
  p.unlocked += 1;
  p.words = [];
  return true;
}

/** A level is complete once every key is unlocked AND a fresh lesson clears speed and accuracy. */
export function completeIfReady(p) {
  if (p.completed[p.level] || p.unlocked < KEY_ORDER.length) return false;
  const s = windowStats(p);
  if (s.count < LESSON_SIZE || s.wpm < wpmTarget(p.level) || s.accuracy < ACCURACY_TARGET) return false;
  p.completed[p.level] = true;
  return true;
}

export function recordLesson(p, stats) {
  p.lessonHistory.push({ accuracy: stats.accuracy, wpm: stats.wpm });
  if (p.lessonHistory.length > LESSON_HISTORY_LIMIT) p.lessonHistory.splice(0, p.lessonHistory.length - LESSON_HISTORY_LIMIT);
}

/** A personal best counts only with every key unlocked and a whole accurate lesson. Whole wpm. */
export function recordBestIfFaster(p, stats) {
  if (p.unlocked < KEY_ORDER.length || stats.count < LESSON_SIZE || stats.accuracy < ACCURACY_TARGET) return null;
  const wpm = Math.floor(stats.wpm);
  if (wpm <= (p.bests[p.level] ?? 0)) return null;
  p.bests[p.level] = wpm;
  return wpm;
}

/** 0..1: how well a chord is known (reps, speed and accuracy together). */
export function confidence(p, id) {
  const st = p.chords[id];
  if (!st || st.ewmaMs === null) return 0;
  return Math.min(1, st.samples / SAMPLES_TO_LEARN) * Math.min(1, TARGET_MS / st.ewmaMs)
    * Math.min(1, chordAccuracy(p, id) / CHORD_ACCURACY_TARGET);
}

/** How well a key is known: its best chord. Never typed reads 0. */
export function keyConfidence(p, name) {
  let best = 0;
  for (const id of Object.keys(p.chords)) if (unitKeys(id).includes(name)) best = Math.max(best, confidence(p, id));
  return best;
}

/** The practiced chords in order of need: lowest recent accuracy first. For the header. */
export function weakChords(p, count = 3) {
  return Object.keys(p.chords)
    .filter((id) => p.chords[id].recent.length >= CHORD_MIN_SAMPLES)
    .map((id) => ({ id, accuracy: chordAccuracy(p, id), confidence: confidence(p, id) }))
    .sort((a, b) => a.accuracy - b.accuracy || a.confidence - b.confidence)
    .slice(0, count);
}

// ---- words ----
/**
 * The word list from site/data/words.json ({word: [outline...]}, commonest first): plain
 * lower-case words with every outline that parses. A variant is a list of stroke masks.
 */
export function buildWords(dict) {
  const out = [];
  let rank = 0;
  for (const [text, outlines] of Object.entries(dict)) {
    rank += 1;
    if (!/^[a-z]+$/.test(text) || text.length > MAX_WORD_LENGTH) continue;
    const variants = [];
    for (const o of outlines) {
      const strokes = parseOutline(o);
      if (strokes) variants.push(strokes.map(maskOf));
    }
    if (variants.length) out.push({ text, rank, variants });
  }
  return out;
}

const freqWeight = (rank) => 0.15 + 0.85 * Math.exp(-rank / 2000);
const lengthWeight = (n) => (n <= 1 ? 0.2 : n === 2 ? 0.6 : n <= 6 ? 1 : 0.6);
const strokeWeight = (n) => (n === 1 ? 2 : n === 2 ? 1 : 0.3);

const usable = (word, mask) => word.variants.filter((v) => v.every((s) => (s & ~mask) === 0));

/** The ways to write a word with unlocked keys: fewest strokes, then fewest keys, first. */
export function displayVariants(word, mask) {
  return usable(word, mask)
    .map((v) => ({ v, keys: v.reduce((n, s) => n + popcount(s), 0) }))
    .sort((a, b) => a.v.length - b.v.length || a.keys - b.keys)
    .map((x) => x.v);
}

/** 0 (always right) .. 1 (always wrong); a chord not yet met counts as UNSEEN_WEAKNESS. */
export function chordWeakness(p, id) {
  const st = p.chords[id];
  if (!st) return UNSEEN_WEAKNESS;
  const deficit = 1 - chordAccuracy(p, id);
  return st.samples < CHORD_GRACE_SAMPLES ? Math.max(UNSEEN_WEAKNESS, deficit) : deficit;
}

const weakIds = (p) => Object.keys(p.chords).filter((id) => chordAccuracy(p, id) < DRILL_ACCURACY);

/** The chord a drill word works and how much of the stream it gets; null before any evidence. */
export function drillOptions(p) {
  const weak = weakIds(p);
  let target = null;
  let steady = false;
  if (weak.length) {
    target = weak.reduce((w, id) => (chordAccuracy(p, id) < chordAccuracy(p, w) ? id : w));
  } else {
    let lowest = Infinity;
    for (const id of Object.keys(p.chords)) {
      if (p.chords[id].recent.length < CHORD_MIN_SAMPLES) continue;
      const c = confidence(p, id);
      if (c < lowest) { lowest = c; target = id; }
    }
    steady = true;
  }
  if (!target) return null;
  return {
    share: steady ? STEADY_DRILL_SHARE : DRILL_SHARE,
    options: { requireUnit: target, excludeUnits: new Set(weak.filter((id) => id !== target)), maxLength: DRILL_MAX_LENGTH },
  };
}

/**
 * Weighted pick of `count` distinct words, each with a spelling in the unlocked keys. A word's
 * weight is frequency x length x strokes, times a boost for its single weakest chord (squared,
 * so real weakness dominates). `avoid` skips recent words where possible.
 */
export function pickWords(words, p, { count = 1, avoid = [], random = Math.random, maxLength = MAX_WORD_LENGTH, requireUnit, excludeUnits, strictAvoid = false } = {}) {
  const mask = unlockedMask(p);
  const skip = new Set(avoid);
  const eligible = [];
  for (const word of words) {
    if (word.text.length > maxLength) continue;
    let ok = usable(word, mask);
    if (requireUnit || excludeUnits) {
      ok = ok.filter((v) => {
        let found = !requireUnit;
        for (const s of v) for (const id of unitsOfMask(s)) {
          if (excludeUnits?.has(id)) return false;
          if (id === requireUnit) found = true;
        }
        return found;
      });
    }
    if (ok.length) eligible.push({ word, ok });
  }
  const pool = eligible.filter(({ word }) => !skip.has(word.text));
  const source = pool.length >= count || strictAvoid ? pool : eligible;
  const weak = new Map();
  const weakOf = (id) => { if (!weak.has(id)) weak.set(id, chordWeakness(p, id)); return weak.get(id); };
  const weighted = source.map(({ word, ok }) => {
    const best = ok.slice().sort((a, b) => a.length - b.length)[0];
    let w = lengthWeight(word.text.length) * strokeWeight(best.length) * freqWeight(word.rank);
    let worst = 0;
    for (const s of best) for (const id of unitsOfMask(s)) worst = Math.max(worst, weakOf(id));
    return { word, weight: w * (1 + WEAKNESS_GAIN * worst * worst) };
  });
  const chosen = [];
  const left = new Set(weighted);
  while (chosen.length < count && left.size) {
    let total = 0;
    for (const it of left) total += it.weight;
    let roll = random() * total;
    let picked = null;
    for (const it of left) { picked = it; roll -= it.weight; if (roll < 0) break; }
    left.delete(picked);
    chosen.push(picked.word);
  }
  return chosen;
}

/** The next word: sometimes a drill word on the weakest chord, else the weighted pick. */
export function nextWord(words, p, { avoid = [], random = Math.random } = {}) {
  const drill = drillOptions(p);
  let word = null;
  if (drill && random() < drill.share) word = pickWords(words, p, { avoid, random, strictAvoid: true, ...drill.options })[0];
  // few keys can mean the whole vocabulary was just typed: shorten the avoid list before repeating
  for (const keep of [avoid.length, 20, 8, 3, 1, 0]) {
    if (word || keep > avoid.length) continue;
    word = pickWords(words, p, { avoid: avoid.slice(avoid.length - keep), random, strictAvoid: keep > 0 })[0];
  }
  return word ?? null;
}

// ---- reading a stroke against a word ----
/**
 * How a finished stroke fits a word. `candidates` are the spellings still possible, `all` every
 * spelling, `strokeIdx` the stroke due. { type: 'next', matches } | { type: 'restart', matches }
 * | { type: 'partial' } (keys let go early: keep going) | null (a real mistake).
 */
export function classifyStroke({ chord, candidates, all, strokeIdx }) {
  if (!chord) return null;
  const matches = candidates.filter((v) => v[strokeIdx] === chord);
  if (matches.length) return { type: 'next', matches };
  if (strokeIdx > 0) {
    const restart = all.filter((v) => v[0] === chord);
    if (restart.length) return { type: 'restart', matches: restart };
  }
  if (candidates.some((v) => v[strokeIdx] !== undefined && (chord & ~v[strokeIdx]) === 0)) return { type: 'partial' };
  return null;
}

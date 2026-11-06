// "Learn the keys": the engine behind #/keys. Pure functions, no DOM, so it can be tested in
// node. Start with a few keys, type words made only of the keys you have, and unlock one more key
// whenever your recent words are good enough. Accuracy is judged per chord (the exact keys held
// together in one hand or the vowels), and the word picker leans hard toward whichever chord you
// are missing or have not met yet, so the weak spot always gets the reps.
//
// "Good enough" is a trade between speed and accuracy, not two fixed bars: the faster you are,
// the less accurate you need to be (someone at 60 wpm obviously knows the keys), and the slower
// you are, the cleaner you must be, with a floor under the speed.
//
// One adaptive guide replaces fixed levels. It starts as the diagram plus the QWERTY hints, then
// the hints fade, then the diagram, as you get fast and clean, and each comes back if you start to
// struggle. The stroke under each word is never hidden.

import { parseOutline } from './steno.js';

// The order keys are introduced in: vowels and the commonest consonant sounds first, the rest of
// the left hand, the right hand, the star, and last the join key (it only matters for words
// longer than one stroke).
export const KEY_ORDER = [
  'E', 'A', 'T-', 'S-', 'K-', '-T', '-S', 'R-', '-R', 'H-', 'O', 'P-',
  'W-', 'U', '-L', '-D', '-P', '-B', '-G', '-F', '*', '-Z', '+',
];
export const START_KEYS = 6;
export const WINDOW = 20; // the newest words that judge an unlock
export const MIN_WORDS = 10; // fewest words before an unlock can be judged
export const MIN_WPM = 14; // below this nothing passes, however clean (in work-adjusted wpm, see workOf)
export const FAST_WPM = 60; // at and above this the loosest accuracy applies
export const ACC_SLOW = 0.97; // accuracy needed at MIN_WPM
export const ACC_FAST = 0.75; // accuracy needed at FAST_WPM
export const CHORD_ACCURACY_TARGET = 0.8; // what a chord's confidence is measured against
export const CHORD_SKIP_WPM = 45; // this fast, individual chords are not held against you
export const LABEL_REVEAL_MISSES = 3; // the diagram comes back for one word after this many wrong chords
export const GUIDE = { FULL: 2, DIAGRAM: 1, MEMORY: 0 }; // diagram + hints, diagram only, strokes only
const ASSIST_WORDS = 12; // words judged for a change of guide
const ASSIST_WORDS_FAST = 6;
const ASSIST_ACCURACY = 0.9;
const FADE_WPM = { [GUIDE.FULL]: 18, [GUIDE.DIAGRAM]: 24 }; // speed that removes the next piece of guide
const FAST_FADE_WPM = 40; // this fast, the guide fades after ASSIST_WORDS_FAST words
const RESTORE_WORDS = 8;
const RESTORE_ACCURACY = 0.8;
const RESTORE_WPM = 10;

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
const blankStat = () => ({ samples: 0, ewmaMs: null, misses: 0, recent: [] });
const freshLevel = () => ({ unlocked: START_KEYS, chords: {}, words: [], lessonHistory: [] });

export function emptyProgress() {
  return {
    ...freshLevel(),
    guide: GUIDE.FULL,
    recent: [], // the newest words, kept across unlocks: what the guide adapts to
    sinceGuide: 0, // words since the guide last changed
    totalTyped: 0, // every word ever finished, never reset
    done: false, // every key unlocked and a clean run from memory
    best: null, // fastest whole-window wpm with every key unlocked
    skipped: false, // "I already know steno": lessons stop nagging
  };
}

const validWord = (w) => w && Number.isFinite(w.ms) && Number.isFinite(w.strokes) && Number.isFinite(w.misses);

/** Keep only what the engine understands, so an old or damaged save still loads. */
export function normalize(saved) {
  const p = emptyProgress();
  if (!saved || typeof saved !== 'object') return p;
  Object.assign(p, normalizeLevel(saved));
  if (Number.isFinite(saved.totalTyped)) p.totalTyped = Math.max(0, Math.floor(saved.totalTyped));
  // saves from the two-level version: finishing the second level was finishing the course
  if (saved.done === true || saved.completed?.[2] === true) p.done = true;
  const best = Number(saved.best ?? saved.bests?.[saved.level] ?? 0);
  if (Number.isFinite(best) && best > 0) p.best = best;
  if (Object.values(GUIDE).includes(saved.guide)) p.guide = saved.guide;
  if (Array.isArray(saved.recent)) p.recent = saved.recent.filter(validWord).slice(-ASSIST_WORDS);
  if (Number.isFinite(saved.sinceGuide)) p.sinceGuide = Math.max(0, Math.floor(saved.sinceGuide));
  if (typeof saved.skipped === 'boolean') p.skipped = saved.skipped;
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
    out.words = s.words.filter(validWord).slice(-HISTORY_LIMIT);
  }
  if (Array.isArray(s.lessonHistory)) {
    out.lessonHistory = s.lessonHistory.filter((e) => e && Number.isFinite(e.accuracy) && Number.isFinite(e.wpm)).slice(-LESSON_HISTORY_LIMIT);
  }
  return out;
}

export const unlockedKeys = (p) => KEY_ORDER.slice(0, p.unlocked);
export const unlockedMask = (p) => maskOf(unlockedKeys(p));
/** The accuracy a given speed must reach: strict when slow, loose when fast; impossible under MIN_WPM. */
export function requiredAccuracy(wpm) {
  if (wpm < MIN_WPM) return Infinity;
  const t = Math.min(1, (wpm - MIN_WPM) / (FAST_WPM - MIN_WPM));
  return ACC_SLOW + (ACC_FAST - ACC_SLOW) * t;
}

/** Do these stats (from windowStats) clear the speed/accuracy trade? */
export const passes = (s) => s.count >= MIN_WORDS && s.accuracy >= requiredAccuracy(s.wpm);

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

/**
 * How much typing a word is, in units of one ordinary three-key stroke (never under 1): its key
 * presses over three, plus half a unit for each stroke after the first. Speed is measured in these,
 * so a word of six keys or two strokes is not "slow" just because it takes longer than a short one.
 */
export function workOf(masks) {
  const keys = masks.reduce((n, m) => n + popcount(m), 0);
  return Math.max(1, keys / 3 + 0.5 * (masks.length - 1));
}

export function recordWord(p, { ms, strokes, misses, masks = [] }) {
  const w = masks.length ? workOf(masks) : 1;
  p.words.push({ ms, strokes, misses, w, g: p.guide }); // w: its work; g: the guide it was typed under
  if (p.words.length > HISTORY_LIMIT) p.words.splice(0, p.words.length - HISTORY_LIMIT);
  p.recent.push({ ms, strokes, misses, w });
  if (p.recent.length > ASSIST_WORDS) p.recent.splice(0, p.recent.length - ASSIST_WORDS);
  p.totalTyped += 1;
}

/** Words typed from memory (no diagram) since the last unlock: the only ones an unlock is judged on. */
export const memoryStats = (p) => windowStats(p, p.words.filter((w) => w.g === GUIDE.MEMORY));

/** Work-adjusted speed (`wpm`, a typical three-key word per minute) and accuracy over the newest `size` words. */
export function windowStats(p, words = p.words, size = WINDOW) {
  const recent = words.slice(-size);
  const count = recent.length;
  const ms = recent.reduce((s, w) => s + w.ms, 0);
  const work = recent.reduce((s, w) => s + (w.w ?? 1), 0);
  const strokes = recent.reduce((s, w) => s + w.strokes, 0);
  const misses = recent.reduce((s, w) => s + w.misses, 0);
  return { count, wpm: ms > 0 ? (work * 60000) / ms : 0, rawWpm: ms > 0 ? (count * 60000) / ms : 0, accuracy: strokes + misses > 0 ? strokes / (strokes + misses) : 1 };
}

export function chordAccuracy(p, id) {
  const st = p.chords[id];
  if (!st || st.recent.length < CHORD_MIN_SAMPLES) return 1;
  return st.recent.reduce((s, h) => s + h, 0) / st.recent.length;
}

// individual chords: a slow typist must have every chord sound, a fast one is not held to them
function chordsOk(p, s) {
  if (s.wpm >= CHORD_SKIP_WPM) return true;
  const floor = Math.max(0.5, requiredAccuracy(s.wpm) - 0.25);
  return Object.keys(p.chords).every((id) => chordAccuracy(p, id) >= floor);
}

export function meetsTargets(p) {
  const s = memoryStats(p);
  return passes(s) && chordsOk(p, s);
}

/**
 * Unlock the next key if the newest words typed from memory clear the bar. The window restarts and
 * the guide comes back in full: the new key has to be learned with the diagram, then faded again.
 */
export function unlockIfReady(p) {
  if (p.guide !== GUIDE.MEMORY || p.unlocked >= KEY_ORDER.length || !meetsTargets(p)) return false;
  p.unlocked += 1;
  p.words = [];
  p.guide = GUIDE.FULL;
  p.sinceGuide = 0;
  return true;
}

/** Done: every key unlocked, and the newest words clear the bar with the diagram gone. */
export function completeIfReady(p) {
  if (p.done || p.unlocked < KEY_ORDER.length || p.guide !== GUIDE.MEMORY) return false;
  if (!passes(memoryStats(p))) return false;
  p.done = true;
  return true;
}

export function recordLesson(p, stats) {
  p.lessonHistory.push({ accuracy: stats.accuracy, wpm: stats.wpm });
  if (p.lessonHistory.length > LESSON_HISTORY_LIMIT) p.lessonHistory.splice(0, p.lessonHistory.length - LESSON_HISTORY_LIMIT);
}

/** A personal best counts only with every key unlocked and a whole passing window. Whole wpm. */
export function recordBestIfFaster(p, stats) {
  if (p.unlocked < KEY_ORDER.length || stats.count < WINDOW || !passes(stats)) return null;
  const wpm = Math.floor(stats.wpm);
  if (wpm <= (p.best ?? 0)) return null;
  p.best = wpm;
  return wpm;
}

/**
 * Adapt the guide to how the last words went. Fast and clean removes the next piece (hints, then
 * the diagram); a run of misses or a crawl puts one back. Returns 'fade', 'restore' or null.
 */
export function adaptGuide(p) {
  p.sinceGuide += 1;
  const fastNow = windowStats(p, p.recent, ASSIST_WORDS_FAST);
  const need = fastNow.wpm >= FAST_FADE_WPM ? ASSIST_WORDS_FAST : ASSIST_WORDS;
  if (p.guide > GUIDE.MEMORY && p.sinceGuide >= need) {
    const s = windowStats(p, p.recent, need);
    if (s.count >= need && s.accuracy >= ASSIST_ACCURACY && (s.wpm >= FADE_WPM[p.guide] || s.wpm >= FAST_FADE_WPM)) {
      p.guide -= 1;
      p.sinceGuide = 0;
      return 'fade';
    }
  }
  if (p.guide < GUIDE.FULL && p.sinceGuide >= RESTORE_WORDS) {
    const s = windowStats(p, p.recent, RESTORE_WORDS);
    if (s.accuracy < RESTORE_ACCURACY || s.wpm < RESTORE_WPM) {
      p.guide += 1;
      p.sinceGuide = 0;
      return 'restore';
    }
  }
  return null;
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

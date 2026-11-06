// The stroke-to-text engine: what a run of strokes types, the way the real dictionary reads them.
// Pure (no DOM): `createEngine(data)` takes parsed JSON; `loadEngine()` fetches it in the browser.
//
// A stream is split into runs: a stroke without the join key starts a run, every stroke with it
// continues the run before. A run is read in this order (src/rules/resolve.rs, site/reference/fallback.json):
//   1. the run as one dictionary entry (`ED/+-D` is "ended" if it is an entry), or the reserved
//      punctuation and commands;
//   2. otherwise `resolve`: a single-stroke word as the stem, then each continuation as a
//      suffix from rules/suffixes.toml (a two-stroke suffix first), or as the sounds its keys
//      spell; a silent final `e` of the stem drops before a vowel (`have` + `ing` = `having`);
//   3. a stroke no table reads is shown raw.
// `accepts` and `prefixOf` are what a drill asks: does this way of typing give the word?

import { formatStroke, parseOutline, parseStroke } from './steno.js';

// bit per key, in steno order, as the Rust `Stroke` has them
const ORDER = ['S-', 'T-', 'K-', 'P-', 'W-', 'H-', 'R-', 'A', 'O', '*', 'E', 'U',
  '-F', '-R', '-P', '-B', '-L', '-G', '-T', '-S', '-D', '-Z'];
const BIT = new Map(ORDER.map((k, i) => [k, 1 << i]));
const JOIN = 1 << 22;
const LEFT = 0b1111111;
const STAR = 1 << 9;
const VOWELS = (1 << 7) | (1 << 8) | (1 << 10) | (1 << 11);
const RIGHT = ((1 << 22) - 1) & ~(LEFT | STAR | VOWELS);

const bitsOf = (keys) => {
  let b = 0;
  for (const k of keys) b |= k === '+' ? JOIN : BIT.get(k);
  return b;
};
const bitsOfText = (text) => {
  const keys = parseStroke(text);
  return keys ? bitsOf(keys) : null;
};
const ctz = (n) => 31 - Math.clz32(n & -n);
const ones = (n) => { let c = 0; for (let x = n; x; x &= x - 1) c += 1; return c; };
const key = (keys) => formatStroke(keys);

// ---- reading a stroke back into sounds (the Rust `resolve_stroke`) ----

const plain = (sound) => sound.replace(/\d+$/, '').split('.')[0];
const SPELL = {
  P: 'p', B: 'b', T: 't', D: 'd', K: 'k', G: 'g', M: 'm', N: 'n', NG: 'ng', F: 'f', V: 'v', TH: 'th', DH: 'dh',
  S: 's', Z: 'z', SH: 'sh', ZH: 'zh', CH: 'ch', JH: 'j', HH: 'h', L: 'l', R: 'r', W: 'w', Y: 'y',
  IY: 'ee', IH: 'ih', EY: 'ay', EH: 'eh', AE: 'a', AA: 'ah', AH: 'uh', UH: 'uh', AO: 'aw', OW: 'oh', UW: 'oo',
  AY: 'eye', AW: 'ow', OY: 'oy', ER: 'er',
};
const spelling = (sound) => SPELL[plain(sound)] ?? plain(sound);

const cmpTokens = (a, b) => {
  for (let i = 0; i < Math.min(a.length, b.length); i += 1) if (a[i] !== b[i]) return a[i] < b[i] ? -1 : 1;
  return a.length - b.length;
};

function invert(table) {
  const rev = [];
  for (const [sounds, stroke] of Object.entries(table)) {
    const s = bitsOfText(stroke);
    if (s) rev.push({ s, tokens: sounds.split(/\s+/) });
  }
  return rev.sort((a, b) => cmpTokens(a.tokens, b.tokens));
}

function decompose(bits, catalog) {
  let left = bits;
  const out = [];
  while (left) {
    const lowest = ctz(left);
    let best = null;
    let bestKey = null;
    for (const e of catalog) {
      if (e.s === 0 || ctz(e.s) !== lowest || (left & e.s) !== e.s) continue;
      const k = [ones(e.s), -e.tokens.length, e.tokens.some((t) => t.includes('.')) ? 0 : 1];
      // `max_by_key` keeps the last of equals
      if (!bestKey || k[0] > bestKey[0] || (k[0] === bestKey[0] && (k[1] > bestKey[1]
        || (k[1] === bestKey[1] && k[2] >= bestKey[2])))) {
        best = e;
        bestKey = k;
      }
    }
    if (!best) return null;
    out.push(...best.tokens);
    left &= ~best.s;
  }
  return out;
}

function buildRules(chart) {
  const onsetRev = [...invert(chart.onset), ...invert(chart.onset_cluster)];
  const codaRev = [...invert(chart.coda), ...invert(chart.coda_cluster)];
  const rank = (label) => [!label.includes('.') && label !== 'UH', label];
  const pairs = Object.entries(chart.vowel).map(([label, stroke]) => ({ label, s: bitsOfText(stroke) }))
    .filter((p) => p.s !== null)
    .sort((a, b) => {
      const [ab, as] = rank(a.label);
      const [bb, bs] = rank(b.label);
      return ab === bb ? (as < bs ? -1 : as > bs ? 1 : 0) : (ab ? 1 : -1);
    });
  const vowelRev = new Map(pairs.map((p) => [p.s, p.label]));
  const vowelCodaRev = Object.entries(chart.vowel_coda).map(([sounds, stroke]) => {
    const at = sounds.indexOf(' ');
    return { s: bitsOfText(stroke), vowel: sounds.slice(0, at), coda: sounds.slice(at + 1) };
  }).filter((e) => e.s !== null)
    .sort((a, b) => (a.vowel === b.vowel ? (a.coda < b.coda ? -1 : a.coda > b.coda ? 1 : 0) : (a.vowel < b.vowel ? -1 : 1)));
  return { onsetRev, codaRev, vowelRev, vowelCodaRev };
}

function decodeRight(r, vowelBits, rightAll) {
  for (const [right, marker] of [[rightAll, false], [rightAll & ~STAR, true]]) {
    if (marker && right === rightAll) continue;
    for (const e of r.vowelCodaRev) {
      const comboRight = e.s & RIGHT;
      if ((e.s & VOWELS) === vowelBits && (right & comboRight) === comboRight) {
        const tail = decompose(right & ~comboRight, r.codaRev);
        if (tail) return { vowel: e.vowel, coda: [e.coda, ...tail], marker };
      }
    }
    if (vowelBits === 0) {
      const coda = decompose(right, r.codaRev);
      if (coda) return { vowel: null, coda, marker };
    } else if (r.vowelRev.has(vowelBits)) {
      const coda = decompose(right, r.codaRev);
      if (coda) return { vowel: r.vowelRev.get(vowelBits), coda, marker };
    }
  }
  return null;
}

function resolveStroke(r, bits) {
  const onset = decompose(bits & LEFT, r.onsetRev);
  if (!onset) return null;
  const right = decodeRight(r, bits & VOWELS, bits & (RIGHT | STAR));
  if (!right) return null;
  if (!onset.length && !right.vowel && !right.coda.length) return null;
  return { onset, ...right, join: (bits & JOIN) !== 0 };
}

function dropSilentE(stem, nextStartsWithVowel) {
  if (nextStartsWithVowel && stem.length > 1 && stem.endsWith('e') && !'aeiou'.includes(stem[stem.length - 2])) {
    return stem.slice(0, -1);
  }
  return stem;
}

// ---- reserved strokes: punctuation, commands, letters ----

/** What a reserved entry such as `{.}` or `{^ing^}` does to the text around it. */
function reserved(value) {
  const tokens = [...value.matchAll(/\{([^}]*)\}/g)].map((m) => m[1]);
  if (!tokens.length) return { kind: 'word', text: value };
  if (tokens.some((t) => t.startsWith('#'))) return { kind: 'command', text: `⟨${tokens.find((t) => t.startsWith('#')).slice(1)}⟩` };
  let text = '';
  let cap = false;
  let lower = false;
  let attachPrev = false;
  let attachNext = false;
  for (const t of tokens) {
    if (t === '>') lower = true;
    else if (t === '-|') cap = true;
    else if (t.startsWith('&')) { text += t.slice(1); attachPrev = true; attachNext = true; }
    else if (t.startsWith('^') && t.endsWith('^') && t.length > 1) { text += t.slice(1, -1); attachPrev = true; attachNext = true; }
    else if (t.startsWith('^')) { text += t.slice(1); attachPrev = true; }
    else if (t.endsWith('^')) { text += t.slice(0, -1); attachNext = true; }
    else if (/^[.?!]$/.test(t)) { text += t; attachPrev = true; cap = true; }
    else if (/^[,:;]$/.test(t)) { text += t; attachPrev = true; }
    else text += t;
  }
  return { kind: 'punct', text, attachPrev, attachNext, cap, lower };
}

// ---- the engine ----

export function createEngine({ dict, joins = {}, chart }) {
  const entries = new Map(); // outline text -> word
  const byWord = new Map(); // word -> [outline text]
  const add = (word, outline) => {
    if (!entries.has(outline)) entries.set(outline, word);
    if (!byWord.has(word)) byWord.set(word, []);
    byWord.get(word).push(outline);
  };
  for (const [word, outlines] of Object.entries(dict.d)) for (const o of outlines) add(word, o);
  for (const [word, outlines] of Object.entries(joins)) for (const o of outlines) add(word, o);
  const punct = new Map(Object.entries(dict.p ?? {}));

  const rules = buildRules(chart);
  const suffix = new Map(); // join stroke bits -> text
  const suffix2 = new Map(); // "bitsA/bitsB" -> text
  for (const [chord, text] of Object.entries(dict.s ?? {})) {
    const strokes = chord.split('/').map(bitsOfText);
    if (strokes.some((s) => s === null)) continue;
    if (strokes.length === 1) suffix.set(strokes[0], text);
    else if (strokes.length === 2) suffix2.set(strokes.join('/'), text);
  }
  // a single-stroke word is a stem for a run that goes on from it
  const stems = new Map();
  for (const [word, outlines] of Object.entries(dict.d)) {
    if (!/^[a-z']+$/.test(word)) continue;
    for (const o of outlines) if (!o.includes('/') && !o.startsWith('+') && !stems.has(o)) stems.set(o, word);
  }

  const outlineText = (strokes) => strokes.map(key).join('/');

  function piece(bitsList, at) {
    if (at > 0 && at + 1 < bitsList.length) {
      const a = bitsList[at];
      const b = bitsList[at + 1];
      const two = suffix2.get(`${a}/${b}`);
      if (two !== undefined && (a & b & JOIN)) return { text: two, space: false, used: 2, kind: 'suffix' };
    }
    const bits = bitsList[at];
    if (at > 0 && (bits & JOIN) && suffix.has(bits)) return { text: suffix.get(bits), space: false, used: 1, kind: 'suffix' };
    const r = resolveStroke(rules, bits);
    if (!r) return null;
    const text = [...r.onset, ...(r.vowel ? [r.vowel] : []), ...r.coda].map(spelling).join('') + (r.marker ? '*' : '');
    return { text, space: !r.join, used: 1, kind: 'sounds' };
  }

  /** The fallback reading of one run (a list of Sets), or null if some stroke reads as nothing. */
  function resolveRun(strokes) {
    const bitsList = strokes.map(bitsOf);
    let text = '';
    let at = 0;
    let kind = 'sounds';
    const stem = strokes.length > 1 ? stems.get(key(strokes[0])) : undefined;
    if (stem !== undefined) {
      const next = piece(bitsList, 1);
      if (!next) return null;
      text += dropSilentE(stem, /^[aeiou]/.test(next.text)) + (next.space ? ' ' : '') + next.text;
      at = 1 + next.used;
      kind = 'stem';
    }
    while (at < bitsList.length) {
      const p = piece(bitsList, at);
      if (!p) return null;
      if (p.space && text) text += ' ';
      text += p.text;
      at += p.used;
    }
    return { text, kind };
  }

  function readRun(strokes) {
    const text = outlineText(strokes);
    if (entries.has(text)) return { text: entries.get(text), kind: 'word' };
    if (punct.has(text)) return reserved(punct.get(text));
    const r = resolveRun(strokes);
    if (r) return { text: r.text, kind: 'guess', how: r.kind };
    return { text: text, kind: 'raw' };
  }

  /** Runs of a stroke list: a stroke without the join key opens one, a joined stroke continues it. */
  function runsOf(strokes) {
    const runs = [];
    for (const s of strokes) {
      if (runs.length && s.has('+')) runs[runs.length - 1].push(s);
      else runs.push([s]);
    }
    return runs;
  }

  /**
   * strokes: an array of Sets of key ids (as steno.js parses them).
   * Returns { text, parts: [{ strokes: [text], text, kind, how? }] }: kind is `word`, `punct`,
   * `command`, `guess` (read back from the keys) or `raw` (nothing reads it).
   */
  function translate(strokes) {
    const runs = runsOf(strokes);
    const parts = [];
    for (let i = 0; i < runs.length; i += 1) {
      // a reserved outline can span runs (`-FP/PHOEUFP`): the longest one that is an entry wins
      let took = 1;
      for (let k = Math.min(3, runs.length - i); k >= 2; k -= 1) {
        const joined = runs.slice(i, i + k).flat();
        const t = outlineText(joined);
        if (entries.has(t) || punct.has(t)) { took = k; break; }
      }
      const group = runs.slice(i, i + took).flat();
      parts.push({ strokes: group.map(key), ...readRun(group) });
      i += took - 1;
    }
    let text = '';
    let capNext = false;
    let attachNext = false;
    for (const p of parts) {
      let t = p.text;
      const glue = p.kind === 'punct' && p.attachPrev;
      if (capNext && t) t = t[0].toUpperCase() + t.slice(1);
      capNext = false;
      if (text && !glue && !attachNext && p.kind !== 'command') text += ' ';
      text += t;
      attachNext = p.kind === 'punct' && !!p.attachNext;
      if (p.kind === 'punct' && p.cap) capNext = true;
      p.shown = t;
    }
    return { text, parts };
  }

  const norm = (s) => s.trim().toLowerCase();

  /** Do these strokes, however they were found, type exactly this word or phrase? */
  function accepts(target, strokes) {
    if (!strokes.length) return false;
    return norm(translate(strokes).text) === norm(target);
  }

  /** Could more strokes still finish the word? Known outlines of it, or a start of its spelling. */
  function prefixOf(target, strokes) {
    if (!strokes.length) return true;
    const typed = strokes.map(key);
    for (const o of byWord.get(norm(target)) ?? byWord.get(target) ?? []) {
      const parts = o.split('/');
      if (parts.length > typed.length && typed.every((s, i) => parts[i] === s)) return true;
    }
    const got = norm(translate(strokes).text);
    return got.length >= 2 && got.length < norm(target).length && norm(target).startsWith(got);
  }

  /** Every outline the dictionary has for a word, fewest strokes first. */
  const outlinesOf = (word) => [...(byWord.get(norm(word)) ?? [])].sort((a, b) =>
    (a.split('/').length - b.split('/').length) || (a.length - b.length));

  /**
   * What an outline is defined as, in Plover's dictionary syntax where there is one: `{^ed}` for a
   * continuation stroke that is a suffix (it attaches to the word before), the raw value of a
   * punctuation or command entry (`{.}`), the word of an ordinary entry, and otherwise what the
   * fallback reads the keys as. { text, how: 'suffix' | 'entry' | 'guess' | 'raw' }.
   */
  function define(strokes) {
    const bits = strokes.map(bitsOf);
    const lone = bits.every((b) => b & JOIN);
    const sfx = !lone ? undefined : bits.length === 1 ? suffix.get(bits[0]) : bits.length === 2 ? suffix2.get(bits.join('/')) : undefined;
    if (sfx !== undefined) return { text: `{^${sfx}}`, how: 'suffix' };
    const t = outlineText(strokes);
    if (punct.has(t)) return { text: punct.get(t), how: 'entry' };
    if (entries.has(t)) return { text: entries.get(t), how: 'entry' };
    const read = translate(strokes);
    return { text: read.text, how: read.parts.some((p) => p.kind === 'raw') ? 'raw' : 'guess' };
  }

  return { translate, define, accepts, prefixOf, outlinesOf, resolveRun, parseOutline };
}

let loading = null;

/** Fetches the engine's data once (about 1.2 MB gzipped) and resolves to an engine. */
export function loadEngine(base = 'data/') {
  loading ??= Promise.all(['engine.json', 'joins.json', 'chart.json'].map((f) => fetch(base + f).then((r) => {
    if (!r.ok) throw new Error(`${f}: ${r.status}`);
    return r.json();
  }))).then(([dict, joins, chart]) => createEngine({ dict, joins, chart }));
  return loading;
}

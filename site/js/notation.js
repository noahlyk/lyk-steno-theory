// lyk's short notation on the site: how an outline is written for people (`TPHRAOEUT` is `FLĪT`),
// and read back. Only an outline whose split is known (lyk wrote it from its sounds, or lists it
// written its own way) is short; any other (a brief, a stroke no entry has) is raw, a key each. The same table keymux and the `lyk` CLI use, written by
// `lyk build` (`out/lyk/.keymux-notation.json`) and copied to `data/notation.json` by
// `tools/site_data.py`; the rules mirror `src/notation.rs` (and keymux's `steno/notation.rs`).
// Page sources and data files stay in raw notation: only what is shown is short. Pure, no DOM.

// The keys in steno order; the index is the bit, as in lyk and keymux.
const KEYS = '#STKPWHRAO*EUFRPBLGTSDZ';
const JOIN = 1 << 23;
const SPELL = 1 << 24;
const NUMBER = 1;
const LEFT = 0b1111_1110;
const VOWELS = (0b11 << 8) | (0b11 << 11);
const STAR = 1 << 10;
const MIDDLE = VOWELS | STAR;
const RIGHT = 0b11_1111_1111 << 13;

const bitsOf = (mask) => [...Array(25).keys()].filter((i) => mask & (1 << i)).map((i) => 1 << i);
const letter = (bit) => KEYS[31 - Math.clz32(bit)];
const count = (n) => bitsOf(n).length;
const lowest = (n) => n & -n;

/** A raw stroke (`TPHAEPL`, `+-D`, `#TR-RL`) as bits; null if it is not one. */
export function rawBits(text) {
  let bits = 0;
  let rest = text;
  if (rest.startsWith('+')) { bits |= JOIN; rest = rest.slice(1); } else if (rest.startsWith('~')) { bits |= SPELL; rest = rest.slice(1); }
  let cursor = 0;
  for (const c of rest) {
    if (c === '-') { cursor = Math.max(cursor, 13); continue; }
    const at = KEYS.indexOf(c, cursor);
    if (at < 0) return null;
    bits |= 1 << at;
    cursor = at + 1;
  }
  return (bits & ~(JOIN | SPELL)) !== 0 || bits === JOIN ? bits : null;
}

/** Bits as raw notation, as lyk and the dictionary files write them. */
export function rawText(bits) {
  let out = bits & JOIN ? '+' : bits & SPELL ? '~' : '';
  let hyphen = (bits & (0b11111 << 8)) === 0;
  for (let i = 0; i < 23; i += 1) {
    if (!(bits & (1 << i))) continue;
    if (hyphen && i >= 13) { out += '-'; hyphen = false; }
    out += KEYS[i];
  }
  return out;
}

const rawLetters = (bits) => rawText(bits).replace('-', '');

let table = null;

/** Use this notation file (the parsed JSON); `null` writes raw notation. */
export function setNotation(json) {
  if (!json) { table = null; return; }
  const hand = (obj, mask, dash) => {
    const out = [];
    for (const [keys, letters] of Object.entries(obj ?? {})) {
      const bits = rawBits(`${dash}${keys}`);
      const text = String(letters).toUpperCase();
      if (bits === null || bits & ~mask || !text) continue;
      if (out.some(([b, t]) => b === bits || t === text)) continue;
      out.push([bits, text]);
    }
    return out;
  };
  const t = { left: hand(json.left, LEFT, ''), right: hand(json.right, RIGHT, '-'), middle: hand(json.middle, VOWELS, ''), ascii: Object.entries(json.ascii ?? {}), outlines: new Map(), byShort: new Map(), agree: new Set(json.agree ?? []) };
  table = t;
  for (const [raw, short] of Object.entries(json.outlines ?? {})) {
    if (parseOutlineBits(short)?.map(rawText).join('/') === raw) { t.outlines.set(raw, short); t.byShort.set(short, raw); }
  }
}

/** Whether a notation file is in use. */
export const hasNotation = () => table !== null;

// One hand's keys as tokens: from the lowest key left, the alias with the most keys that starts
// there, is all down and has no key still to write inside its span; else the key's own letter.
function tokens(aliases, bits) {
  const out = [];
  let left = bits;
  while (left) {
    const low = lowest(left);
    let best = null;
    for (const [keys, text] of aliases) {
      const span = (2 ** (32 - Math.clz32(keys))) - lowest(keys);
      if (lowest(keys) === low && (keys & ~left) === 0 && (left & span & ~keys) === 0 && (!best || count(keys) > count(best[0]))) best = [keys, text];
    }
    const [keys, text] = best ?? [low, letter(low)];
    out.push({ text, keys });
    left &= ~keys;
  }
  return out;
}

/** One stroke in raw notation as pieces, a key each. */
export function rawPieces(bits) {
  const keys = bitsOf(bits & ((1 << 23) - 1));
  return [...rawText(bits)].map((c) => ({ text: c, keys: c === '-' ? 0 : c === '+' ? JOIN : c === '~' ? SPELL : keys.shift() ?? 0 }));
}

/** Whether a raw outline's split is known: written its own way, or split by its sounds as the table writes it. */
export const known = (raw) => !!table && (table.outlines.has(raw) || table.agree.has(raw));

/** One stroke (bits) as the pieces the table writes it in: `{text, keys}`, keys 0 for the `-` that only parts the hands. */
export function strokePieces(bits) {
  if (!table) return rawPieces(bits);
  const out = [];
  for (const [key, text] of [[JOIN, '+'], [SPELL, '~'], [NUMBER, '#']]) if (bits & key) out.push({ text, keys: key });
  out.push(...tokens(table.left, bits & LEFT));
  if (bits & MIDDLE) {
    const vowels = bits & VOWELS;
    const mark = vowels && table.middle.find(([keys]) => keys === vowels);
    if (mark) {
      out.push({ text: mark[1], keys: vowels });
      if (bits & STAR) out.push({ text: '*', keys: STAR });
    } else {
      out.push(...bitsOf(bits & MIDDLE).map((b) => ({ text: letter(b), keys: b })));
    }
  } else if (bits & RIGHT) {
    out.push({ text: '-', keys: 0 });
  }
  out.push(...tokens(table.right, bits & RIGHT));
  return out;
}

/** An outline (raw text) as each stroke's pieces: one written its own way as its own text reads, one whose split
 *  agrees with the table by the table, any other raw, a key a piece. */
export function outlinePieces(raw) {
  const strokes = raw.split('/').map(rawBits);
  if (strokes.some((s) => s === null)) return null;
  if (!known(raw)) return strokes.map(rawPieces);
  const own = table.outlines.get(raw);
  if (own) {
    const parts = own.split('/').map(readShort);
    if (parts.length === strokes.length && parts.every((p, i) => p && p.reduce((a, x) => a | x.keys, 0) === strokes[i])) return parts;
  }
  return strokes.map(strokePieces);
}

/** A raw outline as people read it; text that is no outline comes back as it is. */
export function shortOutline(raw) {
  const pieces = outlinePieces(raw);
  return pieces ? pieces.map((s) => s.map((p) => p.text).join('')).join('/') : raw;
}

function handTokens(aliases, mask) {
  const out = aliases.map(([keys, text]) => [text, keys]);
  out.push(...bitsOf(mask).map((b) => [letter(b), b]));
  for (const [symbol, ascii] of table.ascii) {
    const alias = aliases.find(([, text]) => text === symbol);
    if (alias && !aliases.some(([, text]) => text === ascii)) out.push([ascii, alias[0]]);
  }
  return out.sort((a, b) => [...b[0]].length - [...a[0]].length);
}

function readTokens(text, toks, out) {
  let rest = text;
  while (rest) {
    const hit = toks.find(([t]) => rest.startsWith(t));
    if (!hit) return false;
    out.push({ text: hit[0], keys: hit[1] });
    rest = rest.slice(hit[0].length);
  }
  return true;
}

// The short form read back as pieces: `+`, `~`, `#` anywhere (each once), the left hand up to the
// first character of a vowel token, the vowel part (marks, their ASCII, the vowel keys, the star,
// one `-`, each key once, any order), then the right hand; each part greedily, longest token first.
function readShort(text) {
  const out = [];
  let rest = '';
  for (const c of text) {
    const key = c === '+' ? JOIN : c === '~' ? SPELL : c === '#' ? NUMBER : null;
    if (key === null) { rest += c; continue; }
    if (out.some((p) => p.keys === key)) return null;
    out.push({ text: c, keys: key });
  }
  const middle = table.middle.map(([keys, t]) => [t, keys]);
  for (const [symbol, ascii] of table.ascii) {
    const m = table.middle.find(([, t]) => t === symbol);
    if (m && !table.middle.some(([, t]) => t === ascii)) middle.push([ascii, m[0]]);
  }
  middle.push(...bitsOf(MIDDLE).map((b) => [letter(b), b]), ['-', 0]);
  middle.sort((a, b) => [...b[0]].length - [...a[0]].length);
  const isMiddle = (c) => middle.some(([t]) => t.includes(c));
  const chars = [...rest];
  let start = chars.findIndex(isMiddle);
  if (start < 0) start = chars.length;
  let end = start;
  while (end < chars.length && isMiddle(chars[end])) end += 1;
  if (!readTokens(chars.slice(0, start).join(''), handTokens(table.left, LEFT), out)) return null;
  const at = out.length;
  if (!readTokens(chars.slice(start, end).join(''), middle, out)) return null;
  let taken = 0;
  let dashed = false;
  for (const p of out.slice(at)) {
    if (p.keys === 0) { if (dashed) return null; dashed = true; } else if (p.keys & taken) return null;
    taken |= p.keys;
  }
  if (!readTokens(chars.slice(end).join(''), handTokens(table.right, RIGHT), out)) return null;
  return out;
}

/** One stroke, raw or short: exactly raw notation is raw, else short, else raw read leniently. Bits, or null. */
export function parseStrokeBits(text) {
  const raw = rawBits(text);
  if (raw !== null && rawText(raw) === text) return raw;
  const short = table ? readShort(text) : null;
  const bits = short ? short.reduce((a, p) => a | p.keys, 0) : 0;
  return bits || raw;
}

/** An outline, raw or short (spaces around a `/` ignored), as bits per stroke; null if it is not one. */
export function parseOutlineBits(text) {
  const t = text.split('/').map((s) => s.trim()).join('/');
  const raw = table?.byShort.get(t);
  if (raw) return raw.split('/').map(rawBits);
  const out = t.split('/').map(parseStrokeBits);
  return out.some((s) => s === null || s === undefined) ? null : out;
}

/** An outline typed raw or short, as raw notation (what the dictionary and the engine read); null if it is not one. */
export function toRaw(text) {
  const bits = parseOutlineBits(text);
  return bits ? bits.map(rawText).join('/') : null;
}

/** The site's key ids (`S-`, `A`, `-F`, `+`, `#`) of a piece's bits. */
export function keyIds(bits) {
  return bitsOf(bits).map((b) => {
    if (b === JOIN) return '+';
    if (b === NUMBER) return '#';
    if (b === SPELL) return '~';
    const i = 31 - Math.clz32(b);
    return i < 8 ? `${KEYS[i]}-` : i < 13 ? KEYS[i] : `-${KEYS[i]}`;
  });
}

/** The site's key-id Set of a stroke as bits. */
export function bitsOfKeys(keys) {
  let bits = 0;
  for (const k of keys) {
    if (k === '+') bits |= JOIN;
    else if (k === '#') bits |= NUMBER;
    else if (k.endsWith('-')) bits |= 1 << KEYS.indexOf(k[0]);
    else if (k.startsWith('-')) bits |= 1 << KEYS.indexOf(k[1], 13);
    else bits |= 1 << KEYS.indexOf(k, 8);
  }
  return bits;
}

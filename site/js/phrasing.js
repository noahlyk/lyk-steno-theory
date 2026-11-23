// What the phrasing grammar says about a query, for the lookup page: the phrase (or the start of
// one) a text is, and what a stroke is made of. Pure, no DOM; the same reading as `lyk lookup`
// (src/answer.rs). The tables are data/grammar.json (tools/site_data.py), the strokes are the
// dictionary's own phrasing file.

import { LEFT, RIGHT, VOWELS, formatStroke, parseBarStroke, parseStroke } from './steno.js';

/** The words of a query as the grammar's texts have them: lowercase, a curly apostrophe straight,
 *  punctuation gone, `I'm`, `they're`, `I've`, `I'll` spelt out. */
export function tokens(query) {
  const out = [];
  const clean = query.toLowerCase().replace(/[‘’]/g, "'").replace(/[^\p{L}\p{N}']+/gu, ' ');
  for (let token of clean.split(/\s+/)) {
    token = token.replace(/^'+|'+$/g, '');
    if (!token) continue;
    const split = [["'m", 'am'], ["'re", 'are'], ["'ve", 'have'], ["'ll", 'will']].find(([tail]) => token.endsWith(tail) && token.length > tail.length);
    if (split) out.push(token.slice(0, -split[0].length), split[1]);
    else out.push(token);
  }
  return out;
}

const NEGATABLE = new Set(['is', 'are', 'was', 'were', 'do', 'does', 'did', 'has', 'have', 'had', 'could', 'would', 'should', 'must']);

/** `do not` is `don't`, `will not` is `won't`, `can not` and `cannot` are `can't`; and how many tokens each stands for. */
export function contractNot(toks) {
  const out = [];
  const spans = [];
  for (const t of toks) {
    const prev = out[out.length - 1];
    if (t === 'not' && prev === 'will') { out[out.length - 1] = "won't"; spans[spans.length - 1] += 1; }
    else if (t === 'not' && prev === 'can') { out[out.length - 1] = "can't"; spans[spans.length - 1] += 1; }
    else if (t === 'not' && NEGATABLE.has(prev)) { out[out.length - 1] = `${prev}n't`; spans[spans.length - 1] += 1; }
    else if (t === 'cannot') { out.push("can't"); spans.push(1); }
    else { out.push(t); spans.push(1); }
  }
  return { out, spans };
}

const indexes = new WeakMap();

/** The grammar's phrases by lower-case text and by stroke. */
export function index(g) {
  if (!indexes.has(g)) {
    const byText = new Map();
    const byStroke = new Map();
    for (const [text, stroke] of Object.entries(g.entries)) {
      byText.set(text.toLowerCase(), [text, stroke]);
      byStroke.set(stroke, text);
    }
    indexes.set(g, { byText, byStroke });
  }
  return indexes.get(g);
}

const longest = (idx, toks) => {
  for (let n = toks.length; n >= 2; n -= 1) {
    const hit = idx.byText.get(toks.slice(0, n).join(' '));
    if (hit) return { covered: n, text: hit[0], stroke: hit[1] };
  }
  return null;
};

/** The longest start of a query (two words at least) the grammar writes: `{ covered, total, text, stroke, rest }`, or null.
 *  `covered` counts the query's words, so `I do not know` is covered by `I don't know` to its 4th word. */
export function leading(g, query) {
  const idx = index(g);
  const toks = tokens(query);
  const typed = longest(idx, toks);
  const { out, spans } = contractNot(toks);
  const contracted = longest(idx, out);
  if (contracted) contracted.covered = spans.slice(0, contracted.covered).reduce((a, b) => a + b, 0);
  const hit = typed && contracted ? (contracted.covered > typed.covered ? contracted : typed) : typed ?? contracted;
  return hit ? { ...hit, total: toks.length, rest: toks.slice(hit.covered).join(' ') } : null;
}

const keyText = (ids) => formatStroke(new Set(ids));

/** What a stroke is made of, as `[{ kind, keys, says }]` in the order question, starter, auxiliary, not, verb; null if it is not a grammar stroke. */
export function explain(g, stroke) {
  const keys = parseBarStroke(stroke);
  if (!keys || keys.has('+')) return null;
  const left = LEFT.filter((k) => keys.has(`${k}-`));
  const starter = g.starters.find(([, chord]) => chord === left.join(''));
  if (!starter) return null;
  const vowels = VOWELS.filter((k) => k !== '*' && keys.has(k));
  const past = vowels.includes('E');
  // the modal's keys with the past `E` taken out: `U` and `E` together are *shall* in the past, *should*
  const plain = vowels.filter((k) => k !== 'E').join('');
  const modal = plain ? g.modals.find((m) => m[1] === plain) : null;
  if (plain && !modal) return null;
  const parts = [];
  if (keys.has('#')) parts.push({ kind: 'question', keys: '#', says: 'question' });
  parts.push({ kind: 'starter', keys: starter[1], says: starter[0] });
  if (past || modal) {
    const says = modal ? (past ? modal[3] : modal[2]) : 'past';
    if (says === null) return null;
    parts.push({ kind: 'auxiliary', keys: keyText(vowels), says });
  }
  if (keys.has('*')) parts.push({ kind: 'not', keys: '*', says: 'not' });
  const right = RIGHT.filter((k) => keys.has(`-${k}`)).join('');
  if (right) {
    const verb = [...g.special, ...g.verbs.map((v) => [v[0], v[3]])].find(([, code]) => code.replace('-', '') === right);
    if (!verb) return null;
    parts.push({ kind: 'verb', keys: keyText(RIGHT.filter((k) => keys.has(`-${k}`)).map((k) => `-${k}`)), says: verb[0] });
  }
  return parts;
}

/** The parts as one line: `SWR starter (I) + E auxiliary (past) + -B verb (be)`. */
export const describe = (parts) => parts.map((p) => (p.kind === 'not' ? `${p.keys} not` : p.kind === 'question' ? `${p.keys} question` : `${p.keys} ${p.kind} (${p.says})`)).join(' + ');

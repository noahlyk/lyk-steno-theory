// How a stroke or an outline is drawn wherever it is meant to be read (the key trainer, lookup; an
// outline in lyk's notation, see notation.js):
// a row of letters in steno order, coloured by hand (amber left, blue thumbs, purple right, green
// for the star and join key), keys one finger plays together underlined as a pair, and a `-` only
// where the right hand could not otherwise be told from the left. Strokes of an outline are
// divided by a thin rule. No columns: a steno tape spaces every key out and is hard to read.

import { el } from './dom.js';
import { KEYS, needsHyphen } from './steno.js';
import { keyIds, outlinePieces, shortOutline } from './notation.js';

export const GROUP_OF = (k) => (k === '+' || k === '*' ? 'misc' : /^[AOEU]$/.test(k) ? 'vowel' : k.startsWith('-') ? 'right' : 'left');
// keys one finger plays together (a column; the two keys of a thumb pair)
const PAIRS = [['T-', 'K-'], ['P-', 'W-'], ['H-', 'R-'], ['A', 'O'], ['E', 'U'],
  ['-F', '-R'], ['-P', '-B'], ['-L', '-G'], ['-T', '-S'], ['-D', '-Z']];
const PAIR_OF = new Map(PAIRS.flatMap(([a, b]) => [[a, { pos: 'a', with: b }], [b, { pos: 'b', with: a }]]));
const DISPLAY_ORDER = ['+', ...KEYS.filter((k) => k !== '+')];

export const sound = (id) => id.replace('-', ''); // `+` stays `+`

/** One stroke (a Set of key ids) as coloured letters. `held` (a Set) lights the keys being pressed. */
export function strokeEl(keys, held = null) {
  const out = el('span', { class: 'kst' });
  let dashed = !needsHyphen(keys);
  for (const k of DISPLAY_ORDER) {
    if (!keys.has(k)) continue;
    if (!dashed && k.startsWith('-')) { out.append(el('span', { class: 'ks ks--dash' }, '-')); dashed = true; }
    const pair = PAIR_OF.get(k);
    const paired = pair && keys.has(pair.with) ? ` pair-${pair.pos}` : '';
    out.append(el('span', { class: `ks ks--${GROUP_OF(k)}${paired}${held?.has(k) ? ' is-held' : ''}` }, sound(k)));
  }
  return out;
}

/** One stroke's pieces (site/js/notation.js) as coloured text: a group of keys written as one
 *  symbol (`Š`, `Ī`) is one piece in its hand's colour; single keys are underlined in pairs as `strokeEl` does. */
function piecesEl(pieces) {
  const out = el('span', { class: 'kst' });
  const ids = new Set(pieces.flatMap((p) => keyIds(p.keys)));
  for (const p of pieces) {
    if (p.keys === 0) { out.append(el('span', { class: 'ks ks--dash' }, '-')); continue; }
    const keys = keyIds(p.keys);
    const pair = keys.length === 1 ? PAIR_OF.get(keys[0]) : null;
    const paired = pair && ids.has(pair.with) ? ` pair-${pair.pos}` : '';
    const group = keys[0] === '#' || keys[0] === '~' ? 'misc' : GROUP_OF(keys[0]);
    out.append(el('span', { class: `ks ks--${group}${paired}${keys.length > 1 ? ' ks--group' : ''}` }, p.text));
  }
  return out;
}

/** An outline (`TKOG/+-Z`, raw as the pages and data hold it) as its strokes in a row, divided by a thin rule, in
 *  lyk's notation: short where its split is known, else raw; the text if it is not an outline. */
export function outlineEl(text) {
  const pieces = outlinePieces(text);
  if (!pieces) return el('span', { class: 'outline outline--raw' }, text);
  return el('span', { class: 'outline', 'aria-label': shortOutline(text) }, ...pieces.map(piecesEl));
}

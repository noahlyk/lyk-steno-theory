// How a stroke or an outline is drawn wherever it is meant to be read (the key trainer, lookup):
// a row of letters in steno order, coloured by hand (amber left, blue thumbs, purple right, green
// for the star and join key), keys one finger plays together underlined as a pair, and a `-` only
// where the right hand could not otherwise be told from the left. Strokes of an outline are
// divided by a thin rule. No columns: a steno tape spaces every key out and is hard to read.

import { el } from './dom.js';
import { KEYS, needsHyphen, parseOutline } from './steno.js';

export const GROUP_OF = (k) => (k === '+' || k === '*' ? 'misc' : /^[AOEU]$/.test(k) ? 'vowel' : k.startsWith('-') ? 'right' : 'left');
// keys one finger plays together (a column; the two keys of a thumb pair)
const PAIRS = [['+', 'S-'], ['T-', 'K-'], ['P-', 'W-'], ['H-', 'R-'], ['A', 'O'], ['E', 'U'],
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

/** An outline (`TKOG/+-Z`) as its strokes in a row, divided by a thin rule; the text if it is not one. */
export function outlineEl(text) {
  const strokes = parseOutline(text);
  if (!strokes) return el('span', { class: 'outline outline--raw' }, text);
  return el('span', { class: 'outline', 'aria-label': text }, ...strokes.map((s) => strokeEl(s)));
}

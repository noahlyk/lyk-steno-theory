// What a typed stroke stream reads as, for the "type and see the output" pages. The engine's data
// loads on first use; until then `translate` returns an empty reading with `loading: true`, and
// `ready` resolves once it is there.

import { loadEngine } from './engine.js';
import { parseOutline } from './steno.js';

let engine = null;
export const ready = loadEngine().then((e) => { engine = e; return e; });

/**
 * strokes: an array of key Sets (as steno.js makes them) or of stroke texts (`TKOG`, `+-Z`), or
 * one outline text (`ED/+-D`). Returns { text, parts: [{ strokes, text, kind, how? }], loading? }.
 */
export function translate(strokes) {
  const list = typeof strokes === 'string' ? parseOutline(strokes) ?? [] :
    strokes.map((s) => (typeof s === 'string' ? parseOutline(s)?.[0] : s)).filter(Boolean);
  if (!engine) return { text: '', parts: [], loading: true };
  return engine.translate(list);
}

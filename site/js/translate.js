// What a typed stroke stream reads as, for the "type and see the output" pages. The engine's data
// loads on first use; until then `translate` returns an empty reading with `loading: true`, and
// `ensure()` (or awaiting `ready`) starts the load and resolves once it is there.

import { loadEngine } from './engine.js';
import { parseOutline } from './steno.js';

let engine = null;
let loading = null;
/** Starts loading the engine's data (once) and resolves when it is there. Nothing loads until asked. */
export const ensure = () => (loading ??= loadEngine().then((e) => { engine = e; return e; }));
export const ready = { then: (a, b) => ensure().then(a, b) }; // `await ready` starts the load too

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

/**
 * What an outline (`+-D`, `ED/+-D`) is defined as: { text, how } (see engine.define), or null while
 * the data is still loading or the outline is not a stroke sequence.
 */
export function define(outline) {
  const list = parseOutline(outline);
  if (!list || !engine) return null;
  return engine.define(list);
}

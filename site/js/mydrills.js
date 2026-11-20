// "My drills": a personal drill set made by `lyk mistakes --drill` from the user's own steno tape.
// It never ships with the site: the file is gitignored, loaded from a file the user picks or from
// `local/my-drills.json` when a local server has one (a 404 on the public site, which is fine), and
// kept in this browser's localStorage only.

import { parseOutline } from './steno.js';

const KEY = 'lyk-my-drills-v1';

const okItem = (it) => it && typeof it.p === 'string' && typeof it.w === 'string' && Array.isArray(it.o)
  && it.o.length > 0 && it.o.every((o) => typeof o === 'string' && parseOutline(o));
const clean = (it) => ({ p: it.p, o: it.o, w: it.w, n: typeof it.n === 'string' ? it.n : '' });

/** The drill file as the drills want it, or null when it is not one. */
export function normalise(raw) {
  if (!raw || raw.v !== 1 || !Array.isArray(raw.items)) return null;
  const items = raw.items.filter(okItem).map(clean);
  const pairs = (Array.isArray(raw.pairs) ? raw.pairs : [])
    .map((p) => ({ title: String(p.title ?? ''), items: (Array.isArray(p.items) ? p.items : []).filter(okItem).map(clean) }))
    .filter((p) => p.items.length > 1);
  return items.length || pairs.length ? { title: String(raw.title ?? 'My drills'), items, pairs } : null;
}

export function loadSaved() {
  try {
    return normalise(JSON.parse(localStorage.getItem(KEY) ?? 'null'));
  } catch {
    return null;
  }
}

export function saveDrills(raw) {
  try {
    localStorage.setItem(KEY, JSON.stringify(raw));
  } catch {
    // private window or blocked storage: it lasts until the page closes
  }
}

export function clearDrills() {
  try {
    localStorage.removeItem(KEY);
  } catch {
    // nothing to clear
  }
}

/** `local/my-drills.json` beside the page, when there is one (null everywhere else). */
export async function fetchLocal() {
  try {
    const r = await fetch('local/my-drills.json', { cache: 'no-cache' });
    return r.ok ? await r.json() : null;
  } catch {
    return null;
  }
}

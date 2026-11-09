import { el } from './dom.js';
import { BAR_H, BAR_Y, HEIGHT, KEYS, STEP, WIDTH, radius } from './boardgeom.js';
import { cover, labelFits } from './merge.js';

// The on-screen steno board (layout in boardgeom.js, the same as Keymux's overlay). Keys are grey;
// only a held or target key has colour. Keys that form a steno letter chord (T P H = N) light as one
// merged shape (merge.js), exactly as Keymux's overlay does. This is the only place a keyboard is
// drawn: drills, the Try it pad, the key stream, lessons and the hover board all use createBoard.

const SVG = 'http://www.w3.org/2000/svg';
const LABEL = (id) => id.replace('-', '');
const u = (n) => `calc(var(--u) * ${n})`;

function place(el, x, y, w, h, top, bottom) {
  el.style.left = u(x);
  el.style.top = u(y);
  el.style.width = u(w);
  el.style.height = u(h);
  const t = u(radius(top));
  const b = u(radius(bottom));
  el.style.borderRadius = `${t} ${t} ${b} ${b}`;
}

const svg = (name, attrs = {}) => {
  const n = document.createElementNS(SVG, name);
  for (const [k, v] of Object.entries(attrs)) n.setAttribute(k, v);
  return n;
};

/**
 * Draws a board into `host`. Returns a controller:
 *   show(keys)        light these keys as the target
 *   press(keys)       mark these as held down (the learner's stroke)
 *   diff(want, got)   mark missing and extra keys after a miss
 *   clear()
 * The board is a display, not an input: there is nothing to click. The `#` bar is drawn as on the
 * keyboard but is not a lyk key, so it never lights. `merge: false` lights every key on its own.
 */
export function createBoard(host, { compact = false, merge = true } = {}) {
  host.classList.add('board');
  if (compact) host.classList.add('board--compact');
  host.textContent = '';
  host.style.width = u(WIDTH);
  host.style.height = u(HEIGHT);
  // merged shapes live on this layer, under the keys: a fused key is hidden so the shape shows
  // through, and a bridge's connector runs behind the unlit keys it passes
  const layer = svg('svg', { class: 'mlayer', viewBox: `0 0 ${WIDTH} ${HEIGHT}`, 'aria-hidden': 'true' });
  layer.style.width = u(WIDTH);
  layer.style.height = u(HEIGHT);
  host.append(layer);
  const els = new Map();
  const make = (id, label, group, x, y, w, h, top, bottom) => {
    const e = document.createElement('div');
    e.className = `key key--${group}`;
    e.dataset.key = id;
    e.textContent = label;
    place(e, x, y, w, h, top, bottom);
    host.append(e);
    els.set(id, e);
    return e;
  };
  for (const [id, col, y, h, group, top, bottom] of KEYS) make(id, LABEL(id), group, col * STEP, y, 1, h, top, bottom);
  make('#', '#', 'misc', 0, BAR_Y, WIDTH, BAR_H, 'soft', 'soft').classList.add('key--bar');

  const state = new Map(); // key id -> Set of is-target / is-down / is-miss / is-extra
  const flag = (id, c, on) => {
    const s = state.get(id) ?? new Set();
    if (on) s.add(c); else s.delete(c);
    state.set(id, s);
  };
  // how the key-state CSS draws a key; keys of one look can merge, keys of different looks cannot
  const look = (id) => {
    const s = state.get(id);
    if (!s) return null;
    if (s.has('is-miss')) return 'miss';
    if (s.has('is-extra')) return 'wrong';
    if (s.has('is-down')) {
      if (!host.classList.contains('has-target')) return 'down';
      return s.has('is-target') ? 'down-ok' : 'wrong';
    }
    return s.has('is-target') ? 'target' : null;
  };
  const paint = () => {
    for (const [id, e] of els) {
      e.classList.remove('is-target', 'is-down', 'is-miss', 'is-extra', 'is-fused');
      for (const c of state.get(id) ?? []) e.classList.add(c);
    }
    layer.replaceChildren();
    if (!merge) return;
    const looks = new Map();
    for (const id of state.keys()) looks.set(id, look(id));
    for (const { shape, look: l } of cover(looks)) {
      for (const k of shape.keys) els.get(k).classList.add('is-fused');
      const g = svg('g', { class: `mkey mkey--${shape.group} mk-${l}` });
      g.append(svg('path', { d: shape.d }));
      if (labelFits(shape)) {
        const t = svg('text', { x: shape.center[0], y: shape.center[1] });
        t.textContent = shape.label;
        g.append(t);
      }
      // the tick or cross a single key shows (a hinted key held, or a held key not wanted)
      const mark = l === 'down-ok' ? '✓' : l === 'wrong' ? '×' : '';
      if (mark) {
        const m = svg('text', { class: 'mark', x: shape.bounds.x + shape.bounds.w - 0.1, y: shape.bounds.y + 0.25 });
        m.textContent = mark;
        g.append(m);
      }
      layer.append(g);
    }
  };
  const reset = () => {
    host.classList.remove('has-target');
    state.clear();
  };
  return {
    clear() { reset(); paint(); },
    show(keys) {
      reset();
      // with a hint on, a held key the hint does not want turns red (see the key-state CSS)
      host.classList.toggle('has-target', Boolean(keys?.size));
      for (const id of keys ?? []) flag(id, 'is-target', true);
      paint();
    },
    press(keys) {
      for (const id of state.keys()) flag(id, 'is-down', false);
      for (const id of keys ?? []) flag(id, 'is-down', true);
      paint();
    },
    diff(want, got) {
      reset();
      for (const id of want) flag(id, got.has(id) ? 'is-down' : 'is-miss', true);
      for (const id of got) if (!want.has(id)) flag(id, 'is-extra', true);
      paint();
    },
  };
}

/** The key-state legend: what a hinted, held, wrong and missing key each look like. */
export function boardLegend() {
  const item = (cls, text) => el('span', {}, el('i', { class: `sw ${cls}` }), text);
  return el('div', { class: 'keylegend', 'aria-label': 'key colours' },
    item('is-target', 'hold this'), item('is-down', 'you are holding'),
    item('is-wrong', 'not needed'), item('is-miss', 'you missed'));
}

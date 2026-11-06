import { el } from './dom.js';

// The on-screen steno board, laid out exactly like Keymux's overlay (src/overlay/geometry.rs and
// render.rs) with the lyk config's `+: SplitKey(slot: "S-", half: Top)`: the `S-` key is split in
// two, `+` on top and `S-` below, each as tall as a finger key; the `#` bar across every column,
// two finger rows with a half-size gap inside a column, a tall `*`, and the thumbs at half columns
// under the inner fingers. Keys are grey; only a held or target key has colour.

// All sizes in key widths (`--u`), as in Keymux.
const GAP = 0.16;
const PAIR_GAP = GAP * 0.5; // between the two keys of one column
const KEY_H = 1.75;
const BAR_H = 0.5;
const THUMB_H = 1.35;
const THUMB_DROP = 0.35;
const SOFT = 0.1; // the small corner radius
const STEP = 1 + GAP;

const BAR_Y = 0;
const TOP_Y = BAR_Y + BAR_H + GAP;
const BOTTOM_Y = TOP_Y + KEY_H + PAIR_GAP;
const THUMB_Y = BOTTOM_Y + KEY_H + GAP + THUMB_DROP;
const TALL_H = KEY_H * 2 + PAIR_GAP;
export const WIDTH = 9 * STEP + 1;
export const HEIGHT = THUMB_Y + THUMB_H;

// [id, column, y, height, group, top corners, bottom corners]: `soft` / `square` / `round`
const finger = (id, col, row, group) => [id, col, row === 'top' ? TOP_Y : BOTTOM_Y, KEY_H, group,
  row === 'top' ? 'soft' : 'square', row === 'top' ? 'square' : 'round'];
const KEYS = [
  ['+', 0, TOP_Y, KEY_H, 'misc', 'soft', 'square'],
  ['S-', 0, BOTTOM_Y, KEY_H, 'left', 'square', 'round'],
  finger('T-', 1, 'top', 'left'), finger('K-', 1, 'bottom', 'left'),
  finger('P-', 2, 'top', 'left'), finger('W-', 2, 'bottom', 'left'),
  finger('H-', 3, 'top', 'left'), finger('R-', 3, 'bottom', 'left'),
  ['A', 2.5, THUMB_Y, THUMB_H, 'vowel', 'soft', 'round'],
  ['O', 3.5, THUMB_Y, THUMB_H, 'vowel', 'soft', 'round'],
  ['*', 4, TOP_Y, TALL_H, 'misc', 'soft', 'round'],
  ['E', 5.5, THUMB_Y, THUMB_H, 'vowel', 'soft', 'round'],
  ['U', 6.5, THUMB_Y, THUMB_H, 'vowel', 'soft', 'round'],
  finger('-F', 5, 'top', 'right'), finger('-R', 5, 'bottom', 'right'),
  finger('-P', 6, 'top', 'right'), finger('-B', 6, 'bottom', 'right'),
  finger('-L', 7, 'top', 'right'), finger('-G', 7, 'bottom', 'right'),
  finger('-T', 8, 'top', 'right'), finger('-S', 8, 'bottom', 'right'),
  finger('-D', 9, 'top', 'right'), finger('-Z', 9, 'bottom', 'right'),
];

const LABEL = (id) => id.replace('-', '');
const radius = (kind) => (kind === 'soft' ? SOFT : kind === 'round' ? 0.5 : 0);
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

/**
 * Draws a board into `host`. Returns a controller:
 *   show(keys)        light these keys as the target
 *   press(keys)       mark these as held down (the learner's stroke)
 *   diff(want, got)   mark missing and extra keys after a miss
 *   clear()
 * `onToggle(id)` makes the keys clickable. The `#` bar is drawn as on the keyboard but is not a
 * lyk key, so it never lights.
 */
export function createBoard(host, { onToggle = null, compact = false } = {}) {
  host.classList.add('board');
  if (compact) host.classList.add('board--compact');
  host.textContent = '';
  host.style.width = u(WIDTH);
  host.style.height = u(HEIGHT);
  const els = new Map();
  const make = (id, label, group, x, y, w, h, top, bottom, interactive) => {
    const el = document.createElement(onToggle && interactive ? 'button' : 'div');
    el.className = `key key--${group}`;
    el.dataset.key = id;
    el.textContent = label;
    place(el, x, y, w, h, top, bottom);
    if (onToggle && interactive) {
      el.type = 'button';
      el.setAttribute('aria-label', id === '+' ? 'join key' : `key ${label}`);
      el.addEventListener('click', () => onToggle(id));
    }
    host.append(el);
    els.set(id, el);
    return el;
  };
  for (const [id, col, y, h, group, top, bottom] of KEYS) make(id, LABEL(id), group, col * STEP, y, 1, h, top, bottom, true);
  make('#', '#', 'misc', 0, BAR_Y, WIDTH, BAR_H, 'soft', 'soft', false).classList.add('key--bar');

  const reset = () => {
    host.classList.remove('has-target');
    for (const el of els.values()) el.classList.remove('is-target', 'is-down', 'is-miss', 'is-extra');
  };
  return {
    clear: reset,
    show(keys) {
      reset();
      // with a hint on, a held key the hint does not want turns red (see the key-state CSS)
      host.classList.toggle('has-target', Boolean(keys?.size));
      for (const id of keys ?? []) els.get(id)?.classList.add('is-target');
    },
    press(keys) {
      for (const el of els.values()) el.classList.remove('is-down');
      for (const id of keys ?? []) els.get(id)?.classList.add('is-down');
    },
    diff(want, got) {
      reset();
      for (const id of want) els.get(id)?.classList.add(got.has(id) ? 'is-down' : 'is-miss');
      for (const id of got) if (!want.has(id)) els.get(id)?.classList.add('is-extra');
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

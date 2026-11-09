// The board's layout, pure data (no DOM) so the merge module and its tests can use it. Laid out
// exactly like Keymux's overlay (src/overlay/geometry.rs and render.rs) with the lyk config's
// `+: SplitKey(slot: "S-", half: Top)`: the `S-` key is split in two, `+` on top and `S-` below,
// each as tall as a finger key; the `#` bar across every column, two finger rows with a half-size
// gap inside a column, a tall `*`, and the thumbs at half columns under the inner fingers.

// All sizes in key widths (`--u`), as in Keymux.
const GAP = 0.16;
const PAIR_GAP = GAP * 0.5; // between the two keys of one column
export const KEY_H = 1.75;
export const BAR_H = 0.5;
const THUMB_H = 1.35;
const THUMB_DROP = 0.35;
export const SOFT = 0.1; // the small corner radius
export const STEP = 1 + GAP;

export const BAR_Y = 0;
const TOP_Y = BAR_Y + BAR_H + GAP;
const BOTTOM_Y = TOP_Y + KEY_H + PAIR_GAP;
const THUMB_Y = BOTTOM_Y + KEY_H + GAP + THUMB_DROP;
const TALL_H = KEY_H * 2 + PAIR_GAP;
export const WIDTH = 9 * STEP + 1;
export const HEIGHT = THUMB_Y + THUMB_H;

// [id, column, y, height, group, top corners, bottom corners]: `soft` / `square` / `round`
const finger = (id, col, row, group) => [id, col, row === 'top' ? TOP_Y : BOTTOM_Y, KEY_H, group,
  row === 'top' ? 'soft' : 'square', row === 'top' ? 'square' : 'round'];
export const KEYS = [
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

/** Every key's rectangle and corner radii, in key widths: id -> { x, y, w, h, top, bottom, group }. */
export const radius = (kind) => (kind === 'soft' ? SOFT : kind === 'round' ? 0.5 : 0);
export const KEY_BOXES = new Map(KEYS.map(([id, col, y, h, group, top, bottom]) =>
  [id, { x: col * STEP, y, w: 1, h, top: radius(top), bottom: radius(bottom), group }]));

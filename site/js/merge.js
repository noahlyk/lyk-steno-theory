// Merged keys: while every key of a steno letter chord is lit, the board draws them as one bigger
// key (T P H as one N). Pure functions, no DOM (tested by test.mjs). A port of keymux's overlay
// (src/overlay/merge.rs): the group table, the cover selection and the outline geometry are the
// same; drawing is in board.js.
//
// A shape is the union of the members' rectangles with the gaps between neighbours filled. Each
// outer corner keeps the radius of the member that owns it; corners where members meet are flat; a
// corner the union folds inwards (an L) gets a small fillet. A thumb pair loses the round ends in
// the middle and keeps a quarter circle at each outer bottom corner. Keys with keys between them
// (S R = V, K R = C) are two pieces joined by a thin connector.

import { KEY_BOXES, KEYS, SOFT } from './boardgeom.js';

/** Key names as the board has them; the order breaks ties between groups. Right labels have no hyphen. */
export const DEFAULTS = [
  [['T-', 'P-', 'H-'], 'N'],
  [['P-', 'H-'], 'M'],
  [['K-', 'W-', 'R-'], 'Y'],
  [['T-', 'K-'], 'D'],
  [['P-', 'W-'], 'B'],
  [['H-', 'R-'], 'L'],
  [['T-', 'P-'], 'F'],
  [['T-', 'K-', 'P-', 'W-'], 'G'],
  [['S-', 'K-', 'W-', 'R-'], 'J'],
  [['S-', 'T-', 'K-', 'P-', 'W-'], 'Z'],
  [['-P', '-B'], 'N'],
  [['-P', '-L'], 'M'],
  [['-B', '-G'], 'K'],
  [['-F', '-P'], 'CH'],
  [['-R', '-B'], 'SH'],
  [['-P', '-B', '-L', '-G'], 'J'],
  [['-G', '-S'], 'SHUN'],
  [['K-', 'W-'], 'Q'],
  [['-B', '-G', '-S'], 'X'],
  [['-F', '-R', '-P', '-B'], 'NCH'],
  [['A', 'O'], 'AO'],
  [['E', 'U'], 'EU'],
  [['S-', 'R-'], 'V'],
  [['K-', 'R-'], 'C'],
];

/** How far apart (key widths) two keys may be and still be neighbours (columns are 0.16 apart). */
export const NEIGHBOUR_GAP = 0.3;
/** How far apart two pieces of a bridged group may be (S- to R- is 2.5). */
export const BRIDGE_REACH = 4.0;
const SELECT_BUDGET = 20000;
const EPS = 0.01;
const K = 0.5522848;

const bottom = (r) => r.y + r.h;

/** Whether two rects are neighbours: they overlap along one axis and the gap along the other is within `tol`. */
export function adjacent(a, b, tol) {
  const dx = Math.max(b.x - (a.x + a.w), a.x - (b.x + b.w));
  const dy = Math.max(b.y - bottom(a), a.y - bottom(b));
  return (dx <= tol && dy < -EPS) || (dy <= tol && dx < -EPS);
}

/** The rectangle filling the gap between two neighbours, over the span they share. */
function gapFill(a, b) {
  const dx = Math.max(b.x - (a.x + a.w), a.x - (b.x + b.w));
  const dy = Math.max(b.y - bottom(a), a.y - bottom(b));
  if (dx > EPS && dy < 0) {
    const y = Math.max(a.y, b.y);
    return { x: b.x > a.x ? a.x + a.w : b.x + b.w, y, w: dx, h: Math.min(bottom(a), bottom(b)) - y };
  }
  if (dy > EPS && dx < 0) {
    const x = Math.max(a.x, b.x);
    return { x, y: b.y > a.y ? bottom(a) : bottom(b), w: Math.min(a.x + a.w, b.x + b.w) - x, h: dy };
  }
  return null;
}

/**
 * For a group whose keys fall into several pieces (each connected through neighbours): the thin
 * connectors joining them left to right, a third of a key wide, centred on the height two pieces
 * share. `null` if they cannot be joined; empty if the group is one piece.
 */
export function links(rects, tol) {
  const n = rects.length;
  const piece = rects.map((_, i) => i);
  for (let changed = true; changed;) {
    changed = false;
    for (let i = 0; i < n; i++) {
      for (let j = 0; j < n; j++) {
        if (piece[i] < piece[j] && adjacent(rects[i], rects[j], tol)) { piece[j] = piece[i]; changed = true; }
      }
    }
  }
  const ids = [...new Set(piece)].sort((a, b) => a - b);
  const left = (id) => Math.min(...rects.filter((_, i) => piece[i] === id).map((r) => r.x));
  ids.sort((a, b) => left(a) - left(b));
  const out = [];
  for (let p = 0; p + 1 < ids.length; p++) {
    let best = null;
    for (let a = 0; a < n; a++) {
      if (piece[a] !== ids[p]) continue;
      for (let b = 0; b < n; b++) {
        if (piece[b] !== ids[p + 1]) continue;
        const ra = rects[a];
        const rb = rects[b];
        const thick = Math.min(ra.w, rb.w) / 3;
        const dx = rb.x - (ra.x + ra.w);
        const lo = Math.max(ra.y, rb.y);
        const hi = Math.min(bottom(ra), bottom(rb));
        if (dx <= EPS || dx > BRIDGE_REACH * ra.w || hi - lo < thick) continue;
        const rect = { x: ra.x + ra.w, y: (lo + hi) / 2 - thick / 2, w: dx, h: thick };
        const overlap = hi - lo;
        if (!best || dx < best.dx - EPS || (dx < best.dx + EPS && overlap > best.overlap)) best = { dx, overlap, rect };
      }
    }
    if (!best) return null;
    const { rect } = best;
    const hits = rects.some((r) => rect.x < r.x + r.w - EPS && rect.x + rect.w > r.x + EPS && rect.y < bottom(r) - EPS && bottom(rect) > r.y + EPS);
    if (hits) return null;
    out.push(rect);
  }
  return out;
}

function coords(values) {
  const v = [...values].sort((a, b) => a - b);
  return v.filter((x, i) => i === 0 || Math.abs(x - v[i - 1]) >= EPS);
}

/** A member key: its rect and the radii of its top and bottom corners, clamped as a lone key's are. */
export function member(rect, top, bottomR) {
  const t = Math.min(top, rect.w / 2, rect.h / 2);
  const b = Math.min(bottomR, rect.w / 2, rect.h - t);
  return { rect, top: t, bottom: b };
}

/**
 * The outline of the union of the members and the gaps between neighbours, clockwise on screen from
 * the top-left-most corner: [{ x, y, radius, convex }]. Convex corners take the radius of the member
 * that owns them, concave ones `concaveRadius`; straight-through points are dropped.
 */
export function outline(members, tol, concaveRadius) {
  const plain = members.map((m) => m.rect);
  const rects = [...plain, ...(links(plain, tol) ?? [])];
  members.forEach((a, i) => {
    for (const b of members.slice(i + 1)) {
      if (adjacent(a.rect, b.rect, tol)) { const f = gapFill(a.rect, b.rect); if (f) rects.push(f); }
    }
  });
  const xs = coords(rects.flatMap((r) => [r.x, r.x + r.w]));
  const ys = coords(rects.flatMap((r) => [r.y, bottom(r)]));
  if (xs.length < 2 || ys.length < 2) return [];
  const nx = xs.length - 1;
  const ny = ys.length - 1;
  const filled = Array.from({ length: nx }, (_, i) => Array.from({ length: ny }, (_, j) => {
    const cx = (xs[i] + xs[i + 1]) / 2;
    const cy = (ys[j] + ys[j + 1]) / 2;
    return rects.some((r) => cx > r.x && cx < r.x + r.w && cy > r.y && cy < bottom(r));
  }));
  // whatever the outside cannot reach is inside: the hole in the middle of a 2x2 block
  const outside = Array.from({ length: nx }, () => new Array(ny).fill(false));
  const queue = [];
  for (let i = 0; i < nx; i++) {
    for (let j = 0; j < ny; j++) {
      if ((i === 0 || j === 0 || i === nx - 1 || j === ny - 1) && !filled[i][j]) { outside[i][j] = true; queue.push([i, j]); }
    }
  }
  while (queue.length) {
    const [i, j] = queue.shift();
    for (const [a, b] of [[i - 1, j], [i + 1, j], [i, j - 1], [i, j + 1]]) {
      if (a >= 0 && b >= 0 && a < nx && b < ny && !filled[a][b] && !outside[a][b]) { outside[a][b] = true; queue.push([a, b]); }
    }
  }
  for (let i = 0; i < nx; i++) for (let j = 0; j < ny; j++) filled[i][j] = !outside[i][j];
  const isFilled = (i, j) => i >= 0 && j >= 0 && i < nx && j < ny && filled[i][j];
  const id = (i, j) => i * 10000 + j;
  const next = new Map();
  for (let i = 0; i < nx; i++) {
    for (let j = 0; j < ny; j++) {
      if (!filled[i][j]) continue;
      if (!isFilled(i, j - 1)) next.set(id(i, j), [i + 1, j]);
      if (!isFilled(i + 1, j)) next.set(id(i + 1, j), [i + 1, j + 1]);
      if (!isFilled(i, j + 1)) next.set(id(i + 1, j + 1), [i, j + 1]);
      if (!isFilled(i - 1, j)) next.set(id(i, j + 1), [i, j]);
    }
  }
  if (!next.size) return [];
  const startId = Math.min(...next.keys());
  const start = [Math.floor(startId / 10000), startId % 10000];
  const points = [start];
  let at = next.get(startId);
  while (at && id(...at) !== startId && points.length <= next.size) {
    points.push(at);
    at = next.get(id(...at));
  }
  const n = points.length;
  const corners = [];
  for (let k = 0; k < n; k++) {
    const prev = points[(k + n - 1) % n];
    const cur = points[k];
    const nxt = points[(k + 1) % n];
    const din = [cur[0] - prev[0], cur[1] - prev[1]];
    const dout = [nxt[0] - cur[0], nxt[1] - cur[1]];
    const cross = din[0] * dout[1] - din[1] * dout[0];
    if (cross === 0) continue;
    const x = xs[cur[0]];
    const y = ys[cur[1]];
    const convex = cross > 0;
    let radius = concaveRadius;
    if (convex) {
      radius = 0;
      for (const m of members) {
        const r = m.rect;
        // arriving rightwards and turning down is a top-right corner, and so on round
        let cx; let cy; let rad;
        if (din[0] > 0) { cx = r.x + r.w; cy = r.y; rad = m.top; }
        else if (din[1] > 0) { cx = r.x + r.w; cy = bottom(r); rad = m.bottom; }
        else if (din[0] < 0) { cx = r.x; cy = bottom(r); rad = m.bottom; }
        else { cx = r.x; cy = r.y; rad = m.top; }
        if (Math.abs(cx - x) < 0.05 && Math.abs(cy - y) < 0.05) radius = Math.max(radius, rad);
      }
    }
    corners.push({ x, y, radius, convex });
  }
  return corners;
}

/** The SVG path data of an outline; radii are clamped to half the shorter edge so corners never cross. */
export function pathData(corners) {
  const n = corners.length;
  if (n < 3) return '';
  const len = (a, b) => Math.abs(b.x - a.x) + Math.abs(b.y - a.y);
  const f = (v) => +v.toFixed(4);
  const toward = (from, to, d) => { const l = len(from, to); return [from.x + (to.x - from.x) / l * d, from.y + (to.y - from.y) / l * d]; };
  let d = '';
  for (let k = 0; k < n; k++) {
    const prev = corners[(k + n - 1) % n];
    const cur = corners[k];
    const nxt = corners[(k + 1) % n];
    const r = Math.min(cur.radius, len(prev, cur) / 2, len(cur, nxt) / 2);
    const a = toward(cur, prev, r);
    const b = toward(cur, nxt, r);
    d += `${k === 0 ? 'M' : 'L'}${f(a[0])} ${f(a[1])}`;
    if (r > 0) {
      d += `C${f(a[0] + (cur.x - a[0]) * K)} ${f(a[1] + (cur.y - a[1]) * K)} ${f(b[0] + (cur.x - b[0]) * K)} ${f(b[1] + (cur.y - b[1]) * K)} ${f(b[0])} ${f(b[1])}`;
    } else d += `L${f(cur.x)} ${f(cur.y)}`;
  }
  return `${d}Z`;
}

/** The box around an outline. */
export function bounds(corners) {
  const xs = corners.map((c) => c.x);
  const ys = corners.map((c) => c.y);
  const x = Math.min(...xs);
  const y = Math.min(...ys);
  return { x, y, w: Math.max(...xs) - x, h: Math.max(...ys) - y };
}

/** The centre of area of an outline (on the long arm of an L); the box's middle if that is outside. */
export function labelCenter(corners) {
  const b = bounds(corners);
  const middle = [b.x + b.w / 2, b.y + b.h / 2];
  const n = corners.length;
  let twice = 0; let cx = 0; let cy = 0;
  for (let i = 0; i < n; i++) {
    const p = corners[i];
    const q = corners[(i + 1) % n];
    const cross = p.x * q.y - q.x * p.y;
    twice += cross; cx += (p.x + q.x) * cross; cy += (p.y + q.y) * cross;
  }
  if (Math.abs(twice) < EPS) return middle;
  const at = [cx / (3 * twice), cy / (3 * twice)];
  let inside = false;
  for (let i = 0; i < n; i++) {
    const p = corners[i];
    const q = corners[(i + 1) % n];
    if ((p.y > at[1]) !== (q.y > at[1]) && at[0] < (q.x - p.x) * (at[1] - p.y) / (q.y - p.y) + p.x) inside = !inside;
  }
  return inside ? at : middle;
}

/** A bridged group's label goes on its biggest key (the first on a tie); otherwise `labelCenter`. */
export function labelSpot(members, corners, tol) {
  const rects = members.map((m) => m.rect);
  const l = links(rects, tol);
  if (!l || !l.length) return labelCenter(corners);
  const big = rects.reduce((best, r) => (r.w * r.h > best.w * best.h + EPS ? r : best), rects[0]);
  return [big.x + big.w / 2, big.y + big.h / 2];
}

const popcount = (v) => { let c = 0; for (let x = v; x; x &= x - 1) c++; return c; };

/** Lexicographic compare of two ascending index lists. */
function cmpList(a, b) {
  for (let i = 0; i < Math.min(a.length, b.length); i++) if (a[i] !== b[i]) return a[i] - b[i];
  return a.length - b.length;
}

/**
 * Which groups to draw merged for a set of lit keys: groups that are fully lit and share no key,
 * chosen by (1) the most lit keys covered, (2) the biggest single group, (3) the fewest groups,
 * (4) the earliest groups. So S T K P W H R is Z + L (7 keys, a 5-key group), not J + N. Lit keys in
 * no chosen group light alone. Deterministic; the search is capped. `groups` are { bits, size };
 * returns indices, ascending.
 */
export function select(groups, lit) {
  const cands = [];
  groups.forEach((g, i) => { if (g.bits && (g.bits & lit) === g.bits) cands.push(i); });
  let steps = 0;
  let best = null;
  const chosen = [];
  const better = (a, b) => {
    if (a.covered !== b.covered) return a.covered > b.covered;
    if (a.big !== b.big) return a.big > b.big;
    if (a.count !== b.count) return a.count < b.count;
    return cmpList(a.list, b.list) < 0;
  };
  const search = (from, taken) => {
    if (chosen.length) {
      const key = { covered: popcount(taken), big: Math.max(...chosen.map((i) => groups[i].size)), count: chosen.length, list: [...chosen] };
      if (!best || better(key, best)) best = key;
    }
    for (let k = from; k < cands.length; k++) {
      if (++steps > SELECT_BUDGET) return;
      const i = cands[k];
      if ((groups[i].bits & taken) === 0) {
        chosen.push(i);
        search(k + 1, taken | groups[i].bits);
        chosen.pop();
      }
    }
  };
  search(0, 0);
  return best ? best.list : [];
}

/** The built-in groups on this board: bit mask over KEYS order, outline, path, label spot. Built once. */
export function buildShapes() {
  const slot = new Map(KEYS.map((k, i) => [k[0], i]));
  const tol = NEIGHBOUR_GAP;
  const shapes = [];
  for (const [names, label] of DEFAULTS) {
    const members = names.map((n) => { const b = KEY_BOXES.get(n); return member({ x: b.x, y: b.y, w: b.w, h: b.h }, b.top, b.bottom); });
    const rects = members.map((m) => m.rect);
    const l = links(rects, tol);
    if (l === null) continue; // cannot be joined on this layout: dropped, as keymux does
    const corners = outline(members, tol, SOFT);
    if (corners.length < 3) continue;
    shapes.push({
      keys: names,
      label,
      bits: names.reduce((b, n) => b | (1 << slot.get(n)), 0),
      size: names.length,
      bridge: l.length > 0,
      corners,
      bounds: bounds(corners),
      d: pathData(corners),
      center: labelSpot(members, corners, tol),
      group: KEY_BOXES.get(names[0]).group,
    });
  }
  return shapes;
}

let cached = null;
export const shapes = () => (cached ??= buildShapes());

/** Slot (bit position) of a key id, in KEYS order. */
export const slotOf = (id) => KEYS.findIndex((k) => k[0] === id);

/**
 * Which shapes to draw for keys that carry a state. `looks` maps key id -> a look string (keys of
 * one look can merge, keys of different looks cannot); `null`/absent keys are unlit. Returns
 * [{ shape, look }]. The cover is chosen over the groups whose members all share one look.
 */
export function cover(looks) {
  let lit = 0;
  const lookOf = [];
  for (const [id, look] of looks) {
    if (!look) continue;
    const s = slotOf(id);
    if (s < 0) continue;
    lit |= 1 << s;
    lookOf[s] = look;
  }
  // a hinted key and a hinted key already held (`down-ok`) are one family, as in keymux where a
  // partly held group stays fused: the shape is `down-ok` only when every member is held
  const family = (l) => (l === 'down-ok' ? 'target' : l);
  const all = shapes();
  const uniform = all.map((sh) => {
    const ls = sh.keys.map((k) => lookOf[slotOf(k)]);
    return ls.every((l) => l && family(l) === family(ls[0]));
  });
  const usable = all.map((sh, i) => (uniform[i] ? sh : { bits: 0, size: sh.size }));
  return select(usable, lit).map((i) => {
    const ls = all[i].keys.map((k) => lookOf[slotOf(k)]);
    return { shape: all[i], look: ls.every((l) => l === 'down-ok') ? 'down-ok' : family(ls[0]) };
  });
}

/** Whether a label of `chars` characters fits the shape at font size `font` (key widths); the board drops it if not. */
export function labelFits(shape, font = 0.56) {
  const width = shape.label.length * font * 0.68;
  const b = shape.bridge ? { w: 1, h: 1 } : shape.bounds; // a bridged label sits on one key
  return width <= b.w * 0.92 && font <= b.h * 0.92;
}

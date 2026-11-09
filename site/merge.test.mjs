// Unit tests for the merged-keys module (js/merge.js): pure functions, no DOM. Run by test.mjs
// (which calls mergeTests) or on its own: `node site/merge.test.mjs`.

import { parseStroke } from './js/steno.js';
import { KEY_BOXES } from './js/boardgeom.js';
import { DEFAULTS, NEIGHBOUR_GAP, adjacent, cover, labelCenter, labelFits, links, member, outline, select, shapes } from './js/merge.js';

export function mergeTests() {
  const errors = [];
  const expect = (what, got, want) => {
    const g = JSON.stringify(got);
    const w = JSON.stringify(want);
    if (g !== w) errors.push(`merge: ${what}: got ${g}, want ${w}`);
  };
  const near = (what, got, want, eps = 0.02) => { if (Math.abs(got - want) > eps) errors.push(`merge: ${what}: got ${got}, want ${want}`); };
  const looksOf = (stroke) => new Map([...parseStroke(stroke)].map((k) => [k, 'target']));
  // the labels the cover of a stroke draws, in selection order
  const labels = (stroke) => cover(looksOf(stroke)).map((c) => c.shape.label);
  const lone = (stroke) => {
    const covered = new Set(cover(looksOf(stroke)).flatMap((c) => c.shape.keys));
    return [...parseStroke(stroke)].filter((k) => !covered.has(k));
  };

  // the table: 24 groups, the labels of keymux's README
  expect('24 groups', DEFAULTS.length, 24);
  expect('every group has a shape', shapes().length, 24);
  const want = { 'T-P-H-': 'N', 'P-H-': 'M', 'K-W-R-': 'Y', 'T-K-': 'D', 'P-W-': 'B', 'H-R-': 'L', 'T-P-': 'F', 'T-K-P-W-': 'G',
    'S-K-W-R-': 'J', 'S-T-K-P-W-': 'Z', '-P-B': 'N', '-P-L': 'M', '-B-G': 'K', '-F-P': 'CH', '-R-B': 'SH', '-P-B-L-G': 'J',
    '-G-S': 'SHUN', 'K-W-': 'Q', '-B-G-S': 'X', '-F-R-P-B': 'NCH', 'AO': 'AO', 'EU': 'EU', 'S-R-': 'V', 'K-R-': 'C' };
  expect('labels', DEFAULTS.map(([k, l]) => `${k.join('')}=${l}`).sort(), Object.entries(want).map(([k, l]) => `${k}=${l}`).sort());
  expect('bridged groups are exactly V and C', shapes().filter((s) => s.bridge).map((s) => s.label), ['V', 'C']);

  // cover selection
  expect('STKPWHR = Z + L', labels('STKPWHR').sort(), ['L', 'Z']);
  expect('TKPH = D + M', labels('TKPH').sort(), ['D', 'M']);
  expect('TPH = N', labels('TPH'), ['N']);
  expect('TPHAEU: N and EU, A alone is not a group', labels('TPHAEU'), ['N', 'EU']);
  expect('a partial press merges nothing (T and H are not a group)', labels('TH'), []);
  expect('one key merges nothing', labels('T'), []);
  expect('-FRPB = NCH', labels('-FRPB'), ['NCH']);
  expect('AO', labels('AO'), ['AO']);
  expect('EU', labels('EU'), ['EU']);
  expect('A and U across the gap merge nothing', labels('AU'), []);
  expect('SR = V', labels('SR'), ['V']);
  expect('KR = C', labels('KR'), ['C']);
  expect('keys outside the groups light alone', lone('STKPWHR*'), ['*']);
  expect('S beside T K lights alone', lone('STK'), ['S-']);
  expect('-BGS = X', labels('-BGS'), ['X']);
  expect('-PBLG = J, not N + M', labels('-PBLG'), ['J']);
  expect('keys of different looks do not merge', cover(new Map([['T-', 'target'], ['P-', 'down'], ['H-', 'down']])).map((c) => c.shape.label), ['M']);
  const hinted = cover(new Map([['T-', 'target'], ['P-', 'down-ok'], ['H-', 'down-ok']]));
  expect('a partly held hint stays one shape, not yet held', hinted.map((c) => [c.shape.label, c.look]), [['N', 'target']]);
  expect('a fully held hint is held', cover(new Map([['P-', 'down-ok'], ['H-', 'down-ok']])).map((c) => c.look), ['down-ok']);
  expect('a key with no look is not lit', cover(new Map([['T-', null], ['P-', 'target'], ['H-', 'target']])).map((c) => c.shape.label), ['M']);
  for (const stroke of ['STKPWHR', 'STKPWHRAO*EU-FRPBLGTSDZ', 'TKPWHR-PBLGSDZ']) {
    let seen = 0;
    for (const { shape } of cover(looksOf(stroke))) {
      if (seen & shape.bits) errors.push(`merge: overlapping groups in ${stroke}`);
      seen |= shape.bits;
    }
    expect(`deterministic ${stroke}`, labels(stroke), labels(stroke));
  }
  expect('select: nothing lit', select(shapes(), 0), []);
  const dense = Array.from({ length: 30 }, (_, i) => ({ bits: (1 << (i % 20)) | (1 << ((i + 1) % 20)), size: 2 }));
  expect('select is capped but still answers', select(dense, 0xfffff).length > 0, true);

  // geometry
  const box = (id) => { const b = KEY_BOXES.get(id); return { x: b.x, y: b.y, w: b.w, h: b.h }; };
  const sh = (label, keys) => shapes().find((s) => s.label === label && s.keys.join() === keys);
  const area = (c) => c.reduce((a, p, i) => { const q = c[(i + 1) % c.length]; return a + (p.x * q.y - q.x * p.y) / 2; }, 0);
  const N = sh('N', 'T-,P-,H-');
  expect('N is a plain rectangle', N.corners.length, 4);
  near('N width', N.bounds.w, 3 * 1.16 - 0.16);
  near('N area is the whole rectangle, gaps filled', area(N.corners), N.bounds.w * N.bounds.h);
  expect('N top corners soft, bottom square', N.corners.map((c) => c.radius), [0.1, 0.1, 0, 0]);
  const Z = sh('Z', 'S-,T-,K-,P-,W-');
  expect('Z is an L with a filleted inner corner', Z.corners.filter((c) => !c.convex).length, 1);
  const L2 = outline([member({ x: 0, y: 0, w: 10, h: 42 }, 0, 0), member({ x: 12, y: 0, w: 10, h: 20 }, 0, 0)], 5, 2);
  expect('L has six corners', L2.length, 6);
  const [cx, cy] = labelCenter(L2);
  expect('L label is on the long arm', cx < 10 && cy < 42, true);
  const rect = outline([member({ x: 0, y: 0, w: 10, h: 20 }, 0, 0), member({ x: 12, y: 0, w: 10, h: 20 }, 0, 0)], 5, 1);
  expect('a rectangle label is its middle', labelCenter(rect), [11, 10]);

  // bridged V and C: two pieces and a thin connector a third of a key wide, through the unlit keys
  const rects = (keys) => keys.map(box);
  for (const keys of [['S-', 'R-'], ['K-', 'R-']]) {
    const l = links(rects(keys), NEIGHBOUR_GAP);
    expect(`${keys} one connector`, l.length, 1);
    near(`${keys} connector is a third of a key`, l[0].h, 1 / 3, 0.001);
    const row = box(keys[0]);
    near(`${keys} connector is centred on the row`, l[0].y + l[0].h / 2, row.y + row.h / 2);
  }
  const V = sh('V', 'S-,R-');
  expect('V outline has the connector in it', V.corners.length >= 8, true);
  expect('V label is on the first piece (S-)', V.center, [box('S-').x + 0.5, box('S-').y + box('S-').h / 2]);
  expect('a group in one piece has no connector', links(rects(['T-', 'P-', 'H-']), NEIGHBOUR_GAP), []);
  expect('S- and T- are not neighbours (diagonal)', adjacent(box('S-'), box('T-'), NEIGHBOUR_GAP), false);
  expect('S- and K- are', adjacent(box('S-'), box('K-'), NEIGHBOUR_GAP), true);
  expect('a thumb is not next to a finger', adjacent(box('A'), box('K-'), NEIGHBOUR_GAP), false);

  // thumb paddles: the round ends in the middle go, quarter circles stay at the outer bottom corners
  const AO = sh('AO', 'A,O');
  expect('AO has four corners', AO.corners.length, 4);
  expect('AO: soft top corners, half-key radius at the outer bottom corners, flat between', AO.corners.map((c) => c.radius), [0.1, 0.1, 0.5, 0.5]);
  near('AO is two keys and the gap wide', AO.bounds.w, 2.16);
  expect('AO path has curves (the bottom corners)', /C/.test(AO.d), true);

  // labels fit or are dropped
  expect('SHUN fits its shape', labelFits(sh('SHUN', '-G,-S')), true);
  expect('a wide label does not fit a narrow key', labelFits({ label: 'SHUN', bounds: { w: 1, h: 1.7 }, bridge: false }), false);
  expect('a bridged label measures one key', labelFits({ label: 'V', bounds: { w: 4, h: 1.7 }, bridge: true }), true);
  return errors;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const errors = mergeTests();
  for (const e of errors) console.error('FAIL', e);
  console.log(`merge: ${errors.length} failures`);
  process.exit(errors.length ? 1 : 0);
}

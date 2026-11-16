// Unit tests for the physical keyboard mapping (js/steno.js QWERTY) and the board layout it must
// match (js/boardgeom.js): pure data, no DOM. Run by test.mjs or on its own.

import { QWERTY, KEYS as STENO_KEYS } from './js/steno.js';
import { KEY_BOXES, KEYS as BOARD_KEYS } from './js/boardgeom.js';

export function keymapTests() {
  const errors = [];
  const expect = (what, got, want) => {
    const g = JSON.stringify(got);
    const w = JSON.stringify(want);
    if (g !== w) errors.push(`keymap: ${what}: got ${g}, want ${w}`);
  };
  const pressing = (id) => Object.entries(QWERTY).filter(([, k]) => k === id).map(([c]) => c).sort();
  expect('the join key + is T and G (the left index inner column)', pressing('+'), ['KeyG', 'KeyT']);
  expect('the star is Y and H only (the right index)', pressing('*'), ['KeyH', 'KeyY']);
  expect('S- is A and Q (a tall key)', pressing('S-'), ['KeyA', 'KeyQ']);
  expect('every steno key has a keyboard key', STENO_KEYS.filter((k) => pressing(k).length === 0), []);
  expect('every key of the map is a board key', [...new Set(Object.values(QWERTY))].filter((k) => !KEY_BOXES.has(k)), []);
  expect('the board draws exactly the steno keys', BOARD_KEYS.map((k) => k[0]).sort(), [...STENO_KEYS].sort());
  // + sits between the H-/R- column and the star, and no other key shares its column
  const b = (id) => KEY_BOXES.get(id);
  expect('+ is right of H- and left of *', b('H-').x < b('+').x && b('+').x < b('*').x, true);
  expect('+ shares a column with no other key', [...KEY_BOXES].filter(([id, r]) => id !== '+' && r.x === b('+').x).map(([id]) => id), []);
  return errors;
}

if (process.argv[1]?.endsWith('keymap.test.mjs')) {
  const errors = keymapTests();
  for (const e of errors) console.error('FAIL', e);
  console.log(`keymap: ${errors.length} failures`);
  process.exit(errors.length ? 1 : 0);
}

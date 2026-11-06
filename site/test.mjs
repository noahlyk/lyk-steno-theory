// Tests the stroke-to-text engine against the real data: `node site/test.mjs`.

import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createEngine } from './js/engine.js';
import { parseOutline } from './js/steno.js';

const root = dirname(fileURLToPath(import.meta.url));
const read = (p) => JSON.parse(readFileSync(join(root, p), 'utf8'));
const engine = createEngine({ dict: read('data/engine.json'), joins: read('data/joins.json'), chart: read('data/chart.json') });
const words = read('data/words.json').words;

const errors = [];
const strokes = (o) => parseOutline(o);
const say = (o) => engine.translate(strokes(o)).text;
const expect = (what, got, want) => { if (got !== want) errors.push(`${what}: got ${JSON.stringify(got)}, want ${JSON.stringify(want)}`); };

// the join key decides word or ending
expect('+-D alone', say('+-D'), 'ed');
expect('ED/+-D', say('ED/+-D'), 'ended');
expect('-D', say('-D'), 'do');
expect('ED -D are two words', engine.translate([...strokes('ED'), ...strokes('-D')]).text, 'end do');
// no entry for the run: the stem and the ending still compose, with the silent e dropped
expect('have + ing', say('SR/+-G'), 'having');
expect('and + er', say('SKP/+-R'), 'ander');
// a stroke nothing reads
expect('empty', engine.translate([]).text, '');

// every outline the lookup shows for a word reads back as that word (a few outlines are shared
// by two words, and the dictionary chose one of them)
let total = 0;
let wrong = 0;
for (const [word, outlines] of Object.entries(words)) {
  for (const o of outlines) {
    if (!strokes(o)) continue; // the number bar (`#KOPL`) is not on the board
    total += 1;
    if (!engine.accepts(word, strokes(o))) {
      wrong += 1;
      if (wrong <= 10) errors.push(`${word}: ${o} reads as ${JSON.stringify(say(o))}`);
    }
  }
}
if (wrong / total > 0.02) errors.push(`${wrong} of ${total} outlines do not read back as their word`);

// any way of writing it passes, and an unrelated stroke does not
expect('accepts a backup', engine.accepts('walking', strokes(engine.outlinesOf('walking').at(-1))), true);
expect('refuses a wrong word', engine.accepts('walking', strokes('KAT')), false);
expect('prefix: the stem of a word', engine.prefixOf('ended', strokes('ED')), true);
expect('prefix: nothing typed', engine.prefixOf('ended', []), true);
expect('prefix: wrong', engine.prefixOf('ended', strokes('KAT')), false);

for (const e of errors) console.error('FAIL', e);
console.log(`engine: ${total} outlines read back (${wrong} differ), ${errors.length} failures`);
process.exit(errors.length ? 1 : 0);

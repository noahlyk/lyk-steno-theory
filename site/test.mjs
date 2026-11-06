// Tests the stroke-to-text engine against the real data: `node site/test.mjs`.

import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createEngine } from './js/engine.js';
import { parseOutline } from './js/steno.js';
import { store } from './js/store.js';

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

// what an outline is defined as, in Plover's syntax where there is one
const def = (o) => engine.define(strokes(o));
expect('+-D is {^ed}', def('+-D').text, '{^ed}');
expect('+-G is {^ing}', def('+-G').text, '{^ing}');
expect('-FP is {.}', def('-FP').text, '{.}');
expect('ED/+-D is the word', def('ED/+-D').text, 'ended');
expect('ED/+-D is an entry', def('ED/+-D').how, 'entry');
expect('SKP/+-R is read back, not an entry', def('SKP/+-R').how, 'guess');

// a run with the keys shown counts as a run but never sets the best
store.recordRun('t-peek', { accuracy: 80, wpm: 30, pass: 90 });
const peeked = store.recordRun('t-peek', { accuracy: 100, wpm: 60, pass: 90, assisted: true });
expect('assisted run: best untouched', JSON.stringify([peeked.best, peeked.wpm, peeked.runs]), "[80,30,2]");
expect('assisted run: still passes', store.recordRun('t-peek', { accuracy: 95, wpm: 20, pass: 90, assisted: true }).passed, true);

for (const e of errors) console.error('FAIL', e);
console.log(`engine: ${total} outlines read back (${wrong} differ), ${errors.length} failures`);
process.exit(errors.length ? 1 : 0);

// Tests the stroke-to-text engine against the real data: `node site/test.mjs`.

import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createEngine } from './js/engine.js';
import { parseOutline } from './js/steno.js';
import { classify, classifyList } from './js/hovermatch.js';
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
// the lone star is the smart undo, a command that types nothing
expect('* is a command', engine.translate(strokes('*')).text, '⟨undo⟩');
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
expect('* defined as undo_or', def('*').text, '{:undo_or:Control_L(BackSpace)}');
expect('+-D is {^ed}', def('+-D').text, '{^ed}');
expect('+-G is {^ing}', def('+-G').text, '{^ing}');
expect('-FP is {.}', def('-FP').text, '{.}');
expect('ED/+-D is the word', def('ED/+-D').text, 'ended');
expect('ED/+-D is an entry', def('ED/+-D').how, 'entry');
expect('SKP/+-L is read back, not an entry', def('SKP/+-L').how, 'guess');

// what hover.js may put a board on (the same dictionary shape data.js builds)
{
  const dict = { words: new Map(), reverse: new Map() };
  for (const [w, outs] of Object.entries(words)) {
    const typable = outs.filter((o) => !o.includes('#'));
    if (!typable.length) continue;
    dict.words.set(w, typable);
    for (const o of typable) dict.reverse.set(o, [...(dict.reverse.get(o) ?? []), w]);
  }
  const hit = (what, text, want) => expect(`hover ${what}`, classify(text, dict), want);
  hit('word ends', 'ends', words.ends[0]);
  hit('word girls (best outline only)', 'girls', words.girls[0]);
  hit('word with spaces round it', ' ends ', words.ends[0]);
  hit('outline KAT (a real entry)', 'KAT', 'KAT');
  hit('outline TKPWAEUPL', 'TKPWAEUPL', 'TKPWAEUPL');
  hit('outline ED/+-D', 'ED/+-D', 'ED/+-D');
  hit('suffix stroke -D', '-D', '-D');
  hit('left-hand pattern S-', 'S-', 'S-');
  hit('the star', '*', '*');
  hit('English in capitals AND', 'AND', null);
  hit('English in capitals NOTE', 'NOTE', null);
  hit('a one-letter key name A', 'A', null);
  hit('bare key group TKPW (not an entry)', 'TKPW', null);
  hit('empty stroke between slashes', 'A//B', null);
  hit('lone join key', '+', null);
  hit('lone hyphen', '-', null);
  hit('hyphenated English', 'E-MAIL', null);
  hit('a word the dictionary lacks', 'zzzzqx', null);
  hit('a sentence', 'ends the line', null);
  hit('mixed case', 'Ends', null);
  hit('a number', '12', null);
  hit('a one-letter word', 'a', null);
  hit('code-ish text', 'foo()', null);
  hit('a path', 'rules/words.toml', null);
  expect('furniture word in a cell', classify('no', dict, true), null);
  expect('furniture word in code', classify('no', dict, false), words.no[0]);
  const list = classifyList('ends, girls', dict, true);
  expect('list pieces', list?.map((p) => `${p.outline ? 'W' : 's'}:${p.text}`).join('|'), 'W:ends|s:, |W:girls');
  expect('list with a non-match', classifyList('ends, zzzzqx', dict), null);
  expect('prose is not a list', classifyList('ends the line', dict), null);
  expect('plain separator-only', classifyList(', ', dict), null);
}

// the `!name` opt-out (js/lesson.js): only on lower-case words, where it means something, and never
// on an outline or a word the page teaches as a chip
for (const dir of ['reference', 'lessons']) {
  for (const f of readdirSync(join(root, dir)).filter((x) => x.endsWith('.json'))) {
    const text = readFileSync(join(root, dir, f), 'utf8');
    for (const m of text.matchAll(/`!([^`]+)`|\{\{!([^}]+)\}\}/g)) {
      const w = m[1] ?? m[2];
      if (!/^[a-z][a-z' -]*$/.test(w)) errors.push(`${dir}/${f}: opt-out !${w} is not a lower-case word (opt-outs only matter on words)`);
    }
  }
}

// a run with the keys shown counts as a run but never sets the best
store.recordRun('t-peek', { accuracy: 80, wpm: 30, pass: 90 });
const peeked = store.recordRun('t-peek', { accuracy: 100, wpm: 60, pass: 90, assisted: true });
expect('assisted run: best untouched', JSON.stringify([peeked.best, peeked.wpm, peeked.runs]), "[80,30,2]");
expect('assisted run: still passes', store.recordRun('t-peek', { accuracy: 95, wpm: 20, pass: 90, assisted: true }).passed, true);

for (const e of errors) console.error('FAIL', e);
console.log(`engine: ${total} outlines read back (${wrong} differ), ${errors.length} failures`);
process.exit(errors.length ? 1 : 0);

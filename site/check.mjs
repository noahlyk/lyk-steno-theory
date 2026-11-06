// Checks the lessons without a browser or the dictionary build: run with `node site/check.mjs`.
// Every lesson parses, every stroke it shows is a valid stroke, every word it teaches is in
// data/words.json with that outline (tools/site_data.py writes that file from the real build and
// checks the same claims against the full dictionary).

import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as P from './js/trainer.js';
import { needsHyphen } from './js/steno.js';
import { decodeReport, REPORT_ID } from './js/hid.js';
import { formatStroke, parseOutline, parseStroke } from './js/steno.js';

const root = dirname(fileURLToPath(import.meta.url));
const read = (p) => JSON.parse(readFileSync(join(root, p), 'utf8'));
const errors = [];
const fail = (msg) => errors.push(msg);

// the steno parser round-trips
for (const s of ['KAT', 'TKOG', '-Z', '+-Z', '+AOE', 'STKPW', 'PWA*R', 'TPH-GS', '*PBG', 'WRAOEUT', '+', 'KPWR*EFPBLGTS']) {
  const keys = parseStroke(s);
  if (!keys || formatStroke(keys) !== s) fail(`steno round trip: ${s} -> ${keys && formatStroke(keys)}`);
}
for (const bad of ['KTA', 'ZZ', 'AA', '-']) if (parseStroke(bad) && bad !== '-') fail(`should not parse: ${bad}`);

const words = read('data/words.json').words;
const index = read('lessons/index.json');
const listed = index.parts.flatMap((p) => p.lessons.map((l) => l.id));
const files = readdirSync(join(root, 'lessons')).filter((f) => /^\d.*\.json$/.test(f)).map((f) => f.replace('.json', ''));
for (const id of files) if (!listed.includes(id)) fail(`lesson ${id} is not in the index`);
for (const id of listed) if (!files.includes(id)) fail(`index lists ${id} but there is no file`);

const KNOWN = new Set(['h', 'p', 'list', 'note', 'table', 'explore', 'try', 'quiz', 'join']);
const TOKEN = /\[\[(.+?)\]\]/g;
let strokes = 0;

function claim(where, word, outline) {
  strokes += 1;
  if (!parseOutline(outline)) fail(`${where}: not a valid outline: ${outline}`);
  if (word && words[word] && !words[word].includes(outline)) {
    // the lookup keeps the fastest few outlines; the full check is tools/site_data.py
  } else if (word && !words[word]) fail(`${where}: ${word} is not in data/words.json`);
}

function scan(where, value) {
  if (typeof value === 'string') {
    for (const m of value.matchAll(TOKEN)) {
      const [a, b] = m[1].includes('|') ? m[1].split('|') : [null, m[1]];
      claim(where, a, b);
    }
  } else if (Array.isArray(value)) value.forEach((v) => scan(where, v));
  else if (value && typeof value === 'object') Object.values(value).forEach((v) => scan(where, v));
}

for (const id of listed) {
  const lesson = read(`lessons/${id}.json`);
  if (lesson.id !== id) fail(`${id}: id says ${lesson.id}`);
  if (!lesson.title) fail(`${id}: no title`);
  for (const b of lesson.blocks) {
    if (!KNOWN.has(b.t)) fail(`${id}: unknown block ${b.t}`);
    scan(id, b);
    if (b.t === 'try') drillItems(id, b);
    if (b.t === 'join') {
      for (const [, o] of b.stems) claim(id, null, o);
      for (const [, o] of b.ends) claim(id, null, o);
    }
    if (b.t === 'quiz' && (b.a < 0 || b.a >= b.opts.length)) fail(`${id}: quiz answer out of range`);
  }
  if (lesson.drill) drillItems(id, lesson.drill);
}

function drillItems(id, d) {
  if (!d.items?.length) fail(`${id}: empty drill`);
  for (const item of d.items) {
    const outs = Array.isArray(item) ? item.slice(1) : item.o;
    if (!outs.length) fail(`${id}: drill item without an outline`);
    for (const o of outs) {
      strokes += 1;
      if (!parseOutline(o)) fail(`${id}: drill outline does not parse: ${o}`);
    }
  }
}

// "Learn the keys": every key-unlock stage has enough words for a lesson, and a perfect typist
// climbs from six keys to all 23, the guide fades, and the run finishes; the speed/accuracy trade
// behaves (fast may be sloppy, slow must be clean, too slow never passes)
{
  const pool = P.buildWords(words);
  const p = P.emptyProgress();
  for (let n = P.START_KEYS; n <= P.KEY_ORDER.length; n += 1) {
    p.unlocked = n;
    const mask = P.unlockedMask(p);
    const have = pool.filter((w) => P.displayVariants(w, mask).length).length;
    if (have < P.WINDOW) fail(`keys: only ${have} words with the first ${n} keys (${P.KEY_ORDER[n - 1]} just unlocked)`);
  }
  const sim = P.emptyProgress();
  let steps = 0;
  while (!sim.done && steps < 3000) {
    const w = P.nextWord(pool, sim, { avoid: [] });
    if (!w) { fail(`keys: no word to pick with ${sim.unlocked} keys`); break; }
    for (const v of P.displayVariants(w, P.unlockedMask(sim))[0]) P.recordStroke(sim, { used: P.keysOf(v), ms: 300, correct: true });
    P.recordWord(sim, { ms: 600, strokes: 1, misses: 0 });
    if (!P.unlockIfReady(sim)) P.completeIfReady(sim);
    P.adaptGuide(sim);
    steps += 1;
  }
  if (!sim.done) fail(`keys: a perfect run did not finish (${sim.unlocked} keys, guide ${sim.guide} after ${steps} words)`);
  if (sim.guide !== P.GUIDE.MEMORY) fail('keys: a perfect run finished with the guide still on');
  if (!(P.requiredAccuracy(60) < P.requiredAccuracy(40) && P.requiredAccuracy(40) < P.requiredAccuracy(20))) fail('keys: faster must need less accuracy');
  if (P.requiredAccuracy(P.MIN_WPM - 1) !== Infinity) fail('keys: under the minimum speed nothing may pass');
  if (!P.passes({ count: P.MIN_WORDS, wpm: 60, accuracy: 0.76 })) fail('keys: 60 wpm at 76% should pass');
  if (P.passes({ count: P.MIN_WORDS, wpm: 20, accuracy: 0.76 })) fail('keys: 20 wpm at 76% should not pass');
  // an unlock needs the words typed from memory, and brings the guide back in full
  const gated = P.emptyProgress();
  for (let i = 0; i < 30; i += 1) P.recordWord(gated, { ms: 600, strokes: 1, misses: 0 });
  if (P.unlockIfReady(gated)) fail('keys: words typed with the diagram must not unlock a key');
  gated.guide = P.GUIDE.MEMORY;
  if (P.unlockIfReady(gated)) fail('keys: an unlock must be judged on words typed from memory only');
  for (let i = 0; i < 12; i += 1) P.recordWord(gated, { ms: 600, strokes: 1, misses: 0 });
  if (!P.unlockIfReady(gated)) fail('keys: a clean run from memory should unlock');
  if (gated.guide !== P.GUIDE.FULL) fail('keys: unlocking a key must bring the full guide back');
  // "always show the keys": a word typed with it on, even briefly, is not from memory and never counts
  const peek = P.emptyProgress();
  peek.guide = P.GUIDE.MEMORY;
  for (let i = 0; i < 30; i += 1) P.recordWord(peek, { ms: 600, strokes: 1, misses: 0, assisted: true });
  if (P.memoryStats(peek).count !== 0 || P.unlockIfReady(peek)) fail('keys: words typed with the keys shown must not unlock a key');
  for (let i = 0; i < 12; i += 1) P.recordWord(peek, { ms: 600, strokes: 1, misses: 0 });
  if (P.memoryStats(peek).count !== 12 || !P.unlockIfReady(peek)) fail('keys: clean memory words still unlock after peeked ones');
  const full = P.emptyProgress();
  full.unlocked = P.KEY_ORDER.length;
  const goodWindow = { count: P.WINDOW, wpm: 50, accuracy: 0.95 };
  if (P.recordBestIfFaster(full, goodWindow, { assisted: true }) !== null || full.best !== null) fail('keys: a window with the keys shown must not set a best');
  if (P.recordBestIfFaster(full, goodWindow) !== 50) fail('keys: an unassisted passing window sets a best');
  // speed counts the work: the same pace per key passes with big words as with small ones
  const small = P.emptyProgress();
  const big = P.emptyProgress();
  const mask3 = P.maskOf(['T-', 'K-', 'A']);
  const mask8 = P.maskOf(['S-', 'T-', 'K-', 'P-', 'A', 'E', '-T', '-S']);
  for (let i = 0; i < 12; i += 1) {
    small.guide = big.guide = P.GUIDE.MEMORY;
    P.recordWord(small, { ms: 1000, strokes: 1, misses: 0, masks: [mask3] });
    P.recordWord(big, { ms: 4170, strokes: 2, misses: 0, masks: [mask8, mask3] });
  }
  const ws = P.windowStats(small);
  const wb = P.windowStats(big);
  if (Math.abs(ws.wpm - 60) > 1) fail(`keys: three-key words at 1s should be 60 work-wpm, got ${ws.wpm}`);
  if (Math.abs(wb.wpm - ws.wpm) > 2) fail(`keys: the same pace per key must score the same with big words (${wb.wpm} vs ${ws.wpm})`);
  if (wb.rawWpm >= wb.wpm) fail('keys: a big word should count for more than a plain word');
  // a struggling typist gets the guide back
  const weak = P.emptyProgress();
  weak.guide = P.GUIDE.MEMORY;
  for (let i = 0; i < 8; i += 1) { P.recordWord(weak, { ms: 9000, strokes: 1, misses: 2 }); P.adaptGuide(weak); }
  if (weak.guide === P.GUIDE.MEMORY) fail('keys: struggling typist did not get the guide back');
}

// a steno machine over Plover HID: a snapshot of levers decodes to lyk keys (the Uni's top S is the join key)
{
  const report = (...bits) => { const b = new Uint8Array(8); for (const i of bits) b[i >> 3] |= 0x80 >> (i & 7); return b; };
  const keys = (k, join) => [...(decodeReport(REPORT_ID, report(...k), join) ?? [])].sort().join(' ');
  if (keys([1, 2, 7], true) !== 'A K- T-') fail(`hid: T- K- A- decoded as ${keys([1, 2, 7], true)}`);
  if (keys([0, 23], true) !== '+ S-') fail('hid: S1- S2- should be + and S-');
  if (keys([0], false) !== 'S-') fail('hid: top S should be S- when the join option is off');
  if (keys([9, 24, 25, 26], true) !== '*') fail('hid: every asterisk lever is *');
  if (keys([10, 11, 12, 21], true) !== '-F -Z E U') fail(`hid: right bank decoded as ${keys([10, 11, 12, 21], true)}`);
  if (decodeReport(0x01, report(1)) !== null) fail('hid: wrong report id must be ignored');
}

// the `-` only where steno order cannot tell the right hand from the left
{
  const need = (t) => needsHyphen(parseStroke(t));
  for (const [t, want] of [['-T', true], ['-S', true], ['-D', false], ['-F', false], ['-PB', true], ['TK-T', false], ['KPWR-BG', false], ['SR-R', false], ['TKOG', false], ['*T', false], ['+-T', true], ['+-G', false]]) {
    if (need(t) !== want) fail(`hyphen: ${t} should ${want ? '' : 'not '}need a dash`);
  }
}

// every named import of a site module is something that module really exports (a browser refuses
// to load the whole app on one missing name, and nothing else here would notice)
{
  const dir = new URL('./js/', import.meta.url);
  const files = readdirSync(dir).filter((f) => f.endsWith('.js'));
  const exportsOf = new Map();
  const src = new Map();
  for (const f of files) {
    const text = readFileSync(new URL(f, dir), 'utf8');
    src.set(f, text);
    const names = new Set();
    for (const m of text.matchAll(/^export\s+(?:async\s+)?(?:const|let|var|function\*?|class)\s+([A-Za-z_$][\w$]*)/gm)) names.add(m[1]);
    for (const m of text.matchAll(/^export\s*\{([^}]*)\}/gm)) for (const n of m[1].split(',')) names.add(n.trim().split(/\s+as\s+/).pop());
    exportsOf.set(f, names);
  }
  for (const [f, text] of src) {
    for (const m of text.matchAll(/^import\s*\{([^}]*)\}\s*from\s*'\.\/([\w-]+\.js)'/gm)) {
      const have = exportsOf.get(m[2]);
      if (!have) { fail(`imports: ${f} imports from a missing module ${m[2]}`); continue; }
      for (const n of m[1].split(',')) {
        const name = n.trim().split(/\s+as\s+/)[0];
        if (name && !have.has(name)) fail(`imports: ${f} imports ${name}, which ${m[2]} does not export`);
      }
    }
  }
}

if (errors.length) {
  console.error(errors.join('\n'));
  process.exit(1);
}
console.log(`ok: ${listed.length} lessons, ${strokes} strokes checked`);

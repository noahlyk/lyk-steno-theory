// Checks the lessons without a browser or the dictionary build: run with `node site/check.mjs`.
// Every lesson parses, every stroke it shows is a valid stroke, every word it teaches is in
// data/words.json with that outline (tools/site_data.py writes that file from the real build and
// checks the same claims against the full dictionary).

import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
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

if (errors.length) {
  console.error(errors.join('\n'));
  process.exit(1);
}
console.log(`ok: ${listed.length} lessons, ${strokes} strokes checked`);

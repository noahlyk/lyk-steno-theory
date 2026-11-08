// The app shell: a hash router and the pages (lesson map, lesson, practice, lookup, key chart, progress).

import { chart, dictionary, index, lesson } from './data.js';
import { createDrill, stopAllDrills } from './drill.js';
import { keysChapter } from './chapter0.js';
import { doneControl } from './done.js';
import { keysStatus, markKeysSkipped } from './keys.js';
import { el, shuffle } from './dom.js';
import { chip, inline, renderLesson } from './lesson.js';
import { initDock } from './dock.js';
import { initHid } from './hid.js';
import { parseOutline } from './steno.js';
import { initPopover } from './popover.js';
import { initHover } from './hover.js';
import { hitRows, refHome, refPageView, searchRef } from './reference.js';
import { store } from './store.js';

const app = document.getElementById('app');
let teardown = [];
let courseCache = null;

const course = async () => (courseCache ??= await index());
const flat = (c) => c.parts.flatMap((p) => p.lessons.map((l) => ({ ...l, part: p.title })));

function state(l) {
  const d = store.drill(l.id);
  if (d?.passed || store.isRead(l.id)) return 'done';
  return 'todo';
}

function setNav(name) {
  for (const a of document.querySelectorAll('[data-nav]')) {
    if (a.dataset.nav === name) a.setAttribute('aria-current', 'page');
    else a.removeAttribute('aria-current');
  }
}

async function route() {
  for (const t of teardown) t();
  teardown = [];
  stopAllDrills();
  const [, page, arg] = location.hash.replace(/^#/, '').split('/');
  app.replaceChildren();
  window.scrollTo(0, 0);
  try {
    if (page === 'keys') { setNav('keys'); teardown.push(await keysChapter(app, flat(await course())[0])); }
    else if (page === 'lesson') await lessonPage(arg);
    else if (page === 'ref') { setNav('ref'); await (arg ? refPageView(app, arg, location.hash.replace(/^#/, '').split('/')[3]) : refHome(app)); }
    else if (page === 'practice') await practicePage();
    else if (page === 'lookup') await lookupPage();
    else if (page === 'chart') await chartPage();
    else if (page === 'progress') await progressPage();
    else await homePage();
  } catch (err) {
    app.replaceChildren(el('h1', {}, 'Something went wrong'), el('p', {}, String(err.message ?? err)));
  }
  document.title = `${document.querySelector('h1')?.textContent ?? 'Learn lyk'} · lyk`;
}

async function homePage() {
  setNav('home');
  const c = await course();
  const all = flat(c);
  const keys = keysStatus();
  const keysDone = keys.done || keys.skipped;
  // exactly one thing is "up next": chapter 0 until it is done or skipped, then the first lesson left
  const nextId = keysDone ? all.find((l) => state(l) === 'todo')?.id : null;
  app.append(
    el('h1', {}, c.title),
    el('p', { class: 'lede' }, c.summary),
    el('div', { class: 'row', style: 'margin-top:.8rem' },
      keysDone
        ? el('a', { class: 'btn btn--primary', href: `#/lesson/${nextId ?? all[0].id}` }, nextId && nextId !== all[0].id ? 'Continue' : 'Start chapter 1')
        : el('a', { class: 'btn btn--primary', href: '#/keys' }, keys.unlocked > 6 ? 'Continue chapter 0: learn the keys' : 'Start with chapter 0: learn the keys'),
      keysDone ? null : el('a', { class: 'btn', href: `#/lesson/${all[0].id}` }, 'Skip to chapter 1'),
      el('a', { class: 'btn', href: '#/chart' }, 'Key chart'),
      el('a', { class: 'btn', href: '#/lookup' }, 'Look up a word')),
  );
  const map = el('div', { class: 'map' });
  map.append(el('h2', { class: 'part' }, 'Warm-up (optional)'),
    el('a', { class: `card card--${keys.done ? 'done' : keys.skipped ? 'skipped' : ''} ${keysDone ? '' : 'card--next'}`, href: '#/keys' },
      el('span', { class: 'card__num' }, 'Chapter 0'),
      el('span', { class: 'card__title' }, 'Learn the keys'),
      el('span', { class: 'card__sum' }, 'A word stream that unlocks one key at a time, until you know where every sound lives.'),
      el('span', { class: 'card__state' }, keys.done ? '✓ Done' : keys.skipped ? 'Skipped' : el('span', { class: 'card__cta' }, keys.unlocked > 6 ? `${keys.unlocked} of 23 keys · keep going →` : 'Up next · start here →'))));
  for (const part of c.parts) {
    map.append(el('h2', { class: 'part' }, part.title));
    for (const l of part.lessons) {
      const s = state(l);
      const d = store.drill(l.id);
      map.append(el('a', { class: `card ${s === 'done' ? 'card--done' : ''} ${l.id === nextId ? 'card--next' : ''}`, href: `#/lesson/${l.id}` },
        el('span', { class: 'card__num' }, `Chapter ${l.num}`),
        el('span', { class: 'card__title' }, l.title),
        el('span', { class: 'card__sum' }, l.summary),
        el('span', { class: 'card__state' },
          s === 'done' ? (d?.passed ? `✓ Done · best ${d.best}%` : '✓ Done')
            : l.id === nextId ? el('span', { class: 'card__cta' }, 'Up next · start here →') : 'Not started')));
    }
  }
  app.append(map);
  app.append(el('p', { class: 'boardbox__hint', style: 'margin-top:2rem' },
    'Do the lessons in order; each builds on the last. Nothing is locked, and your progress is saved in this browser only.'));
}

async function lessonPage(id) {
  setNav('home');
  const c = await course();
  const all = flat(c);
  const at = all.findIndex((l) => l.id === id);
  if (at < 0) return homePage();
  const meta = all[at];
  const data = await lesson(id);
  const { body } = renderLesson(data);
  const keys = keysStatus();
  if (!keys.done && !keys.skipped) {
    const gate = el('div', { class: 'note kgate' },
      el('p', {}, 'Optional: ', el('a', { href: '#/keys' }, 'chapter 0, Learn the keys'), ' trains where every sound sits, and makes these chapters easier.',
        ' ', el('button', { class: 'btn', type: 'button', onclick: () => { markKeysSkipped(); gate.remove(); } }, 'Dismiss')));
    app.append(gate);
  }
  app.append(
    el('div', { class: 'crumbs' }, el('a', { href: '#/' }, 'Lessons'), ` / ${meta.part}`),
    el('h1', {}, `${meta.num}. ${data.title}`),
    el('p', { class: 'lede' }, data.goal ?? meta.summary),
    body,
  );
  if (data.drill) {
    const box = el('div', {});
    app.append(el('h2', {}, data.drill.title ?? 'Chapter drill'), el('p', {}, data.drill.x ??
      `Pass mark: ${data.drill.pass ?? 90}% right on the first try.`), box);
    const drill = createDrill(box, data.drill.items, {
      id, pass: data.drill.pass ?? 90, repeat: data.drill.repeat ?? 1, limit: data.drill.limit ?? 0, notation: Boolean(data.drill.notation),
      onDone: ({ passed }) => { if (passed) { store.setRead(id); done.paint(); } },
    });
    teardown.push(() => drill.stop());
  }
  const prev = all[at - 1];
  const next = all[at + 1];
  const done = doneControl(id);
  app.append(el('div', { class: 'lessonfoot' },
    prev ? el('a', { class: 'btn', href: `#/lesson/${prev.id}` }, `← ${prev.title}`) : el('span'),
    done.foot,
    next ? el('a', { class: 'btn btn--primary', href: `#/lesson/${next.id}` }, `${next.title} →`) : el('a', { class: 'btn', href: '#/practice' }, 'Practice →')));
  if (done.top) app.prepend(done.top);
}

async function practicePage() {
  setNav('practice');
  const c = await course();
  const all = flat(c);
  const withDrills = [];
  for (const l of all) {
    const data = await lesson(l.id);
    if (data.drill && !['none', 'grammar'].includes(data.drill.check)) withDrills.push({ ...l, items: data.drill.items });
  }
  const missedKeys = Object.keys(store.missed);
  const upTo = el('select', { 'aria-label': 'words up to lesson' }, withDrills.map((l, i) => el('option', { value: i }, `Lessons 1 to ${l.num}`)));
  const done = withDrills.findLastIndex((l) => store.drill(l.id)?.passed);
  upTo.value = String(Math.max(done, 0));
  const count = el('select', { 'aria-label': 'how many' }, [25, 50, 100].map((n) => el('option', { value: n }, `${n} words`)));
  const box = el('div', { style: 'margin-top:1rem' });
  const start = (items, title) => {
    box.replaceChildren();
    const d = createDrill(box, items, { pass: 90, limit: Number(count.value), title });
    teardown.push(() => d.stop());
  };
  const startMixed = () => {
    const pool = withDrills.slice(0, Number(upTo.value) + 1).flatMap((l) => l.items);
    start(shuffle(pool), 'Mixed practice');
  };
  const startMissed = () => {
    const pool = withDrills.flatMap((l) => l.items).filter((it) => missedKeys.includes(it[0]));
    if (pool.length) start(pool, 'Your misses'); else box.replaceChildren(el('p', {}, 'No misses saved. Nice.'));
  };
  app.append(el('h1', {}, 'Practice'),
    el('p', { class: 'lede' }, 'Mixed words from the lessons you have done, or the ones you keep missing.'),
    el('div', { class: 'row', style: 'margin-top:.8rem' }, upTo, count,
      el('button', { class: 'btn btn--primary', type: 'button', onclick: startMixed }, 'Start mixed'),
      el('button', { class: 'btn', type: 'button', onclick: startMissed }, `Drill my misses (${missedKeys.length})`)),
    box);
}

async function lookupPage() {
  setNav('lookup');
  const input = el('input', { class: 'search', type: 'search', placeholder: 'a word (walking) or a stroke (KAT, TKOG/+-Z)', autofocus: true, 'aria-label': 'search' });
  const out = el('div', { class: 'hits' });
  const row = (label, list) => (list.length ? [el('div', { class: 'hit__label' }, label),
    el('div', { class: 'hit__list' }, list.flatMap((o) => chip(o, { styled: true })))] : []);
  const search = async () => {
    const q = input.value.trim();
    out.replaceChildren();
    if (!q) return;
    const { words, reverse } = await dictionary();
    const hits = [];
    const lower = q.toLowerCase();
    if (words.has(lower)) hits.push(lower);
    const asOutline = reverse.get(q) ?? reverse.get(q.toUpperCase());
    if (asOutline) hits.push(...asOutline.filter((w) => !hits.includes(w)));
    for (const w of words.keys()) {
      if (hits.length >= 12) break;
      if (w.startsWith(lower) && !hits.includes(w)) hits.push(w);
    }
    const asked = q.toUpperCase();
    if (parseOutline(asked)) {
      out.append(el('div', { class: 'hit' }, el('div', { class: 'hit__word' }, `the stroke ${asked}`),
        ...row(asOutline ? 'reads as' : 'not in this dictionary, but you can see its keys', [asked]),
        asOutline ? el('div', { class: 'hit__list' }, asOutline.slice(0, 6).map((w) => el('span', { class: 'word' }, w))) : null));
    }
    if (!hits.length && !parseOutline(asked)) out.append(el('p', {}, 'Not in this dictionary. `resolve` still reads back any stroke the dictionary misses, and any word can be joined from its syllables.'));
    for (const w of hits) {
      const outs = words.get(w);
      const one = outs.filter((o) => !o.includes('/'));
      const joins = outs.filter((o) => o.includes('/'));
      const primary = outs[0];
      out.append(el('div', { class: 'hit' }, el('div', { class: 'hit__word' }, w),
        ...row('fastest', primary ? [primary] : []),
        ...row('other single strokes', one.filter((o) => o !== primary)),
        ...row('joined with +', joins)));
    }
    // the Reference too: a rule, a table or a mechanism that mentions the query
    const refs = await searchRef(q, 5);
    if (refs.length && input.value.trim() === q) out.append(el('div', { class: 'hit' }, el('div', { class: 'hit__word' }, 'In the reference'), ...hitRows(refs, q),
      el('a', { href: '#/ref' }, 'Search all of the reference')));
  };
  input.addEventListener('input', search);
  app.append(el('h1', {}, 'Lookup'),
    el('p', { class: 'lede' }, 'The 12,000 commonest words, plus every word the lessons teach. Search a word or a stroke, then point at any stroke to see its keys.'),
    input,
    out);
}

const SOUND = {
  P: ['p', 'pat'], B: ['b', 'bat'], T: ['t', 'tap'], D: ['d', 'dad'], K: ['k', 'cat'], G: ['g', 'go'], M: ['m', 'map'],
  N: ['n', 'nap'], F: ['f', 'fan'], V: ['v', 'van'], TH: ['th', 'thin'], DH: ['th', 'this'], S: ['s', 'sat'], Z: ['z', 'zoo'],
  SH: ['sh', 'ship'], ZH: ['zh', 'vision'], CH: ['ch', 'chip'], JH: ['j', 'jam'], HH: ['h', 'hat'], L: ['l', 'lap'],
  R: ['r', 'rat'], W: ['w', 'wet'], Y: ['y', 'yes'], NG: ['ng', 'sing'],
  IY: ['ee', 'see'], IH: ['i', 'sit'], EY: ['ay', 'say'], EH: ['e', 'bet'], AE: ['a', 'bat'], AA: ['o', 'hot'], AH: ['u', 'but'],
  AO: ['aw', 'saw'], OW: ['oh', 'go'], UH: ['oo', 'book'], UW: ['oo', 'moon'], AY: ['eye', 'my'], AW: ['ow', 'now'],
  OY: ['oy', 'boy'], ER: ['er', 'her'],
};

async function chartPage() {
  setNav('chart');
  const data = await chart();
  const plain = (obj) => Object.entries(obj).filter(([k]) => !k.includes('.') && !k.includes(' '));
  const table = (title, head, entries) => el('div', {}, el('h3', {}, title),
    el('div', { class: 'tablewrap' }, el('table', {}, el('thead', {}, el('tr', {}, head.map((h) => el('th', {}, h)))),
      el('tbody', {}, entries.map(([a, b, c]) => el('tr', {}, el('td', { 'data-nohover': '' }, a), el('td', {}, chip(b)), el('td', {}, c ? el('span', { class: 'word' }, c) : '')))))));
  const sounds = (obj) => plain(obj).map(([k, v]) => [SOUND[k]?.[0] ?? k, v, SOUND[k]?.[1] ?? '']);
  app.append(el('div', { class: 'chart' },
    el('h1', {}, 'Key chart'),
    el('p', { class: 'lede' }, 'Every sound and the keys it takes. This is generated from the rules file, so it is always what the dictionary uses. Point at any stroke to see its keys.'),
    el('div', { class: 'legend' }, el('span', {}, el('i', { style: 'background:var(--c-left)' }), 'left hand: starting sounds'),
      el('span', {}, el('i', { style: 'background:var(--c-vowel)' }), 'thumbs: vowels'),
      el('span', {}, el('i', { style: 'background:var(--c-right)' }), 'right hand: ending sounds'),
      el('span', {}, el('i', { style: 'background:var(--c-misc)' }), 'star and join key +')),
    table('Starting sounds (left hand)', ['Sound', 'Keys', 'Example'], sounds(data.onset).map(([a, b, c]) => [a, b, c])),
    table('Vowels (thumbs)', ['Sound', 'Keys', 'Example'], sounds(data.vowel)),
    table('Ending sounds (right hand)', ['Sound', 'Keys', 'Example'], sounds(data.coda)),
    table('Folded endings', ['Sounds', 'Keys', ''], Object.entries(data.coda_cluster).filter(([k]) => !k.includes('.')).map(([k, v]) => [k.toLowerCase(), v, ''])),
    table('A vowel and its R', ['Sounds', 'Keys', ''], Object.entries(data.vowel_coda).filter(([k]) => !k.includes('.')).map(([k, v]) => [k.toLowerCase(), v, ''])),
  ));
}

async function progressPage() {
  setNav('progress');
  const c = await course();
  const all = flat(c);
  const bars = el('div', { class: 'bars' });
  let done = 0;
  for (const l of all) {
    const d = store.drill(l.id);
    const s = state(l);
    if (s === 'done') done += 1;
    bars.append(el('div', { class: 'bar' },
      el('a', { href: `#/lesson/${l.id}` }, `${l.num}. ${l.title}`),
      el('div', { class: 'bar__track' }, el('div', { class: 'bar__fill', style: `width:${s === 'done' ? 100 : 0}%` })),
      el('span', { class: 'mono' }, d ? `${d.best}% · ${d.wpm} wpm · ${d.runs} runs` : s === 'done' ? 'done' : '')));
  }
  const missed = Object.entries(store.missed).sort((a, b) => b[1] - a[1]).slice(0, 20);
  app.append(el('h1', {}, 'Progress'),
    el('p', { class: 'lede' }, `${done} of ${all.length} chapter drills passed. Saved in this browser only.`),
    bars,
    el('h2', {}, 'Words you miss most'),
    missed.length ? el('div', { class: 'hit__list' }, missed.map(([w, n]) => el('span', { class: 'stroke' }, el('span', { class: 'word' }, w), ` ×${n}`)))
      : el('p', {}, 'None yet.'),
    el('div', { class: 'row', style: 'margin-top:2rem' },
      el('button', { class: 'btn', type: 'button', onclick: () => { if (confirm?.('Clear all saved progress?') !== false) { store.reset(); route(); } } }, 'Clear saved progress')));
}

initPopover();
initHover(app);
initDock();
initHid(document.getElementById('hid-host'));
window.addEventListener('hashchange', route);
route();

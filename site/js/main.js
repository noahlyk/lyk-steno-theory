// The app shell: a hash router and the pages (lesson map, lesson, practice, lookup, key chart, progress).

import { chart, dictionary, index, lesson } from './data.js';
import { createBoard } from './board.js';
import { createDrill, stopAllDrills } from './drill.js';
import { el, shuffle } from './dom.js';
import { chip, inline, renderLesson } from './lesson.js';
import { store } from './store.js';

const app = document.getElementById('app');
let teardown = [];
let courseCache = null;

const course = async () => (courseCache ??= await index());
const flat = (c) => c.parts.flatMap((p) => p.lessons.map((l) => ({ ...l, part: p.title })));

function state(l) {
  const d = store.drill(l.id);
  if (d?.passed) return 'done';
  if (store.isRead(l.id)) return 'read';
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
    if (page === 'lesson') await lessonPage(arg);
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
  const nextId = all.find((l) => state(l) === 'todo')?.id;
  app.append(
    el('h1', {}, c.title),
    el('p', { class: 'lede' }, c.summary),
    el('div', { class: 'row', style: 'margin-top:.8rem' },
      el('a', { class: 'btn btn--primary', href: `#/lesson/${nextId ?? all[0].id}` }, nextId && nextId !== all[0].id ? 'Continue' : 'Start lesson 1'),
      el('a', { class: 'btn', href: '#/chart' }, 'Key chart'),
      el('a', { class: 'btn', href: '#/lookup' }, 'Look up a word')),
  );
  const map = el('div', { class: 'map' });
  for (const part of c.parts) {
    map.append(el('h2', { class: 'part' }, part.title));
    for (const l of part.lessons) {
      const s = state(l);
      const d = store.drill(l.id);
      map.append(el('a', { class: `card card--${s === 'todo' ? '' : s} ${l.id === nextId ? 'card--next' : ''}`, href: `#/lesson/${l.id}` },
        el('span', { class: 'card__num' }, `Lesson ${l.num}`),
        el('span', { class: 'card__title' }, l.title),
        el('span', { class: 'card__sum' }, l.summary),
        el('span', { class: 'card__state' },
          s === 'done' ? `Passed, best ${d.best}%` : s === 'read' ? 'Read, drill not passed yet' : l.id === nextId ? 'Up next' : 'Not started')));
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
      id, pass: data.drill.pass ?? 90, repeat: data.drill.repeat ?? 1, limit: data.drill.limit ?? 0, deferred: true,
      onDone: () => { store.setRead(id); },
    });
    teardown.push(() => drill.stop());
  }
  const prev = all[at - 1];
  const next = all[at + 1];
  const read = el('button', { class: 'btn', type: 'button' });
  const paintRead = () => { read.textContent = store.isRead(id) ? 'Read ✓ (undo)' : 'Mark as read'; };
  read.addEventListener('click', () => { store.setRead(id, !store.isRead(id)); paintRead(); });
  paintRead();
  app.append(el('div', { class: 'lessonfoot' },
    prev ? el('a', { class: 'btn', href: `#/lesson/${prev.id}` }, `← ${prev.title}`) : el('span'),
    read,
    next ? el('a', { class: 'btn btn--primary', href: `#/lesson/${next.id}` }, `${next.title} →`) : el('a', { class: 'btn', href: '#/practice' }, 'Practice →')));
}

async function practicePage() {
  setNav('practice');
  const c = await course();
  const all = flat(c);
  const withDrills = [];
  for (const l of all) {
    const data = await lesson(l.id);
    if (data.drill && data.drill.check !== 'none') withDrills.push({ ...l, items: data.drill.items });
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
  const stageHost = el('div');
  const stageLabel = el('span', { class: 'boardbox__label' }, ' ');
  const board = createBoard(stageHost, { compact: true });
  const stage = {
    peek() {},
    cycle(outline) {
      import('./steno.js').then(({ parseOutline }) => {
        const s = parseOutline(outline);
        if (!s) return;
        board.show(s[0]);
        stageLabel.textContent = outline;
      });
    },
  };
  const row = (label, list) => (list.length ? [el('div', { class: 'hit__label' }, label),
    el('div', { class: 'hit__list' }, list.map((o) => chip(o, stage)))] : []);
  const search = async () => {
    const q = input.value.trim();
    out.replaceChildren();
    if (!q) return;
    const { words, reverse } = await dictionary();
    const hits = [];
    const lower = q.toLowerCase();
    if (words.has(lower)) hits.push(lower);
    const asOutline = reverse.get(q);
    if (asOutline) hits.push(...asOutline.filter((w) => !hits.includes(w)));
    for (const w of words.keys()) {
      if (hits.length >= 12) break;
      if (w.startsWith(lower) && !hits.includes(w)) hits.push(w);
    }
    if (!hits.length) out.append(el('p', {}, 'Not in this dictionary. Any stroke the dictionary misses is still read back by the fallback, and any word can be joined from its syllables.'));
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
  };
  input.addEventListener('input', search);
  app.append(el('h1', {}, 'Lookup'),
    el('p', { class: 'lede' }, 'The 12,000 commonest words, plus every word the lessons teach. Search a word or a stroke.'),
    input,
    el('div', { class: 'boardbox', style: 'margin-top:1rem' }, el('div', { class: 'boardbox__head' }, stageLabel,
      el('span', { class: 'boardbox__hint' }, 'Click any stroke to light its keys.')), el('div', { class: 'boardbox__scroll' }, stageHost)),
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
  const stageHost = el('div');
  const stageLabel = el('span', { class: 'boardbox__label' }, ' ');
  const board = createBoard(stageHost, { compact: true });
  const stage = { peek() {}, cycle(outline) {
    import('./steno.js').then(({ parseOutline }) => { const s = parseOutline(outline); if (s) { board.show(s[0]); stageLabel.textContent = outline; } });
  } };
  const plain = (obj) => Object.entries(obj).filter(([k]) => !k.includes('.') && !k.includes(' '));
  const table = (title, head, entries) => el('div', {}, el('h3', {}, title),
    el('div', { class: 'tablewrap' }, el('table', {}, el('thead', {}, el('tr', {}, head.map((h) => el('th', {}, h)))),
      el('tbody', {}, entries.map(([a, b, c]) => el('tr', {}, el('td', {}, a), el('td', {}, chip(b, stage)), el('td', {}, c ? el('span', { class: 'word' }, c) : '')))))));
  const sounds = (obj) => plain(obj).map(([k, v]) => [SOUND[k]?.[0] ?? k, v, SOUND[k]?.[1] ?? '']);
  app.append(el('div', { class: 'chart' },
    el('h1', {}, 'Key chart'),
    el('p', { class: 'lede' }, 'Every sound and the keys it takes. This is generated from the rules file, so it is always what the dictionary uses.'),
    el('div', { class: 'boardbox stage' }, el('div', { class: 'boardbox__head' }, stageLabel,
      el('span', { class: 'boardbox__hint' }, 'Click any stroke.')), el('div', { class: 'boardbox__scroll' }, stageHost)),
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
      el('div', { class: 'bar__track' }, el('div', { class: `bar__fill ${s === 'read' ? 'bar__fill--read' : ''}`, style: `width:${s === 'done' ? 100 : s === 'read' ? 50 : 0}%` })),
      el('span', { class: 'mono' }, d ? `${d.best}% · ${d.wpm} wpm · ${d.runs} runs` : s === 'read' ? 'read' : '')));
  }
  const missed = Object.entries(store.missed).sort((a, b) => b[1] - a[1]).slice(0, 20);
  app.append(el('h1', {}, 'Progress'),
    el('p', { class: 'lede' }, `${done} of ${all.length} chapter drills passed. Saved in this browser only.`),
    bars,
    el('h2', {}, 'Words you miss most'),
    missed.length ? el('div', { class: 'hit__list' }, missed.map(([w, n]) => el('span', { class: 'stroke' }, `${w} ×${n}`)))
      : el('p', {}, 'None yet.'),
    el('div', { class: 'row', style: 'margin-top:2rem' },
      el('button', { class: 'btn', type: 'button', onclick: () => { if (confirm?.('Clear all saved progress?') !== false) { store.reset(); route(); } } }, 'Clear saved progress')));
}

window.addEventListener('hashchange', route);
route();

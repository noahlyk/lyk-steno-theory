// The Reference area: everything about how lyk works that is not a lesson (the rules, the briefs,
// the suffix and prefix tables, collisions, punctuation, the build pipeline). Pages are JSON with
// the same blocks as a lesson (no drills), listed in reference/index.json. Searchable as plain text.

import { refIndex, refPage } from './data.js';
import { el } from './dom.js';
import { inline, renderLesson, slug } from './lesson.js';

const flatPages = (idx) => idx.sections.flatMap((s) => s.pages.map((p) => ({ ...p, section: s.title })));

/** Plain text of a block's inline markup: `[[word|OUT]]` becomes "word OUT", the rest loses its marks. */
const plain = (t) => String(t).replace(/\[\[(?:([^|\]]*)\|)?([^\]]*)\]\]/g, (_, w, o) => `${(w ?? '').replace(/^=/, '')} ${o}`.trim())
  .replace(/\{\{(.+?)\}\}/g, '$1').replace(/\*\*(.+?)\*\*/g, '$1').replace(/`(.+?)`/g, '$1').replace(/\[([^\]]+)\]\([^)]*\)/g, '$1');

/** The searchable pieces of a page: one per block, each knowing the heading it sits under. */
function pieces(page, meta) {
  const out = [{ page: meta.id, title: page.title, anchor: '', heading: '', text: `${page.title} ${page.goal ?? ''}` }];
  let heading = '';
  let anchor = '';
  for (const b of page.blocks) {
    if (b.t === 'h' || b.t === 'h3') { heading = plain(b.x); anchor = slug(b.x); out.push({ page: meta.id, title: page.title, anchor, heading, text: heading }); continue; }
    let text = '';
    if (b.x) text = plain(b.x);
    else if (b.items) text = b.items.map((i) => plain(Array.isArray(i) ? i.join(' ') : i)).join(' ');
    else if (b.rows) text = b.rows.map((r) => r.map(plain).join(' ')).join(' \n ');
    if (text) out.push({ page: meta.id, title: page.title, anchor, heading, text });
  }
  return out;
}

let corpus = null;
async function loadCorpus() {
  corpus ??= (async () => {
    const idx = await refIndex();
    const pages = flatPages(idx);
    const all = await Promise.all(pages.map(async (m) => pieces(await refPage(m.id), m)));
    return all.flat();
  })();
  return corpus;
}

/** Reference pieces matching every word of the query, best first (title and heading hits before body hits). */
export async function searchRef(q, limit = 30) {
  const terms = q.toLowerCase().split(/\s+/).filter(Boolean);
  if (!terms.length) return [];
  const hits = [];
  for (const p of await loadCorpus()) {
    const hay = p.text.toLowerCase();
    if (!terms.every((t) => hay.includes(t) || p.title.toLowerCase().includes(t) || p.heading.toLowerCase().includes(t))) continue;
    const score = (p.anchor === '' ? 3 : 0) + (terms.every((t) => p.heading.toLowerCase().includes(t)) ? 2 : 0) + (terms.every((t) => p.title.toLowerCase().includes(t)) ? 1 : 0);
    hits.push({ ...p, score });
  }
  hits.sort((a, b) => b.score - a.score);
  return hits.slice(0, limit);
}

function snippet(text, terms) {
  const low = text.toLowerCase();
  const at = Math.max(0, Math.min(...terms.map((t) => (low.indexOf(t) < 0 ? 1e9 : low.indexOf(t)))));
  const from = at > 40 ? at - 40 : 0;
  const s = text.slice(from, from + 150).replace(/\s+/g, ' ');
  return `${from > 0 ? '… ' : ''}${s}${from + 150 < text.length ? ' …' : ''}`;
}

/** Result rows for a query, as DOM. */
export function hitRows(hits, q) {
  const terms = q.toLowerCase().split(/\s+/).filter(Boolean);
  return hits.map((h) => el('a', { class: 'refhit', href: `#/ref/${h.page}${h.anchor ? `/${h.anchor}` : ''}` },
    el('span', { class: 'refhit__where' }, h.heading ? `${h.title} / ${h.heading}` : h.title),
    el('span', { class: 'refhit__text' }, snippet(h.text, terms))));
}

export async function refHome(app) {
  const idx = await refIndex();
  const input = el('input', { class: 'search', type: 'search', placeholder: 'search the reference: a rule, a word, a stroke (-FT, TKOG)', 'aria-label': 'search the reference' });
  const results = el('div', { class: 'refhits' });
  const sections = el('div', { class: 'map' });
  for (const s of idx.sections) {
    sections.append(el('h2', { class: 'part' }, s.title), s.summary ? el('p', { class: 'refsec' }, s.summary) : null);
    for (const p of s.pages) {
      sections.append(el('a', { class: 'card', href: `#/ref/${p.id}` },
        el('span', { class: 'card__title' }, p.title), el('span', { class: 'card__sum' }, p.summary)));
    }
  }
  const run = async () => {
    const q = input.value.trim();
    results.replaceChildren();
    sections.hidden = Boolean(q);
    if (!q) return;
    const hits = await searchRef(q);
    results.replaceChildren(...(hits.length ? hitRows(hits, q) : [el('p', {}, 'Nothing in the reference matches. Lookup finds words and strokes in the dictionary.')]));
  };
  input.addEventListener('input', run);
  app.append(el('h1', {}, idx.title), el('p', { class: 'lede' }, idx.summary), input, results, sections);
}

export async function refPageView(app, id, anchor) {
  const idx = await refIndex();
  const all = flatPages(idx);
  const at = all.findIndex((p) => p.id === id);
  if (at < 0) return refHome(app);
  const meta = all[at];
  const page = await refPage(id);
  app.append(
    el('div', { class: 'crumbs' }, el('a', { href: '#/ref' }, idx.title), ` / ${meta.section}`),
    el('h1', {}, page.title),
    el('p', { class: 'lede' }, inline(page.goal ?? meta.summary)),
    renderLesson(page).body,
    el('div', { class: 'lessonfoot' },
      all[at - 1] ? el('a', { class: 'btn', href: `#/ref/${all[at - 1].id}` }, `← ${all[at - 1].title}`) : el('a', { class: 'btn', href: '#/ref' }, '← All of the reference'),
      el('span'),
      all[at + 1] ? el('a', { class: 'btn btn--primary', href: `#/ref/${all[at + 1].id}` }, `${all[at + 1].title} →`) : el('a', { class: 'btn', href: '#/ref' }, 'All of the reference')));
  if (anchor) requestAnimationFrame(() => document.getElementById(anchor)?.scrollIntoView());
}

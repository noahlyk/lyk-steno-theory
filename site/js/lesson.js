// Turns a lesson's JSON into a page: text, tables, little try-it drills, quizzes and the join-key
// builder. Any stroke chip shows its keys on hover (popover.js).

import { createBoard } from './board.js';
import { createDrill } from './drill.js';
import { outlineEl } from './outline.js';
import { el } from './dom.js';
import { whenActive } from './input.js';
import { dictionary } from './data.js';
import { formatStroke, parseOutline } from './steno.js';
import { grammarComposer } from './grammar.js';

const TOKEN = /\[\[(.+?)\]\]|`(.+?)`|\*\*(.+?)\*\*|\{\{(.+?)\}\}|\[([^\]]+)\]\(((?:https?:\/\/|#\/)[^)\s]+)\)|\*([^*\s`[][^*`[]*?)\*/g;

// `!name` / {{!name}}: a word that only names something on the page (a step, a command), not an
// example of typing, so the hover board (hover.js) leaves it alone. A table header `!Step` does it for a column.
const optOut = (text) => text.length > 1 && text[0] === '!';

/** Inline markup: [[word|OUTLINE]], [[OUTLINE]], `code`, **bold**, {{italic word}}, *italic*, [text](url). */
export function inline(text) {
  const out = [];
  let last = 0;
  for (const m of text.matchAll(TOKEN)) {
    if (m.index > last) out.push(text.slice(last, m.index));
    if (m[1] !== undefined) out.push(...chip(m[1]));
    else if (m[2] !== undefined) out.push(optOut(m[2]) ? el('code', { 'data-nohover': true }, m[2].slice(1)) : el('code', {}, m[2]));
    else if (m[3] !== undefined) out.push(el('strong', {}, ...inline(m[3])));
    else if (m[5] !== undefined) out.push(el('a', { href: m[6], rel: 'noopener' }, m[5]));
    else if (m[7] !== undefined) out.push(el('em', {}, m[7]));
    else out.push(optOut(m[4]) ? el('span', { class: 'word', 'data-nohover': true }, m[4].slice(1)) : el('span', { class: 'word' }, m[4]));
    last = m.index + m[0].length;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}

export function chip(spec, { styled = false } = {}) {
  const bar = spec.indexOf('|');
  let word = bar < 0 ? null : spec.slice(0, bar);
  const outline = bar < 0 ? spec : spec.slice(bar + 1);
  // `=it was|KPWHEB`: a phrasing-grammar stroke, not a dictionary entry (drawn with a dashed edge)
  const grammar = word?.startsWith('=') ?? false;
  if (grammar) word = word.slice(1);
  // the hover board (popover.js) reads data-outline; an outline that is not a stroke has none
  const node = el('button', { class: `stroke${styled ? ' stroke--outline' : ''}${grammar ? ' stroke--grammar' : ''}`, type: 'button', 'data-outline': parseOutline(outline) ? outline : null,
    title: grammar ? 'a phrasing-grammar stroke: not in the dictionary yet' : null },
    styled ? outlineEl(outline) : outline);
  return word ? [el('span', { class: 'word' }, word), ' ', node] : [node];
}

/** The anchor of a heading: its plain words, so `#/ref/briefs/the-star-ladder` can point at it. */
export const slug = (text) => text.toLowerCase().replace(/\[\[(?:[^|\]]*\|)?([^\]]*)\]\]/g, '$1').replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '');

const BLOCKS = {
  h: (b) => el('h2', { id: slug(b.x) }, inline(b.x)),
  h3: (b) => el('h3', { id: slug(b.x) }, inline(b.x)),
  // a formula or a short block of code, kept as written
  pre: (b) => el('pre', { class: 'formula' }, b.x),
  grammar: () => grammarComposer(),
  p: (b) => el('p', {}, inline(b.x)),
  list: (b) => el(b.ordered ? 'ol' : 'ul', { class: 'steps' }, b.items.map((i) => el('li', {}, inline(i)))),
  note: (b) => el('div', { class: `note ${b.kind ? `note--${b.kind}` : ''}` }, el('p', {}, inline(b.x))),
  table: (b) => el('div', { class: 'tablewrap' }, el('table', {},
    b.head ? el('thead', {}, el('tr', {}, b.head.map((h) => (optOut(h) ? el('th', { 'data-nohover': true }, h.slice(1)) : el('th', {}, h))))) : null,
    el('tbody', {}, b.rows.map((r) => el('tr', {}, r.map((c) => el('td', {}, inline(String(c))))))))),
  explore: (b) => explore(b),
  try: (b) => {
    const box = el('div', { class: 'try' },
      el('div', { class: 'try__title' }, b.title ?? 'Try it'),
      el('p', {}, b.x ?? `${b.items.length} to type. The keys light up after a miss.`));
    // starts by itself when it is on screen (no Start button)
    createDrill(box, b.items, { pass: 0, repeat: b.repeat ?? 1, deferred: true });
    return box;
  },
  quiz: (b) => quiz(b),
  join: (b) => joinBuilder(b),
};

export function renderBlocks(blocks) {
  return blocks.map((b) => {
    const make = BLOCKS[b.t];
    if (!make) throw new Error(`unknown block ${b.t}`);
    return make(b);
  });
}

/** A free board: strike a stroke and see its text and what the dictionary says it is. */
function explore(b) {
  const host = el('div');
  const label = el('span', { class: 'boardbox__label' }, ' ');
  const hint = el('span', { class: 'boardbox__hint' }, 'Strike keys together on your keyboard or steno machine.');
  const keys = new Set();
  const ctl = createBoard(host);
  const paint = async () => {
    ctl.press(keys);
    label.textContent = keys.size ? formatStroke(keys) : ' ';
    if (!keys.size) return;
    const words = (await dictionary()).reverse.get(formatStroke(keys));
    hint.textContent = words ? `reads as: ${words.slice(0, 4).join(', ')}` : 'not a word on its own';
  };
  const box = el('div', { class: 'boardbox' },
    el('div', { class: 'boardbox__head' }, label, hint),
    el('div', { class: 'boardbox__scroll' }, host));
  // the keyboard (or a steno machine) is read while the board is on screen: no click needed, and it
  // lets go of the keys when it scrolls away. Space is not a steno key, so the page still scrolls.
  let timer = 0;
  whenActive(box, {
    onStroke: (s) => {
      clearTimeout(timer);
      keys.clear();
      for (const k of s) keys.add(k);
      paint();
      timer = setTimeout(() => { keys.clear(); paint(); }, 1500);
    },
    onHeld: (held) => {
      if (!held.size) return;
      clearTimeout(timer);
      keys.clear();
      for (const k of held) keys.add(k);
      paint();
    },
    onActivate: () => { hint.textContent = 'Strike keys together on your keyboard or steno machine. Q is the join key.'; },
    onDeactivate: () => {
      clearTimeout(timer);
      keys.clear();
      paint();
      hint.textContent = 'Strike keys together on your keyboard or steno machine.';
    },
  });
  return el('div', {}, b.x ? el('p', {}, b.x) : null, box);
}

function quiz(b) {
  const why = el('div', { class: 'quiz__why' });
  const opts = el('div', { class: 'quiz__opts' }, b.opts.map((o, i) => {
    const btn = el('button', { class: 'btn', type: 'button' }, inline(o));
    btn.addEventListener('click', () => {
      for (const sib of opts.children) sib.classList.remove('is-right', 'is-wrong');
      btn.classList.add(i === b.a ? 'is-right' : 'is-wrong');
      why.replaceChildren(...inline(i === b.a ? `Yes. ${b.why ?? ''}` : 'Not quite, try again.'));
    });
    return btn;
  }));
  return el('div', { class: 'try' }, el('div', { class: 'try__title' }, 'Check yourself'),
    el('div', { class: 'quiz__q' }, inline(b.q)), opts, why);
}

/** Pick a stem and an ending: the join key goes on the second stroke. */
function joinBuilder(b) {
  const stem = el('select', { 'aria-label': 'word' }, b.stems.map(([w], i) => el('option', { value: i }, w)));
  const end = el('select', { 'aria-label': 'ending' }, b.ends.map(([w], i) => el('option', { value: i }, w)));
  const out = el('div', { class: 'builder__out' });
  const note = el('div', { class: 'boardbox__hint' });
  const run = async () => {
    const [w, o] = b.stems[stem.value];
    const [label, e, word] = b.ends[end.value];
    const outline = `${o}/${e}`;
    out.replaceChildren(el('span', { class: 'word' }, w), ' + ', el('span', { class: 'word' }, label), '  =  ', ...chip(outline));
    const found = (await dictionary()).reverse.get(outline);
    note.textContent = found ? `In the dictionary: ${found.slice(0, 3).join(', ')}.` : word ? `Not a dictionary entry here, but the fallback reads it as "${word}".` : 'Not a dictionary entry here; the strokes still read as stem plus ending.';
  };
  stem.addEventListener('change', run);
  end.addEventListener('change', run);
  run();
  return el('div', { class: 'try' }, el('div', { class: 'try__title' }, 'Join builder'),
    el('div', { class: 'builder' }, stem, ' + ', end), out, note);
}

/** The whole lesson page body. */
export function renderLesson(lesson) {
  return { body: el('div', { class: 'lesson' }, ...renderBlocks(lesson.blocks)) };
}

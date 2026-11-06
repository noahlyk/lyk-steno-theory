// Turns a lesson's JSON into a page: text, tables, a board that lights up for any stroke you
// point at, little try-it drills, quizzes and the join-key builder.

import { createBoard } from './board.js';
import { createDrill } from './drill.js';
import { el } from './dom.js';
import { listen } from './input.js';
import { dictionary } from './data.js';
import { formatStroke, parseOutline } from './steno.js';

const TOKEN = /\[\[(.+?)\]\]|`(.+?)`|\*\*(.+?)\*\*|\{\{(.+?)\}\}|\[([^\]]+)\]\((https?:\/\/[^)\s]+)\)|\*([^*\s`[][^*`[]*?)\*/g;

/** Inline markup: [[word|OUTLINE]], [[OUTLINE]], `code`, **bold**, {{italic word}}, *italic*, [text](url). */
export function inline(text, stage) {
  const out = [];
  let last = 0;
  for (const m of text.matchAll(TOKEN)) {
    if (m.index > last) out.push(text.slice(last, m.index));
    if (m[1] !== undefined) out.push(...chip(m[1], stage));
    else if (m[2] !== undefined) out.push(el('code', {}, m[2]));
    else if (m[3] !== undefined) out.push(el('strong', {}, ...inline(m[3], stage)));
    else if (m[5] !== undefined) out.push(el('a', { href: m[6], rel: 'noopener' }, m[5]));
    else if (m[7] !== undefined) out.push(el('em', {}, m[7]));
    else out.push(el('span', { class: 'word' }, m[4]));
    last = m.index + m[0].length;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}

export function chip(spec, stage) {
  const bar = spec.indexOf('|');
  const word = bar < 0 ? null : spec.slice(0, bar);
  const outline = bar < 0 ? spec : spec.slice(bar + 1);
  const strokes = parseOutline(outline);
  const node = el('button', {
    class: 'stroke', type: 'button', title: strokes ? 'show on the board' : null,
    onclick: () => stage?.cycle(outline, node),
    onmouseenter: () => stage?.peek(outline),
    onfocus: () => stage?.peek(outline),
  }, outline);
  return word ? [el('span', { class: 'word' }, word), ' ', node] : [node];
}

/** The sticky board of a lesson: a stroke chip lights its keys; a click steps through a multi-stroke outline. */
function makeStage() {
  const host = el('div');
  const label = el('span', { class: 'boardbox__label' }, ' ');
  const hint = el('span', { class: 'boardbox__hint' }, 'Point at or click any stroke in the lesson.');
  const ctl = createBoard(host, { compact: true });
  const box = el('div', { class: 'boardbox stage' },
    el('div', { class: 'boardbox__head' }, label, hint),
    el('div', { class: 'boardbox__scroll' }, host));
  let current = null;
  let step = 0;
  let lit = null;
  const show = (outline, at, node) => {
    const strokes = parseOutline(outline);
    if (!strokes) return;
    ctl.show(strokes[at]);
    label.textContent = outline.split('/').map((s, i) => (strokes.length > 1 && i === at ? `[${s}]` : s)).join(' / ');
    hint.textContent = strokes.length > 1 ? `stroke ${at + 1} of ${strokes.length} (click for the next)` : '';
    if (node) {
      lit?.classList.remove('is-lit');
      lit = node;
      node.classList.add('is-lit');
    }
  };
  return {
    box,
    peek(outline) {
      current = outline;
      step = 0;
      show(outline, 0, null);
    },
    cycle(outline, node) {
      const n = parseOutline(outline)?.length ?? 1;
      step = current === outline ? (step + 1) % n : 0;
      current = outline;
      show(outline, step, node);
    },
  };
}

const BLOCKS = {
  h: (b) => el('h2', {}, b.x),
  p: (b, stage) => el('p', {}, inline(b.x, stage)),
  list: (b, stage) => el(b.ordered ? 'ol' : 'ul', { class: 'steps' }, b.items.map((i) => el('li', {}, inline(i, stage)))),
  note: (b, stage) => el('div', { class: `note ${b.kind ? `note--${b.kind}` : ''}` }, el('p', {}, inline(b.x, stage))),
  table: (b, stage) => el('div', { class: 'tablewrap' }, el('table', {},
    b.head ? el('thead', {}, el('tr', {}, b.head.map((h) => el('th', {}, h)))) : null,
    el('tbody', {}, b.rows.map((r) => el('tr', {}, r.map((c) => el('td', {}, inline(String(c), stage)))))))),
  explore: (b) => explore(b),
  try: (b) => {
    const box = el('div', { class: 'try' });
    const start = el('button', { class: 'btn btn--primary', type: 'button' }, b.start ?? 'Try it');
    start.addEventListener('click', () => {
      start.remove();
      createDrill(box, b.items, { title: b.title ?? 'Try it', pass: 0, repeat: b.repeat ?? 1 });
    });
    box.append(el('div', { class: 'try__title' }, b.title ?? 'Try it'),
      el('p', {}, b.x ?? `${b.items.length} to type. The keys light up after a miss.`), start);
    return box;
  },
  quiz: (b, stage) => quiz(b, stage),
  join: (b, stage) => joinBuilder(b, stage),
};

export function renderBlocks(blocks, stage) {
  return blocks.map((b) => {
    const make = BLOCKS[b.t];
    if (!make) throw new Error(`unknown block ${b.t}`);
    return make(b, stage);
  });
}

/** A free board: strike or click a stroke and see its text and what the dictionary says it is. */
function explore(b) {
  const host = el('div');
  const label = el('span', { class: 'boardbox__label' }, ' ');
  const hint = el('span', { class: 'boardbox__hint' }, 'Click the board, then strike keys together. Or click the keys.');
  const keys = new Set();
  const ctl = createBoard(host, { onToggle: (k) => {
    if (keys.has(k)) keys.delete(k);
    else keys.add(k);
    paint();
  } });
  const clear = el('button', { class: 'btn', type: 'button', onclick: () => { keys.clear(); paint(); } }, 'Clear');
  const paint = async () => {
    ctl.press(keys);
    label.textContent = keys.size ? formatStroke(keys) : ' ';
    if (!keys.size) return;
    const words = (await dictionary()).reverse.get(formatStroke(keys));
    hint.textContent = words ? `reads as: ${words.slice(0, 4).join(', ')}` : 'not a word on its own';
  };
  const box = el('div', { class: 'boardbox' },
    el('div', { class: 'boardbox__head' }, label, hint),
    el('div', { class: 'boardbox__scroll' }, host), el('div', { class: 'row', style: 'margin-top:.5rem' }, clear));
  // the keyboard is only read while the board has focus, so Space still scrolls the page
  let stop = null;
  let timer = 0;
  box.tabIndex = 0;
  box.addEventListener('focus', () => {
    if (stop) return;
    hint.textContent = 'Listening: strike keys together. Q is the join key.';
    stop = listen((s) => {
      keys.clear();
      for (const k of s) keys.add(k);
      paint();
      clearTimeout(timer);
      timer = setTimeout(() => { keys.clear(); paint(); }, 1500);
    });
  });
  box.addEventListener('blur', () => {
    stop?.();
    stop = null;
  });
  return el('div', {}, b.x ? el('p', {}, b.x) : null, box);
}

function quiz(b, stage) {
  const why = el('div', { class: 'quiz__why' });
  const opts = el('div', { class: 'quiz__opts' }, b.opts.map((o, i) => {
    const btn = el('button', { class: 'btn', type: 'button' }, inline(o, stage));
    btn.addEventListener('click', () => {
      for (const sib of opts.children) sib.classList.remove('is-right', 'is-wrong');
      btn.classList.add(i === b.a ? 'is-right' : 'is-wrong');
      why.replaceChildren(...inline(i === b.a ? `Yes. ${b.why ?? ''}` : 'Not quite, try again.', stage));
    });
    return btn;
  }));
  return el('div', { class: 'try' }, el('div', { class: 'try__title' }, 'Check yourself'),
    el('div', { class: 'quiz__q' }, inline(b.q, stage)), opts, why);
}

/** Pick a stem and an ending: the join key goes on the second stroke. */
function joinBuilder(b, stage) {
  const stem = el('select', { 'aria-label': 'word' }, b.stems.map(([w], i) => el('option', { value: i }, w)));
  const end = el('select', { 'aria-label': 'ending' }, b.ends.map(([w], i) => el('option', { value: i }, w)));
  const out = el('div', { class: 'builder__out' });
  const note = el('div', { class: 'boardbox__hint' });
  const run = async () => {
    const [w, o] = b.stems[stem.value];
    const [label, e, word] = b.ends[end.value];
    const outline = `${o}/${e}`;
    out.replaceChildren(el('span', { class: 'word' }, w), ' + ', el('span', { class: 'word' }, label), '  =  ', ...chip(outline, stage));
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
  const stage = makeStage();
  const body = el('div', { class: 'lesson' }, stage.box, ...renderBlocks(lesson.blocks, stage));
  return { body, stage };
}

// The hover board: pointing at (or focusing) any stroke chip shows its keys in a small floating
// board next to it, and moving away hides it. A multi-stroke outline steps through its strokes.
// On a touch screen a tap toggles it. One listener on the document serves every chip in the app.

import { createBoard } from './board.js';
import { el } from './dom.js';
import { parseOutline } from './steno.js';

const STEP_MS = 900;
let pop = null;
let ctl = null;
let label = null;
let note = null;
let timer = 0;
let shownFor = null;
let touched = false;

function build() {
  const host = el('div');
  ctl = createBoard(host, { compact: true });
  label = el('span', { class: 'pop__label' });
  note = el('span', { class: 'pop__note' });
  pop = el('div', { class: 'pop', role: 'tooltip', hidden: true }, el('div', { class: 'pop__head' }, label, note), host);
  document.body.append(pop);
}

function place(anchor) {
  const r = anchor.getBoundingClientRect();
  const w = pop.offsetWidth;
  const h = pop.offsetHeight;
  const gap = 8;
  const left = Math.min(Math.max(8, r.left + r.width / 2 - w / 2), window.innerWidth - w - 8);
  const below = r.bottom + gap + h <= window.innerHeight || r.top - gap - h < 0;
  pop.style.left = `${left}px`;
  pop.style.top = `${below ? r.bottom + gap : r.top - gap - h}px`;
}

function paint(strokes, outline, at) {
  ctl.show(strokes[at]);
  // the stroke being shown keeps full strength; the rest of the outline is just dimmed (no brackets)
  const parts = outline.split('/');
  label.replaceChildren(...parts.flatMap((s, i) => [
    ...(i ? [el('span', { class: 'pop__sep' }, '/')] : []),
    el('span', { class: `pop__stroke${strokes.length > 1 && i !== at ? ' is-dim' : ''}` }, s),
  ]));
  note.textContent = strokes.length > 1 ? `stroke ${at + 1} of ${strokes.length}` : '';
}

export function hidePreview() {
  clearInterval(timer);
  shownFor = null;
  if (pop) pop.hidden = true;
}

export function showPreview(anchor) {
  const outline = anchor.dataset.outline;
  const strokes = outline && parseOutline(outline);
  if (!strokes) return;
  if (!pop) build();
  if (shownFor === anchor) return;
  clearInterval(timer);
  shownFor = anchor;
  let at = 0;
  paint(strokes, outline, at);
  pop.hidden = false;
  place(anchor);
  if (strokes.length > 1) {
    timer = setInterval(() => {
      at = (at + 1) % strokes.length;
      paint(strokes, outline, at);
    }, STEP_MS);
  }
}

const chipOf = (node) => (node instanceof Element ? node.closest('[data-outline]') : null);

export function initPopover() {
  document.addEventListener('pointerdown', (e) => { touched = e.pointerType === 'touch'; }, true);
  document.addEventListener('mouseover', (e) => {
    const c = chipOf(e.target);
    if (c && !touched) showPreview(c);
  });
  document.addEventListener('mouseout', (e) => {
    const c = chipOf(e.target);
    if (c && !touched && chipOf(e.relatedTarget) !== c) hidePreview();
  });
  document.addEventListener('focusin', (e) => {
    const c = chipOf(e.target);
    if (c) showPreview(c);
  });
  document.addEventListener('focusout', (e) => {
    if (chipOf(e.target)) hidePreview();
  });
  document.addEventListener('click', (e) => {
    const c = chipOf(e.target);
    if (!touched) return;
    if (c && shownFor !== c) showPreview(c);
    else hidePreview();
  });
  window.addEventListener('hashchange', hidePreview);
  window.addEventListener('scroll', hidePreview, { passive: true });
}

// "Try it": a small typing pad, collapsed until asked for. Open it with the button or the `?` key,
// then type on the keyboard (Plover QWERTY, Q is the join key): the board glows the keys you hold
// and the pad shows each stroke and what it translates to.

import { createBoard } from './board.js';
import { el } from './dom.js';
import { listen } from './input.js';
import { formatStroke } from './steno.js';
import { translate } from './translate.js';

export function initDock() {
  const host = el('div');
  const board = createBoard(host, { compact: true });
  const live = el('div', { class: 'dock__live' }, ' ');
  const out = el('div', { class: 'dock__out', 'aria-live': 'polite' });
  const tape = el('div', { class: 'dock__tape' });
  let strokes = [];
  let stop = null;

  const paint = async () => {
    const words = await translate(strokes);
    out.replaceChildren(...words.map((w) => el('span', { class: `dock__word${w.word ? '' : ' is-unknown'}`, title: w.outline }, w.word ?? w.outline)));
    tape.textContent = strokes.map((s) => formatStroke(s)).join('  ');
  };
  const panel = el('section', { class: 'dock__panel', hidden: true, 'aria-label': 'Try it' },
    el('div', { class: 'dock__head' }, el('strong', {}, 'Try it'),
      el('span', { class: 'boardbox__hint' }, 'Plover QWERTY; Q is the join key.'),
      el('button', { class: 'btn', type: 'button', onclick: () => { strokes = []; paint(); } }, 'Clear'),
      el('button', { class: 'btn', type: 'button', 'aria-label': 'close', onclick: () => toggle(false) }, '×')),
    host, live, out, tape);
  const button = el('button', { class: 'dock__btn btn', type: 'button', 'aria-expanded': 'false', title: 'Press ? to open or close', onclick: () => toggle() }, 'Try it ⌨');
  document.body.append(el('div', { class: 'dock' }, panel, button));

  function toggle(on = panel.hidden) {
    panel.hidden = !on;
    button.setAttribute('aria-expanded', String(on));
    stop?.();
    stop = null;
    if (on) {
      stop = listen((s) => {
        strokes.push(s);
        live.textContent = formatStroke(s);
        paint();
      }, (held) => {
        board.press(held);
        if (held.size) live.textContent = formatStroke(held);
      });
    } else {
      board.clear();
    }
  }
  window.addEventListener('keydown', (e) => {
    if (e.key === '?' && !/^(INPUT|TEXTAREA|SELECT)$/.test(e.target.tagName)) {
      e.preventDefault();
      toggle();
    } else if (e.key === 'Escape' && !panel.hidden) {
      toggle(false);
    }
  });
}

// The "Mark as Done" button of a lesson or chapter. One at the bottom always; and if the page is
// opened already marked Done, an exact copy at the very top too, so an accidental mark can be
// undone straight away. Both stay in step.

import { el } from './dom.js';
import { store } from './store.js';

export function doneControl(id) {
  const make = () => el('button', { class: 'btn', type: 'button' });
  const foot = make();
  const top = store.isRead(id) ? make() : null;
  const buttons = top ? [top, foot] : [foot];
  const paint = () => {
    const on = store.isRead(id);
    for (const b of buttons) {
      b.textContent = on ? 'Done ✓ (undo)' : 'Mark as Done';
      b.classList.toggle('btn--done', on);
    }
  };
  for (const b of buttons) b.addEventListener('click', () => { store.setRead(id, !store.isRead(id)); paint(); });
  paint();
  return { foot, top: top ? el('div', { class: 'donetop' }, top) : null, paint };
}

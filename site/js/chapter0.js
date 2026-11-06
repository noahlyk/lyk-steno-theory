// Chapter 0, "Learn the keys": a short explanation, then the key stream (keys.js) as the chapter's
// drill at the bottom. It is optional, and says so.

import { el } from './dom.js';
import { CHAPTER0, keysPage } from './keys.js';
import { renderBlocks } from './lesson.js';
import { store } from './store.js';

const BLOCKS = [
  { t: 'h', x: 'Why bother' },
  { t: 'p', x: 'Every outline in lyk is just sounds, left to right across the keys. If you already know where each sound lives, you can read any outline and write any word without looking anything up, and the chapters after this one go much faster. The stream below trains exactly that, one key at a time.' },
  { t: 'h', x: 'How it works' },
  { t: 'list', items: [
    'A word appears with its stroke underneath. Hold those keys together, then let go.',
    'You start with six keys. Another unlocks whenever your last words are fast and clean enough: the faster you go, the less perfect you need to be.',
    'The guide fades as you improve: the diagram and QWERTY hints first, then the diagram, then nothing. It comes back if you struggle. The stroke is always there.',
    'Colour is the hand: orange left, blue thumbs, purple right, green for the star and join key. Keys one finger plays together are underlined as a pair.',
  ] },
];

/** Renders chapter 0 into `app`; `next` is the first lesson ({ id, title }). Returns a stop function. */
export async function keysChapter(app, next) {
  app.append(
    el('div', { class: 'crumbs' }, el('a', { href: '#/' }, 'Lessons'), ' / Chapter 0'),
    el('h1', {}, 'Learn the keys'),
    el('p', { class: 'lede' }, 'Optional, but memorizing which keys make which sounds makes life easier.'),
    ...renderBlocks(BLOCKS),
    el('h2', {}, 'The drill'));
  const stop = await keysPage(app);
  const done = el('button', { class: 'btn', type: 'button' });
  const paint = () => { done.textContent = store.isRead(CHAPTER0) ? 'Done ✓ (undo)' : 'Mark as Done'; done.classList.toggle('btn--done', store.isRead(CHAPTER0)); };
  done.addEventListener('click', () => { store.setRead(CHAPTER0, !store.isRead(CHAPTER0)); paint(); });
  paint();
  app.append(el('div', { class: 'lessonfoot' }, done,
    el('a', { class: 'btn btn--primary', href: `#/lesson/${next.id}` }, `${next.title} →`)));
  return stop;
}

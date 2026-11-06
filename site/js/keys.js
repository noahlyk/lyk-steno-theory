// The "Learn the keys" page (#/keys): a stream of words made only of the keys you have unlocked,
// one more key each time a lesson is fast and accurate enough. The engine is prestige.js.

import { createBoard } from './board.js';
import { el } from './dom.js';
import { listen } from './input.js';
import * as P from './prestige.js';
import { formatStroke } from './steno.js';
import { store } from './store.js';

const LOOKAHEAD = 4; // upcoming words shown
const DONE_KEEP = 6; // finished words left in view
const IDLE_MS = 3000; // a wait longer than this counts as this much
const GROUP_OF = (k) => (k === '+' || k === '*' ? 'misc' : /^(A|O|E|U)$/.test(k) ? 'vowel' : k.startsWith('-') ? 'right' : 'left');
// The Plover QWERTY key each steno key sits under (input.js has the same map by key code).
const HINT = {
  'S-': 'A', 'T-': 'W', 'K-': 'S', 'P-': 'E', 'W-': 'D', 'H-': 'R', 'R-': 'F', A: 'C', O: 'V', '*': 'T', E: 'N', U: 'M',
  '-F': 'U', '-R': 'J', '-P': 'I', '-B': 'K', '-L': 'O', '-G': 'L', '-T': 'P', '-S': ';', '-D': '[', '-Z': "'", '+': 'Q',
};

const text = (mask) => formatStroke(new Set(P.keysOf(mask)));
const hints = (mask) => P.keysOf(mask).map((k) => HINT[k]).join(' ');
const sound = (id) => id.replace('-', '');

/** What the other pages need: has this learner finished (or skipped) the key learning? */
export function keysStatus() {
  const p = P.normalize(store.loadPrestige());
  return { done: Boolean(p.completed[2]), skipped: p.skipped, unlocked: p.unlocked, level: p.level };
}

export function markKeysSkipped(on = true) {
  const p = P.normalize(store.loadPrestige());
  p.skipped = on;
  store.savePrestige(p);
}

/** Renders the page into `app`; returns a stop function (call it when the route changes). */
export async function keysPage(app) {
  const { words: dict } = await fetch('data/words.json').then((r) => r.json());
  const words = P.buildWords(dict);
  const p = P.normalize(store.loadPrestige());

  const s = {
    live: false,
    queue: [], // the word being typed first
    done: [],
    recent: [],
    word: null, // { text, all, candidates, strokeIdx, doneStrokes, misses, wrongChords, ms, lastEnd }
    held: new Set(),
    heldSince: 0,
    partial: 0,
    typed: 0,
    message: '',
    kind: '',
    sinceLesson: 0,
  };
  let stopListening = () => {};

  // ---- layout ----
  const levels = el('div', { class: 'klevels' });
  const chips = el('div', { class: 'kchips' });
  const stream = el('div', { class: 'kstream', tabindex: '0', 'aria-label': 'word stream' });
  const status = el('div', { class: 'kstatus', 'aria-live': 'polite' });
  const metrics = el('div', { class: 'stats kmetrics' });
  const progressBar = el('span');
  const boardHost = el('div');
  const boardWrap = el('div', { class: 'boardbox kboard' }, el('div', { class: 'boardbox__scroll' }, boardHost));
  const board = createBoard(boardHost, { compact: true });
  const labelsBox = el('input', { type: 'checkbox', id: 'k-labels' });
  labelsBox.addEventListener('change', () => { p.labels = labelsBox.checked; save(); render(); });
  const labelsRow = el('label', { class: 'boardbox__hint' }, labelsBox, ' show the stroke letters under each word');
  const start = el('button', { class: 'btn btn--primary', type: 'button', onclick: () => begin() }, 'Start');
  const skip = el('button', { class: 'btn', type: 'button', onclick: () => { p.skipped = true; save(); renderTop(); } }, 'I already know steno: skip this');
  const weakRow = el('div', { class: 'boardbox__hint kweak' });

  app.append(
    el('h1', {}, 'Learn the keys'),
    el('p', { class: 'lede' }, 'Before the theory, learn where every sound lives. Type the words as they come: you start with six keys, and each time a lesson of 30 words is fast and clean enough, one more key unlocks. Do Prestige 1 with the board in view, then Prestige 2 with it hidden.'),
    levels, chips,
    el('div', { class: 'boardbox kstreambox' },
      el('div', { class: 'row' }, start, skip, labelsRow),
      stream, status,
      el('div', { class: 'progressbar' }, progressBar), metrics, weakRow),
    boardWrap,
    el('p', { class: 'boardbox__hint' },
      'Hold the keys of a stroke together on your keyboard (Plover QWERTY: ', el('code', {}, 'QWER ASDF'), ' left hand, ',
      el('code', {}, 'C V'), ' and ', el('code', {}, 'N M'), ' thumbs, ', el('code', {}, 'T'), ' star, ', el('code', {}, 'UIOP JKL;'),
      ' right hand, ', el('code', {}, 'Q'), ' join key), then let go. Typing a longer word needs the join key, so it unlocks last.'));

  const save = () => store.savePrestige(p);
  const mask = () => P.unlockedMask(p);

  // ---- the word in play ----
  function refill() {
    while (s.queue.length < 1 + LOOKAHEAD) {
      const w = P.nextWord(words, p, { avoid: [...s.recent, ...s.queue.map((q) => q.text)] });
      if (!w) break;
      s.queue.push(w);
      s.recent.push(w.text);
      s.recent.splice(0, Math.max(0, s.recent.length - 60));
    }
  }
  function startWord() {
    const w = s.queue[0];
    if (!w) { s.word = null; return; }
    const all = P.displayVariants(w, mask());
    s.word = { text: w.text, all, candidates: all, strokeIdx: 0, doneStrokes: [], misses: 0, wrongChords: 0, ms: 0, lastEnd: performance.now() };
    s.partial = 0;
  }
  const target = () => s.word?.candidates[0]?.[s.word.strokeIdx] ?? 0;

  function begin() {
    s.live = true;
    s.queue = [];
    s.done = [];
    s.sinceLesson = 0;
    s.message = '';
    refill();
    startWord();
    stopListening();
    stopListening = listen(onStroke, onHeld);
    start.hidden = true;
    stream.focus();
    render();
  }
  function stop() {
    s.live = false;
    stopListening();
    stopListening = () => {};
    start.hidden = false;
    start.textContent = p.totalTyped ? 'Continue' : 'Start';
    board.clear();
  }

  // ---- input ----
  function onHeld(keys) {
    if (keys.size && !s.held.size) s.heldSince = performance.now();
    s.held = keys;
    paintBoard();
    renderStream();
  }

  function onStroke(keys) {
    const w = s.word;
    if (!s.live || !w) return;
    const now = performance.now();
    const hold = now - s.heldSince;
    w.ms += Math.min(Math.max(s.heldSince - w.lastEnd, 0), IDLE_MS) + hold;
    w.lastEnd = now;
    const chord = P.maskOf(keys);
    const fit = (c) => P.classifyStroke({ chord: c, candidates: w.candidates, all: w.all, strokeIdx: w.strokeIdx });
    let used = s.partial ? s.partial | chord : chord;
    let fitted = fit(used);
    if (!fitted && s.partial) { used = chord; fitted = fit(used); }
    s.partial = 0;
    if (fitted?.type === 'partial') {
      s.partial = used;
      say(`Keep going: ${text(used)} is part of ${text(target())}.`, '');
      return render();
    }
    if (fitted) {
      P.recordStroke(p, { used: P.keysOf(used), ms: hold, correct: true });
      if (fitted.type === 'restart') { w.doneStrokes = [used]; w.strokeIdx = 1; } else { w.doneStrokes.push(used); w.strokeIdx += 1; }
      w.candidates = fitted.matches;
      say('', '');
      if (w.candidates.some((v) => v.length === w.strokeIdx)) wordDone();
      else render();
      return undefined;
    }
    // a real miss: the stroke fits no spelling. Blame only the chords that came out wrong.
    w.misses = 1;
    w.wrongChords += 1;
    P.recordStroke(p, { target: P.keysOf(target()), used: P.keysOf(used), ms: hold, correct: false });
    const spellings = [...new Set(w.all.map((v) => v.map(text).join('/')))];
    say(`You pressed ${text(used)}; this word needs ${spellings.join(' or ')}.`, 'bad');
    board.diff(new Set(P.keysOf(target())), new Set(P.keysOf(used)));
    render(false);
    return undefined;
  }

  function wordDone() {
    const w = s.word;
    P.recordWord(p, { ms: w.ms, strokes: w.strokeIdx, misses: w.misses });
    s.typed += 1;
    s.done.push({ text: w.text, strokes: w.doneStrokes, missed: w.misses > 0 });
    s.done.splice(0, Math.max(0, s.done.length - DONE_KEEP));
    s.queue.shift();
    // the lesson's own numbers, taken before an unlock clears the judging window
    const lesson = P.windowStats(p);
    let toast = false;
    if (P.unlockIfReady(p)) {
      toast = true;
      const name = P.KEY_ORDER[p.unlocked - 1];
      const all = p.unlocked >= P.KEY_ORDER.length;
      say(all ? 'All keys unlocked. One more clean lesson finishes this level.' : `Unlocked ${sound(name)}${name === '+' ? ' (the join key)' : ''}.`, 'good');
      s.queue = [];
    } else if (P.completeIfReady(p)) {
      toast = true;
      say(`Prestige ${p.level} complete!`, 'good');
    }
    s.sinceLesson += 1;
    if (s.sinceLesson >= P.LESSON_SIZE) {
      s.sinceLesson = 0;
      P.recordLesson(p, lesson);
      const best = toast ? null : P.recordBestIfFaster(p, lesson);
      if (best) say(`New prestige ${p.level} best: ${best} wpm!`, 'good');
      else if (!toast) say(`Lesson done: ${Math.round(lesson.wpm)} wpm, ${Math.round(lesson.accuracy * 100)}% clean. Keep going.`, 'good');
    }
    save();
    refill();
    startWord();
    render();
  }

  const say = (m, kind) => { s.message = m; s.kind = kind; };

  // ---- drawing ----
  function showBoard() {
    return p.level === 1 || Boolean(s.word && s.word.wrongChords >= P.LABEL_REVEAL_MISSES);
  }
  function showLabels() {
    return p.level === 1 || p.labels || Boolean(s.word && s.word.wrongChords >= P.LABEL_REVEAL_MISSES);
  }

  function paintBoard() {
    boardWrap.hidden = !showBoard();
    if (!s.live || !s.word) { board.clear(); return; }
    // target keys light (the keys still to press for this stroke); held keys show on top
    board.show(new Set(P.keysOf(target())));
    board.press(s.held);
  }

  function renderTop() {
    const { done, skipped } = { done: Boolean(p.completed[2]), skipped: p.skipped };
    skip.hidden = done || skipped;
    levels.replaceChildren(...P.levelSummary(p).map((l) => el('button', {
      class: `klevel${l.active ? ' is-active' : ''}${l.done ? ' is-done' : ''}`, type: 'button',
      'aria-pressed': String(l.active),
      onclick: () => { if (!l.active) { stop(); P.switchLevel(p, l.level); save(); renderAll(); } },
    },
    el('span', { class: 'klevel__name' }, `Prestige ${l.level}`, l.done ? ' ✓' : ''),
    el('span', { class: 'klevel__what' }, l.level === 1 ? 'board and key hints on' : 'board hidden, letters only'),
    el('span', { class: 'klevel__n' }, `${l.unlocked} / ${l.total} keys · ${l.wpm} wpm to unlock`, l.best ? ` · best ${l.best} wpm` : ''))));
    labelsRow.hidden = p.level === 1;
    labelsBox.checked = p.labels;
    const names = P.unlockedKeys(p);
    chips.replaceChildren(...P.KEY_ORDER.map((k, i) => el('span', {
      class: `kchip kchip--${GROUP_OF(k)}${i < p.unlocked ? '' : ' is-locked'}${i === p.unlocked - 1 ? ' is-new' : ''}`,
      title: i < p.unlocked ? `${sound(k)}: QWERTY ${HINT[k]}` : 'locked',
    }, k === '+' ? 'join' : sound(k), p.level === 1 && i < names.length ? el('small', {}, HINT[k]) : null)));
  }

  function renderStream() {
    const rows = [];
    const cell = (w, cls, strokes, current) => {
      const c = el('div', { class: `kcell ${cls}` }, el('span', { class: 'kcell__word' }, w));
      const label = el('span', { class: 'kcell__stroke' });
      if (strokes && (showLabels() || !current)) {
        strokes.forEach((m, i) => {
          const st = el('span', { class: `kst${current && i === s.word.strokeIdx ? ' is-now' : ''}${current && i < s.word.strokeIdx ? ' is-done' : ''}` }, text(m));
          label.append(st);
          if (current && p.level === 1 && i === s.word.strokeIdx) label.append(el('small', { class: 'kcell__hint' }, hints(m)));
        });
      }
      c.append(label);
      return c;
    };
    for (const d of s.done) rows.push(el('div', { class: `kcell is-done${d.missed ? ' is-missed' : ''}` }, el('span', { class: 'kcell__word' }, d.text), el('span', { class: 'kcell__stroke' })));
    s.queue.forEach((q, i) => {
      if (i === 0 && s.word) rows.push(cell(q.text, 'is-current', s.word.candidates[0], true));
      else rows.push(cell(q.text, 'is-next', p.level === 1 ? P.displayVariants(q, mask())[0] : null, false));
    });
    stream.replaceChildren(...(s.live ? rows : [el('p', { class: 'boardbox__hint' }, 'Press Start, then type the first word.')]));
    const held = s.held.size ? `holding ${formatStroke(s.held)}` : '';
    status.textContent = s.message || held || ' ';
    status.className = `kstatus${s.kind ? ` kstatus--${s.kind}` : ''}`;
  }

  function renderMetrics() {
    const st = P.windowStats(p);
    const want = P.wpmTarget(p.level);
    metrics.replaceChildren(
      el('span', {}, `${st.count} / ${P.LESSON_SIZE} words`),
      el('span', { class: st.count && st.wpm >= want ? 'is-good' : '' }, `${Math.round(st.wpm)} / ${want} wpm`),
      el('span', { class: st.count && st.accuracy >= P.ACCURACY_TARGET ? 'is-good' : '' }, `${Math.round(st.accuracy * 100)}% clean (95% needed)`),
      el('span', {}, `${p.totalTyped} words typed in all`));
    progressBar.style.width = `${(st.count / P.LESSON_SIZE) * 100}%`;
    const weak = P.weakChords(p, 3).filter((c) => c.accuracy < 0.95);
    weakRow.textContent = weak.length ? `Needs work: ${weak.map((c) => `${c.id.split('|').map(sound).join('')} (${Math.round(c.accuracy * 100)}%)`).join(', ')}` : '';
  }

  function render(rePaint = true) {
    renderTop();
    renderStream();
    renderMetrics();
    if (rePaint) paintBoard();
    else boardWrap.hidden = !showBoard();
  }
  function renderAll() {
    s.queue = [];
    s.done = [];
    s.word = null;
    s.message = '';
    render();
    start.hidden = false;
    start.textContent = p.totalTyped ? 'Continue' : 'Start';
  }

  renderAll();
  return stop;
}

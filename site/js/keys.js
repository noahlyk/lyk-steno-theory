// The "Learn the keys" page (#/keys): a stream of words made only of the keys you have unlocked,
// one more key whenever your recent words are fast and clean enough. The engine is trainer.js.
// The guide (diagram + hints, then diagram, then nothing) fades as you get good and comes back if
// you struggle; the stroke under each word is always there.

import { createBoard } from './board.js';
import { confetti } from './confetti.js';
import { el } from './dom.js';
import { whenActive } from './input.js';
import { GROUP_OF, sound, strokeEl as strokeElOf } from './outline.js';
import { store } from './store.js';

const SHOW_ID = 'kshow';
import * as P from './trainer.js';

const LOOKAHEAD = 4; // upcoming words shown
const DONE_KEEP = 6; // finished words left in view
const IDLE_MS = 3000; // a wait longer than this counts as this much
// The Plover QWERTY key each steno key sits under (input.js has the same map by key code).
const HINT = {
  'S-': 'A', 'T-': 'W', 'K-': 'S', 'P-': 'E', 'W-': 'D', 'H-': 'R', 'R-': 'F', A: 'C', O: 'V', '*': 'T', E: 'N', U: 'M',
  '-F': 'U', '-R': 'J', '-P': 'I', '-B': 'K', '-L': 'O', '-G': 'L', '-T': 'P', '-S': ';', '-D': '[', '-Z': "'", '+': 'Q',
};
const GUIDES = [
  [P.GUIDE.FULL, 'Diagram and hints'],
  [P.GUIDE.DIAGRAM, 'Diagram'],
  [P.GUIDE.MEMORY, 'From memory'],
];

const hints = (mask) => P.keysOf(mask).map((k) => HINT[k]).join(' ');

export const CHAPTER0 = 'chapter0'; // the id chapter 0 is marked done under

/** What the other pages need: has this learner finished (or skipped) the key learning? */
export function keysStatus() {
  const p = P.normalize(store.loadKeys());
  return { done: p.done || store.isRead(CHAPTER0), skipped: p.skipped, unlocked: p.unlocked };
}

export function markKeysSkipped(on = true) {
  const p = P.normalize(store.loadKeys());
  p.skipped = on;
  store.saveKeys(p);
}

/** Renders the page into `app`; returns a stop function (call it when the route changes). */
export async function keysPage(app) {
  const { words: dict } = await fetch('data/words.json').then((r) => r.json());
  const words = P.buildWords(dict);
  const p = P.normalize(store.loadKeys());

  const s = {
    live: false,
    queue: [], // the word being typed first
    done: [],
    recent: [],
    word: null, // { text, all, candidates, strokeIdx, doneStrokes, misses, wrongChords, ms, lastEnd }
    held: new Set(),
    heldSince: 0,
    partial: 0,
    message: '',
    kind: '',
    sinceLesson: 0,
    peekWord: false, // always-show was on at some moment of the word in play
    peekWindow: false, // ... or at some moment of the 20-word window a best is judged on
  };
  // Never saved, never cleared by turning it off: a window that was peeked at cannot set a best.
  const showToggle = el('input', { type: 'checkbox', id: SHOW_ID });
  showToggle.checked = store.settings.hint === 'always';
  const peekNote = el('div', { class: 'boardbox__hint peeknote', 'aria-live': 'polite' });
  showToggle.addEventListener('change', () => {
    store.setSetting('hint', showToggle.checked ? 'always' : 'miss');
    if (showToggle.checked) { s.peekWord = true; s.peekWindow = true; }
    render();
  });

  // ---- layout ----
  const guideRow = el('div', { class: 'kguide', 'aria-label': 'guide' });
  const chips = el('div', { class: 'kchips' });
  const stream = el('div', { class: 'kstream', tabindex: '0', 'aria-label': 'word stream' });
  const status = el('div', { class: 'kstatus', 'aria-live': 'polite' });
  const metrics = el('div', { class: 'stats kmetrics' });
  const progressBar = el('span');
  const boardHost = el('div');
  const blind = el('div', { class: 'kblind' }, el('span', {}, 'From your head now…'));
  const boardWrap = el('div', { class: 'kboard' }, el('div', { class: 'boardbox__scroll kboard__scroll' }, boardHost, blind));
  const board = createBoard(boardHost);
  const weakRow = el('div', { class: 'boardbox__hint kweak' });

  app.append(
    guideRow, chips,
    el('div', { class: 'boardbox kstreambox' },
      el('div', { class: 'progressbar' }, progressBar), metrics, weakRow,
      stream, status, boardWrap,
      el('div', { class: 'row row--center' }, el('label', { class: 'boardbox__hint', for: SHOW_ID }, showToggle, ' always show the keys')),
      peekNote),
    el('p', { class: 'boardbox__hint' },
      'Hold the keys of a stroke together on your keyboard (Plover QWERTY: ', el('code', {}, 'QWER ASDF'), ' left hand, ',
      el('code', {}, 'C V'), ' and ', el('code', {}, 'N M'), ' thumbs, ', el('code', {}, 'T'), ' star, ', el('code', {}, 'UIOP JKL;'),
      ' right hand, ', el('code', {}, 'Q'), ' join key), then let go. Typing a longer word needs the join key, so it unlocks last.'));

  const save = () => store.saveKeys(p);
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
    s.peekWord = showToggle.checked;
  }
  const target = () => s.word?.candidates[0]?.[s.word.strokeIdx] ?? 0;

  function begin() {
    s.live = true;
    s.queue = [];
    s.done = [];
    s.sinceLesson = 0;
    s.peekWindow = showToggle.checked;
    s.message = '';
    refill();
    startWord();
    render();
  }
  function stop() {
    s.live = false;
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
      say('', '');
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
    say('Not that one. The stroke is under the word.', 'bad');
    if (showAnswer()) board.diff(new Set(P.keysOf(target())), new Set(P.keysOf(used)));
    render(false);
    return undefined;
  }

  function wordDone() {
    const w = s.word;
    P.recordWord(p, { ms: w.ms, strokes: w.strokeIdx, misses: w.misses, masks: w.doneStrokes, assisted: s.peekWord });
    s.done.push({ text: w.text, strokes: w.doneStrokes, missed: w.misses > 0 });
    s.done.splice(0, Math.max(0, s.done.length - DONE_KEEP));
    s.queue.shift();
    // the numbers behind this word, taken before an unlock clears the judging window
    const stats = P.memoryStats(p);
    let toast = false;
    const was = p.guide;
    if (P.unlockIfReady(p)) {
      toast = true;
      const name = P.KEY_ORDER[p.unlocked - 1];
      say(p.unlocked >= P.KEY_ORDER.length ? 'Every key is unlocked. Fade the guide, then show it from memory.' : `Unlocked ${sound(name)}${name === '+' ? ' (the join key)' : ''}. The guide is back: fade it again to unlock the next.`, 'good');
      s.queue = [];
    } else if (P.completeIfReady(p)) {
      toast = true;
      say('You know every key. That is the whole keyboard.', 'good');
      confetti();
    }
    const change = P.adaptGuide(p);
    if (change && !toast) {
      if (change === 'fade') say(was === P.GUIDE.FULL ? 'Hints off. The diagram stays for now.' : 'Diagram off: from your head now.', 'good');
      else say(was === P.GUIDE.MEMORY ? 'The diagram is back for a bit.' : 'Hints are back for a bit.', '');
    }
    s.sinceLesson += 1;
    if (s.sinceLesson >= P.WINDOW) {
      s.sinceLesson = 0;
      P.recordLesson(p, stats);
      const best = toast ? null : P.recordBestIfFaster(p, stats, { assisted: s.peekWindow });
      if (best) say(`New best: ${best} wpm!`, 'good');
      s.peekWindow = showToggle.checked; // the next window starts clean (or peeked, if it is still on)
    }
    save();
    refill();
    startWord();
    render();
  }

  const say = (m, kind) => { s.message = m; s.kind = kind; };

  // ---- drawing ----
  const revealed = () => Boolean(s.word && s.word.wrongChords >= P.LABEL_REVEAL_MISSES);
  const showDiagram = () => p.guide >= P.GUIDE.DIAGRAM || showToggle.checked || revealed();
  const showHints = () => p.guide === P.GUIDE.FULL || showToggle.checked;
  // the diagram always shows what you press; it only shows what you SHOULD press with the full guide
  const showAnswer = () => p.guide === P.GUIDE.FULL || showToggle.checked || revealed();

  function paintBoard() {
    boardWrap.classList.toggle('is-blind', !showDiagram());
    if (!s.live || !s.word || !showDiagram()) { board.clear(); return; }
    // the target lights only when the answer is meant to be given away; held keys always show
    if (showAnswer()) board.show(new Set(P.keysOf(target())));
    else board.clear();
    board.press(s.held);
  }

  function renderTop() {
    guideRow.replaceChildren(el('span', { class: 'kguide__label' }, 'Guide'), ...GUIDES.map(([g, name]) => el('span', {
      class: `kguide__step${p.guide === g ? ' is-on' : ''}${p.guide < g ? ' is-gone' : ''}`,
    }, name)), ...(p.done ? [el('span', { class: 'kguide__done' }, 'All keys learned ✓')] : []));
    chips.replaceChildren(...P.KEY_ORDER.map((k, i) => el('span', {
      class: `kchip kchip--${GROUP_OF(k)}${i < p.unlocked ? '' : ' is-locked'}${i === p.unlocked - 1 && p.unlocked > P.START_KEYS ? ' is-new' : ''}`,
      title: i < p.unlocked ? `${sound(k)}: QWERTY ${HINT[k]}` : 'locked',
    }, sound(k), showHints() && i < p.unlocked ? el('small', {}, HINT[k]) : null)));
  }

  function renderStream() {
    const rows = [];
    const cell = (word, cls, strokes, current) => {
      const label = el('span', { class: 'kcell__strokes' });
      strokes.forEach((m, i) => {
        const now = current && i === s.word.strokeIdx;
        const group = el('span', { class: `kgroup${now ? ' is-now' : ''}${current && i < s.word.strokeIdx ? ' is-done' : ''}` }, strokeElOf(new Set(P.keysOf(m)), now ? s.held : null));
        if (now && showHints()) group.append(el('small', { class: 'kcell__hint' }, hints(m)));
        label.append(group);
      });
      return el('div', { class: `kcell ${cls}` }, el('span', { class: 'kcell__word' }, word), label);
    };
    for (const d of s.done) rows.push(cell(d.text, `is-done${d.missed ? ' is-missed' : ''}`, d.strokes, false));
    s.queue.forEach((q, i) => {
      if (i === 0 && s.word) rows.push(cell(q.text, 'is-current', s.word.candidates[0], true));
      else rows.push(cell(q.text, 'is-next', P.displayVariants(q, mask())[0] ?? [], false));
    });
    stream.replaceChildren(...(s.live ? rows : [el('p', { class: 'boardbox__hint' }, 'The words start as soon as this is on screen.')]));
    status.textContent = s.message || ' ';
    status.className = `kstatus${s.kind ? ` kstatus--${s.kind}` : ''}`;
  }

  function renderMetrics() {
    const st = P.memoryStats(p);
    const need = P.requiredAccuracy(st.wpm);
    const wpmOk = st.count && st.wpm >= P.MIN_WPM;
    const accOk = st.count >= P.MIN_WORDS && st.accuracy >= need;
    metrics.replaceChildren(
      el('span', {}, `${Math.min(st.count, P.WINDOW)} words judged from memory`),
      el('span', { class: wpmOk ? 'is-good' : '' }, `${Math.round(st.wpm)} wpm (${P.MIN_WPM}+ to unlock; big words count for more)`),
      el('span', { class: accOk ? 'is-good' : '' }, Number.isFinite(need) ? `${Math.round(st.accuracy * 100)}% clean (${Math.ceil(need * 100)}% needed at this speed)` : `${Math.round(st.accuracy * 100)}% clean`),
      el('span', {}, `${p.totalTyped} words typed in all`),
      ...(p.best ? [el('span', {}, `best ${p.best} wpm`)] : []));
    progressBar.style.width = `${Math.min(1, st.count / P.MIN_WORDS) * 100}%`;
    const weak = P.weakChords(p, 3).filter((c) => c.accuracy < 0.95);
    weakRow.textContent = weak.length ? `Needs work: ${weak.map((c) => `${c.id.split('|').map(sound).join('')} (${Math.round(c.accuracy * 100)}%)`).join(', ')}` : '';
  }

  function render(rePaint = true) {
    renderTop();
    renderStream();
    renderMetrics();
    peekNote.textContent = s.peekWindow ? 'Keys were shown in this stretch of words, so it will not count toward your best, and the words typed with them shown do not count toward unlocks.' : '';
    if (rePaint) paintBoard();
    else boardWrap.classList.toggle('is-blind', !showDiagram());
  }

  render();
  // no Start button: the words begin when this becomes the thing being looked at, and it only takes
  // keys while it is (see input.js), so other boards and drills on the page are never fed by it
  let began = false;
  const region = whenActive(stream.parentElement, {
    onStroke,
    onHeld,
    onActivate: () => {
      if (!began) { began = true; begin(); } else if (s.word) s.word.lastEnd = performance.now(); // time spent away is not typing time
    },
    onDeactivate: () => { s.held = new Set(); paintBoard(); renderStream(); },
  });
  return () => { region.dispose(); stop(); };
}

// A drill: a prompt, a chord to strike, a hint after a miss, and a score at the end.

import { boardLegend, createBoard } from './board.js';
import { el, shuffle } from './dom.js';
import { listen } from './input.js';
import { formatStroke, parseOutline, sameKeys } from './steno.js';
import { store } from './store.js';

/**
 * items: [[prompt, outline, ...alternativeOutlines]]
 * opts:  id (progress key, omit to not save), pass (percent), repeat, limit, onDone(result)
 */
export function createDrill(host, rawItems, opts = {}) {
  // an item is [prompt, outline, ...alternatives] or {p: prompt, o: [outlines], w: the word it must be}
  const items = rawItems.map((it) => (Array.isArray(it) ? it : [it.p, ...it.o]));
  const { id = null, pass = 90, repeat = 1, limit = 0, onDone = () => {}, title = '', deferred = false } = opts;
  let stopListening = () => {};
  let queue = [];
  let at = 0;
  let cands = [];
  let strokeAt = 0;
  let itemMissed = false;
  let firstTry = 0;
  let started = 0;
  let strokes = 0;
  let misses = [];
  let held = new Set();
  let live = false;

  const board = el('div');
  const label = el('span', { class: 'boardlabel boardbox__label' });
  const hint = el('span', { class: 'boardbox__hint' });
  const prompt = el('div', { class: 'prompt', 'aria-live': 'polite' });
  const answer = el('div', { class: 'answer' });
  const bar = el('span');
  const stats = el('div', { class: 'stats' });
  const enter = el('button', { class: 'btn', type: 'button', onclick: () => submit(held.size ? held : manual) }, 'Enter stroke');
  const clearBtn = el('button', { class: 'btn', type: 'button', onclick: () => setManual(new Set()) }, 'Clear');
  const showToggle = el('input', { type: 'checkbox', id: `show-${Math.random().toString(36).slice(2)}` });
  showToggle.checked = store.settings.hint === 'always';
  showToggle.addEventListener('change', () => {
    store.setSetting('hint', showToggle.checked ? 'always' : 'miss');
    showTarget();
  });

  let manual = new Set();
  const boardCtl = createBoard(board, { onToggle: (k) => {
    const next = new Set(manual);
    if (next.has(k)) next.delete(k);
    else next.add(k);
    setManual(next);
  } });
  function setManual(keys) {
    manual = keys;
    boardCtl.press(manual);
  }

  const root = el('div', { class: 'drillbox' },
    title ? el('div', { class: 'try__title' }, title) : null,
    el('div', { class: 'boardbox__head' }, label, hint),
    el('div', { class: 'boardbox__scroll' }, board),
    boardLegend(),
    prompt, answer,
    el('div', { class: 'progressbar' }, bar),
    stats,
    el('div', { class: 'row row--center', style: 'margin-top:.6rem' },
      enter, clearBtn,
      el('label', { class: 'boardbox__hint' }, showToggle, ' always show the keys'),
    ),
    el('p', { class: 'boardbox__hint', style: 'text-align:center;margin:.5rem auto 0' },
      'Hold the keys of a stroke together (Plover QWERTY: ', el('code', {}, 'QWER'), ' ', el('code', {}, 'ASDF'), ', ',
      el('code', {}, 'CV'), ' ', el('code', {}, 'TG'), ' ', el('code', {}, 'NM'), ' ', el('code', {}, 'UIOP'), ' ', el('code', {}, 'JKL;'),
      ', join key = Q), then let go. Or click keys and press Enter.'),
  );
  host.append(root);

  function pick() {
    let all = [];
    for (let r = 0; r < repeat; r += 1) all = all.concat(shuffle(items));
    if (limit) all = all.slice(0, limit);
    return all;
  }

  function begin() {
    queue = pick();
    at = 0;
    firstTry = 0;
    strokes = 0;
    misses = [];
    started = 0;
    live = true;
    for (const other of running) if (other !== api) other.stop();
    stopListening();
    stopListening = listen(submit, (keys) => {
      held = keys;
      boardCtl.press(keys);
    });
    root.querySelectorAll('.result').forEach((n) => n.remove());
    nextItem(true);
  }

  function itemOutlines() {
    const [, ...outs] = queue[at];
    return outs.map((o) => ({ text: o, strokes: parseOutline(o) })).filter((o) => o.strokes);
  }

  function nextItem(first = false) {
    if (!first) at += 1;
    if (at >= queue.length) return finish();
    cands = itemOutlines();
    strokeAt = 0;
    itemMissed = false;
    prompt.replaceChildren(queue[at][0]);
    answer.textContent = '';
    answer.className = 'answer';
    setManual(new Set());
    label.textContent = '';
    hint.textContent = '';
    showTarget();
    update();
  }

  function showTarget() {
    if (!live || !cands.length) return;
    const want = cands[0].strokes[strokeAt];
    if (showToggle.checked || itemMissed) {
      boardCtl.show(want);
      label.textContent = cands[0].text.split('/').map((s, i) => (i === strokeAt ? `[${s}]` : s)).join(' / ');
      hint.textContent = cands[0].strokes.length > 1 ? `stroke ${strokeAt + 1} of ${cands[0].strokes.length}` : '';
    } else {
      boardCtl.clear();
      label.textContent = '';
      hint.textContent = '';
    }
  }

  function update() {
    bar.style.width = `${(at / queue.length) * 100}%`;
    const mins = started ? (Date.now() - started) / 60000 : 0;
    const wpm = mins > 0.01 ? Math.round(at / mins) : 0;
    const done = at;
    const acc = done ? Math.round((firstTry / done) * 100) : 100;
    stats.replaceChildren(
      el('span', {}, `${done} / ${queue.length}`),
      el('span', {}, `${acc}% first try`),
      el('span', {}, `${wpm} words/min`),
    );
  }

  function submit(keys) {
    if (!live || !cands.length || keys.size === 0) return;
    if (!started) started = Date.now();
    strokes += 1;
    const got = new Set(keys);
    const matching = cands.filter((c) => c.strokes[strokeAt] && sameKeys(c.strokes[strokeAt], got));
    setManual(new Set());
    if (matching.length) {
      cands = matching;
      strokeAt += 1;
      const done = cands.find((c) => c.strokes.length === strokeAt);
      if (done) {
        if (!itemMissed) firstTry += 1;
        else store.clearMiss(queue[at][0]);
        answer.textContent = `${done.text}  ✓`;
        answer.className = 'answer is-good';
        nextItem();
      } else {
        showTarget();
      }
      return;
    }
    // a miss: show what was missing and what was extra, then start the word over
    const want = cands[0].strokes[strokeAt] ?? cands[0].strokes[0];
    boardCtl.diff(want, got);
    if (!itemMissed) {
      itemMissed = true;
      misses.push({ prompt: queue[at][0], outline: cands[0].text, got: formatStroke(got) });
      store.noteMiss(queue[at][0]);
    }
    answer.textContent = `you struck ${formatStroke(got)}`;
    answer.className = 'answer is-bad';
    label.textContent = cands[0].text;
    hint.textContent = 'try again';
    strokeAt = 0;
    cands = itemOutlines();
  }

  function finish() {
    live = false;
    stopListening();
    const total = queue.length;
    const accuracy = Math.round((firstTry / total) * 100);
    const mins = Math.max((Date.now() - started) / 60000, 0.01);
    const wpm = Math.round(total / mins);
    const passed = accuracy >= pass;
    const saved = id ? store.recordRun(id, { accuracy, wpm, pass }) : null;
    prompt.textContent = '';
    answer.textContent = '';
    boardCtl.clear();
    label.textContent = '';
    hint.textContent = '';
    bar.style.width = '100%';
    const result = el('div', { class: `result result--${passed ? 'pass' : 'fail'}` },
      el('div', { class: 'result__big' }, `${accuracy}%`),
      el('div', {}, passed ? `Passed (${pass}% first try needed). ${wpm} words a minute.`
        : `${pass}% first try needed. ${wpm} words a minute. Run it again.`),
      saved ? el('div', { class: 'boardbox__hint' }, `Best ${saved.best}%, ${saved.runs} run${saved.runs === 1 ? '' : 's'}`) : null,
      misses.length ? el('ul', { class: 'misslist' }, misses.slice(0, 12).map((m) =>
        el('li', {}, el('strong', {}, m.prompt), ' wants ', el('code', {}, m.outline), ', you struck ', el('code', {}, m.got)))) : null,
      el('div', { class: 'row row--center', style: 'margin-top:.7rem' },
        el('button', { class: 'btn btn--primary', type: 'button', onclick: begin }, 'Run again')),
    );
    root.append(result);
    onDone({ accuracy, wpm, passed });
  }

  const api = { stop() { live = false; stopListening(); }, restart: begin };
  running.add(api);
  if (deferred) {
    // wait for a click, so the keyboard is not captured while someone is just reading the page
    const go = el('div', { class: 'row row--center', style: 'margin:.4rem 0' },
      el('button', { class: 'btn btn--primary', type: 'button', onclick: () => { go.remove(); begin(); } }, 'Start drill'));
    prompt.before(go);
  } else {
    begin();
  }
  return api;
}

const running = new Set();

/** Stop every drill still listening to the keyboard (called when the page changes). */
export function stopAllDrills() {
  for (const d of running) d.stop();
  running.clear();
}

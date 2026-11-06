// A drill: a prompt, a chord to strike, a hint after a miss, and a score at the end.

import { boardLegend, createBoard } from './board.js';
import { el, shuffle } from './dom.js';
import { outlineEl } from './outline.js';
import { loadEngine } from './engine.js';
import { whenActive } from './input.js';
import { formatStroke, parseOutline, sameKeys } from './steno.js';
import { store } from './store.js';

/**
 * items: [[prompt, outline, ...alternativeOutlines]]
 * opts:  id (progress key, omit to not save), pass (percent), repeat, limit, onDone(result)
 */
export function createDrill(host, rawItems, opts = {}) {
  // an item is [prompt, outline, ...alternatives] or {p: prompt, o: [outlines], w: the word it must be};
  // `.word` is what the strokes must type
  const items = rawItems.map((it) => (Array.isArray(it) ? Object.assign([...it], { word: it[0] })
    : Object.assign([it.p, ...it.o], { word: it.w ?? it.p })));
  // notation: the prompt is a stroke drawn as written (`PHAP`) and the word it makes is revealed on a hit
  const { id = null, pass = 90, repeat = 1, limit = 0, onDone = () => {}, title = '', notation = false } = opts;
  const wordOf = new Map(rawItems.filter((it) => !Array.isArray(it) && it.w).map((it) => [it.p, it.w]));
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
  let engine = null; // the stroke-to-text engine, once loaded: any way of typing the word passes
  let typed = []; // the strokes struck so far for this word, when the engine is judging
  let note = '';

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
    note = '';
    region.enable(true);
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
    typed = [];
    itemMissed = false;
    prompt.replaceChildren(notation ? outlineEl(queue[at][0]) : queue[at][0]);
    answer.textContent = note;
    answer.className = note ? 'answer is-good' : 'answer';
    note = '';
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
      // the stroke due keeps full strength; the rest of the outline is dimmed (no brackets)
      const parts = cands[0].text.split('/');
      label.replaceChildren(...parts.flatMap((p, i) => [
        ...(i ? [el('span', { class: 'pop__sep' }, '/')] : []),
        el('span', { class: `pop__stroke${parts.length > 1 && i !== strokeAt ? ' is-dim' : ''}` }, p),
      ]));
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
    if (engineJudges()) {
      setManual(new Set());
      if (judgeWithEngine(got)) return;
    }
    const matching = cands.filter((c) => c.strokes[strokeAt] && sameKeys(c.strokes[strokeAt], got));
    setManual(new Set());
    if (matching.length) {
      cands = matching;
      strokeAt += 1;
      const done = cands.find((c) => c.strokes.length === strokeAt);
      if (done) {
        if (!itemMissed) firstTry += 1;
        else store.clearMiss(queue[at][0]);
        answer.textContent = notation && wordOf.has(queue[at][0]) ? `= “${wordOf.get(queue[at][0])}”  ✓` : `${done.text}  ✓`;
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

  // The engine judges an item whose listed outline really types its word (a sound or key drill's
  // "outline" does not, so those stay exact).
  function engineJudges() {
    if (!engine || !cands.length) return false;
    const item = queue[at];
    item.engine ??= engine.accepts(item.word, cands[0].strokes);
    return item.engine;
  }

  // true when the stroke was right or is on the way; false is a miss, and falls to the miss code
  function judgeWithEngine(got) {
    const word = queue[at].word;
    typed.push(got);
    if (engine.accepts(word, typed)) {
      const wrote = typed.map(formatStroke).join('/');
      const listed = itemOutlines();
      if (!listed.some((c) => c.strokes.map(formatStroke).join('/') === wrote)) {
        note = `also fine: you wrote ${wrote}; fastest is ${listed[0].text}`;
      }
      if (!itemMissed) firstTry += 1;
      else store.clearMiss(queue[at][0]);
      nextItem();
      return true;
    }
    if (typed.length < 8 && engine.prefixOf(word, typed)) {
      const on = itemOutlines().filter((c) => typed.every((t, i) => c.strokes[i] && sameKeys(c.strokes[i], t)));
      if (on.length) cands = [...on, ...cands.filter((c) => !on.includes(c))];
      strokeAt = on.length ? typed.length : 0;
      answer.textContent = `so far: ${engine.translate(typed).text}`;
      answer.className = 'answer';
      showTarget();
      return true;
    }
    strokeAt = Math.min(typed.length - 1, cands[0].strokes.length - 1);
    typed = [];
    return false;
  }

  function finish() {
    live = false;
    region.enable(false); // a finished drill takes no keys, so it does not hold them from anything else
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

  // no Start button and no click: the drill listens whenever it is the one being looked at (see
  // input.js), so any number of them can share a page and only that one takes keys
  let warmed = false;
  const region = whenActive(root, {
    onStroke: submit,
    onHeld: (keys) => { held = keys; boardCtl.press(keys); },
    onActivate: () => {
      if (warmed) return;
      warmed = true;
      loadEngine().then((e) => { engine = e; }).catch(() => {}); // without it a drill wants the listed outlines
    },
  });
  const api = { stop() { live = false; region.dispose(); }, restart: begin };
  running.add(api);
  begin();
  return api;
}

const running = new Set();

/** Stop every drill still listening to the keyboard (called when the page changes). */
export function stopAllDrills() {
  for (const d of running) d.stop();
  running.clear();
}

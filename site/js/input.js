// Chord capture from an ordinary keyboard, the way Plover's QWERTY mode does it: keys go down
// together, and the stroke is everything pressed by the time the last one comes up.

import { QWERTY } from './steno.js';

const typing = (el) => el && (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.tagName === 'SELECT' || el.isContentEditable);

const stack = [];
// the listener that hears the keys: the newest, except that a `priority` one (the Try it pad, which
// the learner opened on purpose) outranks everything else
const top = () => stack.findLast((m) => m.priority) ?? stack.at(-1);

/**
 * Hands the listener on top the keys a steno machine is holding right now (a Set of key ids; empty
 * when everything is up). The stroke fires when the last key comes up, as it does for a keyboard.
 */
export function feed(ids) {
  top()?.feed(ids);
}

/**
 * Starts listening. `onStroke(Set<keyId>)` fires when a chord is released; `onHeld(Set<keyId>)`
 * fires as keys go down and up, so the board can show what is held. Returns a stop function.
 */
export function listen(onStroke, onHeld = () => {}, { priority = false } = {}) {
  const down = new Set(); // physical key codes held
  const chord = new Set(); // key ids pressed since the chord began
  const ids = () => new Set([...down].map((c) => QWERTY[c]));
  const me = {
    priority,
    feed(held) {
      for (const id of held) chord.add(id);
      onHeld(new Set(held));
      if (held.size === 0 && chord.size > 0) {
        const stroke = new Set(chord);
        chord.clear();
        onStroke(stroke);
      }
    },
  };
  stack.push(me);

  const onDown = (e) => {
    // only the newest listener hears the keys, so the "Try it" pad can sit on top of a drill
    if (top() !== me) return;
    if (e.ctrlKey || e.metaKey || e.altKey || typing(e.target)) return;
    const id = QWERTY[e.code];
    if (!id) return;
    e.preventDefault();
    if (e.repeat) return;
    down.add(e.code);
    chord.add(id);
    onHeld(ids());
  };
  const onUp = (e) => {
    if (!down.has(e.code)) return;
    e.preventDefault();
    down.delete(e.code);
    onHeld(ids());
    if (down.size === 0 && chord.size > 0) {
      const stroke = new Set(chord);
      chord.clear();
      onStroke(stroke);
    }
  };
  const onBlur = () => {
    down.clear();
    chord.clear();
    onHeld(new Set());
  };
  window.addEventListener('keydown', onDown);
  window.addEventListener('keyup', onUp);
  window.addEventListener('blur', onBlur);
  return () => {
    stack.splice(stack.indexOf(me), 1);
    window.removeEventListener('keydown', onDown);
    window.removeEventListener('keyup', onUp);
    window.removeEventListener('blur', onBlur);
  };
}

// ---- who is listening ----
// Anything on a page that takes strokes (a lesson's try-it board, a drill, the key stream) registers
// a region here instead of listening itself. Exactly one region listens at a time: the one with the
// most of itself on screen (it has to be mostly in view), or the one the learner last clicked into.
// So several can share a page and only the one being looked at ever takes keys; the rest sit idle
// until it scrolls away. A region that is not on the page any more is dropped.

const regions = new Set();
const byEl = new Map();
let io = null;
let active = null;
let pinned = null;

const eligible = (r) => r.enabled && r.el.isConnected && (r.ratio >= 0.4 || (r.height > 0 && r.height >= innerHeight * 0.4));

function elect() {
  // drop regions whose element was on the page and has been removed (a new page); one that has not
  // been attached yet (still being built) is left alone
  for (const r of [...regions]) {
    if (r.el.isConnected) r.seen = true;
    else if (r.seen) dispose(r);
  }
  let best = null;
  if (pinned && eligible(pinned)) {
    best = pinned;
  } else {
    pinned = null;
    for (const r of regions) if (eligible(r) && (!best || r.area > best.area)) best = r;
    // no flip-flopping between two regions that are about equally in view
    if (active && eligible(active) && best && best !== active && best.area <= active.area * 1.15) best = active;
  }
  if (best === active) return;
  if (active) {
    active.stopKeys?.();
    active.stopKeys = null;
    active.onHeld(new Set());
    active.onDeactivate();
  }
  active = best;
  if (active) {
    active.stopKeys = listen(active.onStroke, active.onHeld);
    active.onActivate();
  }
}

function dispose(r) {
  regions.delete(r);
  byEl.delete(r.el);
  io?.unobserve(r.el);
  if (pinned === r) pinned = null;
  if (active === r) {
    active.stopKeys?.();
    active.stopKeys = null;
    active = null;
    r.onDeactivate();
  }
}

function setup() {
  if (io || typeof IntersectionObserver === 'undefined') return;
  io = new IntersectionObserver((entries) => {
    for (const e of entries) {
      const r = byEl.get(e.target);
      if (!r) continue;
      r.ratio = e.isIntersecting ? e.intersectionRatio : 0;
      r.height = e.isIntersecting ? e.intersectionRect.height : 0;
      r.area = e.isIntersecting ? e.intersectionRect.width * e.intersectionRect.height : 0;
    }
    elect();
  }, { threshold: Array.from({ length: 21 }, (_, i) => i / 20) });
  // clicking or tabbing into a region makes it the one that listens, for as long as it stays in view
  const pin = (e) => {
    for (const r of regions) {
      if (r.el.contains(e.target)) {
        if (eligible(r)) { pinned = r; elect(); }
        return;
      }
    }
  };
  document.addEventListener('pointerdown', pin, true);
  document.addEventListener('focusin', pin, true);
  window.addEventListener('resize', elect);
}

/**
 * Registers `el` as something that takes strokes. `onStroke(keys)` and `onHeld(keys)` are heard only
 * while this region is the active one; `onActivate()` / `onDeactivate()` tell it when. Returns
 * { enable(on), dispose() }: a region that has nothing to take (a finished drill) is disabled so
 * it does not hold the keys.
 */
export function whenActive(el, { onStroke, onHeld = () => {}, onActivate = () => {}, onDeactivate = () => {} }) {
  setup();
  const r = { el, onStroke, onHeld, onActivate, onDeactivate, enabled: true, seen: false, ratio: 0, height: 0, area: 0, stopKeys: null };
  regions.add(r);
  byEl.set(el, r);
  io?.observe(el);
  return {
    enable(on) { r.enabled = on; elect(); },
    dispose() { dispose(r); elect(); },
  };
}

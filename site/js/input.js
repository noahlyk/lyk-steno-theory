// Chord capture from an ordinary keyboard, the way Plover's QWERTY mode does it: keys go down
// together, and the stroke is everything pressed by the time the last one comes up.

import { QWERTY } from './steno.js';

const typing = (el) => el && (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.tagName === 'SELECT' || el.isContentEditable);

const stack = [];

/**
 * Starts listening. `onStroke(Set<keyId>)` fires when a chord is released; `onHeld(Set<keyId>)`
 * fires as keys go down and up, so the board can show what is held. Returns a stop function.
 */
export function listen(onStroke, onHeld = () => {}) {
  const down = new Set(); // physical key codes held
  const chord = new Set(); // key ids pressed since the chord began
  const ids = () => new Set([...down].map((c) => QWERTY[c]));
  const me = {};
  stack.push(me);

  const onDown = (e) => {
    // only the newest listener hears the keys, so the "Try it" pad can sit on top of a drill
    if (stack.at(-1) !== me) return;
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

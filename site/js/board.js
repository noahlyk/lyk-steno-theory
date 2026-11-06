// The on-screen steno board: a CSS grid of keys that can light up, show a mistake and be clicked.

// [id, column, row, row span]; a 10-column grid with the thumb row under it.
const LAYOUT = [
  ['S-', 1, 1, 2],
  ['T-', 2, 1, 1], ['P-', 3, 1, 1], ['H-', 4, 1, 1],
  ['K-', 2, 2, 1], ['W-', 3, 2, 1], ['R-', 4, 2, 1],
  ['*', 5, 1, 2],
  ['-F', 6, 1, 1], ['-P', 7, 1, 1], ['-L', 8, 1, 1], ['-T', 9, 1, 1], ['-D', 10, 1, 1],
  ['-R', 6, 2, 1], ['-B', 7, 2, 1], ['-G', 8, 2, 1], ['-S', 9, 2, 1], ['-Z', 10, 2, 1],
  ['A', 3, 3, 1], ['O', 4, 3, 1],
  ['+', 5, 3, 1],
  ['E', 6, 3, 1], ['U', 7, 3, 1],
];

const LETTER = (id) => (id === '+' ? '+' : id.replace('-', ''));
const BANK = (id) => (id === '+' ? 'join' : id === '*' || 'AOEU'.includes(id) ? 'vowel' : id.endsWith('-') ? 'left' : 'right');

/**
 * Draws a board into `host`. Returns a controller:
 *   show(keys)        light these keys as the target
 *   press(keys)       mark these as held down (the learner's stroke)
 *   diff(want, got)   mark missing and extra keys after a miss
 *   clear()
 * `onToggle(id)` makes the keys clickable.
 */
export function createBoard(host, { onToggle = null, compact = false } = {}) {
  host.classList.add('board');
  if (compact) host.classList.add('board--compact');
  host.textContent = '';
  const els = new Map();
  for (const [id, col, row, span] of LAYOUT) {
    const el = document.createElement(onToggle ? 'button' : 'div');
    el.className = `key key--${BANK(id)}`;
    el.dataset.key = id;
    el.textContent = LETTER(id);
    el.style.gridColumn = String(col);
    el.style.gridRow = span === 2 ? `${row} / span 2` : String(row);
    if (onToggle) {
      el.type = 'button';
      el.setAttribute('aria-label', id === '+' ? 'join key' : `key ${LETTER(id)}`);
      el.addEventListener('click', () => onToggle(id));
    }
    host.append(el);
    els.set(id, el);
  }
  const reset = () => {
    for (const el of els.values()) el.classList.remove('is-target', 'is-down', 'is-miss', 'is-extra');
  };
  return {
    clear: reset,
    show(keys) {
      reset();
      for (const id of keys ?? []) els.get(id)?.classList.add('is-target');
    },
    press(keys) {
      for (const el of els.values()) el.classList.remove('is-down');
      for (const id of keys ?? []) els.get(id)?.classList.add('is-down');
    },
    diff(want, got) {
      reset();
      for (const id of want) els.get(id)?.classList.add(got.has(id) ? 'is-down' : 'is-miss');
      for (const id of got) if (!want.has(id)) els.get(id)?.classList.add('is-extra');
    },
  };
}

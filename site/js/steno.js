// Steno notation: parse a stroke or outline into keys and back. Pure, no DOM.
//
// A stroke is written in steno order: S T K P W H R | A O * E U | F R P B L G T S D Z.
// lyk puts a join key (`+`) in front of every stroke after a word's first: `WAUBG/+-G`.

export const LEFT = ['S', 'T', 'K', 'P', 'W', 'H', 'R'];
export const VOWELS = ['A', 'O', '*', 'E', 'U'];
export const RIGHT = ['F', 'R', 'P', 'B', 'L', 'G', 'T', 'S', 'D', 'Z'];

// One id per physical key: left keys `S-`, vowels `A`, right keys `-F`, and the join key `+`.
export const KEYS = [
  ...LEFT.map((k) => `${k}-`),
  ...VOWELS,
  ...RIGHT.map((k) => `-${k}`),
  '+',
];

/** A stroke such as `TKOG`, `-Z` or `+-Z` as a Set of key ids; null if it is not a stroke. */
export function parseStroke(text) {
  let rest = text.trim();
  const keys = new Set();
  if (rest.startsWith('+')) {
    keys.add('+');
    rest = rest.slice(1);
  }
  if (rest === '') return keys.size ? keys : null;
  let i = 0;
  let pos = 0; // how far along the left-vowel-right order we are
  const order = [
    ...LEFT.map((k) => ({ ch: k, id: `${k}-`, bank: 0 })),
    ...VOWELS.map((k) => ({ ch: k, id: k, bank: 1 })),
    ...RIGHT.map((k) => ({ ch: k, id: `-${k}`, bank: 2 })),
  ];
  let sawHyphen = false;
  while (i < rest.length) {
    const c = rest[i];
    if (c === '-') {
      if (sawHyphen) return null;
      sawHyphen = true;
      // jump to the right bank
      const first = order.findIndex((o) => o.bank === 2);
      if (pos > first) return null;
      pos = first;
      i += 1;
      continue;
    }
    let found = -1;
    for (let j = pos; j < order.length; j += 1) {
      if (order[j].ch === c) {
        found = j;
        break;
      }
    }
    if (found < 0) return null;
    // `R`, `P`, `T`, `S` exist on both sides: the first match from `pos` is right because the
    // left bank comes first, and a vowel or hyphen has already moved `pos` past it.
    keys.add(order[found].id);
    pos = found + 1;
    i += 1;
  }
  return keys;
}

/** The text of a set of keys in steno order, with the hyphen only where it is needed. */
export function formatStroke(keys) {
  const has = (id) => keys.has(id);
  const left = LEFT.filter((k) => has(`${k}-`)).join('');
  const vow = VOWELS.filter((k) => has(k)).join('');
  const right = RIGHT.filter((k) => has(`-${k}`)).join('');
  const hyphen = vow === '' && right !== '' ? '-' : '';
  const body = left + vow + hyphen + right;
  return (has('+') ? '+' : '') + body;
}

/** Strokes of an outline `A/+B-C` as an array of key Sets; null if any stroke is not valid. */
export function parseOutline(text) {
  const out = [];
  for (const part of text.split('/')) {
    const s = parseStroke(part);
    if (!s) return null;
    out.push(s);
  }
  return out;
}

export function sameKeys(a, b) {
  if (a.size !== b.size) return false;
  for (const k of a) if (!b.has(k)) return false;
  return true;
}

export function keyCount(outline) {
  const o = parseOutline(outline);
  return o ? o.reduce((n, s) => n + [...s].filter((k) => k !== '+').length, 0) : 0;
}

// The Plover QWERTY mapping, by physical key (`KeyboardEvent.code`), so it works on any layout.
// Space is lyk's join key.
export const QWERTY = {
  KeyQ: 'S-', KeyA: 'S-',
  KeyW: 'T-', KeyS: 'K-',
  KeyE: 'P-', KeyD: 'W-',
  KeyR: 'H-', KeyF: 'R-',
  KeyC: 'A', KeyV: 'O',
  KeyT: '*', KeyG: '*', KeyY: '*', KeyH: '*',
  KeyN: 'E', KeyM: 'U',
  KeyU: '-F', KeyJ: '-R',
  KeyI: '-P', KeyK: '-B',
  KeyO: '-L', KeyL: '-G',
  KeyP: '-T', Semicolon: '-S',
  BracketLeft: '-D', Quote: '-Z',
  Space: '+',
};

/** Read-aloud name of a key, for the key chart. */
export function keyLabel(id) {
  if (id === '+') return 'join';
  return id.replace('-', '');
}

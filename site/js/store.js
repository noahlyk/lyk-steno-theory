// Progress and settings, kept in this browser only (localStorage). Everything works without it.

const KEY = 'lyk-lessons-v1';
const KEYS_KEY = 'lyk-keys-v2'; // "Learn the keys" progress, kept apart from lesson progress
const OLD_KEYS_KEY = 'lyk-prestige-v1'; // the two-level version's save, still read so nobody loses their keys
let mem = null;

function load() {
  if (mem) return mem;
  try {
    mem = JSON.parse(localStorage.getItem(KEY) ?? 'null');
  } catch {
    mem = null;
  }
  if (!mem || typeof mem !== 'object') mem = {};
  mem.read ??= {};
  mem.drills ??= {};
  mem.settings ??= { hint: 'miss' };
  mem.missed ??= {};
  return mem;
}

function save() {
  try {
    localStorage.setItem(KEY, JSON.stringify(mem));
  } catch {
    // private window or blocked storage: keep going in memory
  }
}

export const store = {
  get settings() {
    return load().settings;
  },
  setSetting(name, value) {
    load().settings[name] = value;
    save();
  },
  isRead(id) {
    return Boolean(load().read[id]);
  },
  setRead(id, on = true) {
    if (on) load().read[id] = true;
    else delete load().read[id];
    save();
  },
  drill(id) {
    return load().drills[id] ?? null;
  },
  /**
   * Record a finished run; keeps the best accuracy and fastest speed. A run that was `assisted`
   * (the always-show-the-keys option was on at any moment of it) still counts as a run and can pass
   * the lesson, but never sets the best.
   */
  recordRun(id, { accuracy, wpm, pass, assisted = false }) {
    const d = (load().drills[id] ??= { runs: 0, best: 0, wpm: 0, passed: false });
    d.runs += 1;
    if (!assisted) {
      d.best = Math.max(d.best, accuracy);
      d.wpm = Math.max(d.wpm, wpm);
    }
    d.last = accuracy;
    if (accuracy >= pass) d.passed = true;
    save();
    return d;
  },
  noteMiss(prompt) {
    const m = load().missed;
    m[prompt] = (m[prompt] ?? 0) + 1;
    save();
  },
  clearMiss(prompt) {
    delete load().missed[prompt];
    save();
  },
  get missed() {
    return load().missed;
  },
  /** The raw saved "Learn the keys" progress (trainer.js normalises it), or null. */
  loadKeys() {
    try {
      return JSON.parse(localStorage.getItem(KEYS_KEY) ?? localStorage.getItem(OLD_KEYS_KEY) ?? 'null');
    } catch {
      return null;
    }
  },
  saveKeys(progress) {
    try {
      localStorage.setItem(KEYS_KEY, JSON.stringify(progress));
    } catch {
      // private window or blocked storage: keep going in memory
    }
  },
  reset() {
    mem = { read: {}, drills: {}, settings: load().settings, missed: {} };
    save();
    try {
      localStorage.removeItem(KEYS_KEY);
      localStorage.removeItem(OLD_KEYS_KEY);
    } catch {
      // nothing to clear
    }
  },
};

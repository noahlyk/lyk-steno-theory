// Progress and settings, kept in this browser only (localStorage). Everything works without it.

const KEY = 'lyk-lessons-v1';
const PRESTIGE_KEY = 'lyk-prestige-v1'; // "Learn the keys" progress, kept apart from lesson progress
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
  /** Record a finished run; keeps the best accuracy and fastest speed. */
  recordRun(id, { accuracy, wpm, pass }) {
    const d = (load().drills[id] ??= { runs: 0, best: 0, wpm: 0, passed: false });
    d.runs += 1;
    d.best = Math.max(d.best, accuracy);
    d.wpm = Math.max(d.wpm, wpm);
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
  /** The raw saved "Learn the keys" progress (prestige.js normalises it), or null. */
  loadPrestige() {
    try {
      return JSON.parse(localStorage.getItem(PRESTIGE_KEY) ?? 'null');
    } catch {
      return null;
    }
  },
  savePrestige(progress) {
    try {
      localStorage.setItem(PRESTIGE_KEY, JSON.stringify(progress));
    } catch {
      // private window or blocked storage: keep going in memory
    }
  },
  reset() {
    mem = { read: {}, drills: {}, settings: load().settings, missed: {} };
    save();
    try {
      localStorage.removeItem(PRESTIGE_KEY);
    } catch {
      // nothing to clear
    }
  },
};

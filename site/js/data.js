// Loads the site's data once: the lesson index, a lesson, the dictionary for lookup, the key chart.

const cache = new Map();

async function getJson(path) {
  if (!cache.has(path)) {
    cache.set(path, fetch(path).then((r) => {
      if (!r.ok) throw new Error(`${path}: ${r.status}`);
      return r.json();
    }));
  }
  return cache.get(path);
}

export const index = () => getJson('lessons/index.json');
export const lesson = (id) => getJson(`lessons/${id}.json`);
export const chart = () => getJson('data/chart.json');
export const grammar = () => getJson('data/grammar.json');
export const refIndex = () => getJson('reference/index.json');
export const refPage = (id) => getJson(`reference/${id}.json`);

let dict = null;
/** { words: Map word -> [outline...], reverse: Map outline -> [word...] } */
export async function dictionary() {
  if (dict) return dict;
  dict = (async () => {
    const { words } = await getJson('data/words.json');
    // outlines on the number bar (`#KOPL`) have no key on the board, so they are not offered
    const map = new Map();
    for (const [w, outs] of Object.entries(words)) {
      const typable = outs.filter((o) => !o.includes('#'));
      if (typable.length) map.set(w, typable);
    }
    const reverse = new Map();
    for (const [w, outs] of map) {
      for (const o of outs) {
        if (!reverse.has(o)) reverse.set(o, []);
        reverse.get(o).push(w);
      }
    }
    return { words: map, reverse };
  })();
  return dict;
}

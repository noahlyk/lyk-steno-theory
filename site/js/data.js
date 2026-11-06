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

let dict = null;
/** { words: Map word -> [outline...], reverse: Map outline -> [word...] } */
export async function dictionary() {
  if (dict) return dict;
  dict = (async () => {
    const { words } = await getJson('data/words.json');
    const map = new Map(Object.entries(words));
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

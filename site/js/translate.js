// Strokes to words. The one place the site turns what was typed into text; `translate` reads the
// dictionary's outlines (data/words.json). A word is a stroke plus every `+` stroke after it, so
// strokes are grouped at each stroke without the join key and each group is looked up whole.

import { dictionary } from './data.js';
import { formatStroke } from './steno.js';

/**
 * strokes: an array of Sets of key ids, in the order they were struck.
 * Returns [{ outline, word }], one per word; `word` is null when the dictionary has no such outline.
 */
export async function translate(strokes) {
  const { reverse } = await dictionary();
  const groups = [];
  for (const s of strokes) {
    const text = formatStroke(s);
    if (s.has('+') && groups.length) groups.at(-1).push(text);
    else groups.push([text]);
  }
  return groups.map((g) => {
    const outline = g.join('/');
    return { outline, word: reverse.get(outline)?.[0] ?? null };
  });
}

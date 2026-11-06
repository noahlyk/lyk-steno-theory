// Which text deserves a hover board? Pure (no DOM), so `check.mjs` can test it. Two rules, both
// conservative, both needing the dictionary (`dictionary()` in data.js: { words, reverse }):
//
//   outline  all capitals/stars/`+`/`-`/`/`, every stroke parses on the board with at least one key,
//            AND (it carries a steno mark `/ + - *`, OR it is a real dictionary outline of 3+
//            characters). So `-D`, `ED/+-D`, `S-`, `KAT`, `TKPWAEUPL` match; `AND`, `NOTE`, `A`,
//            `A//B`, `E-MAIL`, `--` and `+` do not.
//   word     lower-case letters (and `'` `-`), 2+ characters, and a key of the dictionary with a
//            typable outline: it gets its first (fastest) outline, only that one.
//
// A comma/semicolon/slash-separated list matches when every part does (`ends, girls`).

import { parseOutline } from './steno.js';

const OUTLINE_SHAPE = /^[+A-Z*/-]+$/;
const MARK = /[/+*-]/;
const WORD_SHAPE = /^[a-z][a-z'’-]*$/;
// words that are table furniture, not examples: a cell reading just "no" or "none" is an answer
export const FURNITURE = new Set(['yes', 'no', 'none', 'never', 'always', 'both', 'any', 'all', 'sometimes', 'maybe',
  'left', 'right', 'rule', 'note', 'name', 'keys', 'key', 'word', 'words', 'example', 'examples', 'stroke', 'strokes',
  'outline', 'outlines', 'sound', 'sounds', 'type', 'kind', 'form', 'when', 'why', 'how', 'what', 'same', 'other']);
const SEPARATOR =/(\s*[,;]\s*|\s+\/\s+)/;

/** The outline to show for a piece of text, or null. */
export function classify(text, dict, furniture = false) {
  const t = text.trim();
  if (t.length < 2 && !/^[*-]$/.test(t)) return null;
  if (OUTLINE_SHAPE.test(t)) {
    const strokes = parseOutline(t);
    if (!strokes || strokes.some((s) => ![...s].some((k) => k !== '+'))) return null;
    if (MARK.test(t) || (t.length >= 3 && dict.reverse.has(t))) return t;
    return null;
  }
  if (WORD_SHAPE.test(t) && t.length >= 2 && !(furniture && FURNITURE.has(t))) {
    const outs = dict.words.get(t);
    return outs?.[0] ?? null;
  }
  return null;
}

/**
 * Splits text into [{ text, outline }] pieces (outline null for separators), or null unless every
 * non-separator part matches. One part is the usual case; `ends, girls` is two.
 */
export function classifyList(text, dict, furniture = false) {
  const lead = text.match(/^\s*/)[0];
  const trail = text.match(/\s*$/)[0];
  const body = text.slice(lead.length, text.length - trail.length);
  if (!body) return null;
  const pieces = [];
  if (lead) pieces.push({ text: lead, outline: null });
  // split with a capture group alternates part, separator, part...
  const bits = body.split(SEPARATOR);
  for (let i = 0; i < bits.length; i += 1) {
    if (i % 2) { pieces.push({ text: bits[i], outline: null }); continue; }
    const outline = classify(bits[i], dict, furniture);
    if (!outline) return null;
    pieces.push({ text: bits[i], outline });
  }
  if (trail) pieces.push({ text: trail, outline: null });
  return pieces.some((p) => p.outline) ? pieces : null;
}

// Makes every word and outline on the page hoverable. One enhancer, applied after any content
// renders (and again to whatever is added later): it finds `code` spans, `.word` spans and the
// text-only cells/list items, asks hovermatch.js whether each is a dictionary outline or word, and
// gives the ones that are a `data-outline` (so popover.js shows their board) and a faint dotted
// underline (`.hov`). Nothing is rewritten in lessons/reference JSON, so new content is covered.
//
// Not touched: links, buttons, existing chips and anything with data-outline, the drills and the
// key stream (their prompts are the answers), quiz options, `pre`, form fields, `[data-nohover]`.
// Also not touched: words that only NAME something in the doc rather than show a word being typed.
// Three rules, all central: (1) a table column whose header names things (NAME_HEADS: "Step",
// "Command", "Mark"...) is labels all the way down, and one whose header names spelling patterns
// (PATTERN_HEADS: "When", "Sound") skips its code and plain cells but keeps `{{word}}` examples;
// (2) a code span directly followed by a label noun (`brevity` table, `shell` step) is a name;
// (3) the explicit opt-out in the JSON content: `!name` in backticks, `{{!name}}`, or a table
// header starting with `!` (that column, see lesson.js). Anything ambiguous is opted out by hand.
// Data: the word list (data.js `dictionary()`, which Lookup already needs) loads once, when idle,
// after a page that has candidates renders. The engine (define/translate) still loads on first hover.

import { dictionary } from './data.js';
import { classify, classifyList } from './hovermatch.js';

const SKIP = 'a, button, [data-outline], .pop, .drillbox, .kstreambox, .quiz__opts, input, select, textarea, pre, svg, script, style, [data-nohover]';
const CELLS = 'td, li, .hit__word'; // never th: those are labels ("keys", "example")
const NAME_HEADS = new Set(['step', 'steps', 'command', 'commands', 'name', 'names', 'stage', 'pass', 'mark', 'kind',
  'table', 'field', 'setting', 'file', 'files', 'what is in it']);
const PATTERN_HEADS = new Set(['when', 'sound', 'how they differ']);
const NAMED_NOUN = /^\s+(step|steps|command|commands|table|pass|passes|stage|file|files|flag|option|mode|function|field|column|source|ladder)\b/i;
const hasElements = (node) => [...node.childNodes].some((c) => c.nodeType === 1);

// the header text of the column a cell sits in, lower-cased, or '' (th with data-nohover: a column opted out)
function columnHead(cell) {
  const td = cell.closest('td');
  const table = td?.closest('table');
  if (!table) return '';
  const th = table.tHead?.rows[0]?.cells[[...td.parentElement.children].indexOf(td)];
  if (!th) return '';
  return th.hasAttribute('data-nohover') ? 'data-nohover' : th.textContent.trim().toLowerCase().replace(/[.:]$/, '');
}

/** Is this node a label (a step name, a command, a spelling pattern) rather than an example? */
function isLabel(node, kind) {
  const head = columnHead(node);
  if (head === 'data-nohover' || NAME_HEADS.has(head)) return true;
  if (PATTERN_HEADS.has(head) && kind !== 'word') return true;
  return kind === 'code' && /^[a-z]/.test(node.textContent) && NAMED_NOUN.test(node.nextSibling?.nodeType === 3 ? node.nextSibling.data : '');
}

function tag(node, outline) {
  node.dataset.outline = outline;
  node.classList.add('hov');
  node.tabIndex = 0;
}

function enhanceLeaf(node, dict) {
  // a `.word` right before its own chip (`[[word|OUTLINE]]`) shows that chip's outline; a
  // phrasing-grammar chip is not a dictionary entry, so its word claims nothing
  const next = node.nextElementSibling;
  if (node.classList.contains('word') && next?.matches('.stroke')) {
    if (!next.dataset.outline || next.classList.contains('stroke--grammar')) return false;
    tag(node, next.dataset.outline);
    return true;
  }
  const outline = classify(node.textContent, dict);
  if (!outline) return false;
  tag(node, outline);
  return true;
}

// a cell that is nothing but a word, an outline or a comma-separated list of them
function enhanceCell(cell, dict) {
  if (cell.childNodes.length !== 1 || cell.firstChild.nodeType !== 3) return 0;
  const pieces = classifyList(cell.firstChild.data, dict, true);
  if (!pieces) return 0;
  let n = 0;
  cell.firstChild.replaceWith(...pieces.map((p) => {
    if (!p.outline) return document.createTextNode(p.text);
    const s = document.createElement('span');
    s.textContent = p.text;
    tag(s, p.outline);
    n += 1;
    return s;
  }));
  return n;
}

/** Enhances `scope` (an element) and returns how many items became hoverable. */
export function enhance(scope, dict) {
  let n = 0;
  const inside = (node) => !node.closest(SKIP);
  const leaves = [...scope.querySelectorAll('code, .word')];
  if (scope.matches('code, .word')) leaves.push(scope);
  for (const node of leaves) {
    const kind = node.matches('code') ? 'code' : 'word';
    if (inside(node) && !hasElements(node) && !isLabel(node, kind) && enhanceLeaf(node, dict)) n += 1;
  }
  const cells = [...scope.querySelectorAll(CELLS)];
  if (scope.matches(CELLS)) cells.push(scope);
  for (const cell of cells) if (inside(cell) && !isLabel(cell, 'cell')) n += enhanceCell(cell, dict);
  return n;
}

export const hoverableCount = (root = document) => root.querySelectorAll('.hov').length;

/** Watches `app` (and enhances what is in it now) so every page, and anything added later, is covered. */
export function initHover(app) {
  let pending = [];
  let queued = false;
  const run = async () => {
    queued = false;
    const batch = pending;
    pending = [];
    let dict;
    try { dict = await dictionary(); } catch { return; }
    for (const node of batch) if (node.isConnected) enhance(node, dict);
    app.dataset.hovered = String(Number(app.dataset.hovered ?? 0) + 1);
  };
  const queue = (node) => {
    if (node.nodeType !== 1 || node.classList.contains('hov')) return;
    pending.push(node);
    if (queued) return;
    queued = true;
    (window.requestIdleCallback ?? ((f) => setTimeout(f, 30)))(run, { timeout: 400 });
  };
  new MutationObserver((records) => {
    for (const r of records) for (const added of r.addedNodes) queue(added);
  }).observe(app, { childList: true, subtree: true });
  queue(app);
}

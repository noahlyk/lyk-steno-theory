// The phrasing grammar's composer: pick a starter, an auxiliary, "not" and a verb, see the one
// stroke and the phrase it writes. The strokes are not computed here: they come from
// data/grammar.json, which tools/site_data.py writes from the generator's own prototype
// (tools/phrasing_prototype.py). This file only works out the English text of a choice, and
// check.mjs proves every choice's text is a phrase in that file, and every phrase is reachable.
// The grammar is not in the built dictionary yet (plans/08); nothing here claims it is.

import { createBoard } from './board.js';
import { el } from './dom.js';
import { chip } from './lesson.js';
import { parseOutline } from './steno.js';
import { grammar } from './data.js';

const beForm = (person, past, neg) => {
  if (past) { const w = person === '2' || person === 'pl' ? 'were' : 'was'; return neg ? `${w}n't` : w; }
  const w = { '1s': 'am', 2: 'are', pl: 'are' }[person] || 'is';
  return neg ? (w === 'am' ? 'am not' : `${w}n't`) : w;
};

/** Every auxiliary choice: [vowel keys, past?, label]. */
export function auxChoices(g) {
  const out = [['', false, 'present'], ['', true, 'past (E)']];
  for (const [, keys, pres, past] of g.modals) {
    out.push([keys, false, `${pres} (${keys})`]);
    if (past) out.push([keys, true, `${past} (${keys}E)`]);
  }
  return out;
}

/** The English a choice writes, or null where the grammar writes nothing. `verb` is null, a base verb, or be/have/have been. */
export function phraseText(g, who, modalKeys, past, neg, verb) {
  const [subject, , person] = who;
  const third = person === '3s';
  const modal = g.modals.find((m) => m[1] === modalKeys) ?? null; // [name, keys, pres, past, negPres, negPast]
  if (modal && past && modal[3] === null) return null;
  if (subject === 'there' && verb && !['be', 'have been'].includes(verb)) return null;
  if (modal) {
    const aux = neg ? (past ? modal[5] : modal[4]) : (past ? modal[3] : modal[2]);
    return [subject, aux, ...(verb ? [verb] : [])].join(' ');
  }
  if (!verb) {
    if (subject === 'there') return null;
    const d = past ? 'did' : third ? 'does' : 'do';
    return `${subject} ${neg ? `${d}n't` : d}`;
  }
  if (verb === 'be') return `${subject} ${beForm(person, past, neg)}`;
  if (verb === 'have' || verb === 'have been') {
    const h = past ? 'had' : third ? 'has' : 'have';
    return `${subject} ${neg ? `${h}n't` : h}${verb === 'have been' ? ' been' : ''}`;
  }
  const [base, s3, ed] = g.verbs.find((v) => v[0] === verb);
  if (neg) return `${subject} ${past ? "didn't" : third ? "doesn't" : "don't"} ${base}`;
  return `${subject} ${past ? ed : third ? s3 : base}`;
}

/** The composer block. */
export function grammarComposer() {
  const box = el('div', { class: 'try gcomp' }, el('div', { class: 'try__title' }, 'Build a stroke'));
  grammar().then((g) => {
    const aux = auxChoices(g);
    const verbs = [[null, 'no verb'], ...g.special.map(([v, c]) => [v, `${v}  ${c}`]), ...g.verbs.map(([v, , , c]) => [v, `${v}  ${c}`])];
    const sel = (label, options, value) => {
      const s = el('select', { 'aria-label': label }, options.map(([v, t], i) => el('option', { value: i }, t)));
      s.value = String(value);
      return [s, options];
    };
    const [whoS] = sel('starter', g.starters.map(([w, c]) => [w, `${w}  ${c}`]), 4);
    const [auxS] = sel('auxiliary', aux.map((a, i) => [i, a[2]]), 1);
    const [verbS] = sel('verb', verbs, 1);
    const notB = el('input', { type: 'checkbox', id: 'gc-not' });
    const out = el('div', { class: 'gcomp__out', 'aria-live': 'polite' });
    const boardHost = el('div');
    const board = createBoard(boardHost, { compact: true });
    const lab = (t, n) => el('label', { class: 'gcomp__pick' }, el('span', {}, t), n);
    box.append(
      el('div', { class: 'gcomp__row' }, lab('Starter', whoS), lab('Auxiliary', auxS), lab('Verb', verbS),
        el('label', { class: 'gcomp__pick gcomp__not' }, notB, el('span', {}, ' not (*)'))),
      out, el('div', { class: 'boardbox__scroll' }, boardHost),
      el('p', { class: 'boardbox__hint' }, 'The board shows the keys; nothing on it is clickable. These strokes are the grammar, not dictionary entries: it is not built into the dictionary yet.'));
    const run = () => {
      const who = g.starters[Number(whoS.value)];
      const [keys, past] = aux[Number(auxS.value)];
      const verb = verbs[Number(verbS.value)][0];
      const text = phraseText(g, who, keys, past, notB.checked, verb);
      const stroke = text ? g.entries[text] : null;
      out.replaceChildren(...(stroke ? [...chip(`${text}|${stroke}`)] : [el('span', { class: 'word' }, 'the grammar writes nothing here')]));
      const parsed = stroke ? parseOutline(stroke) : null;
      if (parsed) board.show(parsed[0]); else board.clear();
    };
    for (const n of [whoS, auxS, verbS, notB]) n.addEventListener('change', run);
    run();
  });
  return box;
}

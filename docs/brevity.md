# How short

[← back to the table of contents](README.md) · [← back to the main README](../README.md)

**How short** (`[[brevity]]` in the rules). Each step is a transformation of the full outline
that can be done by hand, with a frequency rank below which words take it. `lead` drops an
unstressed first syllable that is only a vowel (*about* PWOUT, *again* TKPWEPB, *around*
ROUPBD); `lean` puts the consonants of an unstressed first syllable on the left hand of the
next stroke when they come first in steno order (*believe* PWHRAOEF, *deliver* TKHREUFR,
*complete* KPHRAOET). `fold` is the outline with reduced syllables folded out of steno order
where they would not fit in it, as Plover does (*student* STAOUPBTD, *goodness*
TKPWAOPBSD). The one-stroke steps for long words: `skeleton` is one
stroke of the first consonants, the stressed vowel, and every consonant after the first vowel
(*profitable* PROFBLT, *chemist* KEFPLT, *machete* PH-FPT); `shell` keeps those of the stressed
and last syllables, `ends` those of the last; each with or without the stressed vowel. An empty
list gives a Lapwing-like dictionary, a stroke per syllable; a huge rank on every step gives the
shortest one the rules allow. Words that take no step are written by the plain
**[prefix](prefixes.md)**/**[syllable](syllables.md)** rules.

**Every word gets the skeleton; the other one-stroke steps stop at rank 8,000.** The skeleton
is the one short form a learner can derive without seeing it: keep every consonant, keep the
stressed vowel, drop the rest. `shell`, `ends` and the vowel-less skeleton drop consonants too,
and which ones is not predictable, so they stay a privilege of the 8,000 commonest words. Until
2026-11-04 the skeleton stopped at 8,000 as well, which was a cliff: 81% of words ranked
5,000–10,000 were one stroke and 58% of those ranked 10,000–20,000. Now it is 87% and 73%
(20,000–50,000: 41% to 58%), and a word past 20,000 costs 8.0 keys on average instead of 8.5.
The price is in the hands: a skeleton is a denser chord than two syllable strokes, so the
average ergonomic score past rank 10,000 rose from 269 to 308 (the regression test's ceiling
moved from 290 to 320 with it).

The commonest words never pay for this. Settling runs its family passes exactly as before, with
a word past rank 8,000 holding its skeleton back; then the 5,000 commonest words are locked to
whatever they settled on, and one last pass runs with every skeleton in. A locked word keeps its
outline exactly, nobody else may take it, and no locked word may take another's. So the top
1,000 are byte-identical to the build before, and in 1,000–5,000 eight words changed (a star
where a rarer word's skeleton now holds the plain stroke) against fifty-five that got cheaper.
Afterwards, any word left on a worse outline than one of its own candidates that nobody holds
(a text a root reserved and then walked away from) moves to it: zero stranded words. An
unstressed first
syllable gives a one-stroke form its left hand (*information* TPH-FGS, *condition* K-PB,
*opinion* P*PB), and the glide of *-ion* is not written. A short form never takes an outline that
any word has by the rules, so the forms a learner can work out stay theirs; when a short form is
taken, a star on it is tried before giving up, since a star costs a key and a longer form costs a
stroke. Every word keeps every short form still free beside its own rule outline once its primary
is settled, so the dictionary holds both ends of the spectrum and the cheapest one counts toward
`cheapest` regardless — this now runs as its own pass strictly after every word's primary outline is
final, which closes a real ordering hazard the old, interleaved version had: a common word's bonus
short form could squat on text a later, rarer word needed as its own *primary*, simply because the
common word was processed first. It also runs after the alternate-pronunciation and
**[backup](syllables.md)** passes, so a stem-plus-ending join (*looks* HRAOBG/+-S) or another
pronunciation's reading is never blocked by a one-off short form that wanted the same text: when
the predictable form and the memorised one collide, the predictable one holds it (801 more backups
and 210 more alternates than the other order). A word pinned to a literal `[word]` override (see
**[briefs](briefs.md)**) is the one exception — its single entry is a human decision, not reopened
by a bonus short form any more than by anything else.

`build` prints how many strokes and keys a word takes on average, weighted by how often it is
said, next to Plover and Lapwing on the same words — see
**[the main README](../README.md#where-lyk-stands)** for the numbers.

Next: **[the full suffix table](suffixes.md)** — every standalone continuation stroke this gives a
suffix on its own, derived from the same rules as any other syllable.

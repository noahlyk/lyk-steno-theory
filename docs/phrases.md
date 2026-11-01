# Phrase-briefing

[← back to the table of contents](README.md) · [← back to the main README](../README.md)

Everything up to here settles what *one* word strokes as. This chapter is about something
genuinely different: two already-settled words merged into one shorter outline that writes both of
them at once — a real 2-strokes-to-1 compression for a common word pair, not a cosmetic one. The
clearest case, and the one this chapter opens with, is both words already happy with their own
one-stroke outline, merged into a brand-new single stroke with no join key, no stem, no
"continuation" of anything — two independently complete whole words, each already reachable on its
own, that also happen to share a second, unrelated chord. A later section, "Reaching into a
multi-stroke host's first stroke," extends this to a bigram where only *one* side is single-stroke
and the other carries its own multi-stroke primary unchanged past the merged first stroke.

## The idea

Take "of" (its own real primary: `-F`) and "the" (its own real primary: `-T`). Struck separately,
that's two strokes. But `-F` and `-T` don't share a single key between them — not even the star —
so their bitwise union, `-FT`, is a stroke that was never assigned to anything. Nothing stops this
project from handing that brand-new stroke to the phrase "of the" directly: one stroke, in plain
English, where two were needed before. "of the" is in fact the single highest-count bigram in the
whole frequency corpus (2,766,332,391 occurrences), so this is not a marginal case to chase — it's
the best possible example of what this mechanism is for.

## Why this, and not the obvious alternative

Two ways to build a phrase entry's key were considered.

**Naive concatenation** — key = "of"'s own stroke followed by "the"'s own stroke, joined the
ordinary multi-stroke way (`-F/-T`), value = `"of the"`. This was rejected, not overlooked: under
this project's own cost model (`candidate_cost`, and every benchmark in `quote_corpus.rs`), an
entry like this costs exactly `strokes(of) + strokes(the)` — identical to typing the two words
through their own existing, independent entries. There is nothing for an entry like that to
actually *do*; it's a no-op with a prettier name.

**Merging into one new stroke** (what this chapter describes) is the only one of the two that moves
any number this project measures. It genuinely halves the stroke count for the bigram — 2 strokes
become 1 — the same structural kind of saving a learner feels as a removed stroke *boundary*, not a
tie-broken key count. Mechanically, it's the identical trick the [backup-entry pass](syllables.md)
already uses for its "leading consonant folded onto a placed word's first stroke" direction
(`leading.0 & target_first.0 != 0` as the disjointness gate, the bitwise OR as the merge) — just
applied here to two complete, already-placed whole-word outlines instead of a fragment and a stem.

**A genuinely separate collision class, and why it isn't one.** Could striking "of" and "the" back
to back by accident ever misfire as the phrase? No: the phrase's own stroke, `-FT`, is a *brand-new*
chord nothing else was using. Reaching it requires pressing both halves' keys in the same stroke,
which is simply a different physical action from pressing `-F` then `-T` as two separate strokes.
There's no ambiguity to resolve here the way there is for, say, two homophones competing for the
same text.

## What qualifies

A bigram `(w1, w2)` earns a merged phrase entry only when every one of these holds:

1. **Both words already carry a settled *primary* outline, and at least one of them is exactly one
   stroke long.** "Primary" means literally the outline `generate()`'s own
   peeling/solver/literal-override step assigned — whatever origin won that competition (a gold
   brief, lyk's own plain rules reading, a spelling-told-apart variant, or a star-ladder step). It
   explicitly does **not** mean a bonus `Short`, `Alternate`, or `Backup` entry, even one that
   happens to be single-stroke and even cheaper than the word's own primary (see "Why primary-only"
   below). The other word's primary can be *any* length — one stroke (the original case) or several
   (see "Reaching into a multi-stroke host's first stroke" below).
2. **The single-stroke word's whole primary and the other word's own *first* stroke are
   disjoint** — no key in common, the star included. When both happen to be single-stroke, this is
   just "the two primaries are disjoint," exactly as before.
3. **The merged outline's own rendered text is unclaimed by anything already in the
   dictionary** — a reserved stroke, a suffix, "I", any primary, or any bonus/alternate/backup
   entry. For the original both-single-stroke case this is one stroke's worth of text; when the
   other word is multi-stroke, it's that word's full outline length, first stroke merged, every
   later stroke carried through unchanged.
4. **The bigram is among the top `PHRASE_TOP_N` by raw count** (see "The threshold" below) — a fixed
   bound on how many bigrams are even attempted, not a bound on how many actually make it through
   steps 1–3.

Two different eligible bigrams can coincidentally land on the identical rendered outline text even
though the words differ. When that happens, the winner is whichever bigram was actually said more
often in the corpus, with the phrase text itself as a last-resort, deterministic tie-break — exactly
the same "commonest word wins contested real estate" rule the backup pass already uses for its own
within-round collisions (see [syllables & folding](syllables.md)), just keyed by a pair of words
instead of one.

## Why primary-only

A word's own bonus entries — a `Short` form, a backup composition, an alternate pronunciation — can
sometimes be *cheaper* than its own primary. `sit`, for instance, has a backup entry `ST` (built
from "s" + "it"), two keys fewer than its own phonetic primary `SEUT`. Building a phrase on top of
whichever entry happens to be cheapest, rather than specifically the primary, would catch more real
savings.

This was considered and deliberately scoped out of this first increment. A primary assignment is
`generate()`'s own foundational invariant — one single-stroke outline per word, the thing [the very
first chapter](README.md) states before anything else, and the most heavily tested property in the
whole generator. A backup entry, by contrast, is itself contingent on the backup-composition pass's
own internals and fixed-point ordering, and can change shape if that pass changes. Phrase-briefing
is a brand new mechanism; building it on the most stable thing available (a primary) keeps its own
correctness argument simple, and keeps a future bug in phrase-briefing from ever being confused with
a bug in backup composition, or vice versa. **This is a real, acknowledged scope cut, not a claim
that primaries are always the best base for a phrase** — the best real candidates this pass could
reach are the ones whose primary-to-primary disjointness already works out, not the full set of
every word pair that could in principle share a stroke through some bonus entry. Revisiting this is
reasonable future work once this version's real hit rate is understood.

**A related, real wrinkle worth knowing about, not a bug:** the bigram corpus's own vocabulary is
lowercase. This project's lowercase word `"i"` is not the pronoun — it's the single *letter* "i"
(see `generate()`'s own comment on why: the frequency corpus can't tell the capitalized pronoun "I"
apart from the letter name at all, and the letter won that one shared slot; the pronoun gets its own
separate, hardcoded entry on `EU`). So a bigram like `("i", "am")` is, mechanically, "the letter i"
plus "am" — not "I am" the sentence. In the real dictionary today this particular pair never
actually produces a phrase entry (its merged stroke, `AOEUPL`, is already taken by the contraction
"i'm"), so no incorrect lowercase "i ..." text has ever shipped from this — but it's a real,
pre-existing ambiguity in this project's data, not introduced by phrase-briefing, and worth
re-checking if `PHRASE_TOP_N` is ever raised far enough for a different "i"-led bigram to win a free
stroke under this same quirk.

## Reaching into a multi-stroke host's first stroke

The first version of this chapter scoped out "multi-stroke component words" entirely, reasoning
that reaching into a multi-stroke word's own internal join-key structure to merge one of its
strokes with a neighbor risked corrupting that structure. On closer look, that risk doesn't hold up:
this project already has real, shipped, tested precedent for exactly this pattern.
`compose_backup`'s own direction (b) — "fold a leading consonant onto a placed word's first stroke"
(`never` from `ever`'s `-R`'s leading `n`'s `TPH`, say) — already reaches into `word_outline[stem]`,
a stem's outline of *any* length, and merges a fragment into its first stroke only
(`leading.0 & target_first.0 != 0` as the disjointness gate, the bitwise OR as the merge), leaving
every later stroke — join bits included — completely untouched. `word_outline` was never restricted
to single-stroke stems; it holds every placed word's own outline regardless of length. So "merge
into stroke 0, leave the rest alone" was already a correct, regression-tested operation before
phrase-briefing existed at all — the only genuinely new risk a phrase entry adds is a stale
assumption elsewhere in the code that a phrase's own outline is always one stroke long, not anything
about the merge itself.

That's exactly how this chapter now reaches a multi-stroke word: a bigram `(w1, w2)` is also
eligible when one word (the "filler") has a single-stroke primary and the other (the "host") has a
primary of *any* length — the filler's whole primary is OR'd into the host's own first stroke only,
via `merge_into_host_first_stroke`, the identical trick `compose_backup`'s direction (b) uses. The
committed entry's outline is `[merged_first_stroke, ...host's_remaining_strokes]`; the host's own
later strokes are moved into the phrase entry byte for byte, including whatever join bits they
already carried as part of the host's own multi-stroke primary — they don't need to, and don't,
change just because the first stroke picked up a filler word's content. The merged first stroke
itself carries no join key, same as any entry's first stroke: it still starts a brand-new word
boundary from the stroke stream's point of view (see [the join key](join-key.md)), which is correct,
since this phrase entry is still where the two-word phrase *begins*.

Both directions turn out to matter about equally: one real multi-stroke-primary word acting as the
host with a single-stroke word in front of it (`"of" + "ample"`-shaped), and the symmetric case, a
multi-stroke word followed by a single-stroke one (`"accompanied" + "by"`-shaped). Supporting only
one direction would have left roughly half the real yield on the table — measured, not assumed (see
"Measuring the real effect" below).

`Origin::Phrase` entries built this way are still excluded from `single_stroke_stems` for exactly
the original reason (a phrase's `word` field is two words' text, never a real single word a
continuation stroke could sensibly follow) — and now, for a multi-stroke phrase entry specifically,
`single_stroke_stems`'s own `match e.outline[..] { [stroke] => ..., _ => None }` length check would
already exclude it on length alone, making the explicit `Origin::Phrase` filter partially redundant
*for this specific case*. It stays in the code anyway, and is still load-bearing: the original
both-single-stroke phrase entries (still the large majority of the committed total) are exactly one
stroke long, so the length check alone would *not* exclude them — the explicit origin filter is
still what protects the plugin fallback from reading `"of the"` back as some unrelated real word's
own text.

## Not in scope for this increment, and why

- **Both words multi-stroke.** A single-stroke "filler" word is still required on (at least) one
  side — see "Reaching into a multi-stroke host's first stroke" above. Reaching into *both* words'
  internal join-key structure at once, with no single-stroke anchor on either side, is a different,
  bigger risk than folding one whole single-stroke word onto one multi-stroke word's first stroke,
  and isn't attempted here.
- **Phrase chaining (three or more words).** The backup pass can chain ("green" + "house" →
  "greenhouse", then "greenhouse" + `+-Z` → "greenhouses") because re-indexing "placed words" every
  round is cheap and well-understood. Doing the same for phrases raises real open questions — what
  does "disjoint" even mean for a phrase whose own "primary" is already a two-word merge, and does a
  spelling-based tie-break make sense for a phrase at all — left for a second increment once this
  one's real-world hit rate is measured.
- **Using a bonus/backup entry as a phrase component.** See "Why primary-only" above.
- **PMI/collocation-ratio weighting.** See "The threshold" below.
- **Dropping a redundant join-key boundary inside an existing multi-stroke word.** A different
  mechanism than this chapter builds; not attempted here.

Nothing about a word's own primary outline ever changes because of this pass. The "one single-stroke
outline per word" invariant is untouched — a phrase entry's own "word" field holds two words' worth
of text ("of the"), not a real single word competing for anything.

## The threshold

`setup.bigrams` holds raw, un-normalized counts: how many times one word was immediately followed
by another, from Norvig's Google Books Ngrams data, filtered down to pairs where both words are in
this generator's own vocabulary. A PMI/collocation-ratio test (`count(w1, w2) / (count(w1) *
count(w2))`, the standard way NLP work separates "a genuinely glued phrase" from "two independently
common words that happen to sit next to each other") was considered for ranking bigrams, and
rejected: PMI specifically *penalizes* exactly the pairs real steno theories brief first. "of the",
"in the", "for the" are strong candidates precisely *because* both halves are individually extremely
common function words — which is what gives them their enormous raw co-occurrence counts, not
despite it. Raw count is what actually measures "how many times, in real running text, will this
exact pair fire and save one stroke" — the real return on minting the entry — directly.

**The real eligible-set size was measured, not guessed, before choosing a cutoff.** Scanning
`setup.bigrams` sorted by raw count, and checking each prefix of that list against the real, built
dictionary (both words single-stroke-primary, keys disjoint, merged stroke still genuinely
unclaimed):

| top N bigrams scanned | raw count at the cutoff | eligible (disjoint primaries) | still free (unclaimed) |
|---:|---:|---:|---:|
| 100 | ~90.1M | 89 | 37 |
| 300 | ~40.6M | 228 | 108 |
| 1,000 | ~17.7M | 653 | 346 |
| 3,000 | ~7.6M | 1,789 | 994 |
| 10,000 | ~2.7M | 5,278 | 3,123 |
| 30,000 | ~962K | 13,715 | 8,332 |
| 100,000 | ~274K | 37,146 | 22,564 |

(`generate::tests::dump_phrase_eligible_set_sizes`, an `#[ignore]`d diagnostic, reproduces this —
run it with `--ignored --nocapture` against a fresh build.) Two things fell out of this that weren't
obvious going in:

- Disjointness itself is common (roughly 55–65% of any prefix of the sorted list), but a genuinely
  *free* stroke is scarcer — in this already-dense, ~100,000-word dictionary, a good chunk of the
  highest-count bigrams' own merged strokes are already claimed by some unrelated word's own
  primary (`"in" + "the"` is disjoint but its merged stroke, `TPH-T`, already belongs to `antonio`;
  `"it" + "is"` collides with `sit`). That's an honest miss, not a bug — see
  `generate::tests::dump_specific_bigram_eligibility` for more worked examples.
- The yield doesn't fall off sharply across this whole range — roughly a third of any given prefix
  turns into a real, committed phrase entry, all the way out past rank 10,000. **N = 300**, the
  planning pass's own placeholder guess before this was measured, would have produced only about
  108 real entries, and its own raw-count cutoff (~40.6M) is still a tiny fraction of the full
  245,803-bigram vocabulary.

**`PHRASE_TOP_N` is set to 10,000.** At that cutoff, raw counts are still in the low millions —
comfortably above the long tail, where a bigram's own commonality starts looking more like
coincidence than habit — while the real yield is substantial enough to be a real, felt improvement
rather than a token gesture. Raising it further is reasonable future work once this version's
real-world hit rate (see "Measuring the real effect" below) is better understood; the data above is
there specifically so that decision doesn't have to be re-measured from scratch.

The table and the "3,123" figure above were measured against the dictionary as it stood when this
chapter was first written. The dictionary has grown substantially since (most recently, a
CMUdict-gap pronunciation fallback added over 13,000 words — see `docs/decisions.md`), which shifts
the absolute counts without changing the qualitative shape ("roughly a third of any prefix commits,
all the way past rank 10,000"). As of the build measured for the multi-stroke-host update below, the
both-single-stroke-only rule alone commits **3,034** real entries at `PHRASE_TOP_N = 10,000`;
allowing a multi-stroke host on either side (see "Reaching into a multi-stroke host's first stroke"
above) adds **190** more, for **3,224** total. Re-run
`generate::tests::dump_phrase_eligible_set_sizes` and `dump_multistroke_host_phrase_yield` against a
fresh build for the current precise numbers rather than trusting any number printed here to stay
exact forever.

## Where this runs

A single, independent pass in `generate()`, run once — no fixed-point loop, no chaining — right
after the [backup-composition pass](syllables.md)'s own fixed-point loop finishes, and before the
`stranded` diagnostic is computed. Running after backups means every phrase candidate is checked
against the dictionary's fully-settled state, backups included; running before `stranded` means that
diagnostic still reports against the dictionary's true final shape.

## `Origin::Phrase`, and the one integration risk it created

A phrase entry gets its own `Origin::Phrase` tag, same as every other kind of entry carries its own
origin. `write_json` needed no change at all — it already keys the output purely by rendered outline
text and display text, and a phrase's two-word text (`"of the"`) round-trips through that exactly
like any single word's would.

One real, load-bearing exception: `generate::single_stroke_stems` collects every single-stroke entry
in the dictionary into a `Stroke → String` map, consumed by two real call sites — the `explain`
command, and (more importantly) the `plugin` command that generates the actual deployed keymux WASM
fallback. That logic uses the map to read an unmatched continuation run's first stroke as a *known
word's own real text* rather than raw phonemes (`SKP/+-R` reads as "ander", not "sper"). A phrase
entry's own "word" is two words' text, not a real single word a continuation stroke could ever
sensibly follow — `"of the" + "-s"` would read back as "of thes", nonsense. `single_stroke_stems`
excludes `Origin::Phrase` the same way it already excludes `Origin::Suffix`, and
`quality::single_stroke_stems_never_contains_a_phrase_entry` is the regression guard for it.

## Measuring the real effect

`quote_corpus.rs`'s existing benchmark tokenizes a passage into individual words and looks each one
up independently — a phrase entry keyed by two words together is invisible to that loop even when
the exact bigram appears verbatim in a passage. A separate, additive extension
(`quote::total_cost_with_phrases`, called from `quote_corpus::score` and from the live `quote`
command) walks a passage greedily, left to right: at each position, a phrase entry covering this
word and the next one wins over scoring the two words separately, falling back to the ordinary
per-word lookup otherwise — the same longest-match-first idea Plover/keymux's own stroke-stream
resolution already uses, just over words here instead of raw strokes.

This is reported as its own, clearly separate set of numbers (`report()`'s own "phrase-aware walk"
section) — never folded into the existing word-by-word totals, whose own thresholds are calibrated
against word-by-word scoring and must keep passing unchanged regardless of how many phrase entries
exist.

**Strokes, not keys, are the number this mechanism actually moves — and the benchmark reports both
so that distinction is visible, not papered over.** A phrase entry's merged first stroke is, by
construction, the bitwise OR of two disjoint strokes, and every later stroke a multi-stroke host
contributes is carried through byte for byte — so a phrase entry's total key count is always exactly
the sum of the filler's key count and the host's own full key count, in both the original
both-single-stroke case and the multi-stroke-host case alike. Merging never changes the key total,
only the stroke count, which drops by exactly one per hit, regardless of which direction fired.

Numbers below are a direct before/after measurement of this chapter's multi-stroke-host update
specifically (one file reverted to `HEAD` and restored, both builds verified by `sha256sum`, exactly
the same methodology `docs/decisions.md` uses for every other "real impact" entry) — not the
chapter's original both-single-stroke-only numbers, which have since drifted from a later, unrelated
dictionary-vocabulary expansion (see the note in "The threshold" above) and are not worth re-quoting
here only to go stale again. On the hand-picked 103-passage corpus: before, 102 phrase entries fired,
1,144 strokes with phrases vs. 1,246 word-by-word (102 fewer); after, 103 fired, 1,143 vs. 1,246 (103
fewer) — one additional real stroke saved, the key total unchanged at 4,333 either way (0 fewer keys,
both before and after). On the larger, 416-passage extracted corpus: before, 479 fired, 5,922 vs.
6,401 (479 fewer); after, 483 fired, 5,918 vs. 6,401 (483 fewer) — four more strokes saved, while the
key total went from 4 keys worse (22,311 vs. 22,307) to 8 keys worse (22,315 vs. 22,307) — not a bug,
the same second-order effect described just below, just firing a few more times. **This is a small,
real, additive win, not a second v1**: the newly-eligible multi-stroke-host population is
substantially rarer in real running text than the original both-single-stroke population (190 new
committed dictionary entries out of 3,224 total at `PHRASE_TOP_N = 10,000`, and only 1 and 4 of those
190 actually land inside the two benchmark corpora respectively) — consistent with the "low hundreds
at best" yield this project's own backup-component precedent (92 words with a multi-stroke primary
and a single-stroke `Backup` entry, adding only 7–9 phrase entries when tried) already predicted
before this was built. Shipped anyway: it's strictly additive (zero existing entries changed or lost
— confirmed by diffing the full built dictionary word for word, 190 added, 0 removed, 0 changed), the
risk the first version of this chapter worried about turned out not to apply (see "Reaching into a
multi-stroke host's first stroke" above), and every real stroke saved in a 2-strokes-to-1 compression
is a real, felt improvement for a learner even when the count is modest.

This second-order effect comes from `lyk` (`generate::cheapest`) crediting each word with its own
globally cheapest entry of *any* origin, including a backup composition cheaper than that word's own
primary (`sit`'s `ST` versus its own primary `SEUT`, the same gap "Why primary-only" describes
above), while a firing phrase is necessarily built from both words' own *primaries* specifically. On
rare occasions, that means the greedy phrase-aware walk spends very slightly more keys than
word-by-word scoring would have, even while it always spends strictly fewer strokes. Reporting both
numbers, honestly, side by side, is the point — a single "cheaper" verdict would have hidden exactly
this trade-off.

Next: **[when two words sound the same](collisions.md)**.

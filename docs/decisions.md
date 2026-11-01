# Decisions

[← back to the table of contents](README.md) · [← back to the main README](../README.md)

This is not a learner's chapter — it's a maintainer's log. The chapters in the table of contents
explain how the theory works *now*; this explains why a run of real decisions this session landed
the way they did, so nobody (human or AI) reverts one by accident without knowing it was
deliberate. Each entry: what changed, the concrete problem that forced it, and what regresses if
undone. In commit order.

**Join key renamed `^` → `+` (`38f11c8`).** Pure notation, no behavior change — `^` collided
visually with nothing in particular, `+` reads better next to Plover's own `*`. If this gets
reverted on its own, every doc, every `rules/*.toml` entry, and the plugin template's own parser
all have to move together or the dictionary and the deployed plugin disagree on notation.

**Backup entries: a shorter word's own outline, plus whatever's left over (`f8687f2`, later
rewritten by `ada24ff`).** `build` → `PWEULD`, `building` → the unrelated-looking fused `PWEULGD`,
because every word's outline was derived from its own pronunciation alone — nothing ever checked
"is there already a shorter placed word to build this one from instead." Fixed with a second,
additive pass: for every word, try every split of its own sounds against three composition kinds
(a trailing reduced syllable as a suffix continuation, a leading consonant cluster folded onto a
placed word's first stroke, two whole placed words back to back), and add every safe match as an
extra entry — `build`'s `PWEULD` plus `+-G` also reaches `building`, without touching `building`'s
own primary outline at all. Runs to a fixed point so a backup can chain through another backup:
`green`+`house`→`greenhouse`, then `greenhouse`+`+-Z`→`greenhouses`, with neither needing its own
primary outline to look like that. See [syllables & folding](syllables.md) for the full mechanism.
**Never revert this to "first match wins"** — the whole point of trying every split and running to
a fixed point was that the first-match version missed chained compounds and left real words
(`buildings`) without a sane way in. Deliberately *not* extended to two-syllable suffixes (`-ism`,
`-ity`, `-ary`, `-ical`, `-logy`, `-cy`, `-ify`) — the real derivation needs a consonant-migration
pattern (how the stem's own ending consonant shifts before the suffix lands) too irregular to
guess safely without real design work, not just effort. Don't paper over that gap with a guess.

**One-stroke-per-word, stated as policy (`12927ec`).** A word gets *exactly one* single-stroke
outline — no `NOT`/`OT`/`PHOT`-style competition for the same word — but unlimited `+`-joined
multi-stroke backups, judged on predictability (does it decompose into pieces a learner already
knows) rather than brevity. This was always true of the single-stroke machinery by construction;
what changed is making it an explicit, written invariant so a future change doesn't casually add a
second single-stroke way to reach a word and call it a feature. If you're ever tempted to add an
alternate single-stroke spelling for a word "because it's reasonable too" — don't; that's the
`NOT`/`OT`/`PHOT` problem by definition.

**The `cat`/`captain` fragment bug (`57d99d7`).** `cat` was losing its own natural `KAT` to
`captain` — a word that's actually *less* frequent in the corpus's own ranking here, but got
there first because `matches_real_consonants`/`drop_fragments` only checked a word's first and
last syllable when deciding whether a sound counted as a "fragment" to drop. `cat`'s own `K-A-T`
happened to overlap bit-for-bit with a coda cluster from an *earlier* syllable of `captain` that
gets dropped entirely, so the fragment check falsely flagged `cat`'s legitimate, complete sound as
someone else's leftover. Fixed by checking a syllable's own full structure, not just the
word's endpoints. If this regresses, it won't look like "captain changed" — it'll look like some
unrelated short, common word losing its natural outline to a longer word that merely happens to
contain its sounds as a sub-fragment. Watch for that shape of bug, not just this one instance.

**Shrimple-style spelling mode (`5f1dd71`).** Some words (`stenography`) have no CMUdict
pronunciation at all, so the rules engine has nothing to generate a stroke from — the phonetic
fallback used to garble these since it only knows how to read keys back as *sounds*, not letters.
Added a second marker bit (`~`, alongside the join key's `+`) that switches a run into letter-exact
spelling: each stroke after the marker reads back as one literal letter from `rules/spelling.toml`,
not a guessed sound. This reuses the join key's own "whole run handed to the plugin at once"
mechanism — no new keymux engine state needed. See [spelling mode](spelling-mode.md).

**Suffix VIP fallback (`ff01ea4`).** A suffix stroke (`+-D`, `+-G`, …) only read as its real
suffix text when some specific *combined* outline happened to already be a literal dictionary
entry — struck after a stem with no matching entry, it read as a bare, vowelless consonant guess
instead (`+-D` → `"d"`, not `"ed"`). This defeated the entire point of giving suffixes their own
reserved strokes in the first place: a suffix is supposed to be struck-and-trust, unconditionally,
the same way reserved commands and punctuation are. Fixed by having the phonetic fallback
(`Rules::resolve`, mirrored in the generated plugin) check `rules/suffixes.toml`'s own chords
*first*, for any stroke after the first that carries the join key, before ever falling through to
a generic phonetic guess. **This must stay unconditional.** Reintroducing any "only if it's a known
combination" gate here reopens the exact bug — a suffix silently working sometimes and not others
depending on what it's attached to, which is the single complaint this fix exists to kill.

**Continuation stem reads its real word, not raw phonemes (`f44bfe2`).** Even with the suffix fix
above, a continuation run whose *stem* (the first stroke) wasn't backed by a literal combined
dictionary entry still got re-derived from scratch as raw phonemes — including stems that are
themselves perfectly good, already-placed words. `SKP` is "and"'s own brief; nothing about
`S-K-P` spells "and" phonetically, so `SKP`+`+-R` read back as `"sper"` (`SKP` decoded as raw
onset consonants, with "-er" correctly appended to the wrong base entirely). Fixed by having
`resolve` check an already-placed single-stroke word first for the *first* stroke of an unmatched
run, using its real text instead of re-guessing: `SKP/+-R` now reads `"ander"`, `PWHR/+-D` reads
`"builded"` instead of `"mred"`. Scoped to a single-stroke stem for now — a multi-stroke placed
word as a run's own stem is a known, not-yet-covered extension, not a decision to leave it out
forever. See [the fallback](fallback.md) for the exact mechanics.

**keymux's own plugin-fallback layout bug (keymux commit `5a4460f`, sibling repo).** Found while
chasing a live report that `+` sometimes "made a separate word." Root cause lived in keymux, not
lyk: `Chain::lookup` rendered the stroke window it hands a plugin's fallback export using a
hardcoded default 23-key Plover layout, not the `Layout` the chain was actually built with. Any
extra key a theory defines — lyk's `+`, or any other theory's own join/spelling/whatever key — has
no name in that default layout, so the free-standing `render_strokes` call silently dropped it
before the plugin ever saw it: the plugin had no way to tell a continuation stroke from a fresh one
once dictionary lookup missed, because the one bit that said so was already gone from the text by
the time it got there. Fixed by having `Chain` keep the `Layout` it loaded with and render with
that instead of the hardcoded default. **This fix is generic, not a lyk accommodation** — it
affects any theory with any extra key, which was the explicit condition on making it at all (see
[the join key](join-key.md)'s closing note). It landed on a detached HEAD in the keymux repo and
still needs to be placed on a real branch — if that commit gets lost, every theory with a custom
extra key (not just lyk) silently regresses to this exact bug the moment its dictionary misses.

**CLI default corpus size matched to what's actually deployed (`1f26529`).** `quote`, `resolve`,
`lookup`, and the standalone `plugin` command all defaulted to a smaller word count (20k-30k) than
`build`/`deploy`'s 100k. A mid-frequency word only earns its full brevity treatment once the
generator has placed enough higher-ranked words to know what's still free, so a smaller ad-hoc
build can show a strictly worse outline than what's actually shipped — confirmed concretely with
`quitters`: a 2-stroke backup composition at the old 30k default, a clean 1-stroke `KWEURTZ`
(matching Lapwing's own stroke count) at the real 100k the deployed dictionary uses. Not an engine
bug — a diagnostic tool quietly demonstrating a weaker, un-shipped build of itself. Fixed by
matching every CLI default to 100k. **If these defaults drift apart again, every stand-alone
`quote`/`resolve`/`lookup` run becomes a false demo of lyk's own quality**, understating exactly
the terseness comparisons the whole project is judged by.

**The "stranded outline" investigation turned out smaller than it looked (`ae725e4`).** `take`
sitting on a starred `TA*EUBG` while the plain `TAEUBG` sat unclaimed was the motivating report -
and it was a false alarm. `TA*EUBG` isn't `take`'s own primary at all: it's a `Backup` entry
(`T` + the rare word `ake`'s own starred outline, itself a genuine, permanent collision against
`make`), and `generate::cheapest` already correctly picks `take`'s real primary, the clean 5-key
brief `TAEBG` - the confusion was entirely the next entry's bug (`lookup`/`explain` showing the
wrong thing). A real, much smaller issue *did* turn up while checking: ten words (`wow`, `honest`,
`bottle`, `public`, `hiding`, `gasps`, `flowers`, `grunting`, `honey`, `bloody`) sit on a brief
that costs more than their own plain-rules reading, which is sitting completely free. An automatic
fix was tried - first a blanket "use whichever is cheaper," then a narrower version that only
strips a candidate's own gratuitous star - and **both were reverted**. The blanket version
relitigated briefs as foundational as `a`'s own `AEU` against an unrelated pronunciation's shorter
reading; even the narrow, star-only version rippled into several hundred other words' outlines,
some worse off, because this generator settles every word once, commonest-first, with nothing
revisited - changing even one early word reshuffles everything after it. That ripple is an
accepted cost for a human adding a `[word]` override, not for code deciding on its own. What
shipped instead: a standing count, printed by `build` as "words stranded on a worse outline than
their own, unclaimed plain reading," surfacing exactly these ten for a maintainer to fix by hand
with `[word]` the normal way. **Don't revive the automatic version** without first finding a
comparison that can't reach past the specific word it's meant to help - "cheaper" alone isn't it.

**`lookup`/`explain` now read the real dictionary (`62a2dae`).** Both called the raw phonetic
candidate computation directly (`setup.candidates(...)`), bypassing `generate()`'s full collision
resolution, briefs, and backup entries entirely, and never marking continuation strokes with the
join key either - `lookup quitters` showed a garbled, unjoined `KWEUT/-RZ` even though the real,
deployed dictionary has the clean, single-stroke `KWEURTZ`. This is also what made the `take`
investigation above take as long as it did: the tool being used to check the generator's output
was itself the thing lying. Fixed by routing `lookup`'s word/phrase path through
`generate::generate` + `generate::cheapest`, same as `quote`/`resolve` already do, falling back to
the raw candidate only for a word that never made it into the generated dictionary at all.
`explain` keeps its existing `ours` line exactly as it was on purpose - it's a deliberately
rules-only view, for seeing how the theory derives a word step by step - and gains a new `deployed`
line next to it showing the real answer, rather than conflating the two. **If `ours` and
`deployed` ever get merged back into one line, this exact confusion comes back**: a rules-only
derivation is not the same question as "what does this word actually stroke as," and both answers
are worth keeping visible, not just one.

**Spelling mode gained an encoder (`b6e1107`, `36efc64`).** It could only ever decode: `resolve`
read a hand-written `~`/`+` letter-chord outline back into a word, but nothing went the other way
- a word with no pronunciation at all (`stenography`, not in CMUdict) had to be spelled out by
hand, letter by letter, against `rules/spelling.toml`, by eye, before `resolve` could even confirm
it. `Rules::spell_word` is `resolve_spelling`'s exact inverse: given the word, it produces that
outline directly. Wired into `quote`'s fallback chain, then into `lookup`'s `reverse_word` and
`explain`'s no-pronunciation branch once the concurrent `lookup`/`explain` accuracy fix above
landed and touched exactly those two functions - deliberately sequenced to avoid editing the same
code at the same time as that fix. **Does not need porting to keymux**: keymux only ever decodes
strokes someone actually presses (already true for spelling mode, via the generated plugin's
letter table), it never needs to compute what to press for a word - that's purely a lyk-side
authoring/lookup concern.

**Contractions backfilled into the frequency rank (`CONTRACTIONS` in `src/analysis.rs`).** Every
contraction was silently invisible to the whole generator, no matter how common - `don't` has a
real CMUdict pronunciation but was missing from the generated dictionary entirely, so striking its
own, correct outline fell through to the raw phonetic-miss fallback and garbled it. Root cause: the
frequency corpus strips every apostrophe (`don't` is counted there as `dont`), while CMUdict keeps
the real spelling, and the generator's word list is built from the *intersection* of a rank and a
pronunciation - `dont` has no pronunciation, `don't` has no rank, so neither spelling was ever both
at once. Fixed with a hand-curated table (69 entries, verified against both files) backfilling each
real contraction's rank from its stripped form's. **Deliberately hand-curated, not derived by
stripping every apostrophe CMUdict has**: most of those are unrelated possessive-noun pronunciation
entries (`thing's`) that would otherwise wrongly borrow their plural twin's rank (`things`) - this
table is closed-class English contractions only. A few (`he'll`/`hell`, `we'll`/`well`,
`who're`/`whore`) share their stripped spelling with a real, unrelated word already at that rank;
the contraction borrows an approximate rank from it rather than its own true frequency - an
accepted, minor imprecision, not a reason to revert this. **If this table shrinks or disappears,
every contraction in it goes back to being untypeable as itself**, not just briefed differently.

**`make` = `AEUBG`, dropping its own `M`, is not a bug (investigated, confirmed intentional).** One
of Plover's own gold dictionary entries writes "make" with a bare `P` instead of the real `M` chord
`PH`; `best_brief`/`drop_fragments` (`src/generate.rs`, `src/rules.rs`) correctly flags holding only
*half* of `M`'s real chord as disallowed and "repairs" it by deleting the fragment entirely, same as
it does for **113 other words**, including some of the most common in English (`the`→`-T`,
`not`→`OT`, `that`→`A`, `for`→`-R`, `never`→`EFR`). Confirmed from the code's own doc comments this
is the sanctioned, intended behavior, not a failure mode: "a brief is always free to leave out a
whole sound it isn't attempting to represent." **Do not change this shared heuristic to disallow a
full onset/coda drop** - it would silently reshuffle over a hundred already-memorized, currently-
correct briefs for the most common words in the language. A specific word that still bothers a
learner (`make`, say) gets a one-off `[word]` override in `rules/briefs.toml`, the normal path for
"someone looked at this one and decided differently" - zero blast radius on the other 112.

**A real test suite: a hardcoded quote corpus, and structural QA guards.** Every check of "is lyk
actually terser" so far was a human running `quote` by hand and eyeballing a table - no regression
test ever failed if a rule change quietly made the theory worse. `src/quote_corpus.rs` hardcodes
~100 passages (public-domain lines plus plain, unremarkable prose written for the file - deliberately
*not* a curated set that would flatter lyk, since that would measure the curation, not the theory)
and scores every one exactly the way the live `quote` command does (`quote::rows_for`/`quote::cost`,
shared code, not a reimplementation), then asserts against thresholds set just under a real, actual
run: 103 passages, lyk beat Plover outright on 60 (58.3%) and Lapwing on 76 (73.8%); of 1,216 words,
lyk cost fewer keys than Plover on 18.2% and Lapwing on 33.0% (and cost *more* on only 9.5% / 5.2%);
aggregate keys across the whole corpus: lyk 4,640, Plover 4,694, Lapwing 5,039 (lyk is 1.2% cheaper
than Plover, 7.9% cheaper than Lapwing, in aggregate). Thresholds were set a few points under these
numbers - deliberately harsh, not a loose sanity floor: a real regression should fail this
immediately, and a real improvement should raise the constants back up in the same commit, not
leave them stranded low. `src/quality.rs` adds structural QA guards unrelated to terseness - no two
words ever sharing a stroke, every sufficiently common pronounceable word actually having an
outline (the exact kind of check that would have caught the `don't` bug the day it shipped, instead
of a live user hitting it), the stranded-word count not silently growing past its known baseline
(0, as of the entry below - it was 10 when this test was first written), and no generated word
colliding with a reserved command/punctuation/fingerspelling stroke. Both share one production-scale
dictionary build (`src/dictionary_fixture.rs`, a `LazyLock`) rather than paying the few-second
`generate()` cost per test - the whole suite (32 tests) runs in ~5 seconds. **If this corpus or
these thresholds get deleted "to make CI faster," every regression this session fixed by hand (and
ones nobody's found yet) can silently come back with nothing to catch them.**

**Six top-100 words were paying for a fragment-safety rule they didn't need to (`rules/briefs.toml`:
`on`, `from`, `no`, `then`, `day`, plus `bloody`/`wow`/`bottle`/`honest`/`flowers`/`gasps`/
`grunting`/`hiding`/`honey`/`public` from the stranded-word list, and `make` below).** The
quote-corpus benchmark surfaced these as real, measured losses, not guesses. Root cause for the
first five: Plover often writes an extremely common monosyllable using only half of this theory's
own multi-key onset/coda chord (`on`'s Plover brief `OB` is missing the `P` of coda "N"'s real
`-PB`; `from`'s `PR` is missing the `T` of onset "FR"'s real `TPR`; same pattern for `no`, `then`,
`day`) - a deliberate professional shorthand, but one `matches_real_consonants` always rejects,
since it cannot tell "a brief someone meant" from "a fragment that got there by accident" (the exact
same mechanism investigated for `make` below). Each was checked by hand - confirmed free
(`cargo run -- resolve -- <outline>`), confirmed nowhere else in `rules/briefs.toml` already -
before pinning as a literal override. **`one` was checked the same way and deliberately left alone**:
its own shortest Plover candidate (`WUB`) hits this identical rejection, but `one = "WUPB"` already
exists as a separate, older, deliberate decision (a homophone fight with `won`) documented in
`rules/briefs.toml` itself - not an oversight to pile a second fix onto.

**Two contractions (`they're`, `shouldn't`) still cost more than Plover's own brief even after the
rank-backfill fix above - fixed; seven others (`can't`, `didn't`, `hadn't`, `we've`, `hasn't`,
`haven't`, `wasn't`) checked and deliberately left alone.** All nine were visible to the generator
only because of the `CONTRACTIONS` backfill, but several still land far outside `briefs.toml`'s own
1,500-word window even with a backfilled rank, so `best_brief` never tries Plover's candidate for
them at all unless named explicitly. `they're` (saves 3 keys: a free `THER`, vs. a three-homophone
collision fallback `THA*EUR`) and `shouldn't` (saves 3 keys and a stroke-count tie broken on keys:
`SH-PBT` vs. `SH*UPBTD`) both clear this file's own stated bar for naming a word past the window
(**"saves at least three whole keys… that outline's first and last consonants are a whole cluster…
and claiming it does not take a stroke a commoner word already has"** - see the "past this point"
section) and were added. The other seven save only 1-2 keys each - below that same stated bar - so
per this file's own already-documented policy they were left alone, not force-included. **If someone
"fixes" those seven anyway, they are relitigating an existing, written threshold, not filling a
gap** - raise that, don't just add the entries.

**All ten words `build`'s "stranded on a worse outline" count ever flagged are now fixed
(`rules/briefs.toml`); the count is 0 and `src/quality.rs`'s baseline was lowered to match.** Every
one followed the exact same shape: the word's own plain-rules reading was strictly cheaper (fewer
strokes, or the same strokes with fewer keys) than its deployed outline, and that cheaper reading
was confirmed completely free before pinning it. Three distinct root causes among the ten, worth
knowing so the next one is diagnosed faster instead of re-investigated from scratch: (1) a star
nobody needed (`wow`, `bottle`, `flowers` - Plover's own candidate happened to carry a star for a
reason specific to Plover's theory, not a real collision here); (2) a word split across two strokes
when the rules' own single-stroke fold fits fine (`gasps`, `grunting`, `hiding` - all now one
stroke, matching Lapwing); (3) an onset sound that was never actually attempted getting struck
anyway (`honest` - the rules' own reading correctly has no "h", since it's silent in this word;
Plover and Lapwing both write an "h" nobody says). `honey` and `public` were smaller, unexplained
key differences, not fitting any of the three patterns above - just confirmed cheaper and free. **If
this count goes above 0 again, something in collision resolution regressed - it is not something to
raise the baseline to tolerate.**

**`make = AEUBG` investigated and fixed - confirmed a genuine defect, not the 113-word pattern it
looked like at first (`rules/briefs.toml`: `make = "PHAEUBG"`).** One of Plover's own gold entries
writes "make" with a bare `P` instead of the real `M` chord `PH`; `matches_real_consonants` correctly
flags the bare `P` as holding only half a real chord, and its "repair" deletes it entirely rather
than completing it, landing on `AEUBG` - a vowel-only outline with zero trace of the word's actual
first sound. The earlier investigation found 113 *other* words relying on this exact same
fragment-drop mechanism deliberately (`the`→`-T`, `not`→`OT`, `never`→`EFR`, and so on) and
correctly concluded the shared heuristic itself must not change. `make` is simply one of the rare
cases on that list where the dropped result lands on something confusing enough (a word with no
trace of its own first sound at all, rather than a short known brief) to be worth a one-off
`[word]` override - exactly the per-word escape hatch that finding already recommended, now applied.
**This does not reopen the question of changing `best_brief`/`drop_fragments` themselves** - that
was a closed, investigated decision; this is just one of the data points it already anticipated
getting hand-fixed individually.

**`which` takes Lapwing's `KH`, not Plover's own (rejected) `WEU` (`rules/briefs.toml`:
`which = "KH"`).** Same fragment-check mechanism as `on`/`from`/`no`/`then`/`day` above - Plover's
own candidate `WEU` for "which" trips `matches_real_consonants` too (lyk's onset table gives a
"wh"-spelled word the full `WH` chord, so bare `W` is only half of it) - but here the user asked
for Lapwing's own, even shorter `KH` specifically, not a repair of Plover's rejected candidate.
`KH` belonged to `charge` (its own Plover brief, rank ~62,700) until this override: `which` is
roughly 300x more common (rank ~200) and simply gets processed first in the rank-ordered pass, so
`charge` falls back automatically to its own next candidate, `KHARPBLG` - its own plain rules
reading, which Lapwing agrees with, not an error or a downgrade worth worrying about. Confirmed
nothing else moved. **If `which`'s override is ever removed, don't "fix" it by pointing it back at
Plover's `WEU`** - that candidate is the one that fails the fragment check in the first place; the
literal `"KH"` is what makes this work.

**A continuation's stem drops its own silent `e` before a vowel-initial suffix
(`Rules::resolve`/`drop_silent_e`, mirrored in `generate_plugin`'s template).** `SR/+-G` ("have"
plus the standalone "-ing" suffix) read back as "haveing" - the stem's real text ("have", found by
the earlier stem-recognition fix) concatenated with the suffix's real text ("ing") with nothing in
between to apply basic English spelling. Confirmed this wasn't an isolated case: `AOUS/+-G` ("use"
plus "-ing") had the exact same problem, reading "useing" instead of "using" - anything with a
silent trailing `e` before a vowel-initial continuation was affected, not just "have". Fixed by
computing the very next piece's text *before* committing the stem, so a trailing silent `e` can
drop when it's genuinely silent (preceded by a consonant) and what follows starts with a vowel -
`entire` + `ly` correctly keeps its `e` (consonant-initial suffix), `agree` + `ing` correctly keeps
it too (the letter before the `e` is already a vowel, so it was never silent). Mirrored into the
generated plugin's own template and verified by compiling that generated source standalone and
calling its `resolve` directly - the two must never drift, since the deployed plugin is what a real
keystroke actually reaches. **Known, accepted gap**: "dye"/"singe" keep their `e` before "-ing" in
real English specifically to stay visually distinct from "die"/"sing" - not special-cased, since
these are rare exceptions to an otherwise reliable general rule, not a sign the rule is wrong.

**`but` = `UT` was the `make`/`AEUBG` bug again, not a real win (`rules/briefs.toml`:
`but = "PWUT"`).** `UT` looked like a genuine terse win over both Plover's `PWU` and Lapwing's
`PWUT` - but checking Plover's real dictionary file directly, there is no single-stroke `UT` entry
for "but" anywhere in it. Its actual entries are `PWU`, `PWUT`, `PWHUT`, `PWUTS`, `PWUTD`, `WUT`,
and a two-stroke `PWUT/UT`. `WUT` uses bare `W` for "but"'s real `/b/` sound (the full chord is
`PW`); the same fragment-safety check that produced `make`'s bug flagged the bare `W` and deleted
it outright, landing on `UT` - a candidate that was never one of Plover's own answers, with zero
trace of the word's actual first sound. Pinned to the correct phonetic reading, `PWUT`, which also
matches Lapwing exactly. Cost the quote-corpus benchmark a small amount of real ground (lyk's
Lapwing win rate 74.8% vs the 1.2%/7.5% aggregate savings thresholds, down slightly from before) -
thresholds lowered to match in the same commit, same as every other real-but-small regression this
session accepted for correctness. **If `but` is ever "optimized" back toward `UT`, it is
reintroducing this exact bug, not finding a real improvement** - check `matches_real_consonants`
for a bare, partial-chord Plover candidate before trusting any single-stroke win that looks too
good, the same lesson `make` already taught once.

**A physical ergonomics model, diagnostic only so far (`src/ergonomics.rs`).** Every cost
comparison in this theory until now was pure key count - two outlines with the same number of keys
were treated as equally good, even though some physical shapes are much harder to execute than
others. Built from the user's own real keyboard diagram (`# +S TK PW HR AO - EU * FR PB LG TS DZ`,
a standard steno layout, not a flat-QWERTY finger chart) and two corrections made mid-design by the
user, both load-bearing: (1) a single finger pressing both its own rows (`TK`="D", `PW`="B") is
*not* a penalty by itself - those are foundational, universal steno chords; the real cost is **row
transitions** walking a hand's active fingers in physical order (`SKPH`, bottom-bottom-top-top, one
transition, fine; `STWH`, bottom-top-bottom-top, three transitions, alternates every finger, bad) -
and (2) the join key is excluded from the pinky-usage tax entirely, since it's a separate physical
key present in nearly every continuation regardless of which words are involved, so taxing it would
penalize "more than one stroke," not any real word choice. The right pinky is a real, structural
asymmetry in the layout itself (it alone owns two columns, `T`/`S` and the further-stretch `D`/`Z`,
where every other finger owns one) - not something any theory choice can fix, only something a cost
model can account for.

Scored the real dictionary: ergonomic cost already correlates with frequency somewhat on its own
(top-1,000 words average 2.05, climbing to ~3.8 past 10,000) - reinforcing a trend already present,
not fighting it. The worst offenders at every frequency band are dominated by `STR`/`SPR` onset
clusters (`street`, `strong`, `straight`, `surprise`), which matches commonly-cited real-world
steno complaints, not a false positive in the model. **Deliberately not wired into `cheapest`,
`best_brief`, or any word's actual outline yet** - this is read-only and diagnostic, guarded by its
own ceiling test (`the_real_dictionary_stays_within_known_ergonomic_ceilings`) so a future rule
change can't silently make the dictionary physically worse to type without the test catching it,
but no outline has been changed on ergonomic grounds alone. The next real step, not yet started: a
one-off `[word]`-override pass on the ~20 top-1,000 words that score worst, the same safe,
individually-verified pattern already used for `make`/`on`/`but`/`which` - not a sweeping change to
the shared cost function. **If this module gets deleted "since it doesn't do anything yet,"** the
next regression it would have caught (a rule change that makes a whole class of words less
comfortable without anyone noticing, since key count alone can't see it) comes back with nothing to
catch it.

**Backup generation now composes two-stroke suffixes too (`Rules::two_stroke_suffix`,
`src/generate.rs`'s new "(a2)" composition).** The single-stroke stem+suffix backup pass
(`Rules::suffix_split`) never covered `-ism`, `-ary`/`-ery`/`-ory`, `-ical`, `-logy`, `-cy`/`-sy`,
`-ify`/`-fy` - a documented, deliberate deferral, since these two-syllable suffixes' own chords in
`rules/suffixes.toml` sometimes migrate a consonant backward onto the *first* stroke in a way plain
syllable rules wouldn't reproduce (`-ism`'s own `+EUZ/+-PL` ends its first stroke in a `z` that
ordinary syllabification would give the second syllable instead). Rather than risk re-deriving that
migration and getting it wrong, the fix finds the match by **spelling** instead of sound - a word
ending in one of these suffixes' own letters (`cubism` ending in "ism") has its stem looked up by
spelling directly (`word_outline.get("cub")`), and the suffix's own chord is reused exactly as
written in `rules/suffixes.toml`, never re-derived. Confirmed against a real production build: 327
new backup entries, including `cubism` → `KUB/+EUZ/+-PL`, `trilogy` → `TRAOEU/+OL/+SKWREU`,
`chemical` → `KEPL/+-BG/+UL`, `quantify` → `KWAPBT/+U/+TPAOEU`, `salary` → `SAL/+-R/+AOE`. These are
backups only - `cheapest()` still picks whatever's actually shortest for a word's primary entry, so
a word that already had a cheaper single-stroke fold from an unrelated mechanism keeps it; this
just means a stem a learner already knows now has a predictable, derivable path to these endings
too, when nothing shorter already existed. **If spelling-based stem lookup here ever gets replaced
with a sound-based one "for consistency" with the single-stroke version**, re-derive and verify the
consonant migration for every one of these suffixes first - that was the exact risk this design
choice was made to avoid.

**Three words pinned to a less-terse but more comfortable outline, the first real use of
`src/ergonomics.rs` (`rules/briefs.toml`: `against`, `protect`, `perhaps`).** Scanned the
top-1,000-by-frequency words for the worst `ergonomics::weighted_outline` scores, then checked each
one's own Plover candidate list for a different-shaped answer rather than inventing anything new.
`against` (`TKPWEPBS` vs deployed `STPWEPBS`, score 6 vs 9) cost nothing at all - same 8 keys,
different onset shape Plover already lists. `protect` (`PHREBGT` vs `PREBGT`, score 5 vs 7) and
`perhaps` (`PRAPGS` vs `PRAPS`, score 7 vs 9) each cost one extra key for a real reduction - the
trade the user explicitly asked for. **A fourth candidate, `trouble` (`TRUPBL` vs `TRUBL`, score 6
vs 8), was tried and reverted** - pinning it had no effect at all, because `trouble` also qualifies
for the automatic brevity-ladder pass (`Origin::Short` - see the `ords`/`every short form that is
free is added` design earlier in this log), which independently re-derives and re-claims the
cheaper `TRUBL` regardless of any `[word]` override, and `cheapest()` always prefers the globally
shortest entry for a word, not whichever one the brief table named. **This is a real, structural
limit on this whole approach**: an ergonomics-motivated `[word]` override only sticks when nothing
else in the generator independently reconstructs the word's cheaper natural form - always verify
with `cargo run --release -- resolve -- <old outline>` that it now shows "not in the dictionary"
before trusting an override took effect, the same check that caught this one.

## The core philosophy reversed (day 4): speed over per-word learnability

**This is a genuine, deliberate reversal of this project's original founding value, not an
incremental tweak — recorded here so nobody mistakes it for drift.** Every entry above this line
was written under the original rule: "a learner can read a finite rule and predict a stroke,"
applied to *every* word, which is why so many entries above justify an arbitrary brief as an
exception scoped to short, extremely common words only (`into`, `will`, `under`, `and`) while
treating rules-derived predictability as the default everywhere else. The user explicitly revoked
that default after reflecting on it directly: *"I want some balance, I get that rare words may need
to be learnable but we are making too many words 'easily learnable'... full speed everywhere is my
new ideology. break all of my old rules... we can always build multistroke phonetic fallbacks with
our + join key."*

**What this actually means, precisely** — because "full speed" alone is too vague to build against:
- A word's **primary, cheapest outline no longer needs any individual phonetic or spelling
  justification at all**, for any word, not just common ones. If a shorter or more ergonomic
  outline exists for a rare, multi-stroke word by any legitimate means (a gold-dictionary brief,
  brevity-ladder compression, a globally-optimized reassignment), it should be taken, even if
  nothing about the result is guessable from the word's sound or spelling.
- **The safety net stays, and is the reason this reversal is viable rather than reckless**: the
  `+`-joined, phonetically-derived multi-stroke fallback (the whole `docs/fallback.md` mechanism,
  `Rules::resolve`, the deployed plugin) is explicitly *not* going anywhere. A learner who forgets
  or never learned a word's optimized brief can still compose it sound-by-sound, slower, exactly as
  today. Speed optimization only ever applies to what counts as the *best* outline a real dictionary
  hands out; it never removes the ability to spell a word out by sound as a fallback of last resort.
- This directly motivates treating dictionary generation as a **global optimization problem**
  (assign every word its cheapest-available outline across the whole dictionary at once, not
  commonest-word-first-greedy-never-revisit) rather than a predictability-constrained rules engine
  with hand-picked exceptions - see the live discussion for the specific plan (an exact min-cost
  bipartite assignment over per-word candidates now, local-search refinement for cross-word costs
  like phrase-briefing and inter-stroke ergonomics later). That work is in progress as of this
  entry; treat any claim elsewhere in this doc that a word "should" look guessable as **historical
  record of a now-superseded default**, not a constraint binding future changes.

## The one rule that still overrides everything above

This was stated repeatedly and remains non-negotiable - speed-over-learnability does not touch it,
because it isn't about learnability at all, it's about correctness of method:

**No semantic, fuzzy, or embedding-based matching anywhere in dictionary generation.** Only exact
phonetic containment (is one word's pronunciation exactly a prefix/suffix of another's), exact
spelling-boundary tie-breaks, and exact, auditable cost functions (key count, the ergonomics score,
frequency weight) - including inside any future optimizer. Phonetic containment has a symbolic
ground truth already sitting in the pronunciation data — there's nothing to approximate, so a
learned similarity score would only add a miscalibrated threshold and a new source of false
positives that exact matching can't produce by construction. A global optimizer is not an exception
to this rule: minimizing an exact, readable cost function over a finite candidate set is still exact
computation, not a learned approximation — there is no gradient, no training data, and no model
whose internal weights replace an auditable reason. If a future change wants embeddings, cosine
similarity, or a trained model deciding *which outline a word gets* for *anything* in this pipeline,
that is a reversal of an explicit, considered decision, not a neutral addition — raise it as one.

**The structural one-stroke-per-word policy itself is unaffected by the philosophy reversal above**
— every word still gets exactly one primary single-stroke outline, with unlimited `+`-joined
multi-stroke backups available underneath it. What changed is *how arbitrary that one primary
outline is allowed to be*, not whether the one-outline/many-backups shape exists at all. See the
one-stroke-per-word entry above and
[the table of contents](README.md)'s own opening paragraph for the canonical wording.

## Dictionary generation became a real global optimization problem

**The day-4 philosophy reversal above was implemented, not just declared: `generate()`'s old
commonest-first greedy pass is gone, replaced by enumerate every candidate → peel the easy ones →
solve the rest as a real min-cost assignment problem (`src/generate.rs`: `Candidate`,
`candidate_cost`, `enumerate_candidates`, `solve_contested`).** The two complaints that motivated
this are both fixed directly, not worked around: `best_brief` used to source candidates from
*either* `setup.gold.plover` *or* `setup.gold.lapwing`, gated by a frequency window - lyk's own
rules-derived reading never got a chance to compete at all, which is why `take`'s own short Plover
brief (`TAEBG`) worked but `takes`'s otherwise-identical case (Plover's own listed candidate is the
longer `TAEUBGS`; lyk's own shorter spelling-variant reading, `TAEBGS`, was never tried against it)
didn't - confirmed fixed (`generate::tests::the_take_takes_inconsistency_is_gone_from_the_candidate_pool`).
And a word settled early (commonest-first) could never be revisited once a later, rarer word's own
circumstances might have made a different global arrangement cheaper overall - the root cause the
plan behind this traced `make`=`AEUBG`, `but`=`UT`, the ten stranded-word fixes, and the `on`/
`from`/`no`/`then`/`day`/`which` fragment-check overrides all back to.

**Every word's own candidate pool is now genuinely exhaustive**, merging in one list: both gold
dictionaries' listed outlines (`matches_real_consonants` now a blanket filter on every raw one, not
just a sort key `best_brief` used to apply only when ranking - new rigor over what shipped before),
lyk's own plain rules reading (always present, brief or not), the spelling-told-apart variant for a
homophone group (`rules.variant`, offered alongside the plain reading rather than replacing it - the
solver decides who keeps which, on cost and frequency, the same way it settles any other contested
text), every brevity-ladder shortening, and the full four-step star ladder on top of both the plain
reading and the spelling variant. A `[word]` literal override in `rules/briefs.toml` is the one
exception: it still forces exactly one candidate, a human decision never reopened to competition,
resolved first into `taken` the same way reserved outlines always have been.

**The headline acceptance gate**: all 18 `rules/briefs.toml` `[word]` overrides whose own comments
cited this exact architectural gap (`make`, `but`, `on`/`from`/`no`/`then`/`day`, `which`, and the
ten stranded-word fixes) were removed, and the new pipeline was confirmed to reproduce or beat every
one of them unprompted (`generate::tests::the_acceptance_gate_reproduces_or_beats_every_removed_override`).
Twelve reproduce or beat their own override's exact outline directly. The other six
(`on`/`from`/`no`/`then`/`day`/`which`) are a different shape of result worth spelling out: their
own literal text (`"OB"`, `"PR"`, `"PHO"`, `"THEB"`, `"TAEU"`, `"KH"`) was itself a hand-picked
exploit of a gap in `best_brief` - a half-held chord `matches_real_consonants` would reject from any
ordinary candidate, bypassed only because a literal override skips that check entirely. The new
blanket filter closes that exact gap, so none of those six literal texts is a legal candidate
anymore at all - what the new pipeline reproduces instead, unprompted, is each word's own correct,
full phonetic reading (`OPB`, `TPR`, `TPHO`, `THEPB`, `TKAEU`, `WHEUFP`) - exactly the alternative
each override's own comment already named as what the exploit was avoiding. That is "beats the
override," read correctly: the exploit is gone, and the fallback it was built to improve on is
exactly what ships now.

**Three real bugs were found and fixed while building this, each worth knowing about on its own:**

1. **Peeling by candidate-list length left ~99% of the dictionary "contested."** The first
   implementation tried to settle the easy majority of words before handing the hard remainder to
   the solver by looking for words with exactly one candidate left. That number is almost never one
   for any word with a plain rules reading, because the full four-step star ladder (source 5 in
   `enumerate_candidates`) is *always* present as a fallback, regardless of whether the word's own
   best pick is actually contested by anyone - so nearly every word starts with five-plus
   candidates. Measured directly: 99.3% of the dictionary "contested," and a solver built for "a
   sparse, few-hundred-to-low-thousand-word subgraph" turned a 100,000-word build into a multi-
   minute hang. Fixed by changing the peeling test to a word's own *cheapest remaining* candidate
   being the unique cheapest among every other remaining word (sort each list once, compare heads).
2. **That fix was still unsafe, in a way that silently produced a worse dictionary rather than
   hanging.** A word is only safe to peel when its own cheapest candidate's *text* has no other
   remaining word as a claimant *anywhere* in that word's own list - not merely "no one else's own
   *top* pick is this text." The difference showed up as a real regression: `on`'s own natural
   second choice, `OPB`, has no competing top pick from anyone else (`on` was still holding out for
   its own cheaper, contested `O`), so the far rarer word `gone` - for whom `OPB` genuinely was the
   cheapest, uncontested-by-anyone-else-at-the-top pick - got to claim it immediately, before `on`
   ever got a say. `on` fell all the way to `AUPB` (4 keys) instead of its own correct `OPB` (3
   keys) - worse than even the removed override. Fixed by only peeling a word's cheapest candidate
   when it appears in no other remaining word's list at all, full stop.
3. **Edge weight was first `common - rank`, per the plan's own original formula - measurably wrong
   once built.** The idea was "strictly frequency-ordered, bounded well under `u64` overflow," but
   `common - rank`'s own dynamic range collapses for any reasonably common word once `common` is
   100,000: "he" (rank 17) and "heh" (rank 2,191) differ by over 130x in actual usage count but
   under 2% in `common - rank`. That let a rare word's merely-somewhat-worse fallback out-arbitrate
   a far commoner word's much better option purely because the two words' weights looked nearly
   identical to the solver - measured directly as a real regression (lyk losing to Plover in
   aggregate keys/stroke, not just a small one). Fixed by using `setup.count[word]` - actual usage
   count, the same field `build`'s own "weighted by how often it is said" report already uses -
   which has the dynamic range frequency weighting is actually for, with no overflow risk at the
   scale this dictionary ever reaches.

**A real Hall-violation (more mutually-exclusive words in one connected component than it has
distinct texts for - a genuine multi-way homophone/near-homophone cluster tighter than its own star
ladder can resolve) is still possible, and is handled inside `solve_contested` itself now, not left
to the caller.** Every word gets one extra, private edge straight to the sink, costed at 1,000x that
word's own worst real option (which still scales with that word's own frequency weight, since the
real edges it's computed from already have it folded in) - this is what makes the solver correctly
sacrifice whichever word in an over-subscribed cluster has the least to lose, rather than whichever
one a raw max-flow search happens to run out of augmenting paths on first (confirmed this mattered:
an earlier version of this exact mechanism, before the per-word scaling, once left "cat" - the
common word in a `cat`/`kat`/`katt`/`khat` true-homophone cluster - without any outline at all,
while its three far rarer cluster-mates kept theirs).

**Every `HashMap`/`HashSet`-iteration-order-dependent point in the new pipeline now sorts
explicitly.** Rust's default hasher is randomized per process; collecting a word's own candidates,
a connected component's own members, or a text's own claimant list straight out of a `HashMap`
without an explicit, deterministic order meant a genuine cost-tie's *specific* winner (not the
total cost, which a true tie keeps identical either way) could differ run to run - confirmed
directly: the quote-corpus benchmark's own word-level counts moved by one or two words between
otherwise-identical runs before this fix. This project's whole ethos is exact and reproducible, not
"equally good, whichever one happened to come up" - every such collection point now sorts by
(cost, outline text) or by word text before being used.

**The contested-set-size invariant the whole algorithm choice rests on turned out to be a real,
different number than the plan estimated, not a bug to chase back to the original guess.** The plan
expected peeling to leave "hundreds to low thousands" of words genuinely contested (under 5% of the
dictionary); a real, correctly-peeled run measures roughly a quarter of it. The blanket
`matches_real_consonants` filter's deeper fragment-drops mean far more words now legitimately
converge on the same small handful of very short, nearly-bare outlines than a single-source brief
pool ever produced - a real, structural property of this design, confirmed not to be the 99.3%-style
bug above (see `quality::the_contested_set_after_peeling_stays_a_minority_of_the_dictionary`, set at
a real, measured ceiling rather than the plan's original estimate). What keeps the solver fast at
this scale isn't a small raw contested count - it's that the contested set splits into many small,
mutually-independent connected components (two words share an edge only if they share a candidate
text, transitively) rather than one giant tangle, so `solve_contested` runs once per small component
instead of once over the whole quarter of the dictionary at once.

**Net result, measured against the same quote-corpus benchmark and the same README benchmark this
session's earlier entries already cite**: lyk's aggregate keys across the 103-passage corpus went
from 4,640 to 4,409 (now 6.1% cheaper than Plover's 4,694 and 12.5% cheaper than Lapwing's 5,039, up
from 1.2%/7.5%); the words-individually-worse-than-Plover share, the one number this rewrite was
expected to cost something on, ended up landing back within a hair of its original value (9.5% ->
9.9%) once the peeling bug above was fixed, not stranded at the 11-12% an earlier, buggy measurement
of this same rewrite briefly showed. The README's own overall number moved from 3.40 to 3.37
keys/stroke, still beating Plover's 3.51 -> 3.52 and Lapwing's 3.93 -> 3.94. `quote_corpus`'s
thresholds and the README's own tables were both raised/refreshed in the same change, per this
project's established practice of never leaving a number stranded below what the theory now
actually does.

**A blanket fragment-safety filter on gold-brief candidates was added, then reverted the same
day, for directly contradicting the day-4 philosophy reversal (`gold_candidates`,
`src/generate.rs`).** The solver-rewrite plan (above) specified rejecting any raw Plover/Lapwing
candidate `matches_real_consonants` flagged as holding only part of a real sound - reasonable
under the *old* "every outline should be sound-derivable" philosophy, since that's exactly the
rigor `best_brief` lacked. But the plan was written and approved in the same session as, and
without re-checking against, the explicit reversal that a candidate holding no trace of a word's
own sound is no longer disqualifying by itself. The filter forced `which`, `on`, `from`, `no`,
`then`, and `day` back to their full phonetic readings, undoing six fixes made earlier the same
session specifically to shorten them - confirmed as a real regression when the user named `which`
→ `KH` as their favorite example of the new philosophy, immediately after the rewrite had taken
it away. Reverted: a raw gold candidate is now offered regardless of whether it holds a complete
or only a partial real sound: both the raw and `drop_fragments`' own, more aggressive repair
compete purely on cost, same as everything else. Net effect was not neutral - it was a further
*improvement*, not just restoring the six words: aggregate savings rose again (6.1%/12.5% ->
7.5%/13.8% vs Plover/Lapwing), word-level losses to Plover dropped (9.9% -> 5.7%). **If a future
change wants to reintroduce sound-correspondence as a requirement for any candidate, that is
reopening the day-4 reversal, not a neutral correctness fix** - the two are in direct, confirmed
tension, and the reversal is the one the user actually wants.

**The ergonomics model was rebuilt from a real hand-kinematics literature review, overturning its
own founding assumption (`src/ergonomics.rs`).** The original model (see its earlier entry above)
taxed the pinky specifically as "the weak finger" and weighted every row-mismatched adjacent finger
pair identically, both chosen by eye. A literature review (Häger-Ross & Schieber, *J Neurosci* 2000,
instrumented-glove finger-individuation measurements; Zatsiorsky/Li/Latash 1998 on multi-finger
force enslaving; standard hand anatomy on shared extensor tendons) found **the ring finger, not the
pinky, is the least independently-movable digit** - individuation indices of ~0.978 (index), ~0.944
(little), ~0.936 (middle), ~0.903 (ring), out of 1.0. Index and little both have at least a partial
independent extensor tendon; middle and ring share tendon slips with no independent muscle of their
own at all, and ring measures worst of the two. Revised: row-transition and gap costs are now
weighted by *which* fingers are involved (`index`=1.0x, `middle`/`little`=1.3x, `ring`=1.8x, a
direct, deliberately-conservative linear mapping from the measured individuation spread, kept under
2x per the research's own caution against overfitting an ~8%-spread proxy into a dramatic
multiplier) instead of a flat per-transition/per-gap constant. The pinky-usage tax is kept, but
re-justified on grip-strength/leverage research and this layout's own structural fact (the right
pinky alone owns two columns) rather than on independence, since the independence data says pinky
is *not* the worst digit - folding it into the same coupling table would have misattributed the
reason. Same-finger double presses (`TK`, `PW`) stay free, the one factor the research fully
confirmed as already correct. **A real, stated ceiling, not completeness**: the one study that
measured chord-typing performance directly against a motor-difficulty model (Raij/Gopher/Kimchi
1987) needed a second, independent "pattern-recognizability" factor to reach ~60% explained
variance - this module still doesn't attempt memorability, only the biomechanical half of the
picture. The dictionary-wide score scale grew roughly 7-8x as a direct consequence (top-1,000
average 2.05 -> 9.99 on the old vs. new scale), so `the_real_dictionary_stays_within_known_
ergonomic_ceilings`'s ceilings and its "bad" cutoff (8 -> 40) were rescaled proportionately, not
loosened - measured against the real post-revision distribution the same way every other threshold
in this project is. No change to `quote_corpus`'s numbers resulted (ergonomics remains a small
tie-break term under `candidate_cost`'s stroke/key weighting, and no candidate in that 103-passage
corpus happened to be exactly tied on strokes+keys with a different ergonomics ranking under the
new weights) - this revision is about correctness and rigor for future tie-breaks, not a
terseness win in itself. **If this gets reverted to flat per-transition weighting "for simplicity,"
it's reintroducing an assumption (pinky is the weak finger, all mismatches cost the same) the
literature specifically contradicts** - re-derive the research summary above before touching the
coupling weights, don't just restore the old numbers.

**The backup-entry composition pass (`src/generate.rs`, see `docs/syllables.md`) now arbitrates a
within-round text collision by cost and frequency, the same way the solver rewrite above already
fixed primary outlines, instead of leaving it first-come-first-served.** The solver rewrite
deliberately left this pass alone (it has to stay iterative - a round's own compositions depend on
the previous round's committed entries, so it can't be folded into one global solve), but
`try_add_backup` kept whichever of two colliding words `generate`'s own round loop happened to
visit first, with no cost or frequency comparison at all - the exact unfairness the rewrite fixed
everywhere else, just left over here. Worse than merely unfair: since that visit order comes from
iterating a `HashMap`'s own keys (`word_outline`), and Rust's default hasher is randomized per
process, this made the backup pass - and with it, the deployed dictionary and the quote-corpus
benchmark - genuinely non-deterministic across runs, not just "equally good, whichever word
happened to come up." Confirmed directly: two consecutive `build` runs of the pre-fix binary,
nothing else changed, differed on 259 of roughly 105,000 outline-to-word mappings, and
`quote_corpus`'s own word-level counts (and total key count) swung between runs of the unfixed
`cargo test` binary too.

Fixed by splitting `try_add_backup` into three pieces: `compose_backup` (the fragment-safety check
and text rendering, same as before, just without committing anything yet), `arbitrate_backup_round`
(new - groups one round's full list of composed candidates by text and, for any text more than one
candidate wants, keeps only the one minimizing `weight(word) * candidate_cost(outline)`, the
identical `edge_cost` formula and `setup.count`-based frequency weighting the real solver already
uses - no second, parallel cost system), and `commit_backup` (the final `taken` check and insert,
unchanged, still first-come-first-served but now only *across* rounds, where that is actually
correct: an earlier round's winner already committed fair and square under this same rule). Every
round now collects every direction's every candidate before committing any of them, instead of
committing eagerly as each is found. A collision's loser - whether cost-arbitrated away within the
round, or simply already `taken` from an earlier round - still counts in `backup_skipped`, so that
report line stays accurate. Covered by a new, standalone unit test
(`backup_round_arbitration_picks_lowest_weighted_cost_not_whoever_is_tried_first`) that feeds
`arbitrate_backup_round` the same two colliding candidates in both orders and checks the winner
doesn't move.

**One honest wrinkle, not a flaw in the fix**: a within-round collision is always on the exact same
rendered outline text, which means the same outline, which means `candidate_cost` is identical for
every contender in a group - the formula's cost term cancels, and the arbitration reduces, in every
real collision found, to simply "the lower-frequency word wins," not "the more common word gets the
nicer outline" the way intuition might expect. This is the formula this change was specified to use
(mirroring the solver's own `edge_cost` exactly, deliberately not inventing a new one), so it is
intentional, but worth flagging rather than glossing over.

**Measured effect**: a full build finds 4,838 real within-round collisions across roughly 105,000
words - not rare at all. One concrete example: `spade` (frequency weight 1,796) and `spayed`
(weight 76) both compose to the exact same text, `SPAEUD`; under this fix, `spayed` keeps it every
run, and `spade` falls back to its own separately-derived `SPAED` - no worse off (one fewer key,
if anything). `quote_corpus`'s own numbers land squarely inside the range the *unfixed* code's own
run-to-run noise already covered (4,341-4,342 total keys observed across repeated runs of the old
binary); the fixed binary now settles at 4,342 on every single run. No threshold in that file
needed raising - there is no real improvement or regression to bank here beyond the noise band
this fix eliminated, which is itself the point. **If this gets reverted to eager, per-candidate
commits "for simplicity," the backup pass goes back to being non-deterministic across runs**, not
just unfair in the abstract - re-confirm the 259-mapping, two-identical-runs test above before
assuming a revert is harmless.

**A second, much larger `quote_corpus` benchmark over real Project Gutenberg prose, run side by
side with the 103 hand-picked passages rather than replacing them.** A prior read-only research
pass raised a fair concern about the original corpus (`src/quote_corpus.rs`'s `PASSAGES`): 103
lines picked by a person, even one trying to be fair, is small enough that nobody could rule out
unconscious cherry-picking toward sentences that happen to flatter lyk's own brief list. The fix
isn't to delete the hand-picked corpus (it's cheap, fast, and still a real regression trip-wire)
but to add a much bigger, mechanically-sampled one nobody hand-picked at all, and compare the two
side by side for a cycle before trusting either alone. `src/fetch.rs` now downloads five
pre-1929, public-domain Gutenberg books across different registers into `data/corpus/` (not
committed, same as every other `data/` source): *Pride and Prejudice* (Austen, 1813, `files/1342/
1342-0.txt`), *Adventures of Huckleberry Finn* (Twain, 1884, `files/76/76-0.txt`), *A Tale of Two
Cities* (Dickens, 1859, `files/98/98-0.txt`), *Moby-Dick* (Melville, 1851, `files/2701/2701-0.txt`),
and *Walden* (Thoreau, 1854, `files/205/205-0.txt`) - the same transcendentalist tradition
`PASSAGES` already draws several of its own lines from, in full this time. A new, dependency-free
module, `src/corpus_extract.rs`, turns each raw download into short passages: strips the standard
`*** START OF ... ***`/`*** END OF ... ***` boilerplate markers, joins each blank-line-delimited
paragraph's wrapped lines and drops anything that reads as structure rather than prose (a heading,
an `[Illustration: ...]` caption - no lowercase letter, or a leftover `[`/`_` formatting marker),
splits on `.`/`!`/`?` with an abbreviation list (`Mr`, `Dr`, `St`, ...) and a single-capital-initial
check so "Mr. Darcy" and "J. Austen" don't get cut in half, keeps only 8-20 word sentences of
ordinary prose (no stray digits or brackets), and samples every Nth sentence (a fixed stride, not
random) down to roughly the target count - all unit-tested in isolation in the same file. A new
test, `lyk_beats_plover_and_lapwing_across_extracted_gutenberg_prose`, runs the identical scoring
code (`quote::rows_for`/`quote::cost`, same as the original) over the resulting 416 passages, with
its own threshold module (`extracted_threshold`) calibrated the same way as the original: measured
first, set a few points under. Needs `cargo run --release -- fetch` to have downloaded the books;
panics with that instruction rather than silently skipping if they're missing. Both tests share the
one production-scale dictionary build the whole suite already shares, so the second test adds
negligible wall-clock time to `cargo test --release` - the fixed dictionary-build cost (several
seconds) dominates either way; scoring 416 passages instead of 103 is sub-second extra work.

**One real formatting bug found and fixed along the way, worth flagging since it would otherwise
silently bias any text pulled from this era's prose against lyk specifically.** Gutenberg's own
plain-text convention writes an em dash as two bare hyphens with no surrounding space
(`word--word`), and this era's prose hyphenates whole multi-word phrases far more freely than
modern English does (`four-and-twenty`, `bread-and-cheese`, `printing-office`,
`Season-on-the-Line`). Left alone, either one reads as a single whitespace-delimited token, and
`quote::clean` (shared, unchanged, by both corpora and the live `quote` command) strips the
punctuation but keeps the letters - fusing two or more real words into one nonsense word
(`candlesthats`, `fourandtwenty`, `bathingplace`) that costs lyk an expensive spelled-out fallback
outline for a "word" that doesn't exist, while Plover and Lapwing simply have no entry for it
either and are silently excluded from that word's comparison entirely (`score`'s own
`if let (Some(ours), Some(theirs))` pattern) - inflating lyk's aggregate key total with no
counterweight on the other side, over a pure formatting artifact. `corpus_extract::normalize_dashes`
fixes this at the source by turning every hyphen and em/en dash into a literal space before
sentence-splitting, so a hyphenated phrase becomes separate real words - the fair, representative
choice anyway, since this theory's whole join-key design (`docs/join-key.md`) already treats
word boundaries as explicit and types a hyphenated phrase as separate strokes for its separate
words, never as one fused compound entry. Confirmed directly: before this fix, 374 words had a lyk
outline but no Plover/Lapwing entry at all, totaling 5,454 keys; after it, 322 words and 3,769 keys
- the remainder is genuine proper nouns and period vocabulary (`netherfield`, `pemberley`,
`queequeg`, `breezelessness`), not a tokenization bug. **If `normalize_dashes` gets simplified back
to only handling `--`/em-dash "for clarity," every hyphenated phrase in this corpus goes back to
being silently mangled into a fake word** - re-run `cargo test --release
lyk_beats_plover_and_lapwing_across_extracted_gutenberg_prose -- --nocapture` and check the "more
keys" and aggregate-savings lines haven't quietly gotten worse before assuming it's harmless.

**Measured, real numbers - and one genuinely surprising result among them, not swept under a
rug.** Today's run of the original 103-passage corpus (the baseline this entry's own thresholds
track, now ahead of this session's solver-rewrite threshold raises - see the "A real test suite"
entry above, whose own quoted 58.3%/73.8%/18.2%/33.0% numbers predate that rewrite and are stale
relative to current code): lyk beat Plover outright on 79/103 (76.7%) and Lapwing on 92/103
(89.3%); of 1,216 words, lyk cost fewer keys than Plover on 24.8% and Lapwing on 40.2% (more on
only 5.7%/2.5%); aggregate keys lyk 4,341 vs. Plover 4,694 vs. Lapwing 5,039 (lyk 7.5% cheaper than
Plover, 13.9% cheaper than Lapwing). The new 416-passage extracted corpus: lyk beat Plover outright
on only 177/416 (42.5%) and Lapwing on 201/416 (48.3%); of 5,595 words, lyk cost fewer keys than
Plover on 21.9% and Lapwing on 34.5% (close to the hand-picked corpus's own 24.8%/40.2% - individual
words are not where these two corpora disagree), more on 6.7%/3.1%; but in aggregate, lyk is
*more* expensive than both - 22,867 keys vs. Plover's 20,384 (12.2% worse) and Lapwing's 21,313
(7.3% worse). That aggregate flip is real and explained, not noise: real public-domain fiction is
dense with proper nouns and period-specific rare words Plover and Lapwing have no entry for at all
(see the formatting-bug paragraph above for the exact count - 322 words, 3,769 keys, all landing on
lyk's side of the ledger with zero counterweight on the other), which the clean, modern-vocabulary
103 essentially never hits. **This complicates, rather than confirms or refutes, the cherry-picking
concern that motivated adding this corpus**: the per-word numbers say the rules themselves aren't
meaningfully different on real prose than on the hand-picked lines, but the *aggregate* benchmark
headline number both corpora are built around turns out to be sensitive to a corpus's own
proper-noun density in a way that has nothing to do with how good the theory is - worth watching
before leaning on either corpus's aggregate-savings number alone in the future. The extracted
corpus's thresholds (`extracted_threshold` in `src/quote_corpus.rs`) are set a few points under
these measured numbers, same practice as everywhere else in this project - `AGGREGATE_SAVINGS_PCT_
PLOVER`/`_LAPWING` are deliberately negative (`-14.0`/`-9.0`), not a typo, to honestly reflect what
this corpus actually measures rather than silently exempting it from that assertion. **If these
negative thresholds get "fixed" to positive numbers without re-measuring, the test will simply fail
on the very first `cargo test` run** - re-run with `--nocapture` and read the real number before
touching either threshold module.

**Audited the top 20 Category-B (stray-fragment-leftover) words by usage weight from
`dump_stray_fragment_leftovers` (`src/rules.rs`); no `rules/briefs.toml` change resulted, and that
is the correct outcome, not a dodge.** The post-reversal philosophy (see the "core philosophy
reversed" entry above) already says a candidate holding only part of a required multi-key chord is
fine as long as it's cheapest, including a literal gold-dictionary entry - so the test applied here
was objective: is each word's *exact deployed outline* a real, literal entry in Plover's
(`data/dictionaries/plover-main.json`) or Lapwing's (`data/dictionaries/lapwing-base.json`) own
dictionary for that word? Checked directly against both files (the same two `Gold` loads, see
`src/gold.rs`) via `cargo run --release -- explain <word>`, not by eye. 19 of the 20 - `that` (TA),
`on` (OB), `no` (PHO), `like` (HRAOEUB), `got` (TKWOT), `from` (PR), `going` (KPW-G), `them` (THEP),
`then` (THEB), `day` (TAEU), `than` (THAP), `guys` (TKPAOEUS), `guy` (TKPAOEU), `which` (KH),
`believe` (PHREF), `next` (TPHEGT), `seen` (SAOEB), `guess` (TKWES), `open` (OEP) - are confirmed
verbatim in Plover's or Lapwing's own listing for that exact word: a real, established convention
these rules happened to reproduce, not an accident of `drop_fragments`'s own repair logic. Overriding
any of these would mean lyk diverging from steno convention for no benefit, so none were touched.

**The 20th, `beautiful` (deployed `PW-FL`), is not a literal gold entry, but turned out to be a
diagnostic false positive, not a real defect - confirmed experimentally, not assumed.** Gold's own
shortest single-stroke answer for this word is `PWAOUFL` (both Plover and Lapwing list it; lyk's
own full-phonetic two-stroke reading, `PWAOUT/-FL`, is also a literal Plover/Lapwing entry but costs
an extra stroke). Temporarily pinning `beautiful = "PWAOUFL"` and re-running the diagnostic
reproduced the *identical* flag (`onset /B/ (PW) stray=W; onset /Y/ (KWR) stray=W`) on gold's own
literal, fully-phonetic candidate - proof the flag isn't about which candidate wins, since even the
dictionaries' own real answer trips it. Root cause: `explained_elsewhere` (`src/rules.rs`) ORs in
every *other* syllable's own onset/coda target chord unconditionally, regardless of whether the
final outline actually spells that other syllable's sound at all. Here, the last syllable's onset
`F` has a plain target of `TP`, which shares the `P` key with the first syllable's `B Y` cluster
target `PW` - so `explained_elsewhere` strips `P` out of the first syllable's own onset bits as
"already accounted for," even though no candidate for this word ever puts a `T` anywhere near its
first stroke; `beautiful` never had `TP`'s own `T` down to begin with. That false subtraction leaves
only `W`, which then reads as a partial hit against both `B`'s target (`PW`) and `Y`'s target
(`KWR`) - a bug in the *checker*, not in outline selection, and unfixable by picking a different
candidate, since every real candidate shares the same first-syllable cluster and last-syllable `F`.
Left `beautiful` alone (no override - one would be pure churn, since the flag follows the word, not
the outline) and made no change to `explained_elsewhere` itself: this is the same shape of bug the
`district`/`describe`/`captain` fix already solved for a *shared coda*, now found going the other
direction (a *later* syllable's onset clashing with the *first* syllable's own onset cluster) -
`explained_elsewhere` would need to check that the other syllable's target chord is actually present
in the candidate's own bits before subtracting it, not just that the syllable exists. **If someone
revisits `dump_stray_fragment_leftovers`'s own word list later and sees `beautiful` (or anything
built from an onset cluster whose own consonants recur as a later syllable's plain onset) still
flagged, re-derive this analysis before assuming it needs a brief - check whether the flag survives
swapping in the word's own literal gold candidate first, the same test used here.**

**Correction to the entry immediately above this one: the extracted-corpus "aggregate flip" was a
real bug in `score()` itself, not an honest property of the corpus - found and fixed, not merely
documented.** The entry above describes `total_keys_lyk` (lyk's key total summed over *every* word
it resolves) being compared against `total_keys_plover`/`total_keys_lapwing` (each summed only over
words *that theory* has an entry for) and calls the resulting "aggregate flip" real and explained.
That framing was wrong about what kind of problem this was: comparing a sum over one population
(all words) against a sum over a different, smaller population (shared words) isn't a surprising
property of proper-noun-dense prose, it's an apples-to-oranges arithmetic error - the "explanation"
in the prior entry was actually a description of the bug's mechanism, not a justification for
leaving it. Fixed in `src/quote_corpus.rs`: `Stats` now tracks `total_keys_lyk_vs_plover` and
`total_keys_lyk_vs_lapwing` separately, each summed over exactly the same word population as the
gold theory it's compared against (see their doc comments on `Stats` for the exact reasoning);
`total_keys_lyk` is kept as a distinct, clearly-labeled "full coverage" figure in `report()`'s
output, never mixed into a savings ratio. The per-passage win/loss tally (`passage_wins_plover`
etc.) had the identical bug one level up - `p_keys` (lyk's whole-passage total) was compared against
`l_keys`/`w_keys` (each gold theory's shared-vocabulary total) - and is fixed the same way.

**The corrected numbers reverse the prior entry's headline finding entirely.** Both corpora now
show lyk cheaper than both theories, in aggregate, by consistent margins: the hand-picked 103 at
9.97% cheaper than Plover and 18.02% cheaper than Lapwing (passage win rate 85.4%/97.1%, up from the
stale pre-fix 76.7%/89.3% quoted above, which had the identical passage-level bug); the 416-passage
extracted corpus at 7.20% cheaper than Plover and 14.35% cheaper than Lapwing (passage win rate
80.8%/95.0%, not the prior entry's 42.5%/48.3%). That consistency between a hand-picked and a real,
unpicked corpus is the actual answer to the cherry-picking concern that motivated building the
extracted corpus in the first place: the win is real, not an artifact of which 103 passages got
picked, and it was never actually in doubt - the measurement was. Per-word numbers are essentially
unchanged by this fix (they were never part of the bug), confirming the prior entry's own aside that
"individual words are not where these two corpora disagree" was correct even though its aggregate
conclusion wasn't. Both `threshold` and `extracted_threshold` in `src/quote_corpus.rs` are
recalibrated to these corrected numbers, a few points under each as usual;
`AGGREGATE_SAVINGS_PCT_PLOVER`/`_LAPWING` for the extracted corpus are positive again (`6.0`/`13.0`,
not `-14.0`/`-9.0`) - if a future change makes them need to go negative again, that is a real
regression to investigate, not a number to "fix" by reverting this entry's logic.

**Phrase-briefing: two already-settled single-stroke primaries merged into one new, unclaimed
stroke when their keys are disjoint.** `setup.bigrams` (Norvig's Google Books Ngrams, loaded since
the ergonomics-model session but explicitly marked unused pending this work) finally gets a
consumer. Two candidate mechanisms for a phrase entry's key were considered: naive concatenation
(a word's own stroke sequence followed by the other's, joined normally) was rejected as a measured
no-op - it costs exactly `strokes(w1) + strokes(w2)`, identical to striking the two words
separately, nothing for the entry to actually do. Merging the two words' own single-stroke
*primaries* into one brand-new stroke (their bitwise OR, when disjoint) is the only one of the two
that moves any number this project measures: a real 2-strokes-to-1 compression. See
[phrase-briefing](phrases.md) for the full mechanism, scope boundary, and reasoning - this entry
covers only what was measured and decided, not re-derived here.

Scoped deliberately narrow for this first increment: both words' own settled *primaries* only
(never a bonus Short/Alternate/Backup entry, even a cheaper one - a primary is this project's most
stable invariant, and phrase-briefing is new enough to want the simplest possible correctness
argument), single-stroke components only (no reaching into a multi-stroke word's own join-key
structure), no phrase chaining (3+ words), a single non-fixed-point pass run once after the
backup-composition pass's own fixed-point loop finishes. `Origin::Phrase` is a new variant;
`generate::single_stroke_stems` (feeds `Rules::resolve`'s stem-recognition fallback - the `explain`
command and the deployed keymux WASM plugin's own fallback source) had to add it to its existing
`Origin::Suffix` exclusion, since a phrase's own "word" is two words' text, not a real single word a
continuation stroke could ever sensibly follow - confirmed by reading `resolve` in full, not just
tracing the one call site: `resolve` only ever *consumes* whatever map it's handed, so
`single_stroke_stems`'s own filter is the only place this needed fixing.

**The real eligible-set size was measured before picking a cutoff, not guessed.** The planning
pass's own placeholder, N=300 (ranked by raw bigram count), was explicitly flagged as unmeasured.
Scanning `setup.bigrams` (245,803 vocabulary-filtered pairs) sorted by count and checking each
prefix against the real, built dictionary: top 300 by count yields only 108 real (eligible *and*
still-unclaimed) phrase entries; the yield doesn't taper off sharply going further out - roughly a
third of any prefix of the sorted list converts into a real entry all the way past rank 10,000 (top
1,000 → 346; top 3,000 → 994; top 10,000 → 3,123; top 30,000 → 8,332). Chose **`PHRASE_TOP_N` =
10,000**: raw counts at that cutoff are still in the low millions (comfortably above the point
where a pair's commonality starts looking like coincidence rather than habit), and the real yield
(3,123 entries in the measured build) is substantial, not a token gesture. Full table and method in
[phrases.md](phrases.md)'s own "The threshold" section.

**A hand-worked check against "of the", "in the", and "I am" (per the verification plan) found a
real, honest mix, not a uniform win.** "of the" - the single highest-count bigram in the whole
corpus (2,766,332,391) - has genuinely disjoint primaries (`-F`/`-T`) and its merged stroke (`-FT`)
was genuinely free; it is the clearest, highest-value real hit this mechanism produces. "in the" is
*also* genuinely disjoint, but its merged stroke (`TPH-T`) was already claimed by `antonio` - an
honest miss, not a bug. "I am" surfaced a real, pre-existing data wrinkle: the bigram corpus's own
lowercase `"i"` is this project's single *letter* "i", not the capitalized pronoun (CMUdict/the
frequency corpus can't tell them apart; the letter won that one shared slot - see `generate()`'s own
comment on the pronoun "I"'s carve-out) - so `("i", "am")` is mechanically "letter i" + "am", and its
merged stroke (`AOEUPL`) was already claimed by the contraction "i'm" regardless. No incorrect
lowercase-"i" phrase text has ever shipped from this, but it's worth re-checking if `PHRASE_TOP_N`
is ever raised far enough for a different "i"-led bigram to win a free stroke under the same quirk.

**`quote_corpus.rs` gained a phrase-aware scoring extension, reported as its own, separate figures -
and it surfaced a real, honest lesson about which metric actually matters.** A naive
"total keys with phrases vs. without" comparison looked like a wash on the 103-passage corpus (0 net
change) and a very slight *regression* on the larger 416-passage corpus (-4 keys) - not a bug: a
phrase's merged stroke is, by construction, the bitwise OR of two disjoint strokes, so its key count
is always exactly the sum of the two words' own *primary* key counts, never fewer - merging only
ever drops the *stroke* count, by exactly one per hit, never the key count. The tiny net key
regression on the larger corpus comes from a second-order interaction: `generate::cheapest` credits
each word with its own globally cheapest entry of *any* origin (occasionally a backup cheaper than
that word's own primary - `sit`'s backup `ST` versus its own primary `SEUT`), while a firing phrase
is necessarily built from both words' own primaries specifically. Once this was understood, the
benchmark was changed to report *strokes* as the headline phrase-aware number (keys kept alongside,
for transparency) - strokes are the number this mechanism was built to move, and on both corpora
they strictly improve: 105 fewer strokes on the 103-passage corpus (1,266 → 1,161), 492 fewer on the
416-passage corpus (6,756 → 6,264) - these exact counts shifted slightly from the numbers first
measured here once the `explained_elsewhere` fix below changed which candidates compete, but the
mechanism's own real effect (strictly fewer strokes, roughly flat keys) is unchanged. **Do not
revert to a keys-only phrase-aware metric** - it
measures the wrong thing for this mechanism and will look flat or slightly negative even when the
feature is working exactly as designed.

**Fixed the real root cause behind `beautiful`'s diagnostic false positive (the entry above left it
unfixed, correctly, since the fix wasn't understood yet) - `explained_elsewhere` (`src/rules.rs`)
was subtracting bits a later syllable's own sound merely *intended*, not ones it actually struck on
the stroke being checked.** `explained_elsewhere` ORs every *other* syllable's own onset/coda target
chord into "already explained" bits unconditionally - it never checked whether that target's own
keys were actually present on the stroke at all. Concretely: `beautiful`'s last syllable's onset
"f" has a plain target of `TP`; its own gold brief, `PWAOUFL`, never has a `T` down anywhere, so
"f"'s sound isn't struck on this stroke at all - but `explained_elsewhere` subtracted `TP`'s `P` key
from the *first* syllable's own onset-fragment check regardless, since all that mattered before this
fix was that a later syllable existed, not whether its sound was actually there. Fixed by requiring
full containment - `bits & target.0 == target.0`, not just `syllable.len() > exclude` - before
treating a target as explained; this doesn't disturb the cases `explained_elsewhere` exists for
(`district`/`describe`/`captain`, see its own doc comment), where the other syllable's target really
is the stroke's whole content, not just a partial trace.

**Real impact, measured, not assumed: this was never a one-word bug, and it is not a diagnostic-only
change either - an earlier version of this entry claimed it was, from a flawed before/after
comparison (both "before" and "after" snapshots were accidentally built from the same, pre-fix
`rules.rs`, so the diff trivially showed nothing). Redone correctly, it shows a real, measurable
effect.** Re-running `dump_stray_fragment_leftovers`: Category B (stray leftover) dropped from 5,852
words (10.23% of words, 6.28% of weighted typing) to 2,950 words (5.159%, 5.890%) - nearly halved.
Rebuilding `out/lyk.json` from the real committed (pre-fix) `rules.rs` and from this fix, properly
isolated this time (one file reverted to `HEAD`, built, restored, built again, diffed both outputs
directly): **3,739 of 60,377 distinct words land on a different outline**, cascading through backup
composition and even which bigrams win a phrase-briefing merge (since both pull from the same
settled primaries). The headline frequency-weighted numbers move slightly against raw terseness -
overall keys/stroke 3.31 → 3.32, with every syllable tier at 2+ syllables also very slightly higher
(see this session's README table updates) - because the old, buggy `explained_elsewhere` was
*over*-stripping bits it wrongly treated as explained, which by accident often produced a shorter
(if mislabeled) repair candidate; correcting the check means some of those words lose that accidental
extra compression. **Surfaced this exact tradeoff - correctness of a still-current, documented policy
(README's own "Main features": a candidate may be arbitrary or a clean drop, never a confusing
partial fragment) versus this session's speed-above-nearly-everything direction - and the fix was
kept anyway**: the cost is small (≈0.3% on the overall number) and, per that explicit choice, not
worth keeping a bug whose only virtue was accidentally overachieving on a number nobody vouches for
dishonestly. If a future audit of this fix ever needs to re-derive why the dictionary's real output
changed here, this is the actual mechanism - not "none, it was diagnostic-only."

**A second, separate, genuinely unfixed bug remains in `beautiful` specifically - documented, not
swept under the rug.** `PWAOUFL`'s first syllable has *two* onset tokens, "b" (target `PW`) and "y"
(target `KWR`) - both legitimately struck at once on the same stroke (the brief's own real content).
"b"'s own flag is gone after this fix. "y"'s is not: "y"'s target `KWR` shares its `W` key with "b"'s
own, separate, legitimate `PW` - since "b" genuinely does strike `W`, checking "y"'s token against
the *same shared* onset bits finds a real partial overlap that has nothing to do with cross-syllable
explanation at all, since both tokens live in the *same* syllable. `explained_elsewhere` only ever
excludes *other syllables'* fully-struck targets - it has no notion of a *sibling* onset/coda token
within the same syllable explaining away a key that token legitimately needs too. Confirmed via
`rules::tests::beautiful_s_cross_syllable_false_flag_is_fixed_but_a_separate_sibling_token_overlap_
remains`, which asserts the real, current behavior (fixed half, unfixed half) rather than papering
over it. **Deliberately not fixed here**: a sibling-token-aware version would need to change what
counts as "explained" for *every* word whose onset or coda cluster has component tokens sharing a
physical key - given this narrower, already-understood fix alone already moved 3,739 words' outlines
(see above), a sibling-token-aware version should be assumed to have at least as broad a reach, not
assumed safe by analogy. Revisit only with the same before/after whole-dictionary diff and real
`quote_corpus`/README numbers this fix was ultimately measured against, and expect to make the same
kind of explicit speed-versus-correctness call this entry records.

**Phrase-briefing v2 (letting a word's cheaper `Origin::Backup` entry stand in for its primary as a
phrase component, not just `Origin::Brief|Rules|Spelling|Ladder(_)`) was measured and found not worth
building - a negative result worth recording, not a dead end to silently drop.** The idea (plan at
`/home/fib/.claude/plans/phrase-briefing-v2.md`, which also considered and rejected 3+-word chaining
for lack of any real trigram data source) was gated on a real measurement before writing any merge
logic, the same discipline `PHRASE_TOP_N` itself was chosen with. Real counts against the built
dictionary: 89 words have a multi-stroke primary but a single-stroke `Backup` entry (the plan's own
structural reasoning predicted ~0 here - a real, minor correction: direction-(b) backup composition's
stem lookup can pull from an *earlier round's own Backup entry*, not only a primary, so a chain of
backups can occasionally end up single-stroke even when the primary never was); 224 words have a
single-stroke primary *and* a cheaper single-stroke `Backup` (the real `sit`/`ST`-style count). Despite
313 candidate words between the two, letting them compete for a phrase merge at `PHRASE_TOP_N =
10,000` only added **7 more free entries** (3,119 → 3,126) from **10 more eligible candidates** (5,257
→ 5,267) - two orders of magnitude below the plan's own "material" bar (at least low hundreds, a
visible fraction of the existing ~3,108). Almost none of the 313 candidate words happen to also sit in
a top-10,000 bigram whose merge is both disjoint and still unclaimed. **Not implemented** - no
`phrase_component_strokes` function, no change to the phrase pass's call site. The diagnostic that
measured this (`generate::tests::dump_backup_phrase_component_gate`, `#[ignore]`d) is kept, the same
way `dump_phrase_eligible_set_sizes` was kept after informing `PHRASE_TOP_N`'s own choice - if
`PHRASE_TOP_N` is ever raised substantially, or the backup-composition pass changes meaningfully, this
is the diagnostic to re-run before assuming the conclusion still holds, rather than re-deriving it.

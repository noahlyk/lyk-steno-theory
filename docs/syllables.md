# Syllables & folding

[← back to the table of contents](README.md) · [← back to the main README](../README.md)

**Several syllables.** An unstressed syllable with a reduced vowel is not written: its
consonants join the stroke before it, in steno order (*living* HREUFG, *special* SPERBL),
except that a lone `-er` or `-ly` may land out of order as in Plover (*better* PWERT,
*lately* HRAEULT, *color* KORL). When they don't fit, the stem keeps the consonant it owns
and the rest is a continuation stroke of consonants only on the right hand — this is the
**[join key](join-key.md)** at work: *ended* EPBD/+-D, *breaking* PWRAEUBG/+-G, *goodness*
TKPWAOD/+-PBS, *harder* HARD/+-R, *confidence* KOPB/+-FD/+-PBS. When the reduced syllable's own
onset consonant can't go on the right hand either, it goes on the left of that same continuation
stroke and the rest stays on the right: *amber* APL/+PW-R, *gardner* TKPWARD/+TPH-R, *-nial*
+TPH-L. A reduced "-er" is always its `R`, in either shape. So the suffixes words fold onto
are a fixed set that never needs memorising apart from the rules, and `rules/suffixes.toml` gives
every one of them a stroke on its own too, Plover-suffix-dictionary style, so it can be added to
a stem that never happened to need it while the dictionary was built — see
**[the full suffix table](suffixes.md)** for every stroke this gives, old and new. Final `-ly` is
`+PHREU` after a cluster and a double `l` (*silly* SEUL/+EU); any other final `y` is `+EU`, and a
final `a` is a syllable of its own (*china* KHAOEU/+TPHA). Suffix clusters both dictionaries use
are rules: `-ment` -PLT, `-tion` -GS, `-ting` -GT, `-ding` -GD, and Plover's F for an S before
another consonant (*castle* KAFL, *lesson* HREFPB).

**The stem-plus-suffix reading is a candidate for the primary too.** A word whose last syllable
is reduced has two rules readings: the folded one (*pearly* PERL, *rabble* RABL) and the same
word with that syllable struck as its own standalone continuation stroke after the reading of what
is left (*pearly* PERL/+EU, *rabble* RAB/+-L; a second syllable can be peeled the same way,
*needlessly* TPHAOED/+-LS/+-L). The unfolded form has at least as many strokes as the folded one,
so it never wins for a word that can have the fold; it is the predictable way down when every
one-stroke form is taken (`PERL` is *pearl*, `P*ERL` is *peril*), ahead of a Plover two-stroke
brief or a bare `+*` marker stroke — see **[collisions](collisions.md)** for that order. It is
built from the word's own sounds alone, so it is in the pool from the start, unlike the backup
entries below, which need the stem to be a placed word.

**Backup entries: a shorter word's own outline, plus whatever is left over.** This is where [the
design principle stated up front](README.md) — one single-stroke outline per word, but unlimited
multi-stroke ones judged on predictability rather than brevity — turns into actual generated
entries. The fold above only
ever happens to keep a word consistent with a shorter relative when cramming everything onto one
stroke isn't physically possible — *breaking* keeps *break*'s own `PWRAEUBG` only because `PWRAEUBG`
plus a second "k" has nowhere to go; *building* has no such excuse, so its own cheapest outline is
the fused `PWEULGD`, unrelated to *build*'s own `PWEULD`. A separate, later pass in `generate()`
fixes this on purpose instead of by accident: for every word, if its last syllable is a reduced
syllable that could be struck as a standalone continuation stroke on its own ([`Rules::suffix_split`
in `src/rules/mod.rs`]), and the sounds left over exactly match some other already-placed word's own
pronunciation, that word's own real outline plus the continuation stroke is added as a *second*,
additional entry — `build`'s `PWEULD` plus `+-G` also reaches *building*, whether or not that agrees
with *building*'s own cheapest outline above. This never changes which outline wins as a word's own
primary (shown by `explain` and used everywhere else); it only ever adds an extra way in, so a
learner who knows the stem and the ending can always reach the word that way too.

**This pass is exhaustive, and runs to a fixed point.** Every word tries every split of its own
sounds against every direction below — not just the first one that works — and every safe,
non-colliding composed outline it finds is added; a word regularly ends up with several backup
entries at once, not just one. The index of "already-placed words" this searches is rebuilt after
each pass over the whole dictionary, now including whatever the previous pass just added, and the
whole thing repeats until a pass adds nothing new (capped at a handful of rounds as a safety net,
not a target to hit). This is what lets a backup chain through another backup: *green* + *house*
reaches *greenhouse* in the first pass; once *greenhouse* itself has that entry, the second pass
finds it as an already-placed word in its own right and reaches *greenhouses* by adding `+-Z` —
`RAOEPB/+HOUS/+-Z` — without *greenhouses* ever needing its own primary outline to be that shape.
*in* + *still* reaching *instill*, then *instill* + `+-G` reaching *instilling*, is the same
mechanism one level deeper.

The same pass also runs the other way, for a leading sound in front of a shorter word's own
*exact* pronunciation rather than a trailing one (`never` = the extra sound "N" in front of
*ever*'s own exact sounds `EFR`). Since the join key only ever continues a stroke forward, this
direction has to land on the *same* stroke as the matched word's own first one — the leading
sound's own keys folded onto it, the same way a prefix with its own fixed keys already does (see
[prefixes](prefixes.md)). When the leading sound's keys collide with one the matched word's first
stroke already uses, this direction can't rescue the word (`unclear` needs `un-`'s own `TPH`, which
shares an `H` with *clear*'s own "cl" onset cluster `KHR`) — skipped, not forced. Such a word is
still reached the other way round: a prefix struck as a stroke of its own, with the stem joined
after it (*unclear* `UPB/+KHRAOER`, see the prefix direction below).

**A third direction: two whole words back to back.** Neither of the above fits a word like
*myself* — *self* isn't a reduced syllable (`suffix_split` never fires), and there's no leading
sound left over once *self*'s own exact sounds are accounted for (the prefix direction has
nothing to attach). What *myself* actually is, is two already-placed whole words, each keeping its
own full outline: *my*'s `PHEU` followed by *self*'s own `SEFL`, every stroke of *self* marked as a
join continuation the same way any multi-stroke word's second stroke already is — `PHEU/+SEFL`.
This pass tries every split of a word's sounds and keeps the ones where an already-placed word
exactly accounts for each half (ignoring stress — a word often carries secondary stress inside a
compound that it keeps as primary stress alone, *myself* is `M AY2 S EH1 L F` but *my* alone is
`M AY1`). Sound alone finds too many candidates to trust on its own, though: *myself* also sounds
exactly like *mice* + *elf*, a real phonetic split with no relation to the word's own meaning or
spelling at all. Spelling is the filter — only a split where the word's own spelling is the literal
concatenation of both halves' spelling is kept (*myself* really is `"my"` + `"self"`, letter for
letter; `"mice"` + `"elf"` isn't `"myself"`'s spelling). More than one split can still pass that bar
at once (*outside* is genuinely both `"out"` + `"side"` and `"outs"` + `"ide"`) — both are kept, as
separate entries, the same exhaustive "add every one that's safe" rule every direction here
follows.

This direction is more fragile than the other two, because real speech routinely reshapes a
compound's internal boundary beyond a simple stress shift: `cannot`'s two `N`s collapse into one,
`yourself`'s `"your"` reduces from `Y AO1 R` to a single `ER0`, `anybody`'s `"body"` loses its
stressed vowel entirely. None of that is a stress-only difference an exact-phoneme match (even a
stress-blind one) can see past, so these are correctly left alone rather than guessed at — a real,
accepted gap, not a bug to chase. `everyone`, `yourself`, `cannot`, and `anybody` all miss this way
today.

None of the three sound-based directions above requires the match to *mean* anything: `doctor`'s own sounds happen
to be exactly `docked`'s plus an "-er", and `crash`'s are exactly a "k" in front of `rash`'s —
coincidences with no relation to either word's own sense, reused anyway, the same way a brief never
needed one either (see [the main README](../README.md#where-lyk-stands)). When more than one
already-placed word ties on sound alone (`build` and `billed` are exact homophones), the word's own
*spelling* breaks the tie if it can — `building` starts with `build`, not `billed` — the same way
spelling already settles a tie between words that sound alike elsewhere in this theory (see
[spelling disambiguation](spelling.md)); if spelling doesn't settle it either, the word is left
alone rather than guessed at. A collision with an outline something else already has is skipped and
counted (`build`'s own report line), never a hard failure, since this is a best-effort pass over
the whole dictionary rather than an explicit, human-authored override.

**A fourth direction: word families.** The directions above find a stem by sound, and the first
only fires when the ending is a syllable of its own, so `called` (`call` + a bare "d") and `looks`
(`look` + a bare "s") got no stem-plus-ending entry at all: in the top 10,000 words, only 22% of
"-ed" forms and 7% of "-s" forms had one. `src/family.rs` links words by *spelling*, confirmed by
*sound*: a word is a stem plus one of the endings `-s`, `-ed`, `-ing`, `-er`, `-est`, `-ly`,
`-ness`, `-ment`, `-able`, `-ful`, `-less`, `-ship`, `-hood`, where

- the stem's spelling is the word minus the ending, allowing a dropped "e" (*making* / *make*), an
  undoubled consonant (*running* / *run*), "i" back to "y" (*happiness* / *happy*), and the
  "-es"/"-ies" plurals (*boxes*, *cities*);
- some pronunciation of the word is some pronunciation of the stem followed by exactly the sounds
  that ending adds (`-ed`: D, T, or IH D; `-s`: S, Z, or IH Z; `-ing`: IH NG; and so on), so *used*
  is *use* + "-ed", never *us* + "-ed";
- the stem is said at least 1/50th as often as the word, so *need* is not *ne* + "-ed".
- when several stems pass, the most frequent one is kept, so *robber* is *rob* + "-er", not the
  name *robb*, and *lived* is *live* + "-d", not *liv*;
- the word is not listed in `rules/families.toml`'s `not_family`, which holds the words that pass
  every check above by coincidence: an ending that is really part of the word (*better*, *letter*,
  *moment*, *forest*, *feed*), or a name (*walter*, *peter*). Add a word there if `family` shows it
  linked to something it isn't built from.

Every linked word gets its stem's own outline, whatever it is (a brief, a rules reading, a star
form), plus the ending's standalone stroke from [the suffix table](suffixes.md): `KAUL/+-D` for
*called*, `HRAOBG/+-S` for *looks* (`+-S` when the word really ends in an "s" sound, `+-Z`
otherwise), `TPAOEUT/+-G` for *fighting*, `HAEP/+-PBS` for *happiness*. These skip the fragment
check the other directions run: both halves are already real outlines, so there is no partial chord
to catch. They chain like every other backup (*needlessly* = *needless* + `+-L` = *need* + `+-LS` +
`+-L`). With this, 94–100% of every ending's members in the top 10,000 can be typed as stem plus
ending. The rare misses are texts a primary outline already holds (*needed*'s `TPHAED/+-D` is
*kneaded*'s primary).

**The same links, with every common ending that attaches to a whole word.** The list above is the
one that also shapes a family's one-stroke outline (below), so it stays short. The backup joins use
a longer one, which never touches a primary: on top of those endings, `-ation` (*inform* + `+AEUGS`),
`-ible` (*sense* + `+-BL`, the same sound and stroke as `-able`), `-ward`, `-ism`, `-ist` (*art* +
`+EUFT`), `-ity`, `-ize`, `-ous`, `-ish` (*fool* + `+-RB`), `-ive`, `-age`, `-ic`, `-al`, `-en`,
`-or` (*sail* + `+-R`, the same sound and stroke as `-er`) and `-y`, each with the sounds it adds,
checked the same way (`BACKUP_ENDINGS` in `src/family.rs`). A word gets a join for *every*
ending it matches, not just the first: *artistic* is *artist* + `-ic`, and any other split that
checks out is kept beside it.

These endings match by coincidence far more often than the family ones (*final* sounds and spells
like *fine* + `-al`, *mayor* like *may* + `-or`, *andy* like *and* + `-y`), so a join through one
is held to three rules. It builds on the stem's own plain rules reading, never its brief, so it
always types the word's sounds: *andy* is `APBD/+AOE`, never *and*'s brief `SKP` + `+AOE`, and
*information* is *inform*'s `EUPB/+TPORPL` + `+AEUGS`. It passes the fragment check like any other
composition. And the stem is three letters or more, while a word in "-ly" never splits as a stem
in "l" plus `-y` (*really* is *real* + `-ly`). A word listed in `rules/families.toml`'s
`not_family` gets no stem-plus-ending join at all, by any direction, and no two-word join either
(*wither* is not *with* + `-er`).

**Prefixes struck before their stem.** A prefix can't fold onto a stem's first stroke when they
share a key, but it can always be a stroke of its own, with the stem's whole outline joined after
it: *unclear* `UPB/+KHRAOER`, *decode* `TKE/+KOED`, *rebuild* `RE/+PWHR`. The word is the prefix's
spelling plus a placed word, its pronunciation ends in the stem's, and the stem is at least three
letters and said at least 1/50th as often (*reason* is not *re* + *ason*). The prefixes and their
strokes, each the prefix's own syllable by the ordinary rules or, for one that is a word, that
word's outline: *un-* (*un*), *dis-* (*dis*), *mis-* (*mis*), *re-* `RE`, *pre-* `PRE`, *de-* `TKE`,
*co-* `KOE`, *en-* `EPB`, *im-* `EUPL`, *in-* `EUPB`, *non-* `TPHOPB`, *sub-* `SUB`, *trans-*
`TRAPBS`, *anti-* `APB/+TEU`, *inter-* `EUPB/+TER`, *semi-* `SEPL/+EU`, *over-* (*over*), *under-*
(*under*) (`BACKUP_PREFIXES`). A prefix that is a word of its own (*out*, *up*, *self*) needs no
entry: two whole words back to back already covers it.

**Every word, syllable by syllable.** All the directions above need some other word to build on.
The last one needs nothing: every word of two syllables or more also gets itself written one
syllable per stroke, joined, from each of its pronunciations (`Rules::syllable_paths`), so however
short its primary is (*profitable* `PROFBLT`, *machete* `PHEFPT`) there is always a way in a learner
can work out from the sound alone. The forms:

- its own folded outline, when that is more than one stroke (*profitable* `PROFT/+-BL`);
- every syllable a stroke, a reduced *last* syllable struck as its continuation with the vowel
  dropped, exactly as a suffix is (*table* `TAEU/+-BL`, *information* `EUPB/+TPOR/+PHAEU/+-GS`);
- every syllable a stroke with its vowel written (*table* `TAEU/+PWUL`, *machete*
  `PHU/+SHE/+TAOE`);
- each of those with every way of breaking the syllables, not one: the rule above gives a stressed
  short vowel the consonant after it (*lem-on* `HREPL/+UPB`), and the other split is offered too
  (*le-mon* `HRE/+PHUPB`). Between any two vowels, any run of consonants that can start a syllable
  may go to the next one, or none of them: *compromise* is *com-pro-mise* `KOPL/+PRO/+PHAOEUZ`,
  *comp-ro-mise* `KOPL/+PR/+PHAOEUZ` or *compro-mise* `KOPL/+PROPL/+AOEUZ`. One boundary is moved
  at a time, so a long word has a handful of splits, not hundreds. There is no one right place to
  break a word; the join key means none has to be the only one;
- for a word with a fixed prefix (*ex-* `KP`, *dis-* `STK`), each of those with the prefix folded
  onto the first stroke and with it struck as a syllable of its own.

In a syllable that keeps its vowel, an unstressed schwa can be written two ways: `U` and an
unstressed short i `EU`, whatever letter spells it, as a learner sounding the word out hears it
(`PRAF/+U/+TU/+-BL`); or with the vowel the spelling gives it, as a learner reading the word sees
it (`PRAF/+O/+TO/+-BL`, *compromise* `KOPL/+PRO/+PHAOEUZ`). Both are offered. A middle reduced
syllable keeps its vowel rather than becoming a bare consonant stroke: `PRAF/+U/+TU/+-BL` reads as
the word, `PRA/+-F/+-T/+-BL` does not. These candidates go through the same round as every other
backup, the same fragment check and the same rule when two words want one text (the commoner word
keeps it). With them, 98–99% of the words of two syllables or more in every frequency band have a
joined way in, and 91–97% have at least two (six to eight on average); before, 63–83% had one.

**The same joins without the join key.** Every join with two strokes or more also exists as plain
strokes, in `02-lyk-joinless.json` (`src/joinless.rs`): *compromise* `KOPL/PRO/PHAOEUZ` as well as
`KOPL/+PRO/+PHAOEUZ`. Struck as three separate strokes those are three words (*column*, *prah*,
*meyez*); the multi-stroke entry is what makes the engine read them as one. The file is derived
from the backups when the dictionary is written, so the two can never disagree, and it never
takes an outline anything else has. A spelling made only of strokes that are each a common word
(top 3,000, alone on a stroke) is left out, so two ordinary words struck one after the other stay
two words (*compress* `KUPL/PRES` would be *come* *press*). The join key stays the way to be sure:
with it a stroke can never be read as a word by itself.

**What a stem joins through.** Every direction above builds on a placed word's outline. Each
stem offers two: its fastest outline, and its own plain rules reading when that is a different
form of two strokes or more (*profitable* joins as `PROFBLT` and as `PROFT/+-BL`, so *profitably*
is reachable from either). A star-ladder marker stroke (`+*`, which only tells a word from one
that sounds the same) is dropped from a stem before it is joined: *sorting* is `SORT/+-G`, never
`SOR/+*/+TEUG`. And a join of three strokes or more whose last stroke fits on the stroke before it
(right-hand keys only, none already down, all later in steno order) is offered folded too, the
way a reduced syllable joins the stroke before it: *sensible* `SEPB/+SU/+-BL` and `SEPB/+SUBL`.
A join never folds down to one stroke: one-stroke outlines are the primaries' business.

**Families share one shape in their one-stroke outlines too.** Backups make every member
reachable as stem plus ending, but a member's own fastest outline used to be chosen alone, so
*build* was Plover's `PWHR` while *builds* was the unrelated `PWEULDZ`. Settling primaries now
runs twice. The first pass settles every word on cost alone. Every family root (a word with
family members) is then locked to the outline it won there, and every member is offered its
root's outline written by the **family rules**, applied to the root's last stroke
(`src/family.rs`, `shapes`):

1. **Add the ending's key.** *build* `PWHR` → *builds* `PWHR-Z`, *building* `PWHR-G`, *buildings*
   `PWHR-GZ`, *builder* `PWHR-R`, *builders* `PWHR-RZ`; *day* `AEU` → *days* `AEUZ`, *daily*
   `AEUL`; *start* `SART` → *started* `SARTD`, *starting* `SARGT`.
2. **If the root already holds that key, add the star too.** *need* `TPHAED` → *needed*
   `TPHA*ED`; *think* `-PBG` → *thinking* `*PBG`.
3. **"-ing" on a root ending in the K chord `-BG` swaps it for a bare `-G`** (Plover's own
   convention): *talk* `TAUBG` → *talking* `TAUG`, *look* `HRAOBG` → *looking* `HRAOG`, *speak*
   `SPAEBG` → *speaking* `SPAEG`. Rule 2 offers the star form as well, and cost picks.

**Irregular forms follow the same rules.** `rules/families.toml`'s `[past]` table ties about 150
irregular past tenses and participles to their stem as if they ended in "-ed", and `[plural]` ties
irregular plurals to theirs as if they ended in "-s". So a past tense is always written as its
stem plus `D`, however it's spelled: *built* `PWHR-D` (*build* `PWHR`), *said* `SAEUD` (*say*
`SAEU`), *told* `TELD`, *made* `AEUBGD`, *left* `HREFD` (*leave* `HREF`), *came* `KUPLD`, *given*
`EUFD`, *did* `*D` (*do* `-D` already holds the `D`, so rule 2 adds the star); plurals likewise:
*men* `PHAPBZ`, *women* `WOPLZ`, *children* `AOEULDZ`. A literal pin in `rules/briefs.toml` still
wins (*knew* `-PZ`, *thought* `-LGT`, *got* `-GTD`); each irregular form also gets its stem plus
`+-D`/`+-Z` as a backup.

**Prefixed words follow their stem.** A word that is `re-`, `pre-`, `un-`, `dis-`, `mis-`, `over-`,
or `under-` plus another word (by spelling, its pronunciation ending in the stem's, the stem at
least three letters and not far rarer; `rules/families.toml`'s `not_family` lists coincidences
like *remorse* and *present*) is offered the prefix's stroke followed by its stem's settled
outline: `re-` and `pre-` write `RE` and `PRE`, the rest write the prefix word's own outline (`un`
`UPB`, `dis` `TKEUS`, `over` `AUFR`). *rebuild* `RE/+PWHR`, *rebuilt* `RE/+PWHR-D`, *rebuilding*
`RE/+PWHR-G`, *remake* `RE/+AEUBG`. A one-stroke brief still wins where there is one (*unlock*
`TPHROBG`), since the prefix shape is always two strokes. `family build` lists prefixed members
under their stem's family.

Only one-stroke, right-hand-only endings take part (`-S`, `-Z`, `-D`, `-G`, `-R`, `-L`, `-FT`,
`-PBS`, ...). Then every word is settled again from scratch with those offers in its pool.
Locking the roots matters: without it, a root could trade its outline away in the second pass
(*come* gave `KUPL` to *cum* for the equally long `KOPL`) after its members had been offered shapes
built on the old one. A locked root may still move to a strictly cheaper outline of its own if the
re-solve frees one up; its family is then re-offered shapes on the new outline in the next pass.
Roots only ever get cheaper, so this settles, usually within two or three solves.

A family offer counts two keys cheaper than its raw cost (`FAMILY_DISCOUNT`). Without that, cost
alone kept a shorter unrelated brief (*building* stayed Plover's 4-key `PW-LG` over the 5-key
`PWHR-G`); with it, a member takes its family's shape unless that costs more than two extra keys.
The discount never buys a stroke: a stroke weighs 25 keys. In the top 10,000, members whose own
outline follows their family went from 66% to 89% for "-ed", 77% to 92% for "-s", and 56% to 83%
for "-ing". The same two-key preference applies when choosing which of a word's entries to call its
outline (`cheapest`, used by `lookup`, `quote`, and phrases), so a family shape isn't shadowed by a
bonus short form that only ties it (*talked* `TAUBGD`, not the bonus `TO*BGT`).

`cargo run --release -- family build` prints one family: every member, its stem and ending, its
own outline, its stem-plus-ending entry, and whether its own outline follows the family (one of
the rule shapes above, or keeping every key of its stem's outline). `cargo run --release --
family` with no word prints the whole dictionary's numbers by ending and the commonest members
that don't (*guys* `AOEUS` against *guy* `TKPAOEU`, *needs* `AOEDZ` against *need* `TPHAED`):
one-stroke briefs that beat the family shape by more than two keys. The stem-plus-ending entry is
always there for them as the predictable way in.

A collision *within the same pass* — two different words (or the same word, by two different
directions) composing the exact same text before either is committed — is not decided by whichever
one the pass happens to visit first. Every pass first collects every direction's every candidate
for every word, then, text by text, keeps only the one minimizing `weight(word) * cost(outline)` —
the identical formula, and the identical frequency weighting, the real solver uses for every
word's own primary outline (see [collisions](collisions.md)) — before committing any of the pass's
winners. Only a collision against a text an *earlier* pass already settled still works the old way
(first pass to settle it keeps it), since a later pass's own compositions genuinely depend on what
an earlier pass already committed and can't be reopened to a full solve the way primary outlines
were.

Next: **[phrase-briefing](phrases.md)**.

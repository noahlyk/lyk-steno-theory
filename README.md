# lyk-steno-theory

**[Learn lyk with the interactive lessons →](https://noahlyk.github.io/lyk-steno-theory/)**

[Lookup](https://noahlyk.github.io/lyk-steno-theory/#/lookup) ·
[Key chart](https://noahlyk.github.io/lyk-steno-theory/#/chart) ·
[Practice layout](https://noahlyk.github.io/lyk-steno-theory/#/keys) ·
[Reference](https://noahlyk.github.io/lyk-steno-theory/#/ref)

**lyk is steno with explicit word boundaries.** Every stroke after the first of a word carries a
join key (`+`), so a stroke without it always starts a word. Continuation strokes live apart from
the words, which lets a suffix be as short as `-G` or `-D` without ever being read as a word, and
lets a stem overlap with a word without a clash: `-D` is *do*, while `ED/+-D` is *ended*, the same
key doing two jobs.

This program generates the dictionary (Plover JSON) from phonetic rules, starting from what Plover
and Lapwing already do and measured against them. It is written in Rust, with the rules as data.
American English only.

## Where lyk stands

One overall number: keys pressed per stroke across every word at once, weighted by how often each
word is actually said — so common short words dominate it and rare long ones barely register,
which is closer to what typing each theory really feels like than counting every distinct word
once would be.

<!-- lyk:overall-table:start -->
| Theory | Keys/Stroke, weighted by how often each word is said |
|---|---|
| Lyk | 3.22 |
| Plover | 3.57 |
| Lapwing | 3.94 |
<!-- lyk:overall-table:end -->

This one number is a composition of two things: how many keys a stroke takes for a word of a
given syllable count, and how much of everything anyone actually types falls into that syllable
count — both broken out in full, along with the rest of the numbers, at
[the bottom of this README](#full-benchmarks). Run and checked automatically
(`score --check-readme`), not hand-typed.

Lyk beats Plover here. A brief is free to be a pure memorized shortcut with no phonetic
connection to its word at all (`it` on the onset `T-`, which "it" doesn't start with, is exactly
as fine as a brief has always been), and free to drop a sound entirely rather than attempt it
(`good` on bare `AOD`, no attempt at its "g" at all). What a brief can never do is take only
*part* of a key combination the rules only ever use *together* for one sound (`good`'s old Plover
brief kept half of its "g" sound's four keys and dropped the rest, reading back as a different,
wrong consonant) — a fragment like that is confusing in a way neither a whole memorized shortcut
nor a cleanly dropped sound ever is. See [the briefs chapter](https://noahlyk.github.io/lyk-steno-theory/#/ref/briefs) for the rule.

## Documentation

**Everything lives on [the site](https://noahlyk.github.io/lyk-steno-theory/)** (source in
[`site/`](site/README.md), deployed from `main`): 20 short lessons with an on-screen board and chord
drills, and the **[Reference](https://noahlyk.github.io/lyk-steno-theory/#/ref)**, which holds every
rule, table and mechanism, searchable: the [cheat sheet](https://noahlyk.github.io/lyk-steno-theory/#/ref/cheatsheet),
[how it works](https://noahlyk.github.io/lyk-steno-theory/#/ref/how-stroke-setup), the rules, the
[briefs](https://noahlyk.github.io/lyk-steno-theory/#/ref/briefs), the
[suffix table](https://noahlyk.github.io/lyk-steno-theory/#/ref/suffixes), punctuation and
fingerspelling, the [stroke read-back](https://noahlyk.github.io/lyk-steno-theory/#/ref/fallback). The
maintainer's logs stay here: [`docs/decisions.md`](docs/decisions.md) and [`docs/roadmap.md`](docs/roadmap.md).

The numbers themselves (this page's "Where lyk stands" above, and the full breakdown at
[the bottom of this README](#full-benchmarks)) aren't a separate page: they're meant to be seen,
not looked up.

## Main features

- **Optimized for speed, not for being guessable**: a word's single-stroke outline comes from
  whichever source — a known brief, the phonetic rules, a brevity-ladder compression — actually
  costs the least to type, not from a fixed rule about which words are "allowed" to be arbitrary.
  Nothing about it needs to be derivable from the word's sound or spelling.
- **One primary outline per word, extras only in free strokes**: a word has exactly one outline
  the theory teaches and fights for. Its other one-stroke forms (a Plover brief, the phonetic
  full form) are added last, only in strokes no other word needed, so they never cost another
  word anything.
- **An explicit join key**: continuation strokes are never mistaken for words, which is what lets
  suffixes be as short as a single key.
- **Measured, not asserted**: `build` and `score` print cost and match-rate against Plover and
  Lapwing on the same words, every time — see [the full benchmarks](#full-benchmarks).
- **A miss can be read back**: `resolve` and `lookup` read a stroke the dictionary doesn't have as
  something sensible, phonetically (no engine runs it; a live miss types its raw keys) — see [the stroke read-back page](https://noahlyk.github.io/lyk-steno-theory/#/ref/fallback).

## Use

```
cargo run --release -- fetch               # download the data into data/ (safe to re-run)
cargo run --release -- report              # what was loaded
cargo run --release -- score               # how often the rules match Plover or Lapwing
cargo run --release -- score --check-readme   # do this README's tables still match?
cargo run --release -- score --write-readme   # ...and rewrite any stale one in place
cargo run --release -- --version           # the git version baked in at compile time (also -V, or the hidden `version` command); `build` stamps a hash of the rules and generator source (not the commit) into out/lyk/.built and the query commands say once if it differs (silence: --no-stale-check or LYK_NO_STALE_NOTE=1); see the Reference, Build and files
cargo run --release -- build               # briefs + rules -> out/lyk/02-lyk-*.json (see the Reference, Build and files); quiet: the files, and a warning only if a word is stranded
cargo run --release -- build --report      # ...plus every diagnostic and the cost tables (the invariants are tests: cargo test --release)
cargo run --release -- deploy              # build, copied straight into keymux's live config
                                            #   (~/.config/keymux/steno/lyk by default; override with --to)
cargo run --release -- explain moon        # which rules wrote a word, next to Plover and Lapwing
cargo run --release -- family build       # build, builds, building...: each one's outline and its stem + ending entry
cargo run --release -- paths complete     # every way to write a word as joined strokes, and what became of each
cargo run --release -- mismatches --syllables 2 --after 500    # where the rules differ
cargo run --release -- learn               # which keys Plover and Lapwing use for each sound group
cargo run --release -- build --report --against magnum.rtf   # add a third dictionary to the cost tables
cargo run --release -- quote                        # a random quote, stroked lyk next to Plover and Lapwing
cargo run --release -- lookup "everybody's"         # a word, phrase or stroke, from the built dictionary (seconds)
cargo run --release -- resolve STKPW                # a stroke not in the dictionary, read back
cargo run --release -- keys                         # no word: type and the answer follows every key, all three theories
```

`lookup` (alias `keys`), `resolve` (alias `stroke`), `explain`, `ergonomics`, `paths` and `family`
with no word open an interactive session that is keymux's `steno keys`, but for all three theories:
type, and the answers update on every key (Tab keys, Ctrl-F best only, Enter keep, Esc clear).
`--rules FILE` and `--briefs FILE` use your own rules and briefs. `learn` is how most rules were
found: it counts what keys Plover and Lapwing use where they agree. [`tools/`](tools/README.md)
has the scripts for measuring what a change did to the built dictionary.

| What | Source |
|---|---|
| Pronunciations | CMUdict (primary); MFA US dictionary, then WikiPron US broad, as a fallback translated from IPA (`src/ipa.rs`) for the words CMUdict has no entry for at all |
| Word frequency | hermitdave/FrequencyWords (OpenSubtitles 2018), each name's count cut to a tenth so a name never outranks the word it sounds like ([collisions](https://noahlyk.github.io/lyk-steno-theory/#/ref/collisions)) |
| Bigram frequency | Norvig's Google Books Ngrams (`count_2w.txt`), which picks the word pairs that become [phrases](https://noahlyk.github.io/lyk-steno-theory/#/ref/phrases) |
| Dictionaries to match | Plover `main.json` and `commands.json`, Lapwing dictionaries |

## Full benchmarks

Everything behind [the one overall number above](#where-lyk-stands), in full. On the 100,000 most
common words that Plover or Lapwing write and have a pronunciation on record (CMUdict, or - for
the small slice CMUdict has no entry for at all - a translated fallback from MFA or WikiPron; see
[spelling mode](https://noahlyk.github.io/lyk-steno-theory/#/ref/spelling-mode)), the rules write 99.9% of them, and write the word the way
Plover or Lapwing does (ignoring the join key and the `KWR` or star they put on a continuation
stroke, which the join key replaces) for:

<!-- lyk:score-table:start -->
| Syllables | Same as Plover or Lapwing | Plover | Lapwing | Keys/Stroke (Lyk) | Keys/Stroke (Plover) | Keys/Stroke (Lapwing) |
|---|---|---|---|---|---|---|
| 1 | 89% | 77% | 82% | 5.50 | 5.65 | 5.84 |
| 2 | 59% | 44% | 51% | 5.76 | 4.90 | 5.25 |
| 3 | 37% | 25% | 31% | 5.54 | 4.78 | 5.14 |
| 4+ | 23% | 17% | 18% | 5.69 | 4.94 | 5.18 |
<!-- lyk:score-table:end -->

"Keys/Stroke" above counts every distinct word once, regardless of how rare it is. The same
figure, with each word counted not once but as many times as it is actually said — so a handful
of very common words dominate it and a long tail of rare ones barely move it:

<!-- lyk:usage-table:start -->
| Syllables | Keys/Stroke (Lyk, by frequency) | Keys/Stroke (Plover, by frequency) | Keys/Stroke (Lapwing, by frequency) |
|---|---|---|---|
| 1 | 2.82 | 3.19 | 3.59 |
| 2 | 4.67 | 4.70 | 5.00 |
| 3 | 4.67 | 4.92 | 5.25 |
| 4+ | 5.08 | 5.22 | 5.48 |
<!-- lyk:usage-table:end -->

One-syllable words drop sharply under this weighting for every theory, not just Lyk, because a
handful of extremely common short words ("the", "a", "to", "of"...) dominate real typing far more
than the long tail of rare one-syllable words.

How common each syllable count actually is — first counting every distinct word once:

<!-- lyk:syllable-words-table:start -->
| Syllables | Share of distinct words |
|---|---|
| 1 | 27% |
| 2 | 41% |
| 3 | 22% |
| 4+ | 10% |
<!-- lyk:syllable-words-table:end -->

...and then weighted by how often each word is actually said, which is "how much of everything
anyone types has this many syllables", not "how many words of this length exist":

<!-- lyk:syllable-usage-table:start -->
| Syllables | Share of words actually said |
|---|---|
| 1 | 80% |
| 2 | 15% |
| 3 | 4% |
| 4+ | 1% |
<!-- lyk:syllable-usage-table:end -->

Most of what anyone actually types is one or two syllables, which is why [the overall number at
the top of this README](#where-lyk-stands) sits so much closer to the one- and two-syllable
frequency-weighted figures above than to the longer-word ones — it's these two tables together:
how much of everything typed is each syllable count, and how many keys a stroke takes at each
one.

Longer words differ mostly where Plover and Lapwing have a brief, and where Lyk writes a bare
`+-R` for an `-er` they write `*ER`. `mismatches --syllables N --after 1500` lists what is left.

`build` on the words all three dictionaries write, strokes and keys per word:

| | Strokes | Keys |
|---|---|---|
| Lyk | 1.010 | 3.278 |
| Plover | 1.040 | 3.721 |
| Lapwing | 1.042 | 4.118 |

By how common the word is (`build` prints this), keys per word weighted by how often each word
is said, and then, counting each word once, how many of the band's words are a single stroke:

| Words | Share of all words said | Lyk | Plover | Lapwing | One stroke (Lyk) | Plover | Lapwing |
|---|---|---|---|---|---|---|---|
| top 100 | 58% | 2.11 | 2.48 | 2.91 | 100% | 100% | 100% |
| 100 to 1,000 | 28% | 4.38 | 4.81 | 5.17 | 100% | 98% | 98% |
| 1,000 to 5,000 | 10% | 5.44 | 6.14 | 6.42 | 98% | 84% | 82% |
| 5,000 to 10,000 | 2.4% | 6.15 | 7.16 | 7.55 | 91% | 63% | 62% |
| 10,000 to 20,000 | 1.4% | 7.25 | 8.00 | 8.43 | 79% | 46% | 44% |
| 20,000 to 50,000 | 0.7% | 7.95 | 8.82 | 9.21 | 68% | 29% | 30% |
| 50,000 to 100,000 | 0.1% | 8.78 | 9.76 | 10.07 | 57% | 17% | 20% |

The top thousand words are briefs in all three, which is why the dictionaries are close: the
rules can only move the rest. Lyk beats Plover across every frequency tier, including the
rarest words — every word's primary outline is settled by a single global optimization over every
legal candidate at once (briefs, the plain rules reading, brevity, the star ladder, all competing
on cost together), not a commonest-word-first pass that only ever revisits a word's own brief
attempt; see [briefs](https://noahlyk.github.io/lyk-steno-theory/#/ref/briefs) for how. Past rank 5,000 the one-stroke share is what
moves: every word gets its one-stroke consonant skeleton (see [how short](https://noahlyk.github.io/lyk-steno-theory/#/ref/brevity)), so
a word ranked 10,000–20,000 is one stroke 79% of the time, against 46% in Plover.

That is with the join key on every continuation stroke counted, and with a Plover outline counted
only when Lapwing has it too: Plover's `main.json` is full of misstrokes (`O` for *to*, `SP` for
*and*) that would otherwise make it look a third of a key cheaper than it is. About 4,600 words
end up with a star form because some other, more valuable word's candidate won the plain outline
instead (`site` and `sight`, `don` and `done`, but also any other collision the global optimizer
settles this way now, not just a sound-alike pair); `build` lists the commonest. About 500 of the
100,000 words can't be written by these rules yet, nearly all names and interjections (`hm`,
`schmidt`, `vlad`). On top of all that, about 205,000 backup entries give every word
`+`-joined ways in a learner can work out: a placed word's own outline plus an ending's stroke
(`build` + `+-G` reaches `building`, `fool` + `+-RB` reaches `foolish`), a prefix's stroke before a
placed word (`UPB/+KHRAOER` for `unclear`), two words back to back, and the word itself one
syllable per stroke (`TAEU/+-BL` for `table`). 98–99% of the words of two syllables or more have at
least one, at four to five each, however short their primary is — see
[syllables & folding](https://noahlyk.github.io/lyk-steno-theory/#/ref/syllables) for the rules.

These numbers move whenever a brief or suffix is added — see [the Reference](https://noahlyk.github.io/lyk-steno-theory/#/ref) for the
rule, and run `cargo run --release -- score --check-readme` before committing a rules change
to catch drift here automatically.

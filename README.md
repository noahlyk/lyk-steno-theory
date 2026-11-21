# lyk-steno-theory

**[Learn lyk with the interactive lessons →](https://noahlyk.github.io/lyk-steno-theory/)**

[Lookup](https://noahlyk.github.io/lyk-steno-theory/#/lookup) ·
[Key chart](https://noahlyk.github.io/lyk-steno-theory/#/chart) ·
[Practice layout](https://noahlyk.github.io/lyk-steno-theory/#/keys) ·
[Reference](https://noahlyk.github.io/lyk-steno-theory/#/ref)

**lyk is steno with explicit word boundaries.** Every stroke after a word's first carries a join key
(`+`), so a stroke without it always starts a word. Continuation strokes are separate from words, so
a suffix can be as short as `-G` or `-D` without being read as a word, and a stem can overlap a
word: `-D` is *do*, `ED/+-D` is *ended*.

The program generates the dictionary (Plover JSON) from phonetic rules, starting from Plover and
Lapwing and measured against them. Written in Rust, rules as data. American English only.

## Where lyk stands

Overall: keys per stroke across all words, weighted by how often each word is said. Common short
words dominate; rare long ones barely register.

<!-- lyk:overall-table:start -->
| Theory | Keys/Stroke, weighted by how often each word is said |
|---|---|
| Lyk | 3.25 |
| Plover | 3.57 |
| Lapwing | 3.94 |
<!-- lyk:overall-table:end -->

The number combines keys per stroke at each syllable count with the share of typed words at that
syllable count; both are in [the full benchmarks](#full-benchmarks) with the other numbers. Generated
and checked by `score --check-readme`, not hand-typed.

Briefs: a brief may be a memorized shortcut with no phonetic link to its word (`it` on the onset
`T-`), or may drop a sound (`good` on bare `AOD`). It may not take only *part* of a key combination
the rules use *together* for one sound (`good`'s old Plover brief kept half of the four keys of its
"g" and read back as a different consonant). See
[the briefs chapter](https://noahlyk.github.io/lyk-steno-theory/#/ref/briefs) for the rule.

## Documentation

**Everything is on [the site](https://noahlyk.github.io/lyk-steno-theory/)** (source in
[`site/`](site/README.md), deployed from `main`): 20 short lessons with an on-screen board and chord
drills, and the **[Reference](https://noahlyk.github.io/lyk-steno-theory/#/ref)**, a searchable page
set holding every rule, table and mechanism: the [cheat sheet](https://noahlyk.github.io/lyk-steno-theory/#/ref/cheatsheet),
[how it works](https://noahlyk.github.io/lyk-steno-theory/#/ref/how-stroke-setup), the rules, the
[briefs](https://noahlyk.github.io/lyk-steno-theory/#/ref/briefs), the
[suffix table](https://noahlyk.github.io/lyk-steno-theory/#/ref/suffixes), punctuation and
fingerspelling, [the solver](https://noahlyk.github.io/lyk-steno-theory/#/ref/fallback). Maintainer
logs: [`docs/decisions.md`](docs/decisions.md) and [`docs/roadmap.md`](docs/roadmap.md).

The numbers ("Where lyk stands" above, [the full benchmarks](#full-benchmarks)) have no separate page.

## Main features

- **Cost-based outlines**: a word's single-stroke outline comes from whichever source (a known
  brief, the phonetic rules, a brevity-ladder compression) costs least to type. It need not be
  derivable from the word's sound or spelling.
- **One primary outline per word**: the theory teaches one outline per word. Other one-stroke forms
  (a Plover brief, the phonetic full form) are added last, only in strokes no other word needed.
- **Explicit join key**: continuation strokes are never read as words, so suffixes can be a single
  key.
- **Measured**: `build` and `score` print cost and match-rate against Plover and Lapwing on the
  same words; see [the full benchmarks](#full-benchmarks).
- **Solver for misses**: a keymux plugin reads a stroke or `+` run missing from the dictionary as
  the word its keys sound like (beam search over every word's pronunciation, about 2 ms a run), so
  `deploy` ships no backups; see [the solver](https://noahlyk.github.io/lyk-steno-theory/#/ref/fallback).

## Install

The binary is `lyk`. Needs a Rust toolchain.

```
make install                   # to /usr/local/bin (PREFIX=/usr and DESTDIR work; make uninstall removes it)
make pkg && sudo pacman -U dist/lyk-steno-theory-*.pkg.tar.zst   # Arch package of this checkout (or makepkg -si)
```

Or unpack a release tarball (`lyk-VERSION-ARCH.tar.gz` on the releases page) and copy `lyk` onto
your `PATH`. Then, from any folder: `lyk fetch`, `lyk build`, `lyk deploy` (needs
[keymux](https://github.com/noahlyk/keymux)). `lyk where` shows the folders in use.

Installed, the data is kept in `${XDG_DATA_HOME:-~/.local/share}/lyk/data` and the build output
in `${XDG_STATE_HOME:-~/.local/state}/lyk/out`; inside a checkout it is `data/` and `out/` as
below. `LYK_DATA`, `LYK_OUT` or `--data DIR` override either (see the Reference, Build and files,
Install and paths). `make check` runs what CI runs.

## Use

The commands below use `cargo run --release --` in a checkout; installed, write `lyk` instead.

```
cargo run --release -- fetch               # download the data into data/ (safe to re-run), the other theories included
cargo run --release -- report              # what was loaded
cargo run --release -- score               # how often the rules match Plover or Lapwing
cargo run --release -- score --check-readme   # do this README's tables still match?
cargo run --release -- score --write-readme   # ...and rewrite any stale one in place
cargo run --release -- --version           # the git version baked in at compile time (also -V, or the hidden `version` command); `build` stamps a hash of the rules and generator source (not the commit) into out/lyk/.built and the query commands say once if it differs (silence: --no-stale-check or LYK_NO_STALE_NOTE=1); see the Reference, Build and files
cargo run --release -- build               # briefs + rules -> out/lyk/02-lyk-*.json (see the Reference, Build and files); quiet: the files, and a warning only if a word is stranded
cargo run --release -- build --report      # ...plus every diagnostic and the cost tables (the invariants are tests: cargo test --release)
cargo run --release -- deploy              # build, copied straight into keymux's live config with the solver
                                            #   plugin and no backups (the solver replaces them)
                                            #   (~/.config/keymux/steno/lyk by default; override with --to)
cargo run --release -- explain moon        # which rules wrote a word, next to Plover and Lapwing
cargo run --release -- family build       # build, builds, building...: each one's outline and its stem + ending entry
cargo run --release -- paths complete     # every way to write a word as joined strokes, and what became of each
cargo run --release -- mismatches --syllables 2 --after 500    # where the rules differ
cargo run --release -- learn               # which keys Plover and Lapwing use for each sound group
cargo run --release -- build --report --against magnum.rtf   # add a third dictionary to the cost tables
cargo run --release -- quote                        # a random quote, stroked lyk next to Plover and Lapwing
cargo run --release -- lookup "everybody's"         # a word, phrase or stroke, from the built dictionary (a tenth of a second; seconds the first time after a build)
cargo run --release -- resolve STKPW                # a stroke not in the dictionary: what the solver types
cargo run --release -- solve KOPL/+PU/+TER          # every reading of a run, cheapest first (--eval N: how often it is right)
cargo run --release -- keys                         # no word: type and the answer follows every key, all three theories
cargo run --release -- practice                     # a typing drill: 50 common words, no outlines or hints (see Practice)
```

`lookup` (alias `keys`), `resolve` (alias `stroke`), `explain`, `ergonomics`, `paths` and `family`
with no word open an interactive session, keymux's `steno keys` for all three theories: answers
update on every key (Tab keys, Ctrl-F best only, Enter keep, Esc clear). `--rules FILE` and
`--briefs FILE` use your own rules and briefs. `learn` counts which keys Plover and Lapwing use
where they agree (the source of most rules). [`tools/`](tools/README.md) has scripts for measuring
what a change did to the built dictionary.

| What | Source |
|---|---|
| Pronunciations | CMUdict (primary); MFA US dictionary, then WikiPron US broad, fallback translated from IPA (`src/ipa.rs`) for words CMUdict lacks |
| Word frequency | hermitdave/FrequencyWords (OpenSubtitles 2018), each name's count cut to a tenth so a name never outranks the word it sounds like ([collisions](https://noahlyk.github.io/lyk-steno-theory/#/ref/collisions)) |
| Bigram frequency | Norvig's Google Books Ngrams (`count_2w.txt`), which picks the word pairs that become [phrases](https://noahlyk.github.io/lyk-steno-theory/#/ref/phrases) |
| Dictionaries to match | Plover `main.json` and `commands.json`, Lapwing dictionaries |
| Other theories' briefs | Stan's (a Magnum derivative), Josiah theory and thirteen Stenovations theories (StenEd, StarTran, Steno Foundation, Tri-C, RealWrite and more), `dictionaries/theory-*.json`: a pool of one-stroke briefs, each kept only if it saves a stroke ([collisions](https://noahlyk.github.io/lyk-steno-theory/#/ref/collisions)) |

## Practice

`lyk practice` is a monkeytype-style word feed in the terminal: a few lines of upcoming words, the
current one highlighted, what you typed green or red against it, a quiet speed and accuracy line
(`--quiet` hides it) and nothing else, no outlines, keyboard or hints. Your steno machine types the
words through keymux as ordinary characters, so it only reads plain key presses.

```
lyk practice                                  # 50 words from the 1,000 commonest
lyk practice --time 60 --rank 1-200           # one minute from the 200 commonest
lyk practice --source mistakes --outlines     # your drill words; the results show their outlines
lyk practice --source phrases --count 100     # the phrasing grammar's phrases, typed as text
lyk practice --history                        # recent runs and the trend
```

Space or Enter commits a word; a wrong word is an error and the feed moves on
(`--stop-on-error` requires it right). Backspace or `*` deletes, Ctrl-Backspace or Ctrl-W deletes a
word, Esc or Ctrl-C ends the run with the results so far. Sources: `common` (`--rank`, `--bands`,
`--weighted`), `mistakes` (`lyk mistakes --drill`), `file` (`--file`), `phrases`, `quotes`.
`--count`, `--time`, `--seed`, `--no-repeat`, `--lowercase-only`, `--min-len`, `--max-len`, `--names`
shape the feed. The results show speed, raw speed, accuracy, errors, time, consistency, the ten
slowest words and the missed ones; each run is added to `practice.jsonl` in the state folder (`out/`
in a checkout). All flags: the Reference, Build and files.

## Full benchmarks

The numbers behind [the overall number above](#where-lyk-stands). Basis: the 100,000 most common
words that Plover or Lapwing write and that have a pronunciation on record (CMUdict, or for the
few it lacks a translated fallback from MFA or WikiPron; see
[spelling mode](https://noahlyk.github.io/lyk-steno-theory/#/ref/spelling-mode)). The rules write
99.9% of them. The table gives the share written as Plover or Lapwing writes them (ignoring the
join key and the `KWR` or star they put on a continuation stroke, which the join key replaces):

<!-- lyk:score-table:start -->
| Syllables | Same as Plover or Lapwing | Plover | Lapwing | Keys/Stroke (Lyk) | Keys/Stroke (Plover) | Keys/Stroke (Lapwing) |
|---|---|---|---|---|---|---|
| 1 | 89% | 77% | 82% | 5.57 | 5.65 | 5.84 |
| 2 | 59% | 44% | 51% | 6.07 | 4.90 | 5.25 |
| 3 | 37% | 25% | 31% | 5.99 | 4.78 | 5.14 |
| 4+ | 23% | 17% | 18% | 6.22 | 4.94 | 5.18 |
<!-- lyk:score-table:end -->

"Keys/Stroke" above counts each distinct word once. The same figure with each word counted as many
times as it is said (common words dominate, rare ones barely move it):

<!-- lyk:usage-table:start -->
| Syllables | Keys/Stroke (Lyk, by frequency) | Keys/Stroke (Plover, by frequency) | Keys/Stroke (Lapwing, by frequency) |
|---|---|---|---|
| 1 | 2.84 | 3.19 | 3.59 |
| 2 | 4.77 | 4.70 | 5.00 |
| 3 | 4.73 | 4.92 | 5.25 |
| 4+ | 5.25 | 5.22 | 5.48 |
<!-- lyk:usage-table:end -->

One-syllable figures drop for every theory under this weighting: a few very common short words
("the", "a", "to", "of"...) dominate real typing.

Share of each syllable count, first counting each distinct word once:

<!-- lyk:syllable-words-table:start -->
| Syllables | Share of distinct words |
|---|---|
| 1 | 27% |
| 2 | 41% |
| 3 | 22% |
| 4+ | 10% |
<!-- lyk:syllable-words-table:end -->

...then weighted by how often each word is said ("how much of everything typed has this many
syllables"):

<!-- lyk:syllable-usage-table:start -->
| Syllables | Share of words actually said |
|---|---|
| 1 | 80% |
| 2 | 15% |
| 3 | 4% |
| 4+ | 1% |
<!-- lyk:syllable-usage-table:end -->

Most typed words have one or two syllables, so [the overall number](#where-lyk-stands) tracks the
one- and two-syllable frequency-weighted figures more than the longer-word ones: it combines these
two tables (share of typed words per syllable count, keys per stroke at each).

Longer words differ mostly where Plover and Lapwing have a brief, and where Lyk writes a bare `+-R`
for an `-er` they write `*ER`. `mismatches --syllables N --after 1500` lists the rest.

`build` on the words all three dictionaries write, strokes and keys per word:

| | Strokes | Keys |
|---|---|---|
| Lyk | 1.009 | 3.300 |
| Plover | 1.040 | 3.721 |
| Lapwing | 1.042 | 4.118 |

By word frequency (`build` prints this): keys per word weighted by how often each word is said,
then the share of the band's words (each counted once) that are a single stroke:

| Words | Share of all words said | Lyk | Plover | Lapwing | One stroke (Lyk) | Plover | Lapwing |
|---|---|---|---|---|---|---|---|
| top 100 | 58% | 2.12 | 2.48 | 2.91 | 100% | 100% | 100% |
| 100 to 1,000 | 28% | 4.41 | 4.80 | 5.16 | 100% | 98% | 98% |
| 1,000 to 5,000 | 10% | 5.52 | 6.17 | 6.45 | 97% | 83% | 81% |
| 5,000 to 10,000 | 2.3% | 6.18 | 7.25 | 7.66 | 90% | 61% | 59% |
| 10,000 to 20,000 | 1.2% | 7.26 | 8.14 | 8.57 | 78% | 43% | 42% |
| 20,000 to 50,000 | 0.6% | 7.88 | 8.91 | 9.29 | 67% | 28% | 29% |
| 50,000 to 100,000 | 0.1% | 8.63 | 9.83 | 10.12 | 57% | 17% | 20% |

The top hundred words are briefs in all three (past that, lyk drops a sound only where it saves a
stroke, see [faithful outlines](https://noahlyk.github.io/lyk-steno-theory/#/ref/collisions)), so
the dictionaries are close there; the rules move the rest. Lyk's keys per word are lower than
Plover's in every frequency tier. Every word's primary outline is settled by one global
optimization over all legal candidates at once (briefs, the plain rules reading, brevity, the star
ladder, competing on cost together), not a commonest-word-first pass; see
[briefs](https://noahlyk.github.io/lyk-steno-theory/#/ref/briefs). Past rank 5,000 the one-stroke
share is what moves: every word gets its one-stroke consonant skeleton (see
[how short](https://noahlyk.github.io/lyk-steno-theory/#/ref/brevity)), so a word ranked
10,000–20,000 is one stroke 78% of the time, against 43% in Plover.

Counting notes: the join key on every continuation stroke is counted, and a Plover outline is
counted only when Lapwing has it too (Plover's `main.json` has misstrokes such as `O` for *to* and
`SP` for *and* that would make it look a third of a key cheaper). About 4,600 words get a star form
because another, more valuable word's candidate won the plain outline (`site` and `sight`, `don`
and `done`, and any other collision the global optimizer settles this way); `build` lists the
commonest. About 500 of the 100,000 words can't be written by these rules yet, nearly all names and
interjections (`hm`, `schmidt`, `vlad`). About 205,000 backup entries give every word `+`-joined
ways a learner can work out: a placed word's own outline plus an ending's stroke (`build` + `+-G`
reaches `building`, `fool` + `+-RB` reaches `foolish`), a prefix's stroke before a placed word
(`UPB/+KHRAOER` for `unclear`), two words back to back, and the word one syllable per stroke
(`TAEU/+-BL` for `table`). 98–99% of words of two syllables or more have at least one, at four to
five each, however short their primary is; see
[syllables & folding](https://noahlyk.github.io/lyk-steno-theory/#/ref/syllables).

These numbers move whenever a brief or suffix is added; see [the Reference](https://noahlyk.github.io/lyk-steno-theory/#/ref)
for the rule, and run `cargo run --release -- score --check-readme` before committing a rules
change to catch drift here.

## License

[GPL-3.0-or-later](LICENSE): free software; you may use, change and share it, and anything you
distribute that is derived from it must be released under the same license with its source. Credits
for derived files and the licenses of the data `lyk fetch` downloads are in [NOTICE](NOTICE).

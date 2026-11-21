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

The program generates the dictionary (Plover JSON) from phonetic rules, measured against Plover and
Lapwing. Rust, rules as data. American English only.

## Where lyk stands

Overall: keys per stroke across all words, weighted by how often each word is said.

<!-- lyk:overall-table:start -->
| Theory | Keys/Stroke, weighted by how often each word is said |
|---|---|
| Lyk | 3.25 |
| Plover | 3.57 |
| Lapwing | 3.94 |
<!-- lyk:overall-table:end -->

The number combines keys per stroke at each syllable count with the share of typed words at that
syllable count ([the full benchmarks](#full-benchmarks)). Generated and checked by
`score --check-readme`.

Briefs: a brief may be a memorized shortcut (`it` on `T-`) or drop a sound (`good` on bare `AOD`); it
may not take only part of a key combination the rules use together for one sound. Rule:
[briefs](https://noahlyk.github.io/lyk-steno-theory/#/ref/briefs).

## Documentation

[The site](https://noahlyk.github.io/lyk-steno-theory/) (source in [`site/`](site/README.md), deployed
from `main`) has 20 lessons with an on-screen board and chord drills, and the
**[Reference](https://noahlyk.github.io/lyk-steno-theory/#/ref)**: every rule, table, command and
mechanism, searchable (start at the [cheat sheet](https://noahlyk.github.io/lyk-steno-theory/#/ref/cheatsheet)).
Maintainer logs: [`docs/decisions.md`](docs/decisions.md), [`docs/roadmap.md`](docs/roadmap.md). The
numbers on this page have no other page.

## Install

The binary is `lyk`. Needs a Rust toolchain.

```
make install                   # to /usr/local/bin (PREFIX=/usr and DESTDIR work; make uninstall removes it)
make pkg && sudo pacman -U dist/lyk-steno-theory-*.pkg.tar.zst   # Arch package of this checkout (or makepkg -si)
```

Or unpack a release tarball (`lyk-VERSION-ARCH.tar.gz` on the releases page) and copy `lyk` onto
your `PATH`. Then, from any folder: `lyk fetch`, `lyk build`, `lyk deploy` (needs
[keymux](https://github.com/noahlyk/keymux)). `lyk where` shows the folders in use.

Installed, data lives in `${XDG_DATA_HOME:-~/.local/share}/lyk/data` and build output in
`${XDG_STATE_HOME:-~/.local/state}/lyk/out`; in a checkout, `data/` and `out/`. `LYK_DATA`, `LYK_OUT` or
`--data DIR` override either ([install and paths](https://noahlyk.github.io/lyk-steno-theory/#/ref/build-and-files/install-and-paths)).
`make check` runs what CI runs.

## Use

In a checkout write `cargo run --release --` for `lyk`. Every command, option and data source:
[Reference, Build and files](https://noahlyk.github.io/lyk-steno-theory/#/ref/build-and-files).

```
lyk fetch                          # download the data (safe to re-run), the other theories included
lyk build                          # briefs + rules -> out/lyk/02-lyk-*.json (--report: every diagnostic and the cost tables)
lyk deploy                         # build, validate with keymux, copy into ~/.config/keymux/steno/lyk (--to), with the solver plugin and no backups
lyk deploy --rollback              # put back the files the last deploy replaced
lyk where                          # the version and the folders in use
lyk score --check-readme           # do this README's tables still match? (--write-readme rewrites a stale one)
lyk lookup "everybody's"           # a word, phrase or stroke, from the built dictionary, next to Plover and Lapwing
lyk verify-lookup                  # simulate keymux (entry, else solver) on every outline lookup lists (--full: all)
lyk resolve STKPW                  # a stroke not in the dictionary: what the solver types
lyk solve KOPL/+PU/+TER            # every reading of a run, cheapest first
lyk explain moon                   # which rules wrote a word
lyk mistakes --drill               # misstrokes on the steno tape, and a personal drill set for the site
lyk quote                          # a random quote stroked in all three theories
lyk practice                       # a typing drill on your own growing vocabulary, no outlines or hints (see Practice)
```

With no word, `lookup`, `resolve`, `explain`, `ergonomics`, `paths` and `family` open an interactive
session. `--rules FILE` and `--briefs FILE` use your own rules and briefs. [`tools/`](tools/README.md)
has scripts for measuring what a change did to the built dictionary.

## Practice

`lyk practice` is a monkeytype-style word feed in the terminal: a few lines of upcoming words, the
current one highlighted, what you typed green or red against it, a live speed, accuracy and time line
above the feed and a one-line key hint below it (`--quiet` hides both); no outlines or keyboard. Your steno machine types the
words through keymux as ordinary characters, so it only reads plain key presses.

```
lyk practice                                  # 25 words: what you are learning, reviews due, a few new ones
lyk practice --source common                  # 25 words from the 1,000 commonest
lyk practice --time 60 --rank 1-200           # one minute from the 200 commonest
lyk practice --source mistakes --outlines     # your drill words; the results show their outlines
lyk practice --source phrases --count 100     # the phrasing grammar's phrases, typed as text
lyk practice --history                        # recent runs and the trend
lyk vocab                                     # the vocabulary: size, frontier, learning, due, lapses
lyk vocab know the and                        # retire words you know: never shown again
lyk vocab gate 70                             # lower the clean-accuracy gate for new words (default 75)
```

Space or Enter commits a word; a wrong word is an error and the feed moves on
(`--stop-on-error` requires it right). Backspace or `*` deletes, Ctrl-Backspace or Ctrl-W deletes a
word. Esc or Ctrl-C quit at once at any time (the words typed so far are printed and saved as an
incomplete run); `q` is a plain letter in a test. Tab
restarts at any time, in a test or on the results screen (Enter too there): a fresh feed with the same
options and a new random seed (with `--seed N`, N+1, N+2, ... so each differs and the session repeats);
an abandoned test is not saved. The clock starts at the first character after a start or restart, and
the live line (`wpm 84   acc 97%   12/25   0:09`) keeps updating about four times a second. On the results
screen Tab or Enter starts again and `q`, Esc or Ctrl-C quit; Esc and Ctrl-C work at once, while the
first 0.7 s ignores Tab, Enter and letters so a stray burst cannot restart or dismiss it. Sources: `adaptive` (the default, below), `common` (`--rank`, `--bands`,
`--weighted`), `mistakes` (`lyk mistakes --drill`), `file` (`--file`), `phrases`, `quotes`.
`--count`, `--time`, `--seed`, `--no-repeat`, `--lowercase-only`, `--min-len`, `--max-len`, `--names`
shape the feed. The results show speed, raw speed, accuracy, errors, stumbles, time, consistency, the ten
slowest words, the missed ones and the stumbled ones (a word you corrected before committing it); each
finished test is added to `practice.jsonl` in the state folder (`out/` in a checkout), with the stumbled
words next to the missed ones, and `--history` shows a stumbled-count column. All flags: the Reference, Build and files.

The default source is **adaptive**: it grows a vocabulary instead of drawing from a fixed band. Each test
holds the words you are learning, reviews that are due, and up to 2 new words, which are earned (a free
slot among 10 learning words, 75% clean over your last 50 attempts) and come in frequency order from
lyk's list merged with Monkeytype's English lists (`lyk fetch` downloads them; words in Monkeytype's
common lists are met a little earlier, `--monkeytype-weight`), all the way down the long tail. An attempt
counts as clean only when the word was right and needed no correction on the way (a `*` or backspace
before the space is a stumble, even if the word then commits right); a stumbled or wrong word comes
back 3 words later in the same test (up to 3 times) and is drilled first in the next. A word 9 of
whose last 10 attempts are clean, over 3 days, becomes a review and returns after 1, 3, 7, 21, 60 and
180 days; a miss or stumble steps it back, two in a row send it back to learning. The state is `vocab.json` beside
`practice.jsonl`; the first run seeds it from your practice history and the steno tape, so words you
already type well are not new. `lyk vocab` shows it (with the stumble rate, the words stumbled most this
week, and why no new word has come in 14 days, if so) and `know`, `skip`, `forget`, `reset --yes`, `gate N`
edit it; `--no-adapt` plays a feed without changing it. The rules are in the Reference, Build and files.

## Full benchmarks

Basis: the 100,000 most common words that Plover or Lapwing write and that have a pronunciation on
record (CMUdict, or a translated fallback from MFA or WikiPron; see
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

Keys/Stroke above counts each distinct word once. With each word counted as many times as it is
said:

<!-- lyk:usage-table:start -->
| Syllables | Keys/Stroke (Lyk, by frequency) | Keys/Stroke (Plover, by frequency) | Keys/Stroke (Lapwing, by frequency) |
|---|---|---|---|
| 1 | 2.84 | 3.19 | 3.59 |
| 2 | 4.77 | 4.70 | 5.00 |
| 3 | 4.73 | 4.92 | 5.25 |
| 4+ | 5.25 | 5.22 | 5.48 |
<!-- lyk:usage-table:end -->

One-syllable figures drop for every theory under this weighting: a few common short words (*the*,
*a*, *to*, *of*) dominate real typing.

Share of each syllable count, counting each distinct word once:

<!-- lyk:syllable-words-table:start -->
| Syllables | Share of distinct words |
|---|---|
| 1 | 27% |
| 2 | 41% |
| 3 | 22% |
| 4+ | 10% |
<!-- lyk:syllable-words-table:end -->

...then weighted by how often each word is said:

<!-- lyk:syllable-usage-table:start -->
| Syllables | Share of words actually said |
|---|---|
| 1 | 80% |
| 2 | 15% |
| 3 | 4% |
| 4+ | 1% |
<!-- lyk:syllable-usage-table:end -->

[The overall number](#where-lyk-stands) combines the two frequency-weighted tables (share of typed
words per syllable count, keys per stroke at each), so it tracks one- and two-syllable words.

Longer words differ mostly where Plover and Lapwing have a brief, and where Lyk writes a bare `+-R`
for an `-er` they write `*ER`. `mismatches --syllables N --after 1500` lists the rest.

`build` on the words all three dictionaries write, strokes and keys per word:

| | Strokes | Keys |
|---|---|---|
| Lyk | 1.003 | 3.281 |
| Plover | 1.040 | 3.722 |
| Lapwing | 1.042 | 4.119 |

By word frequency (`build` prints this): keys per word weighted by how often each word is said,
then the share of the band's words (each counted once) that are a single stroke:

| Words | Share of all words said | Lyk | Plover | Lapwing | One stroke (Lyk) | Plover | Lapwing |
|---|---|---|---|---|---|---|---|
| top 100 | 58% | 2.12 | 2.48 | 2.91 | 100% | 100% | 100% |
| 100 to 1,000 | 28% | 4.40 | 4.80 | 5.16 | 100% | 98% | 98% |
| 1,000 to 5,000 | 10% | 5.48 | 6.17 | 6.45 | 99% | 83% | 81% |
| 5,000 to 10,000 | 2.3% | 5.99 | 7.25 | 7.66 | 98% | 61% | 59% |
| 10,000 to 20,000 | 1.2% | 6.80 | 8.14 | 8.57 | 93% | 43% | 42% |
| 20,000 to 50,000 | 0.6% | 7.37 | 8.91 | 9.29 | 82% | 28% | 29% |
| 50,000 to 100,000 | 0.1% | 8.23 | 9.86 | 10.15 | 66% | 16% | 19% |

The top hundred words are briefs in all three, so the dictionaries are close there; the rules move
the rest. Lyk's keys per word are lower than Plover's in every frequency tier. Past rank 5,000 the
one-stroke share is what moves: every word gets its one-stroke consonant skeleton ([how
short](https://noahlyk.github.io/lyk-steno-theory/#/ref/brevity)), so a word ranked 10,000-20,000 is
one stroke 93% of the time, against 43% in Plover. Primary outlines are settled by one global
optimization over all candidates ([briefs](https://noahlyk.github.io/lyk-steno-theory/#/ref/briefs)).

Counting notes: the join key on every continuation stroke is counted, and a Plover outline is
counted only when Lapwing has it too (Plover's `main.json` has misstrokes such as `O` for *to* and
`SP` for *and* that would make it look a third of a key cheaper). Some words take a star form because
another, more valuable word won the plain outline (`site` and `sight`, `don` and `done`); `build
--report` lists the commonest. 371 of the 100,000 words cannot be written by these rules yet, nearly
all names and interjections (`hm`, `schmidt`, `vlad`). `build` also writes 5,033,547 `+`-joined backup
entries (`build` + `+-G` is `building`, `UPB/+KHRAOER` is `unclear`, `TAEU/+-BL` is `table`; a median of
55 per word), a test set that `deploy` does not ship ([ways to
join](https://noahlyk.github.io/lyk-steno-theory/#/ref/ways-to-join),
[syllables and folding](https://noahlyk.github.io/lyk-steno-theory/#/ref/syllables)).

Run `lyk score --check-readme` before committing a rules change to catch drift in these tables.

## License

[GPL-3.0-or-later](LICENSE): free software; you may use, change and share it, and anything you
distribute that is derived from it must be released under the same license with its source. Credits
for derived files and the licenses of the data `lyk fetch` downloads are in [NOTICE](NOTICE).

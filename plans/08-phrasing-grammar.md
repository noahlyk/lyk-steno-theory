# Plan: the phrasing grammar (starter + auxiliary + verb in one stroke)

Design, measured, with a prototype to type before it is built into the generator; practice sheet:
https://claude.ai/artifact/9j4iofTdr7nHt5FLG3XULe (build any stroke on a keyboard, drill the commonest). Extends
[`06-phrase-briefs.md`](06-phrase-briefs.md) step 2. Status: **prototype for typing**
(`tools/phrasing_prototype.py`); the generator does not make these yet.

## Why a grammar, not more merged pairs

Today's phrases merge two words' own strokes when their keys don't overlap (*of the* `-FT`). That
can never reach the pairs that matter most: *I* is `EU`, which shares its vowels with almost every
verb, and *it was*, *he had*, *they were* overlap or sit on strokes common words hold. A grammar
gives each part of the phrase its own region of the keyboard, so every combination is one stroke
and none of them can overlap:

| Part | Keys | Says |
|---|---|---|
| **Starter** | left bank | who: *I*, *you*, *he*, *she*, *it*, *we*, *they*, *there* |
| **Auxiliary** | vowels | when, and which modal: present, past, *can*, *will*, *shall*, *may*, *must* |
| **Not** | `*` | the negative of whatever the rest says |
| **Verb** | right bank | *be*, *have*, *have been*, or one of 20 common verbs |

About 36 things to learn give 3,758 phrases.

## Starters

A starter is a left-bank chord no English word starts with, so a phrase never collides with a
word's own stroke: only short forms and merged phrases used these chords, and they move.

| Starter | Chord | |
|---|---|---|
| I | `SWR` | |
| you | `KPWR` | |
| he | `STWH` | he, she, it end in `WH` |
| she | `SKWH` | |
| it | `KPWH` | |
| we | `TWR` | I, you, we end in `WR` |
| they | `STWHR` | they and there share `S-T-H-R` |
| there | `STHR` | |

`KWH` (Plover's Jeff phrasing *he*) and `TWH` (*they*) were not free: *yeah*, *yet*, *whole* and
*young* are on `KWH`, and *them* is `TWHEPL`.

## Auxiliary: the vowels, with `E` for the past and `*` for not

| Keys | Present | with `E` (past) | with `*` (not) | with `*E` |
|---|---|---|---|---|
| (none) | *I do*, *he does*, or the verb in the present | *I did*, or the verb in the past | *I don't* | *I didn't* |
| `A` | *can* | *could* (`AE`) | *can't* | *couldn't* |
| `O` | *will* | *would* (`OE`) | *won't* | *wouldn't* |
| `U` | *shall* | *should* (`EU`) | *shall not* | *shouldn't* |
| `AO` | *may* | *might* (`AOE`) | *may not* | *might not* |
| `AU` | *must* | | *mustn't* | |

`E` is the past everywhere: *could* is *can* in the past, *would* is *will*'s, *should*
*shall*'s, *might* *may*'s, exactly as English builds them. The star means *not* and nothing else
in a phrase.

## Verbs: the right bank

| Verb | Code | | Verb | Code |
|---|---|---|---|---|
| *be* (*am/is/are*, *was/were*) | `-B` | | *come* | `-PL` |
| *have* (*has*, *had*) | `-F` | | *want* | `-PBT` |
| *have been* | `-FB` (*have* + *be*) | | *look* | `-BLG` |
| *say* | `-S` | | *tell* | `-L` |
| *go* | `-G` | | *feel* | `-FL` |
| *get* | `-T` | | *mean* | `-PBL` |
| *make* | `-BG` | | *need* | `-D` |
| *know* | `-PB` | | *like* | `-BLGZ` |
| *think* | `-PBG` | | *hear* | `-R` |
| *take* | `-BGT` | | *seem* | `-PLS` |
| *see* | `-SZ` | | *find* | `-PBD` |
| | | | *give* | `-FG` |

**The rule behind the codes**, so a code can be worked out rather than memorised: going down the
verbs from the commonest, a verb's code is its **last consonant** on the right bank (*get* `-T`,
*think* `-PBG`, *come* `-PL`), or its first if it ends in a vowel (*say* `-S`, *go* `-G`, *know*
`-PB`). If a commoner verb already has that code, add the verb's **first consonant** (*take*
`-BGT`: *make* has `-BG`; *feel* `-FL`: *tell* has `-L`). If that is taken too, add **`-Z`** (*see*
`-SZ`, *like* `-BLGZ`). *be* `-B`, *go* `-G` and *think* `-PBG` are the same keys as those words'
own strokes in lyk.

## Putting it together

| Stroke | Writes | Parts |
|---|---|---|
| `KPWHEB` | it was | it + past + be |
| `KPWH-B` | it is | it + be |
| `SWR-B` | I am | I + be |
| `SWR*B` | I am not | I + not + be |
| `SWR-PB` | I know | I + know |
| `SWR*PB` | I don't know | I + not + know |
| `SWR*EPB` | I didn't know | I + past + not + know |
| `KPWHOEB` | it would be | it + would + be |
| `STWHEFB` | he had been | he + past + have been |
| `SWRA*EFB` | I couldn't have been | I + could + not + have been |
| `STWHR*EB` | they weren't | they + past + not + be |
| `KPWR-SZ` | you see | you + see |
| `SWR` | I do | I alone |

*there* takes *be*, *have been* and the modals (*there was*, *there would be*, *there must have
been*), not the other verbs.

## What it saves (measured)

`python3 tools/phrasing_prototype.py`, over today's dictionary with today's merged phrases taken
first wherever they apply:

| Corpus | Words | Strokes today | With the grammar |
|---|---:|---:|---:|
| Gutenberg (`data/corpus/`, five novels) | 712,328 | 667,964 | 653,829 (2.1% fewer) |
| Quote passages (`src/quote_corpus.rs`) | 3,104 | 3,139 | 3,116 (0.7% fewer) |

By tier, on the same text: starter + auxiliary alone saves 0.6%; adding *be*, *have* and *have
been* takes it to 1.4%; the 20 verbs to 2.1%. Five of those verbs (*say*, *see*, *know*,
*think*, *go*) are 46% of the verbs' share. Novels are narration in the third person and the past;
conversation and messages, which are first and second person and the present, have more of these
pairs per word, so this undercounts everyday typing.

**What it takes from today's dictionary.** 37 of the 3,758 strokes are in use today: 19 merged
phrases, and short forms, extras and backups of rare words (*overthrow* `STHROE`, now
`AUFR/+THROE`; *typewriter*; *overweight* keeps `SWRAEUT`; *somewhere* keeps its own `SW-R`). No
word is left without an outline.

## Trying it

```sh
cargo run --release -- build
python3 tools/phrasing_prototype.py         # writes out/grammar-trial/
cp out/grammar-trial/02-lyk-*.json ~/.config/keymux/steno/lyk/
```

The trial folder is the whole `02-lyk-*` set with the 37 displaced entries removed and the grammar
added as `02-lyk-grammar.json`. `lyk deploy` puts the normal dictionary back (it removes any
`02-lyk-` file it did not write).

## Building it

Once the design survives typing:

1. `rules/grammar.toml`: the starters, auxiliaries and verbs (forms and codes) as data; the codes
   checked against the rule above by a test, not trusted.
2. `src/phrases/grammar.rs`: generate the entries, fixed in the `Book` before settling (like
   suffixes), so displaced words settle on their next outline instead of being removed afterwards;
   the merged-pair pass skips any text the grammar already writes.
3. Tests: every stroke typable and unique; every displaced word keeps an outline; the quote corpus
   walk counts grammar strokes.
4. Docs: a `docs/phrasing.md` chapter (this plan's tables, as the rules to learn), the joins sheet
   linking it, `docs/decisions.md`.

Open for after typing: questions (*do you*, *are you*, *can I*: the starter after the auxiliary);
*that*, *this*, *what*, *who* as starters; *I'm*, *it's* contracted positives; more verbs, chosen
by measured gain.

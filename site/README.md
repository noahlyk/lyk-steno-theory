# The lessons site

Interactive lessons for lyk, deployed to GitHub Pages from `main` by
[`.github/workflows/pages.yml`](../.github/workflows/pages.yml). Plain HTML, CSS and ES modules: no
build step, no dependencies. To run it locally:

```sh
python3 -m http.server 8000 --directory site    # then open http://localhost:8000
node site/check.mjs                             # the same check CI runs
```

One-time setup on GitHub: Settings > Pages > Build and deployment > Source: **GitHub Actions**.

## What is in it

| | |
|---|---|
| `lessons/index.json`, `lessons/NN-name.json` | The curriculum: 18 lessons in 5 parts. The only place lesson text lives. |
| `data/words.json` | The 12,000 commonest words, plus every word a lesson names, each with its fastest outline and a few more ways in. **Generated.** |
| `data/chart.json` | The key chart, read from `rules/plover.toml`. **Generated.** |
| `data/engine.json`, `data/joins.json` | What the stroke engine reads: every outline of the dictionary (word -> outlines) plus the reserved strokes and suffixes, then the 12 cheapest join-built backups of each common word. About 1.2 MB gzipped, fetched when a drill starts. **Generated.** |
| `js/engine.js`, `js/translate.js` | The stroke-to-text engine: `translate(strokes)` says what a stream types, `accepts(word, strokes)` whether it types that word by any route, `prefixOf` whether it still can. `translate.js` is the loaded-on-demand wrapper for pages that show the output. |
| `js/steno.js` | Parses and writes strokes (`TKOG/+-Z`), and the QWERTY chord map (`Q` is the join key). |
| `js/board.js`, `js/input.js` | The on-screen board and its key states (hint, held, wrong, missing) plus the legend; chord capture (keys down together, stroke on release; the newest listener gets the keys). |
| `js/popover.js` | The hover board: pointing at or focusing any stroke chip (`data-outline`) floats its keys beside it, stepping through a multi-stroke outline; a tap toggles on touch. No click, nothing sticky. |
| `js/dock.js`, `js/translate.js` | The "Try it" pad (button, or `?`): type with Plover QWERTY, the board glows the held keys, and `translate(strokes)` turns the strokes into words. All stroke-to-text lives behind that one function. |
| `js/drill.js` | A drill: prompt, chord, hint on a miss, first-try accuracy and speed. |
| `js/prestige.js` | The "Learn the keys" engine (pure, tested by `check.mjs`): key unlock order, per-chord accuracy, weakness-weighted word picking, unlock and prestige rules. |
| `js/keys.js`, `css/keys.css` | The `#/keys` page: the word stream, prestige cards, key chips; progress in its own `localStorage` key. |
| `js/lesson.js` | Renders a lesson's blocks; every `[[word|OUTLINE]]` becomes a hover-board chip. |
| `js/main.js`, `js/data.js`, `js/store.js` | The router and pages (lessons, practice, lookup, key chart, progress); data loading; progress in `localStorage`. |

## Learn the keys (Prestige 1 and 2)

The first step of the course, before lesson 1, ported from the steno-lessons project. You start with
six keys (`E A T- S- K- -T`) and type a stream of real words, each written in one stroke (or with the
join key once it is unlocked) from the dictionary's own outlines. Every 30 words are judged
together: at 28 wpm or better, 95% of words with no miss, and every chord you have tried at 80% or
better over its last 30 tries, the next key unlocks (`S R- -R H- O P- W- U -L -D -P -B -G -F * -Z`,
and the join key `+` last). Accuracy is kept per chord (the exact keys held together in one hand
or the vowels), a miss blames only the chords that came out wrong, and the word picker boosts words
containing the weakest or newest chord, so weak spots get the reps. With every key unlocked, a fresh
clean lesson finishes the level and later ones can set a personal best.

- **Prestige 1**: the board shows the keys to press, the stroke and its QWERTY letters are under the word.
- **Prestige 2**: starts again at six keys with the board hidden; the stroke letters can be switched
  off; three wrong chords on one word bring the board back for that word.

Lessons show a soft gate until Prestige 2 is done, with a skip. Everything saves in this browser.

## Writing a lesson

A lesson is JSON: `id`, `title`, `goal`, `blocks`, and an optional `drill`.

- Blocks: `h`, `p`, `list`, `note` (`kind`: `rule` or `warn`), `table`, `explore` (a free board),
  `try` (an inline drill), `quiz`, `join` (the stem-plus-ending builder).
- Inline markup in any text: `[[word|OUTLINE]]` (a word and a stroke chip that lights the board),
  `[[OUTLINE]]` (a chip only), `` `code` ``, `**bold**`, `{{italic word}}`, `[text](https://...)`.
- A drill is `{ title, pass, repeat, check, items }`. An item is `[prompt, outline, ...alternatives]`
  or `{ "p": prompt, "o": [outlines], "w": "the word it must be" }` when the prompt is not the
  dictionary's own text (punctuation, commands). `check: "none"` skips the dictionary check for
  sound and key drills.

Keep a lesson short: one idea per heading, tables over paragraphs, and every outline a real one.

## The engine

A drill does not ask for one outline: it asks whether what you struck *types the word*. The engine
reads a stroke stream the way the dictionary does (`docs/join-key.md`, `docs/fallback.md`):

1. A stroke without the join key opens a run; every `+` stroke continues the run before it. Runs
   are separated by a space; a reserved outline can span runs (`-FP/PHOEUFP`).
2. A run is looked up whole (`ED/+-D` is *ended*), as a dictionary entry or a reserved one
   (punctuation attaches to the word before it and capitalises the next; commands show as `⟨Return⟩`).
3. A run with no entry is read as the real fallback does: a single-stroke word as the stem, then each
   continuation as a suffix (`rules/suffixes.toml`, two-stroke suffixes first) or as the sounds its
   keys spell, dropping a silent final `e` before a vowel (`SR/+-G` is *having*).
4. A stroke nothing reads is shown raw.

So `EPBD/+-D`, `ED/+-D` and any join-built backup all pass as *ended*; a drill says
"also fine: you wrote X; fastest is Y" when it was not the listed outline, and that is not a miss.
Items whose listed outline does not type their word (the sound and key drills) stay exact.
A multi-stroke stem is deliberately not used for the fallback, as in the Rust `resolve`.
`node site/test.mjs` reads every outline in `words.json` back through the engine.

## Keeping it true

Every `[[word|OUTLINE]]` and every drill item is a claim about the dictionary. After changing a
rule or the generator:

```sh
cargo run --release -- build              # so out/lyk is current
python3 tools/site_data.py                # checks every claim; rewrites site/data/*
node site/check.mjs
node site/test.mjs                        # the engine reads the regenerated data back
```

`tools/site_data.py --check` only checks. It exits non-zero and names each lesson, word and outline
that no longer matches, so a rule change cannot silently leave a lesson teaching the old stroke.
CI cannot build the dictionary (the corpus is not in the repository), so CI runs only the offline
check; commit the regenerated `site/data/` with the rule change.

Inspired by [Lapwing Lab](https://github.com/CRAZECRAZE/lapwing-lab). It shares no code with it:
that is a local app that reads a Uni keyboard over WebHID, and this is a static page that reads an
ordinary keyboard.

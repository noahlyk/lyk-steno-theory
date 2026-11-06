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
| `lessons/index.json`, `lessons/NN-name.json` | The curriculum: 18 chapters in 5 parts (plus the optional chapter 0, Learn the keys). The only place lesson text lives. |
| `data/words.json` | The 12,000 commonest words, plus every word a lesson names, each with its fastest outline and a few more ways in. **Generated.** |
| `data/chart.json` | The key chart, read from `rules/plover.toml`. **Generated.** |
| `data/engine.json`, `data/joins.json` | What the stroke engine reads: every outline of the dictionary (word -> outlines) plus the reserved strokes and suffixes, then the 12 cheapest join-built backups of each common word. About 1.2 MB gzipped, fetched when a drill starts. **Generated.** |
| `js/engine.js`, `js/translate.js` | The stroke-to-text engine: `translate(strokes)` says what a stream types, `accepts(word, strokes)` whether it types that word by any route, `prefixOf` whether it still can. `translate.js` is the loaded-on-demand wrapper for pages that show the output. |
| `js/steno.js` | Parses and writes strokes (`TKOG/+-Z`), and the QWERTY chord map (`Q` is the join key). |
| `js/board.js`, `js/input.js` | The on-screen board and its key states (hint, held, wrong, missing) plus the legend; chord capture (keys down together, stroke on release; the newest listener gets the keys). |
| `js/popover.js` | The hover board: pointing at or focusing any stroke chip (`data-outline`) floats its keys beside it, stepping through a multi-stroke outline; a tap toggles on touch. No click, nothing sticky. |
| `js/dock.js`, `js/translate.js` | The "Try it" pad (button, or `?`): type with Plover QWERTY, the board glows the held keys, and `translate(strokes)` turns the strokes into words. All stroke-to-text lives behind that one function. |
| `js/hid.js` | A steno machine over Plover HID (WebHID, Chromium browsers only): decodes the 64-lever snapshot to lyk keys and feeds `input.js`'s `feed()`, so every drill takes it like the keyboard. Header button to connect; reconnects a machine it was already allowed. |
| `js/drill.js` | A drill: prompt, chord, hint on a miss, first-try accuracy and speed. |
| `js/trainer.js` | The "Learn the keys" engine (pure, tested by `check.mjs`): key unlock order, per-chord accuracy, weakness-weighted word picking, the speed/accuracy unlock trade, the fading guide. |
| `js/chapter0.js` | Chapter 0: the intro text, then the key stream as its drill. |
| `js/keys.js`, `css/keys.css` | The `#/keys` page: the word stream, guide, key chips; `js/confetti.js` for the finish. Progress in its own `localStorage` key. |
| `js/lesson.js` | Renders a lesson's blocks; every `[[word|OUTLINE]]` becomes a hover-board chip. |
| `js/main.js`, `js/data.js`, `js/store.js` | The router and pages (lessons, practice, lookup, key chart, progress); data loading; progress in `localStorage`. |

## Steno machine (Plover HID)

In Chrome, Edge or Brave a **Connect steno machine** button appears in the header. The Uni's top S
key is the join key (`+`) and the bottom one is `S-`, matching lyk's board (untick "top S is the
join key" to make both `S-`); the four asterisk levers are `*`, and `#` keys press nothing. Strokes
fire when the last key comes up, the same as with the keyboard. The decoder is unit-tested
(`check.mjs`) on synthetic reports; it has not been run against hardware yet.

## Learn the keys

There is no Start button: the stream (and an inline lesson drill) begins when it scrolls into view
and lets the keyboard go when it scrolls away.

A word stream that teaches where every sound sits, before the theory. You start with six keys; one
more unlocks whenever your newest words (20, at least 10) are fast and clean enough.

- **Speed trades against accuracy.** Fast may be sloppy, slow must be clean: the accuracy needed
  slides from 97% at 14 wpm to 75% at 60 wpm, and under 14 wpm nothing passes. Speed counts the
  work, not the words: a word weighs its key presses over three, plus half a unit per extra stroke
  (never under one), so a six-key or two-stroke word is not "slow" for taking longer than a short one. Individual chords are
  not held against you above 45 wpm.
- **One adaptive guide, no levels.** It starts as the diagram *with the answer* (the keys to press
  light up) and the QWERTY hints; as you get fast and clean the hints and the answer go (the diagram
  stays, showing only what you are pressing), then the diagram itself (greyed, "From your head now..."); a run of misses brings
  one back. The stroke under each word is never hidden. Finishing needs every key unlocked and a
  passing run from memory, then confetti.
- **Unlocks are earned from memory.** Only words typed with the diagram gone count towards the next
  key, and unlocking one puts the full guide (diagram and hints) back, so every new key is learned
  with help first and then faded out again.
- **The `-` only where it is needed.** The stroke under a word shows a dash when the right-hand keys
  could not otherwise be told from the left in steno order (`-T`, `-PB`, `-S`) and leaves it out when
  they can (`-D`, `-F`, `KPWR-BG` written as `KPWRBG`, anything with a vowel or star).
- **Words** come from `site/data/words.json`, using the dictionary's real outlines, so words of
  more than one stroke appear once the join key unlocks (last).
- Weak chords get more of the stream (weakness-weighted picking, ported from steno-lessons).

It is chapter 0 of the course and optional: it has an intro above the stream (`js/chapter0.js`), the stream is its drill at the bottom, and lessons only mention it quietly until it is done or dismissed. Everything saves in this browser.

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

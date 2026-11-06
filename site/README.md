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
| `lessons/index.json`, `lessons/NN-name.json` | The curriculum: 20 chapters in 6 parts (plus the optional chapter 0, Learn the keys; chapters 19 and 20 are the phrasing grammar). The only place lesson text lives. |
| `reference/index.json`, `reference/<id>.json` | The **Reference** area (`#/ref`): every rule, table and mechanism that is not a lesson, 23 pages in 6 sections (cheat sheet, how it works, the rules, briefs/phrases/suffixes, punctuation and letters, the project). The docs/ chapters and the Lyk Pipeline artifact live here now. Same blocks as a lesson, no drill; searchable (`js/reference.js`, also surfaced in Lookup); every heading is an anchor, `#/ref/<page>/<heading-slug>`. |
| `data/grammar.json` | The phrasing grammar's parts and its 3,758 strokes, from `tools/phrasing_prototype.py`. **Generated.** `js/grammar.js` is the composer. |
| `data/words.json` | The 12,000 commonest words, plus every word a lesson names, each with its fastest outline and a few more ways in. **Generated.** |
| `data/chart.json` | The key chart, read from `rules/plover.toml`. **Generated.** |
| `data/engine.json`, `data/joins.json` | What the stroke engine reads: every outline of the dictionary (word -> outlines) plus the reserved strokes and suffixes, then the 12 cheapest join-built backups of each common word. About 1.2 MB gzipped, fetched when a drill starts. **Generated.** |
| `js/engine.js`, `js/translate.js` | The stroke-to-text engine: `translate(strokes)` says what a stream types, `accepts(word, strokes)` whether it types that word by any route, `prefixOf` whether it still can. `translate.js` is the loaded-on-demand wrapper for pages that show the output. |
| `js/steno.js` | Parses and writes strokes (`TKOG/+-Z`), and the QWERTY chord map (`Q` is the join key). |
| `js/board.js`, `js/input.js` | The on-screen board (a display only: nothing on it is clickable, there is no click-to-type anywhere) and its key states (hint, held, wrong, missing) plus the legend; chord capture (keys down together, stroke on release) from the keyboard or a steno machine. |
| | **Who takes the keys:** anything that takes strokes (a try-it board, a drill, the key stream) registers a region with `whenActive` in `input.js`. Exactly one region listens at a time: the one with the most of itself on screen (at least 40% in view), or the one just clicked or tabbed into, so several can share a page and only the one in front of you is ever fed, by keyboard or steno machine. The Try it pad, opened on purpose, outranks them all. |
| `js/popover.js` | The hover board: pointing at or focusing any stroke chip (`data-outline`) floats its keys beside it, stepping through a multi-stroke outline; a tap toggles on touch. No click, nothing sticky. |
| `js/hover.js`, `js/hovermatch.js` | **Every word and outline is hoverable, with no markup.** After any page renders (a `MutationObserver` on `#app` also catches what is added later) `hover.js` finds code spans, `.word` spans and text-only table cells, list items and lookup headings, asks `hovermatch.js` whether the text is a dictionary outline or word, and gives it `data-outline` plus a faint dotted underline (`.hov`), so `popover.js` shows its board exactly as for a chip, on hover, focus (`tabindex=0`) or tap. See "Hoverable words and outlines" below. |
| | The preview's top right says what the outline is defined as (`engine.define`): `{^ed}` for a suffix stroke, `{.}` for punctuation, the word for an entry, an italic reading for anything the fallback composes. The strokes not being shown are dimmed, not bracketed. |
| `js/dock.js`, `js/translate.js` | The "Try it" pad (button, or `?`): type with Plover QWERTY, the board glows the held keys, and `translate(strokes)` turns the strokes into words. All stroke-to-text lives behind that one function. |
| `js/hid.js` | A steno machine over Plover HID (WebHID, Chromium browsers only): decodes the 64-lever snapshot to lyk keys and feeds `input.js`'s `feed()`, so every drill takes it like the keyboard. Header button to connect; reconnects a machine it was already allowed. |
| `js/outline.js` | How a stroke or outline is drawn to be read: letters in steno order coloured by hand, finger pairs underlined, a `-` only where needed, strokes divided by a thin rule. Used by the key trainer and lookup (no tape-style columns). |
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

## Always show the keys, and restarting a run

Every drill has an "always show the keys" box, and so does the key stream. It is allowed, with a price:

- **If it is ever on during a run, that run cannot set a best.** Even for a moment, even if it is
  switched off again before the end, and a run that *starts* with it already on is peeked too. In a
  drill (`js/drill.js`) the flag (`peeked`) is only ever set during a run, never cleared until the next
  run begins; the result card says "Keys were shown during this run, so it does not count toward your
  best", a small note says the same while the run is live, and `store.recordRun(..., { assisted })`
  still counts the run and lets it pass the lesson but leaves the best accuracy and best speed alone.
- **In the key stream** (`js/keys.js`) a word typed with it on at any moment is saved with `a: 1`;
  `memoryStats` (what unlocks and finishing are judged on) ignores such words, so they are never "from
  memory", and the 20-word window it touched cannot set a best wpm (`recordBestIfFaster(..., { assisted })`).
  The window flag is in memory only and is not reset by turning the box off; the next window starts clean.
  The box itself is the same saved setting in both places.
- **A keypress restarts a finished drill.** On the results card, any stroke (a whole chord, from the
  keyboard or a steno machine) starts a fresh run; that stroke is only the restart and is not scored. The
  first 700 ms after the results appear are ignored, since a typist's next stroke is still the last
  word. Mid-run nothing restarts, and the key stream has no results card, so there is nothing to
  restart. The finished drill keeps listening, so it follows the usual who-takes-the-keys rule.

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

- Blocks: `h`, `h3`, `p`, `pre` (a formula or command kept as written), `grammar` (the phrasing composer), `list`, `note` (`kind`: `rule` or `warn`), `table`, `explore` (a free board),
  `try` (an inline drill), `quiz`, `join` (the stem-plus-ending builder).
- Inline markup in any text: `[[word|OUTLINE]]` (a word and a stroke chip that lights the board),
  `[[OUTLINE]]` (a chip only), `` `code` ``, `**bold**`, `{{italic word}}`, `[text](https://...)`.
- A drill is `{ title, pass, repeat, check, items }`. An item is `[prompt, outline, ...alternatives]`
  or `{ "p": prompt, "o": [outlines], "w": "the word it must be" }` when the prompt is not the
  dictionary's own text (punctuation, commands). `check: "none"` skips the dictionary check for
  sound and key drills. `notation: true` makes the prompt a stroke drawn as written (`PHAP`, in the coloured outline style) and
  reveals the word it makes (the item's `w`) on a hit.

`[[=phrase|OUTLINE]]` (and a drill with `check: "grammar"`) marks a phrasing-grammar stroke: it is checked against the generator's prototype, not the dictionary, and drawn dashed. Internal links are `[text](#/ref/page/anchor)` and `[text](#/lesson/id)`; `check.mjs` verifies them.

## Writing a reference page

Same JSON and markup as a lesson (`id`, `title`, `goal`, `blocks`; no drill). It must be complete and concise: every rule, table row and exact outline, no history or justification. State a word's outline as `[[word|OUTLINE]]` so `tools/site_data.py` checks it against `out/lyk`; use `[[OUTLINE]]` for patterns and suffix strokes. A page must be listed in `reference/index.json`. **Docs outrank code**: a change to `rules/*.toml` or `src/*.rs` that changes what a word strokes as, what a brief does or how candidates are picked lands with the matching Reference page edit in the same commit.

## Hoverable words and outlines

You do not mark these up. Writing `` `ends` ``, `` `ED/+-D` ``, a table cell `girls` or `{{shell}}` is enough; the enhancer in `js/hover.js` does the rest. The rules (`js/hovermatch.js`, tested in `test.mjs`):

- **Outline:** only capitals, `*`, `+`, `-` and `/`; every stroke must parse on the board and hold a key; and it either carries a steno mark (`/ + - *`: `-D`, `S-`, `ED/+-D`) or is a real dictionary outline of 3+ characters (`KAT`, `TKPWAEUPL`). `AND`, `NOTE`, a lone `A`, `A//B`, `+`, `E-MAIL` and a bare key group such as `TKPW` get nothing.
- **Word:** lower-case, 2+ letters, a key of `words.json` with a typable outline. It shows the **first (fastest) outline only**. `tools/site_data.py` adds every word a page shows in code, `{{italic}}`, a whole table cell or a list item to `words.json` (and the engine), so a rare word named on a page is covered, not just the 12,000 commonest.
- **Where:** `code`, `.word` (a `[[word|OUTLINE]]` word takes its chip's outline; a phrasing-grammar chip's word claims nothing), and a `td`, `li` or lookup heading whose whole text is one word/outline or a comma/semicolon list of them (`ends, girls`). Not `th` (labels), not a cell that is a sentence, and a cell reading just `no`, `none`, `keys`, ... is table furniture (`FURNITURE` in `hovermatch.js`).
- **Never touched:** links, buttons, existing chips (`[data-outline]`), the drills and the key stream (their prompts are the answers), quiz options, `pre`, form fields, `.pop`, anything inside `[data-nohover]` (set it on a cell or block that must stay plain, as the key chart's sound column does).
- **Cost:** nothing loads for it except `words.json`, which Lookup needs anyway, fetched once when idle after a page with candidates renders. The engine (the definition at the top right) still loads on the first hover.

Keep a lesson short: one idea per heading, tables over paragraphs, and every outline a real one.

## The engine

A drill does not ask for one outline: it asks whether what you struck *types the word*. The engine
reads a stroke stream the way the dictionary does (`site/reference/join-key.json`, `site/reference/fallback.json`):

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

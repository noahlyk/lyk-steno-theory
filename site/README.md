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
| `js/steno.js` | Parses and writes strokes (`TKOG/+-Z`), and the QWERTY chord map (`Q` is the join key). |
| `js/board.js`, `js/input.js` | The on-screen board; chord capture (keys down together, stroke on release). |
| `js/drill.js` | A drill: prompt, chord, hint on a miss, first-try accuracy and speed. |
| `js/lesson.js` | Renders a lesson's blocks, including the sticky board that lights up for any stroke you point at. |
| `js/main.js`, `js/data.js`, `js/store.js` | The router and pages (lessons, practice, lookup, key chart, progress); data loading; progress in `localStorage`. |

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

## Keeping it true

Every `[[word|OUTLINE]]` and every drill item is a claim about the dictionary. After changing a
rule or the generator:

```sh
cargo run --release -- build              # so out/lyk is current
python3 tools/site_data.py                # checks every claim; rewrites site/data/*
node site/check.mjs
```

`tools/site_data.py --check` only checks. It exits non-zero and names each lesson, word and outline
that no longer matches, so a rule change cannot silently leave a lesson teaching the old stroke.
CI cannot build the dictionary (the corpus is not in the repository), so CI runs only the offline
check; commit the regenerated `site/data/` with the rule change.

Inspired by [Lapwing Lab](https://github.com/CRAZECRAZE/lapwing-lab). It shares no code with it:
that is a local app that reads a Uni keyboard over WebHID, and this is a static page that reads an
ordinary keyboard.

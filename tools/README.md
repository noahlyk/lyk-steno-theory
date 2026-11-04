# Analysis tools

Python 3, no dependencies. Every one reads the built dictionary in `out/lyk/` (run
`cargo run --release -- build` first) and the data in `data/`, from wherever it is run.
`lyk_out.py` is the shared loader, not a tool.

| Tool | What it answers |
|---|---|
| `holders.py OUTLINE @word ...` | Who holds an outline (`TKO*BG` is *dock*, in `words`), or every outline a word has, fastest first. Quote outlines with a star. |
| `diff_builds.py BEFORE [AFTER] [N]` | What a change did: per-file entry and word counts, the words only one build has, and every word whose fastest outline changed, commonest first, with counts for the top 1,000 / 5,000 / 20,000. |
| `coverage.py [word ...]` | How many words of two syllables or more can be joined in, by frequency band: one-stroke share, joined share, stem-plus-ending share, joined ways in per word ([joins](../docs/joins.md)). |
| `markers.py [N]` | Words still on a bare `/+*` marker, by band, real words against names, and who holds the plain and starred stroke each real word wanted ([collisions](../docs/collisions.md)). |
| `phrase_sim.py [protect_rank] [show]` | Simulates letting phrases take strokes from words ([plans/06](../plans/06-phrase-briefs.md)). |

Measuring a change:

```sh
cargo run --release -- build && cp -r out/lyk /tmp/before
# ...change the rules or the generator...
cargo run --release -- build && python3 tools/diff_builds.py /tmp/before
```

`cargo run --release -- score --check-readme` checks the README's tables. Checking that a change
moved no word at all is a hash of the output: `sha256sum out/lyk/*.json`.

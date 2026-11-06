# lyk documentation

**The documentation lives on the site**: <https://noahlyk.github.io/lyk-steno-theory/>. Lessons teach
you to type it; the **Reference** (`#/ref`) holds every rule, table and mechanism, searchable. Its
source is JSON under [`site/reference/`](../site/reference) and [`site/lessons/`](../site/lessons)
(see [`site/README.md`](../site/README.md)). Docs outrank code: a change to `rules/*.toml` or
`src/*.rs` that changes what a word strokes as lands with the Reference page edit in the same commit.

What stays in this folder is the maintainers' record, not something to learn from:

- [`decisions.md`](decisions.md): why changes landed the way they did. Its links to chapter files
  that used to live here now point at the Reference pages below.
- [`roadmap.md`](roadmap.md): a working plan.

Where the old chapters went (`https://noahlyk.github.io/lyk-steno-theory/#/ref/<page>`):

| Old file | Reference page |
|---|---|
| `join-key.md` | `join-key` |
| `reserved-strokes.md` | `reserved-strokes` |
| `briefs.md` | `briefs` |
| `phonetic-rules.md` | `phonetic-rules` |
| `spelling.md` | `spelling` |
| `prefixes.md` | `prefixes` |
| `syllables.md` | `syllables` |
| `joins.md` | `ways-to-join` |
| `phrases.md` | `phrases` |
| `collisions.md` | `collisions` |
| `brevity.md` | `brevity` |
| `suffixes.md` | `suffixes` |
| `punctuation.md` | `punctuation` |
| `fingerspelling.md` | `fingerspelling` |
| `fallback.md` | `fallback` |
| `spelling-mode.md` | `spelling-mode` |
| `files.md` | `build-and-files` |
| `typable.md` | `typable` |
| the Lyk Pipeline artifact | `how-stroke-setup`, `how-candidates-cost`, `how-solver`, `how-backups-output` |
| the Lyk Phrasing Grammar artifact | lessons `19-phrasing-grammar`, `20-phrasing-strokes` |

# The dictionary files

[← back to the table of contents](README.md) · [← back to the main README](../README.md)

`build` does not write one big dictionary. It writes several small Plover JSON files, one for each
*way an entry came about*, and `deploy` puts them in keymux's folder next to the reserved-stroke
files. Nothing about what a stroke means depends on this; it is for reading, and for keymux's
reload (below).

## The files

| File | What is in it | Roughly |
|---|---|---|
| `02-lyk-words.json` | each word's own outline: gold briefs, the plain rules reading, spelling and voicing variants, ladder-settled clashes, family members' shapes | 57,000 |
| `02-lyk-short.json` | long words written shorter by the brevity steps (`lead`, `fold`, `lean`, `ends`, `shell`, `skeleton`) | 36,000 |
| `02-lyk-backups.json` | the `+`-joined fallbacks, composed from other words' own outlines ([syllables](syllables.md)) | 39,000 |
| `02-lyk-extras.json` | extra one-stroke ways to write a word, filling strokes nothing else needed ([briefs](briefs.md)) | 12,000 |
| `02-lyk-alternates.json` | another pronunciation's outline for a word | 3,000 |
| `02-lyk-phrases.json` | two common words in one stroke ([phrases](phrases.md)) | 3,000 |
| `02-lyk-suffixes.json` | the standalone suffix strokes ([suffixes](suffixes.md)) | 40 |

Around them, in the same folder: `01-lyk-commands.json`, `03-lyk-punctuation.json`,
`04-lyk-emilys-modifiers.json` (the [reserved strokes](reserved-strokes.md), copied verbatim from
`rules/reserved/`) and `05-lyk-orthographic-fallback.rs` (the [fallback](fallback.md) plugin).

The generator keeps exactly one word per outline, so **no outline appears in two files**, and the
order keymux loads them in never changes what a stroke types. (`build` checks this: the union of the
seven files is exactly the old single dictionary's 151,444 entries, none twice.) Which file an entry goes in is
decided in one place, `Role::of` in `src/output.rs`, from the entry's `Origin`.

## Why split it

- **For you:** the file name says what kind of outline you are looking at. A `+` in `backups` is a
  fallback; the same word's primary is in `words`. Opening one 4 MB file to find out was never a
  good way to learn anything.
- **For keymux:** it caches and reloads per file. A change that moves only some entries rewrites
  only some files, and a file whose content did not change is not rewritten at all (`build` compares bytes first), so keymux sees nothing to do for it. See the keymux README,
  "Per file".

## Building and deploying

```
cargo run --release -- build     # writes out/lyk/02-lyk-*.json; prints each file and "(unchanged)"
cargo run --release -- deploy    # build + plugin, copied into ~/.config/keymux/steno/lyk
```

`deploy` overwrites every generated file, then removes any `02-lyk-*.json` in the destination that
this layout no longer writes (the old single `02-lyk-words.json` is just overwritten by the smaller
`words` file). `build` alone leaves a file untouched when its bytes would not change. Files are sorted by outline and pretty-printed the same way every time, so the same
dictionary always gives the same bytes.

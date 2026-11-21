# Roadmap

[← docs/README.md](README.md) · [← main README](../README.md)

Open items only. Rules live in the [Reference](https://noahlyk.github.io/lyk-steno-theory/#/ref);
what shipped and why is in [`decisions.md`](decisions.md); per-task plans in
[`../plans/`](../plans/README.md).

## Open

- **Quotes, end to end.** Run a real quote through the live keymux daemon (`lyk quote`) to confirm
  punctuation, caps, apostrophes and the solver resolve together.
- **More briefs.** Raise `count` past 1500 in `rules/briefs.toml` and hand-check the next tier;
  `work`/`wore`/`working`/`worrying`/`wok` showed such words need explicit briefs, not improvised
  rule codas.
- **Compound suffix strokes** (low priority). One continuation stroke for a common suffix pair
  (`-er`+`-s` as `-ers`). The join key already makes single suffixes cheap, so the win is small.
- **Phrasing grammar.** Questions (starter after the auxiliary), more starters (*that*, *this*,
  *what*, *who*), contracted positives, more verbs ([phrases](https://noahlyk.github.io/lyk-steno-theory/#/ref/phrases), "Not covered by the grammar"). Waits on typing the built grammar.
- **Homophones and reduced vowels.** The long tail needs deliberate rules, not per-word judgment,
  as briefs run out and the rule-generated tail grows (onyx's write-ups dropped a fully phonetic
  theory for this reason).
- **Pseudo-strokes** (not scheduled). Split a hard coda cluster into an onset-equivalent and a
  coda-equivalent half within one stroke; relevant only if `[coda]`/`[coda_cluster]` in
  `rules/plover.toml` grow toward the denser cluster tables in tarot's chart.
- **`-graphy`/`-ography`** have no standalone stroke: the schwa before them takes a
  spelling-dependent vowel ([suffixes](https://noahlyk.github.io/lyk-steno-theory/#/ref/suffixes), "Not in the table"). `re-`, `pre-` and `non-` have no prefix stroke
  for the reasons given on [prefixes](https://noahlyk.github.io/lyk-steno-theory/#/ref/prefixes).

## Done

- **Reserved strokes** (`rules/reserved/`): commands, punctuation and fingerspelling are reserved
  before any word is assigned an outline, after `TP-BG` (`!`) was shadowed by *factory*; 40 strokes
  affected. See [reserved strokes](https://noahlyk.github.io/lyk-steno-theory/#/ref/reserved-strokes).
- **Brief outlines that split a two-key consonant** (`good`, `day`, `work`; 245 of the 1500
  auto-briefed words), fixed in `93251ef`: a brief must read back as the word's real first and last
  sound under lyk's rules. Cost: 3.49 to 3.73 keys/stroke at the time.
- **AO/UH tie-break** for a bare `AO` stroke (`oo`, not `uh`), fixed in `3822e12` in the since
  removed orthographic fallback plugin.
- **Suffixes and prefixes**: `-tive`, `-ic`/`-ical`, `-al`, `-logy`, `-ity`/`-ety`, `-cy`/`-sy`,
  `-ify`; prefixes `un-`, `mis-`, `over-`, `under-`, `inter-`. `-ious`/`-eous` need no entry (the
  `-y` stroke then `-ous`).
- **Conflict audit (2026-10-29)** against onyx's taxonomy: word-boundary conflicts are solved by the
  join key; proper nouns by `KPA`/`KPA*` (`{-|}`) before a name; chord/stenonym conflicts by the
  star ladder; homophones by the spelling rules ([collisions](https://noahlyk.github.io/lyk-steno-theory/#/ref/collisions), [spelling](https://noahlyk.github.io/lyk-steno-theory/#/ref/spelling)).
- **Docs on the site**: the old `docs/*.md` chapters became Reference pages (map in
  [README.md](README.md)); the order is the lessons, then the cheat sheet and how it works.

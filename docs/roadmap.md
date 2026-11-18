# Roadmap

[← back to the table of contents](README.md) · [← back to the main README](../README.md)

A working plan, not a changelog and not shipped. Written after a round of live testing, Discord
feedback and reading other theory-builders' write-ups. Nothing here is built yet; each item gets its
own commit when it lands.

## 1. Correctness first — done

Several pieces assumed missing already existed in the deployed config
(`~/.config/keymux/steno/lyk/`). Testing them found one structural bug, now fixed:

- **Fingerspelling, misc keys, OS shortcuts** (`04-lyk-emilys-modifiers.py`/`.json`): full A-Z
  fingerspelling, Tab/Backspace/Delete/Escape, arrows, page nav, media keys, typable symbols with
  shift variants, Shift/Alt/Super/Ctrl chording for any key. Forked from Lapwing's "Emily's Modifier
  Dictionary", theory-independent. Tested, working.
- **Punctuation** (`03-lyk-punctuation.json`): periods, commas, question marks, exclamation, colon,
  semicolon, quotes, parens, dashes, a long tail of symbol strokes, forked from Plover's own. Bug
  found: `TP-BG` (`!`, Lapwing's convention) was unreachable. The generated word dictionary loads at
  *higher* priority than punctuation in a real keymux config, so a word the generator picked for
  the same stroke ("factory") shadowed it. Not a one-off: the generator did not know punctuation,
  commands or fingerspelling strokes existed, so this could recur on any regeneration. Fixed
  structurally: `rules/reserved/` holds a copy of the deployed commands, punctuation and
  fingerspelling dictionaries, and `generate()` reserves every stroke in them before any word is
  assigned an outline (the mechanism briefs already use for their own strokes). 40 strokes were
  affected; cost impact negligible (keys/word 3.945 to 3.950).
- **Quotes** (`cargo run -- quote`): pulls live from `zenquotes.io` and strokes the result next to
  Plover/Lapwing. Not yet done: an end-to-end pass through the live daemon on a real quote, to
  confirm punctuation, caps, apostrophes and fallback resolve together.
- **Reverse lookup** (`cargo run -- explain WORD`): goes word → stroke(s) across all three
  theories. A `lookup` alias with terser output is a nice-to-have, not a gap. (A larger
  reverse-lookup redesign is in progress in keymux's own planning, not this repo.)
- **The AO/UH fallback tie-break**: fixed, see `3822e12`. The (since removed) orthographic fallback
  plugin's best guess for a bare `AO` stroke is now "oo" (`UW.oo`), not "uh" (`UH`); "uh" was the
  rarer of three colliding readings but won by an accidental plain-vs-variant tie-break rule.
- **Brief outlines that split a two-key consonant in half** (`good`, `day`, `work`, and 245 of the
  1500 auto-briefed words total): fixed, see `93251ef`. A brief must now read back as the word's
  real first and last sound under lyk's own rules, not just be the fewest keys. Trade-off: the
  headline number moved from 3.49 to 3.73 keys/stroke, behind Plover's 3.51 instead of ahead; see
  [the main README](../README.md#where-lyk-stands).

## 2. Dictionary growth

- **Suffixes** (`rules/suffixes.toml`), done: `-tive`, `-ic`/`-ical`, `-al`, `-logy`, `-ity`/`-ety`,
  `-cy`/`-sy`, `-ify` (two-stroke, like `-ism`). `-ious`/`-eous` needed no entry: the existing "-y"
  stroke followed by the existing "-ous" stroke. `-graphy`/`-ography` stayed out: the schwa before
  it takes a spelling-dependent vowel no fixed standalone stroke can know in advance (see
  `docs/suffixes.md`'s "Not yet covered"). Prefix strokes on the same "fixed stroke that glues onto
  the next one" pattern `ex-`/`dis-` use: `un-`, `mis-`, `over-`, `under-`, `inter-`
  (`rules/plover.toml`). `re-` and `pre-` stayed out: their carve-out ([[spelled]]'s `IH.e`/`IY.e`
  entries) deliberately keeps their reduced vowel, which a fixed prefix would override. `non-`
  stayed out: its sound is a vowel between two copies of the same consonant (`N AA N`), so there is
  no second onset chord to combine with `un-`'s, as `under-`/`inter-` combine two different ones.
- **Briefs** (`rules/briefs.toml`): raise `count` past 1500 and hand-check the next tier. The
  `work`/`wore`/`working`/`worrying`/`wok` collision mess from the Discord log shows these need
  explicit briefs instead of improvised rule codas.
- **Compound suffix strokes** (lower priority): one continuation stroke for a common suffix *pair*
  (e.g. `-er`+`-s` → "-ers" in one stroke instead of two); the one portable idea from tarot's
  Discord chart. The `+` key already makes single suffixes cheap, so this is a smaller win than
  filling out single-suffix coverage; it comes after Plan 2's main suffix work.

## 3. Theory architecture — ideas from onyx's write-ups

Read: `onyx` (+ `/conflicts` and `/regular-outline-grammars`), `onyx-syllabic-splitting`,
`generated-dictionaries`. Verdict: lyk's architecture already matches their conclusions. Their
taxonomy names word-boundary, proper-noun, chord/stenonym and homophonic conflicts, each with a
structural fix. lyk has their recommended fix for the main one, a dedicated join key for
word-boundary conflicts (their "option 1", rated above the vowel-dropping alternative Phoenix uses;
tarot said that alternative "doesn't go all the way", which checks out: it causes
forget/forgot-style collisions the join key avoids by construction).

**Conflict-taxonomy audit — done, 2026-10-29:**
- **Word-boundary**: solved by construction, the join key (rule 1 of this section).
- **Proper-noun**: handled by command strokes, not a dedicated outline shape. The deployed
  punctuation dictionary (`03-lyk-punctuation.json`) carries `KPA`/`KPA*` → `{-|}` (capitalize the
  next word), fired before a name as in Plover. lyk generates no proper-noun outlines; names type as
  their ordinary phonetic outline, capitalized by that stroke.
- **Chord/stenonym** (two words on one outline): handled by the star ladder (`theory.md` rule 6).
  The commonest word keeps the plain outline; the rest take a star on the first stroke, then the
  last, then two star strokes. ~1,200 words use a star form (`README.md`'s full benchmarks
  section). Not audited word by word; no reason found to.
- **Homophonic**: handled orthographically by spelling rules (`theory.md` rule 3: `sail`/`sale`,
  `ea`/`ee`, `oa`/`o-e`, `air`/`are`), the structural fix the article's taxonomy recommends over
  per-word judgment. Confirmed as the deliberate approach.

All four hold. The one finding was unrelated to the taxonomy: the (since removed) orthographic
fallback plugin's bare-`AO` tie-break (section 1) picked the least likely of three colliding
readings; fixed (see `3822e12`).

Two other ideas from the same reading:
- **Pseudo-strokes**: split one hard-to-encode coda cluster into an onset-equivalent and a
  coda-equivalent half within a single stroke. Relevant only if `[coda]`/`[coda_cluster]` in
  `rules/plover.toml` are expanded toward the denser cluster tables in tarot's chart; a much larger,
  separate project, not scheduled.
- **Flag**: the onyx author tried a fully phonetic rule-based theory and dropped it because
  homophone and unstressed-vowel resolution became "vibes" with no system behind it. Not a reason to
  change course, but the long tail of homophones and reduced-vowel collisions is where lyk needs
  deliberate rules, not per-word judgment, as briefs run out and the rule-generated tail grows.

## 4. Docs — reorder into a sequential read

`docs/` is a lookup table (`theory.md`, `suffixes.md`, `fallback.md`), not a start-to-finish read.
Restructure so reading every doc in order teaches the whole theory:

1. Pitch & the join key (why word boundaries are explicit): in `theory.md` rule order already,
   needs pulling out as its own first chapter
2. Briefs: one outline per word, how the common tier is picked
3. Phonetic rules: onset / vowel / coda, how a plain syllable gets its stroke
4. Spelling disambiguation: picking between ways to write the same sound
5. Prefixes with their own keys
6. Syllables & folding: how an unstressed syllable joins the stroke before it
7. Brevity steps: how a long word gets shorter
8. Suffixes: the standalone continuation-stroke table
9. Punctuation & commands (documented; it already existed)
10. Fingerspelling & misc keys ("Emily's") (likewise)
11. The fallback: what a miss resolves to
12. Benchmarks: where lyk stands, last, as a result, not a rule

Either numbered filenames (`01-join-key.md`, `02-briefs.md`, ...) or an explicit "read in this
order" list on `docs/README.md`; pick one when building this out.

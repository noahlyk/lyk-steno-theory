# Strokes you can type

[← back to the table of contents](README.md) · [← back to the main README](../README.md)

Some chords cannot be pressed at all. The generator never writes one.

## The rule

The right pinky has four keys in a 2×2 block:

```
 -T  -D        top row
 -S  -Z        bottom row
```

A pinky can press **one** of them, **two in a line** (`-TS`, `-DZ`, `-TD`, `-SZ`), or **all four**
flat. It cannot press a **diagonal pair** (`-TZ`, `-SD`) or **exactly three** (`-TDZ`, `-TSD`, ...).
Any stroke that needs that is not a stroke. This is a physical limit of the hand (found by typing
`cadence` as `KAEUPBSD`), not a preference, so it is a hard filter and not a cost.

`Stroke::typable` in `src/stroke.rs` is the single definition. Nothing about the left hand or the
other fingers is limited by it (the left pinky has one key, `S-`).

## What happens to a word that needed one

Everywhere a stroke is produced, an untypable one is dropped or repaired before it can compete:

- **Candidates** (gold briefs, the rules' reading, spelling variants, ladder steps, short forms,
  `-S` respellings): the stroke is **split**, with the pinky keys kept in order for as long as they
  can be pressed together and the rest moved to a joined continuation stroke. `-SD` becomes `-S`
  then `+-D`; `-TZ` becomes `-T` then `+-Z`; `-TDZ` becomes `-TD` then `+-Z`. So `cadence` is
  `KAEUPBS/+-D`. A one-stroke candidate that is typable (an `-S` for `-Z` respelling, a different
  pronunciation) still wins on cost, as before.
- **Backups, family members, phrases and alternates:** an untypable result is not offered.
  A phrase whose OR'd strokes would make a diagonal does not exist.
- **Hand-written outlines** in `rules/briefs.toml` are the one case that is refused loudly:
  `build` stops with an error naming the word.
- The stranded-word check ignores a plain reading that is untypable.

## What it cost

Before the rule, 4,760 outlines broke it (2,112 short forms, 1,098 primaries, 911 extras, 429
phrases, 201 backups, 8 alternates), mostly plurals and past tenses (`-TZ`, `-SD`, `-PBGSD`).
After: none, checked over every file by `typable_tests` and the build. Entries went from 154,495 to
152,542; the difference is the one-stroke forms that no longer exist (plus the ones replaced by a
two-stroke form). Four rare words (`cleansed`, `pulsed`, `blitzed`, `repulsed`) have no outline now
because their only repair fails the consonant check on the voiced `-D`; the
[fallback](fallback.md) reads their strokes back.

Next: **[the roadmap](roadmap.md)**.

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
  `-S` respellings) get two kinds of repair, and the solver picks between them by cost like any
  other candidates, so **a one-stroke form always beats a split when one is free**:
  1. **Swap, still one stroke.** One or two pinky keys are swapped for their voiced or unvoiced
     partner (`-T`/`-D`, `-S`/`-Z`) whenever the result is typable. `-TZ` becomes `-TS` or `-DZ`;
     `-SD` becomes `-ZD` or `-ST`. `cadence` is `KAEUPBTS`, `utters` is `URTS`, `silenced` is
     `SAOEUPBLTS`. These cost what the original did (same number of keys), and a swapped stroke
     held by a more common word simply loses it, like any collision.
  2. **Split, two strokes.** If no swap is free, the pinky keys are kept in order for as long as
     they can be pressed together and the rest moved to a joined continuation stroke. `-SD`
     becomes `-S` then `+-D`; `-TDZ` becomes `-TD` then `+-Z`.
  Only a word whose outline has exactly one untypable stroke gets swaps; a word with more is split.
- **Backups, family members, phrases and alternates:** an untypable result is not offered.
  A phrase whose OR'd strokes would make a diagonal does not exist.
- **Hand-written outlines** in `rules/briefs.toml` are the one case that is refused loudly:
  `build` stops with an error naming the word.
- The stranded-word check ignores a plain reading that is untypable.

## What it cost

Before the rule, 4,760 outlines broke it (2,112 short forms, 1,098 primaries, 911 extras, 429
phrases, 201 backups, 8 alternates), mostly plurals and past tenses (`-TZ`, `-SD`, `-PBGSD`).
After: none, checked over every file by `typable_tests` and the build. Of the primaries that were
untypable (about 1,090), 653 are still one stroke (the swap), 245 went from one stroke to two (every
swap was held by a more common word), 191 were two or more strokes already and 4 lost their outline
(below). Entries went from 154,495 to 156,563 (the swapped one-stroke forms also give many words extra
ways in). Four rare words (`cleansed`, `pulsed`, `blitzed`, `repulsed`) have no outline now
because their only repair fails the consonant check on the voiced `-D`; the
[fallback](fallback.md) reads their strokes back.

Next: **[the roadmap](roadmap.md)**.

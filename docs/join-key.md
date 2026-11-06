# The join key

[← back to the table of contents](README.md) · [← back to the main README](../README.md)

lyk writes word boundaries explicitly instead of leaving them to be inferred. Every stroke after
the first one of a word carries a join key (`+`); a stroke without it always starts a new word.

## The clearest example: one key, a word and an ending

The right-hand `D` is both *do* and *-ed*:

| Strokes | Reads |
|---|---|
| `-D` | *do* (a word: no join key) |
| `ED/+-D` | *ended* (the same `-D` with `+` is the ending *-ed*) |
| `ED` `-D` | *end do* (without `+`, the same two strokes are two words) |

A theory with no join key has to give the suffix the bare key (Plover's `-D` is `{^ed}`, always
attaching to the word before), so *do* cannot have it. Here nothing about the keys decides, only
the join key, so a word and an ending share a key. They do for many:

| Key | Alone | With `+` |
|---|---|---|
| `-D` | *do* | *-ed* |
| `-G` | *go* | *-ing* |
| `-Z` | *was* | *-s* |
| `-R` | *for* | *-er* |
| `-L` | *all* | *-ly* |
| `-PB` | *an* | *-en* |
| `-F` | *of* | *-ive* |

That is what lets an ending be one key and the commonest words keep the cheapest keys.

That one convention is what the rest of this theory leans on, chapter after chapter:

- A continuation stroke — a suffix struck on its own, or the consonants of a syllable that didn't
  fit on the stroke before it — is never mistaken for a word of its own, so it can be as short as
  a single key: `+-G` for "-ing", `+-D` for "-ed", with no risk of colliding with some unrelated
  one- or two-key word that happens to share the same letters. See **[suffixes](suffixes.md)** and
  **[syllables & folding](syllables.md)** for what that buys. A suffix's own chord reads correctly
  after *any* stroke this way, dictionary entry or not — see **[the fallback](fallback.md)**.
- A stem's own outline and a continuation stroke can share the same keys with no clash, because
  the join key alone is what tells them apart — nothing about the keys themselves has to.
- It solves, by construction, a problem other theories patch around: a "word-boundary conflict"
  that would otherwise force something cruder, like dropping a vowel to mark a mid-word stroke —
  which causes its own new collisions (*forget*/*forgot*-style pairs) that a dedicated key avoids
  just by existing.

Every way to build a word with it, on one page: **[every way to join a word](joins.md)**.

Keep this in mind reading everything that follows: whenever a later chapter says a stroke "joins
the one before it," "continues," or is a "continuation stroke," this is the key doing that work.

Next: **[reserved strokes](reserved-strokes.md)** — before a single word gets an outline, commands,
punctuation, and fingerspelling are pinned down first.

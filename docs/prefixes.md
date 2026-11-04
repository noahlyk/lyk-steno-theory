# Prefixes with their own keys

[← back to the table of contents](README.md) · [← back to the main README](../README.md)

**Prefixes with keys of their own.** `ex-` is `KP` and `dis-` is `STK`; the keys join the
next stroke's left hand: *expect* KPEBGT, *distance* STKUPBS. The same fixed-prefix pattern also
covers `un-`, `mis-`, `over-`, `under-`, and `inter-` (`rules/plover.toml`'s `[[prefix]]` table).
The first `[[prefix]]` whose spelling and sounds both match wins, so a prefix that is itself the
start of a longer one (`un-` inside `under-`) is listed so the longer one is checked first. `re-`
and `pre-` are deliberately left out of this table: they already have their own carve-out in
`[[spelled]]` (`IH.e`/`IY.e`) that keeps their reduced vowel, which a fixed prefix chord would
silently override. `non-` is left out too, for a different reason: its sound is a vowel between
two copies of the same consonant (`N AA N`), so there is no second onset chord to combine with
`un-`'s own the way `under-`/`inter-` combine two different ones.

**A related but separate mechanism** adds a *backup* entry for a word whose leading sound sits in
front of some other already-placed word's own *exact* pronunciation, whether or not that leading
sound is one of the fixed prefixes above (`never` is the extra sound "N" in front of *ever*'s own
sounds — no `un-`/`dis-`/etc. involved at all). See **[syllables & folding](syllables.md)** for the
rule. It can only ever land on the *same* stroke as the matched word's own first one, the same way
a fixed prefix above folds onto the next stroke's left hand. When the prefix's keys clash with that
first stroke's own (`unclear`: `un-`'s `TPH` shares an `H` with *clear*'s `KHR`), the prefix is
struck as a stroke of its own instead, with the stem's whole outline joined after it: *unclear*
`UPB/+KHRAOER`, *decode* `TKE/+KOED`. Every common prefix has such a stroke, whether or not a fold
would also work — see **[syllables & folding](syllables.md)** for the list and the checks.

Next: **[syllables & folding](syllables.md)** — how the rest of a word, beyond its first stroke's
onset/vowel/coda, gets written.

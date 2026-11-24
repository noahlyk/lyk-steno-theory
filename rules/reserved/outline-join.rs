//! lyk's outline join: a keymux mod (a plugin that reads no words, see keymux's
//! `docs/plugins/README.md`). While `{:outline:next}` (`PHRO*PBT`) or `{:outline:explain}`
//! (`PHRO*EPBT`) is showing the next word's outline, every stroke with the join key `+` is part of
//! that word: `TPHAT/+STKPWAEUGS` is named whole even where the solver does not read the run as
//! one word and keymux would start a new translation with the `+` stroke.
//!
//! keymux has no notion of a key that continues an outline; it asks this mod (`outline_continues`)
//! about each stroke struck while it follows one, and never otherwise. It changes nothing about
//! how strokes translate. `deploy` copies this file into the keymux steno folder as it is; `+` is
//! lyk's one extra key, stroke bit 23 (`extra_keys: ['+']`). See
//! `site/reference/punctuation.json` (outline notation) and `site/reference/join-key.json`.

/// The join key `+`'s stroke bit.
const PLUS: i64 = 1 << 23;

/// A stroke with `+` continues the outline being shown.
#[unsafe(no_mangle)]
pub extern "C" fn outline_continues(stroke: i64) -> i32 {
    i32::from(stroke & PLUS != 0)
}

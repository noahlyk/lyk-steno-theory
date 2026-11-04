//! Reading strokes back: the same tables that write a word, read the other way, so a stroke the
//! dictionary misses still reads as something (`docs/fallback.md`), and the spelling-mode tables
//! read letter by letter (`docs/spelling-mode.md`).

use super::{Rules, plain};
use crate::stroke::Stroke;
use std::collections::HashMap;

impl Rules {
    /// Read a stroke's keys back into sounds, the same tables read the other way: a miss in
    /// the dictionary is not the end, since the rules can still say what it would have meant.
    /// `None` only when no combination of onset, vowel and coda these rules define fits the
    /// keys down: a true miss, not a guess.
    pub fn resolve_stroke(&self, stroke: Stroke) -> Option<Resolved> {
        let join = stroke.0 & Stroke::JOIN != 0;
        let bits = stroke.0 & !Stroke::JOIN;
        let onset = Self::decompose(bits & Stroke::LEFT, &self.onset_rev)?;
        let vowel_bits = bits & Stroke::VOWELS;
        let right_all = bits & (Stroke::RIGHT | Stroke::STAR);
        let (vowel, coda, marker) = self.decode_right(vowel_bits, right_all)?;
        if onset.is_empty() && vowel.is_none() && coda.is_empty() {
            return None;
        }
        Some(Resolved {
            onset,
            vowel,
            coda,
            marker,
            join,
        })
    }

    /// The vowel and coda for a stroke's vowel-area and right-hand keys. No vowel is a bare
    /// continuation or skeleton stroke of consonants only, valid either way. A star left over
    /// once nothing else claims it is read as the generator's own disambiguation flag (as the
    /// star ladder uses it), not a sound, and the `bool` says so.
    pub(super) fn decode_right(
        &self,
        vowel_bits: u32,
        right_all: u32,
    ) -> Option<(Option<String>, Vec<String>, bool)> {
        // Try the star as a sound first (`*T` for TH, `*PBG` for NG K); only drop it, and flag
        // it as a leftover marker, if nothing fits with it in.
        for (right, marker) in [(right_all, false), (right_all & !Stroke::STAR, true)] {
            if marker && right == right_all {
                continue;
            }
            for (combo, vowel, coda) in &self.vowel_coda_rev {
                let combo_right = combo.0 & Stroke::RIGHT;
                if combo.0 & Stroke::VOWELS == vowel_bits
                    && right & combo_right == combo_right
                    && let Some(mut tail) = Self::decompose(right & !combo_right, &self.coda_rev)
                {
                    let mut coda = vec![coda.clone()];
                    coda.append(&mut tail);
                    return Some((Some(vowel.clone()), coda, marker));
                }
            }
            if vowel_bits == 0 {
                if let Some(coda) = Self::decompose(right, &self.coda_rev) {
                    return Some((None, coda, marker));
                }
            } else if let Some(label) = self.vowel_rev.get(&Stroke(vowel_bits))
                && let Some(coda) = Self::decompose(right, &self.coda_rev)
            {
                return Some((Some(label.clone()), coda, marker));
            }
        }
        None
    }

    /// A run of keys as a sequence of sounds: at the lowest key still down, the entry that
    /// starts there, covers the most keys, (tied on that) is the fewest sounds, and (tied on
    /// that) is a plain sound rather than a spelling variant of one — a plain label and a dotted
    /// variant can cover the exact same keys (coda `D`'s own bits are also `T`'s "-ed" spelling,
    /// *walked* sounding like "walkt"), and without this a tie fell to whichever happened to
    /// sort last, which is not a decision, just an accident of alphabetical order — consumed,
    /// repeated until none are left. `None` if some key is left with nothing that starts there:
    /// these keys spell no sound this theory defines.
    pub(super) fn decompose(bits: u32, catalog: &[(Stroke, Vec<String>)]) -> Option<Vec<String>> {
        let mut bits = bits;
        let mut out = Vec::new();
        while bits != 0 {
            let lowest = bits.trailing_zeros();
            let best = catalog
                .iter()
                .filter(|(s, _)| s.0 != 0 && s.0.trailing_zeros() == lowest && bits & s.0 == s.0)
                .max_by_key(|(s, tokens)| {
                    let plain = !tokens.iter().any(|t| t.contains('.'));
                    (s.0.count_ones(), std::cmp::Reverse(tokens.len()), plain)
                })?;
            out.extend(best.1.iter().cloned());
            bits &= !best.0.0;
        }
        Some(out)
    }

    /// A spelling-mode run (`Stroke::SPELL` on its first stroke) read back letter by letter: no
    /// vowel-guessing ambiguity the phonetic fallback has, since each letter was struck
    /// explicitly. `None` if any stroke's bare chord (join/spell bits aside) isn't in
    /// `rules/spelling.toml`. See `docs/spelling-mode.md`.
    pub(super) fn resolve_spelling(&self, outline: &[Stroke]) -> Option<String> {
        let mut text = String::new();
        for &stroke in outline {
            let bits = Stroke(stroke.0 & !(Stroke::JOIN | Stroke::SPELL));
            text.push_str(self.spelling.get(&bits)?);
        }
        Some(text)
    }

    /// The reverse of [`Self::resolve_spelling`]: a word written out as its own spelling-mode
    /// outline, one letter-chord stroke per character, the first marked `Stroke::SPELL` and every
    /// one after it `Stroke::JOIN` like any other continuation. `None` for a word with any
    /// character `rules/spelling.toml` has no chord for (anything outside plain a-z) — the same
    /// condition that makes a word untypeable by spelling mode at all.
    pub fn spell_word(&self, word: &str) -> Option<Vec<Stroke>> {
        word.chars()
            .enumerate()
            .map(|(i, ch)| {
                let letter = ch.to_lowercase().next()?.to_string();
                let (&bits, _) = self.spelling.iter().find(|(_, l)| **l == letter)?;
                let marker = if i == 0 { Stroke::SPELL } else { Stroke::JOIN };
                Some(Stroke(bits.0 | marker))
            })
            .collect()
    }

    /// The best reading the rules can give a stroke outline: each stroke's sounds, read back
    /// and spelled plainly (not real orthography, just enough to read), continuation strokes
    /// run on and others started with a space. `None` only once some stroke in it cannot be
    /// read at all.
    ///
    /// A run that starts with the spelling-mode marker (`Stroke::SPELL`) is read entirely
    /// differently: see [`Self::resolve_spelling`].
    pub fn resolve(&self, outline: &[Stroke], stems: Option<&HashMap<Stroke, String>>) -> Option<String> {
        if outline.first().is_some_and(|s| s.0 & Stroke::SPELL != 0) {
            return self.resolve_spelling(outline);
        }
        let mut text = String::new();
        // A run whose first stroke is an already-placed single-stroke word reads that word's own
        // text as its stem, not the stroke's bare sounds: `SKP/+-R` ("and" plus "-er") is "ander",
        // never "sper". A one-stroke outline that is a placed word was a dictionary hit already,
        // so this is only ever a real continuation run.
        let mut at = 0;
        if outline.len() > 1
            && let Some(stem) = stems.and_then(|stems| stems.get(&outline[0]))
        {
            // A silent final `e` drops before a piece that starts with a vowel letter: `have` +
            // `ing` is `having`.
            let (next, needs_space, used) = self.piece_at(outline, 1)?;
            text.push_str(drop_silent_e(stem, next.starts_with(['a', 'e', 'i', 'o', 'u'])));
            if needs_space {
                text.push(' ');
            }
            text.push_str(&next);
            at = 1 + used;
        }
        while at < outline.len() {
            let (piece, needs_space, used) = self.piece_at(outline, at)?;
            if needs_space && !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&piece);
            at += used;
        }
        Some(text)
    }

    /// The text of the piece that starts at `outline[at]`, whether a space belongs before it, and
    /// how many strokes it used: a two-stroke suffix from `rules/suffixes.toml` when this stroke and
    /// the next are one (`+EUZ/+-PL` is "ism"), otherwise this stroke alone ([`Self::piece_text`]).
    pub(super) fn piece_at(&self, outline: &[Stroke], at: usize) -> Option<(String, bool, usize)> {
        if at > 0
            && let [first, second, ..] = outline[at..]
            && first.0 & second.0 & Stroke::JOIN != 0
            && let Some((text, _)) = self.two_stroke_suffixes.iter().find(|(_, s)| s[..] == [first, second])
        {
            return Some((text.clone(), false, 2));
        }
        let (text, needs_space) = self.piece_text(outline[at], at > 0)?;
        Some((text, needs_space, 1))
    }

    /// A single stroke's own text within a continuation run: a `rules/suffixes.toml` match if
    /// `is_continuation` and the stroke is exactly one of those chords ("ed", not a generic
    /// phonetic guess at bare consonants, "d") — the same reading it would have as the
    /// dictionary's own standalone entry for this suffix, now unconditional instead of only when
    /// some specific composed outline happens to already be a literal dictionary entry — the
    /// plain phonetic read-back otherwise. The second item says whether a space belongs before
    /// this piece when it is not the very first thing assembled: never, for a suffix match or any
    /// other joined stroke.
    pub(super) fn piece_text(&self, stroke: Stroke, is_continuation: bool) -> Option<(String, bool)> {
        if is_continuation && stroke.0 & Stroke::JOIN != 0 && let Some(suffix) = self.suffixes.get(&stroke) {
            return Some((suffix.clone(), false));
        }
        let resolved = self.resolve_stroke(stroke)?;
        let mut text = String::new();
        for sound in resolved
            .onset
            .iter()
            .chain(resolved.vowel.iter())
            .chain(resolved.coda.iter())
        {
            text.push_str(default_spelling(sound));
        }
        if resolved.marker {
            text.push('*');
        }
        Some((text, !resolved.join))
    }
}

/// What a stroke's keys spell out, read back through the tables that write them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub onset: Vec<String>,
    pub vowel: Option<String>,
    pub coda: Vec<String>,
    /// A star down that no coda or vowel combination claims: a disambiguation flag, not a sound
    pub marker: bool,
    pub join: bool,
}

/// English's own rule for a stem ending in a silent `e`: drop it before a suffix that starts with
/// a vowel letter (`have` + `ing` -> `having`, not `haveing`) so a continuation after a known stem
/// (see [`Rules::resolve`]) reads as real spelling, not naive concatenation. Skipped when the `e`
/// isn't silent - already preceded by another vowel (`agree` + `ing` stays `agreeing`; `argue` +
/// `ing` still drops only the one trailing `e`, `arguing`). Known, unhandled exceptions: English
/// keeps the `e` in a few cases to stay visually distinct from another word (`dye` + `ing` =
/// `dyeing`, not `dying`; `singe` + `ing` = `singeing`, not `singing`) - not special-cased here.
pub(super) fn drop_silent_e(stem: &str, next_starts_with_vowel: bool) -> &str {
    let bytes = stem.as_bytes();
    if next_starts_with_vowel
        && bytes.len() > 1
        && bytes[bytes.len() - 1] == b'e'
        && !matches!(bytes[bytes.len() - 2], b'a' | b'e' | b'i' | b'o' | b'u')
    {
        &stem[..stem.len() - 1]
    } else {
        stem
    }
}

/// A plain, readable spelling for a sound: not real orthography (that needs the word's own
/// spelling, which a stroke alone does not carry), just enough that a resolved reading can be
/// read at all.
pub(super) fn default_spelling(sound: &str) -> &str {
    match plain(sound) {
        "P" => "p",
        "B" => "b",
        "T" => "t",
        "D" => "d",
        "K" => "k",
        "G" => "g",
        "M" => "m",
        "N" => "n",
        "NG" => "ng",
        "F" => "f",
        "V" => "v",
        "TH" => "th",
        "DH" => "dh",
        "S" => "s",
        "Z" => "z",
        "SH" => "sh",
        "ZH" => "zh",
        "CH" => "ch",
        "JH" => "j",
        "HH" => "h",
        "L" => "l",
        "R" => "r",
        "W" => "w",
        "Y" => "y",
        "IY" => "ee",
        "IH" => "ih",
        "EY" => "ay",
        "EH" => "eh",
        "AE" => "a",
        "AA" => "ah",
        "AH" | "UH" => "uh",
        "AO" => "aw",
        "OW" => "oh",
        "UW" => "oo",
        "AY" => "eye",
        "AW" => "ow",
        "OY" => "oy",
        "ER" => "er",
        other => other,
    }
}

//! The faithful check: an outline writes every sound of the word's first onset with that sound's
//! own keys. See `site/reference/collisions.json` ("Faithful outlines").
//!
//! The rules tables already say which keys write each sound (`[onset]`, `[onset_cluster]`). An
//! outline is *faithful* when the keys of the word's first syllable's onset (all of them, as the
//! tables would strike it: `gr` is `TKPWR`, `j` is `SKWR`) are down in its first stroke's left
//! hand. The check is one of containment, not equality: vowels, the star, extra keys and the
//! whole right hand are not its business, only an onset sound's own key left out is. A weakened
//! `G` (`TPWRAF` for `graph`, `TKPOT` for `got`) or a stripped onset (`RAB` for `grab`) is
//! unfaithful. Dropping a coda sound (the `N` of `find` `TPAOEUD`) is allowed.
//!
//! It reads the sounds exactly as the rules do (`label`, `syllables`, `next_run`), so it can
//! never disagree with them about what a word's onset is. The check only says what is unfaithful;
//! whether an unfaithful outline may still be a candidate is `enumerate_candidates`' decision (it
//! must save a whole stroke, or the word be one of the commonest, or pinned in
//! `rules/briefs.toml`).
//!
//! Allowances, each derived from a table or a mechanism that already exists:
//! * the rules' own reading of the word is faithful by definition;
//! * a run of sounds the tables cannot write at all (no entry) is not checked;
//! * a lone `Y` before `UW` is the glide of the vowel (`use`, `unit`, `university`), like the `Y` of
//!   `[onset_cluster]`'s `K Y`, and is not keyed;
//! * a first syllable with no onset (a vowel first, or the `lead` step's dropped vowel) requires
//!   nothing on the left.

use super::{Rules, plain};
use crate::stroke::Stroke;
use std::collections::HashMap;

#[cfg(test)]
#[path = "faithful_tests.rs"]
mod tests;

impl Rules {
    /// The frequency rank below which a word keeps every gold outline, faithful or not
    /// (`faithful_exempt_rank` in `rules/plover.toml`).
    #[must_use]
    pub const fn faithful_exempt_rank(&self) -> usize {
        self.faithful_exempt_rank
    }

    /// The union of the keys of a run of consonants as the tables write them, whatever the order
    /// on the keys: the cluster tables first (longest run), then each sound alone.
    fn consonant_keys(sounds: &[String], single: &HashMap<String, Stroke>, clusters: &HashMap<String, Stroke>) -> Option<u32> {
        let (mut bits, mut at) = (0u32, 0usize);
        while at < sounds.len() {
            let (stroke, used) = Self::next_run(sounds, at, single, clusters)?;
            bits |= stroke.0;
            at += used;
        }
        Some(bits)
    }

    /// Whether `outline` writes the first syllable's onset of `word` (pronounced `sounds`) with
    /// the sounds' own keys, as described in this file's header. An outline the check cannot read
    /// (no syllables, no table entry) is faithful: it never rejects what it does not understand.
    #[must_use]
    pub fn faithful(&self, outline: &[Stroke], word: &str, sounds: &[String]) -> bool {
        let bare = |o: &[Stroke]| o.iter().map(|s| s.0 & !Stroke::JOIN).collect::<Vec<_>>();
        if self.outline(word, sounds).is_some_and(|own| bare(&own) == bare(outline)) {
            return true;
        }
        let Some(syllables) = self.syllables(&self.label(word, sounds)) else { return true };
        let (Some(first), Some(first_stroke)) = (syllables.first(), outline.first()) else { return true };
        // A word-initial `Y` before `UW` is the glide of the vowel (`use` `AOUS`, `unit` `AOUBT`), as
        // the `[onset_cluster]` table already says of a consonant and its `Y`: it is not keyed
        if first.onset == ["Y"] && plain(&first.vowel).starts_with("UW") {
            return true;
        }
        let onset = Self::consonant_keys(&first.onset, &self.onset, &self.onset_cluster).unwrap_or(0);
        onset & !first_stroke.0 & Stroke::LEFT == 0
    }

    /// [`Self::faithful`] for any of a word's pronunciations.
    #[must_use]
    pub fn faithful_any(&self, outline: &[Stroke], word: &str, prons: &[Vec<String>]) -> bool {
        prons.iter().any(|sounds| self.faithful(outline, word, sounds))
    }
}

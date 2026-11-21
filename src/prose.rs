//! Which words have no use in ordinary prose, and so never take a plain stroke from one that has.
//!
//! A word has **no prose use** when nothing shows it is read in running text: non-lexical
//! vocalisations and sound effects (*ugh*, *hmm*, *brr*, *zzz*, *shh*), and arbitrary letter strings
//! a pronunciation list happens to carry (*ees*). The generator gives such a word only its
//! star-ladder outlines (`generate::starred`), so it never displaces a word that is used from a
//! plain stroke, and it still has an outline. See `site/reference/collisions.json`.
//!
//! Evidence, in order:
//! 1. a short list of common interjections that stay plain (`PLAIN`);
//! 2. the word occurs in the prose of the five books `fetch` downloads (`BOOK_USES` times or more):
//!    used in prose, never flagged;
//! 3. its letters are a vocal noise: a run of three of one letter, or the shape of an exclamation
//!    (`ugh`, `aah`, `hmm`, `brr`, `shh`, `psst`, `ooh`: see `EXCLAMATIONS`);
//! 4. it is a short unattested string (`STRAY_LETTERS` letters or fewer): said under `RARE_COUNT`
//!    times, written by no steno dictionary (Plover, Lapwing or any other theory), and pronounced
//!    only by the fallback sources (not `CMUdict`).

use crate::analysis::Setup;
use std::collections::HashMap;
use std::path::Path;

/// Common interjections that are ordinary words, whatever their letters.
const PLAIN: &[&str] = &["oh", "ah", "eh", "uh", "um", "yea", "yeah", "yeh", "ok", "okay", "hey", "wow"];
/// An exclamation's letters with every run of one letter cut to one: `hmm` is `hm`, `ooh` is `oh`.
/// Only for words of three letters or more.
const EXCLAMATIONS: &[&str] = &["ah", "oh", "uh", "eh", "ew", "ugh", "hm", "sh", "br", "gr", "psst", "pst", "tsk", "pft", "argh"];
/// A word this many times in the books is used in prose.
const BOOK_USES: u32 = 3;
/// A stray string is this short or shorter: a long unattested word is a name or a rare word, which
/// keeps its plain stroke.
const STRAY_LETTERS: usize = 4;
/// A word said fewer times than this in the frequency list, and attested nowhere else, is a stray string.
const RARE_COUNT: u64 = 1_000;

/// How many times each lowercase word occurs in the books in `data/corpus` (none when absent).
pub fn book_uses(data: &Path) -> HashMap<String, u32> {
    let mut uses: HashMap<String, u32> = HashMap::new();
    let Ok(files) = std::fs::read_dir(data.join("corpus")) else { return uses };
    for file in files.flatten() {
        let Ok(text) = std::fs::read_to_string(file.path()) else { continue };
        for word in text.split(|c: char| !(c.is_ascii_alphabetic() || c == '\'')) {
            if !word.is_empty() {
                *uses.entry(word.to_ascii_lowercase()).or_default() += 1;
            }
        }
    }
    uses
}

/// Whether the letters are a vocal noise: a run of three of one letter, or an exclamation's shape.
pub fn noise_letters(word: &str) -> bool {
    if word.as_bytes().windows(3).any(|w| w[0] == w[1] && w[1] == w[2]) {
        return true;
    }
    let mut skeleton = String::new();
    for c in word.chars() {
        if !skeleton.ends_with(c) {
            skeleton.push(c);
        }
    }
    word.len() > 2 && EXCLAMATIONS.contains(&skeleton.as_str())
}

/// Whether `word` has no use in ordinary prose (see the module doc).
pub fn no_prose_use(setup: &Setup, word: &str) -> bool {
    if PLAIN.contains(&word) || setup.prose.get(word).copied().unwrap_or(0) >= BOOK_USES {
        return false;
    }
    if noise_letters(word) {
        return true;
    }
    let attested =
        setup.gold.plover.contains_key(word) || setup.gold.lapwing.contains_key(word) || setup.gold.extra.contains_key(word);
    word.len() <= STRAY_LETTERS
        && setup.count.get(word).copied().unwrap_or(0) < RARE_COUNT
        && !attested && setup.fallback_pron.contains(word)
}

#[cfg(test)]
mod tests {
    use super::noise_letters;

    #[test]
    fn exclamation_shapes_are_noise_and_words_are_not() {
        for word in ["ugh", "aah", "hmm", "brr", "zzz", "shh", "mmm", "psst", "ooh", "ahh", "eww", "pfft"] {
            assert!(noise_letters(word), "{word} should be noise");
        }
        for word in ["ease", "yeah", "oh", "ah", "ok", "thing", "mr", "tv", "good", "off", "see", "ash", "shy"] {
            assert!(!noise_letters(word), "{word} should not be noise");
        }
    }
}

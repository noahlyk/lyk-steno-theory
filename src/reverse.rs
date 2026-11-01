//! One reverse-lookup entry point, forward or backward.
//!
//! Mirrors keymux's own `src/steno/reverse.rs` (the engine's live lookup and its CLI share) so
//! the two answer the same way: a query with no lowercase letter that parses as a stroke outline
//! resolves forward (what does this stroke type); anything else is text to find the stroke(s)
//! for, a single word or a whole phrase, word by word, next to Plover's and Lapwing's. A word with
//! no pronunciation at all anywhere (CMUdict, MFA, or WikiPron - see `crate::ipa`) falls back to
//! [`crate::rules::Rules::spell_word`]'s spelling-mode outline rather than going unanswered - see
//! `docs/spelling-mode.md`.

use crate::analysis::Setup;
use crate::stroke::{self, Stroke, render_outline};
use std::collections::HashMap;

/// What a query, as typed, means. See keymux's own `classify` for the identical rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Query {
    Stroke(Vec<Stroke>),
    Text(String),
}

#[must_use]
pub fn classify(input: &str) -> Query {
    let trimmed = input.trim();
    let looks_like_a_stroke =
        !trimmed.is_empty() && !trimmed.chars().any(|c| c.is_ascii_lowercase());
    if looks_like_a_stroke {
        if let Some(strokes) = stroke::parse_outline(trimmed) {
            return Query::Stroke(strokes);
        }
    }
    Query::Text(trimmed.to_string())
}

/// One word's reverse lookup: lyk's own best candidate next to Plover's and Lapwing's shortest,
/// any of the three `None` when that theory (or CMUdict) has nothing for the word.
#[derive(Debug, Clone)]
pub struct WordLookup {
    pub word: String,
    pub lyk: Option<String>,
    /// Whether `lyk` is this word's real, settled outline in the dictionary actually built here -
    /// `false` means striking those exact keys will not produce this word, because the word was
    /// never assigned that slot at all (ranked past whatever `common` the dictionary was built
    /// with, or not a real English word). `lyk` is still shown in that case - a real phonetic
    /// reading or a spelling-mode outline is useful even when it is not (yet) deployed - but a
    /// caller must not present the two as equivalent, which is what [`reverse_word`]'s own doc
    /// comment calls out as the bug this field exists to let a caller avoid.
    pub lyk_deployed: bool,
    pub plover: Option<String>,
    pub lapwing: Option<String>,
}

/// A word's real, settled outline - the same answer `resolve`, `quote`, and the deployed
/// dictionary itself would give, not an independently re-derived approximation - when `dictionary`
/// actually has an entry for it (`lyk_deployed: true`). Otherwise, falls back to the rules' own raw
/// candidates for a word `dictionary` has nothing for at all (one ranked past whatever `common` the
/// dictionary was built with, or not a real English word - `ander` has no dictionary entry of its
/// own, since nobody writes "ander", but the rules can still read it back once struck; see
/// `Rules::resolve`'s own stem-recognition fallback for that case instead), then to spelling mode
/// for a word with no pronunciation on record anywhere (CMUdict, nor MFA or WikiPron's translated
/// fallback - see `crate::ipa`) for the rules to run from at all (`aboveground`; see
/// `Rules::spell_word`). Both fallback tiers set `lyk_deployed: false` - a real phonetic reading or
/// a spelling-mode outline is worth showing even when it was never assigned that exact stroke
/// sequence in the live dictionary, but a caller must say so rather than presenting it as if
/// striking those keys today would actually produce this word (the bug a stale report once
/// mistook for a missing-pronunciation problem: the word already had a real reading, the tool
/// just never said it wasn't deployed).
#[must_use]
pub fn reverse_word(setup: &Setup, dictionary: &HashMap<&str, &[Stroke]>, word: &str) -> WordLookup {
    let lower = word.to_lowercase();
    let deployed = dictionary.get(lower.as_str()).map(|o| render_outline(o));
    let lyk_deployed = deployed.is_some();
    let lyk = deployed
        .or_else(|| setup.candidates(&lower).into_iter().next())
        .or_else(|| setup.rules.spell_word(&lower).map(|o| render_outline(&o)));
    let (plover, lapwing) = setup.gold.shortest(&lower);
    WordLookup {
        word: word.to_string(),
        lyk,
        lyk_deployed,
        plover: plover.map(str::to_string),
        lapwing: lapwing.map(str::to_string),
    }
}

/// `phrase`'s reverse lookup, one entry per word, in the order the phrase gave them. `dictionary`
/// is built once by the caller (see [`reverse_word`]) and shared across every word in the phrase,
/// not rebuilt per word.
#[must_use]
pub fn reverse_phrase(setup: &Setup, dictionary: &HashMap<&str, &[Stroke]>, phrase: &str) -> Vec<WordLookup> {
    phrase.split_whitespace().map(|w| reverse_word(setup, dictionary, w)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stroke::parse_outline;

    #[test]
    fn an_all_caps_outline_classifies_as_a_stroke() {
        assert_eq!(classify("KAT/+-D"), Query::Stroke(parse_outline("KAT/+-D").unwrap()));
    }

    #[test]
    fn lowercase_text_classifies_as_text_even_if_it_would_parse() {
        assert_eq!(classify("kat"), Query::Text("kat".to_string()));
        assert_eq!(
            classify("a whole phrase"),
            Query::Text("a whole phrase".to_string())
        );
    }

    #[test]
    fn empty_input_is_text_not_a_stroke() {
        assert_eq!(classify(""), Query::Text(String::new()));
        assert_eq!(classify("   "), Query::Text(String::new()));
    }

    #[test]
    fn a_word_with_no_pronunciation_falls_back_to_spelling_mode() {
        let setup = Setup::load(std::path::Path::new("data"), None, Some(1_000)).unwrap();
        // "stenography" no longer qualifies - it now has an MFA-derived fallback pronunciation
        // (see `crate::ipa`) and a real phonetic outline, so this uses "aboveground" instead: a
        // real word with no entry in CMUdict, MFA, or WikiPron at all (checked directly against
        // the real downloaded files, not assumed).
        let found = reverse_word(&setup, &HashMap::new(), "aboveground");
        assert_eq!(found.lyk.as_deref(), Some("~A/+PW/+O/+SR/+E/+TKPW/+R/+O/+U/+TPH/+TK"));
        assert!(
            !found.lyk_deployed,
            "spelling mode is a real, typeable outline, but it was never assigned this word's \
             own slot in any built dictionary - a caller must not present the two as the same"
        );
    }

    #[test]
    fn lyk_deployed_tells_a_real_dictionary_hit_from_every_fallback_tier() {
        // The regression guard for the bug a stale report once mistook for a missing-pronunciation
        // problem: `lookup` silently treated a rules-derived guess as if it were this word's real,
        // struck-it-and-you-get-it answer, with no way to tell the two apart. `lyk_deployed` is
        // what a caller checks instead of assuming `lyk.is_some()` means "real".
        let setup = Setup::load(std::path::Path::new("data"), None, Some(1_000)).unwrap();

        // A word truly in `dictionary`: `lyk_deployed` is true, and `lyk` is exactly what
        // `dictionary` holds for it, not an independently re-derived reading.
        let mut dictionary: HashMap<&str, &[crate::stroke::Stroke]> = HashMap::new();
        let the = parse_outline("-T").unwrap();
        dictionary.insert("the", &the[..]);
        let found = reverse_word(&setup, &dictionary, "the");
        assert!(found.lyk_deployed);
        assert_eq!(found.lyk.as_deref(), Some("-T"));

        // A word with a real pronunciation but no entry in `dictionary` at all (ranked past
        // whatever `common` a real dictionary was built with, here simulated with an empty one):
        // `lyk` still shows the rules' own real reading, but `lyk_deployed` says plainly that
        // striking it will not actually produce this word today.
        let found = reverse_word(&setup, &HashMap::new(), "stenography");
        assert!(found.lyk.is_some(), "a real phonetic reading should still be shown");
        assert!(!found.lyk_deployed, "but never as if it were this word's real, assigned outline");

        // The spelling-mode tier (covered above) is the same story: `lyk_deployed` is false there
        // too, confirmed by the sibling test.
    }
}

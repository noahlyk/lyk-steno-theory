//! One reverse-lookup entry point, forward or backward.
//!
//! Mirrors keymux's own `src/steno/reverse.rs` (the engine's live lookup and its CLI share) so
//! the two answer the same way: a query with no lowercase letter that parses as a stroke outline
//! resolves forward (what does this stroke type); anything else is text to find the stroke(s)
//! for, a single word or a whole phrase, word by word, next to Plover's and Lapwing's. A word with
//! no pronunciation at all anywhere (`CMUdict`, MFA, or `WikiPron` - see `crate::ipa`) falls back to
//! [`crate::rules::Rules::spell_word`]'s spelling-mode outline rather than going unanswered - see
//! `site/reference/spelling-mode.json`.

use crate::analysis::Setup;
use crate::stroke::{self, Stroke, render_outline};
use std::collections::HashMap;

/// A word the dictionary does not hold as one entry, written from pieces it does: its own words
/// and its standalone suffixes, each piece after the first attached to what came before. In lyk
/// that is the join key on the piece's first stroke; Plover and Lapwing have no join key, so a
/// suffix is its own `{^s}` stroke and a second word needs their `{^}` attach stroke between.
/// `everybody's` is `EFB` (`everybody`) then `+-PBTS` (`'s`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Composition {
    pub strokes: Vec<Stroke>,
    /// The text each piece writes, in order
    pub pieces: Vec<String>,
}

/// How a second word is attached to the one before it.
#[derive(Debug, Clone, Copy)]
pub enum Glue<'a> {
    /// The join key set on the word's first stroke: no extra stroke (lyk)
    JoinKey,
    /// This stroke struck between the two words (`{^}`, in Plover and Lapwing)
    Stroke(&'a [Stroke]),
    /// No way to attach one word to another: only a word and a suffix can be composed
    Unavailable,
}

/// The fewest strokes, then fewest keys, then fewest pieces, that spell `word` from `get`'s own
/// words (two letters or more, so no stray letter is ever a "word") and `suffixes`, or `None`
/// when no whole-word split exists. A word the dictionary holds outright is never a composition
/// of itself; the caller asks only after a miss. This is the same idea as the generator's backup
/// route (two placed words back to back, a stem and a suffix) read at lookup time, so a word the
/// build never placed still gets the outline a learner would reach for.
#[must_use]
pub fn compose(
    get: impl Fn(&str) -> Option<Vec<Stroke>>,
    suffixes: &[(&str, Vec<Stroke>)],
    glue: Glue<'_>,
    word: &str,
) -> Option<Composition> {
    type Cost = (usize, u32, usize);
    let at: Vec<usize> = word.char_indices().map(|(i, _)| i).chain(std::iter::once(word.len())).collect();
    let last = at.len() - 1;
    // best[k] = cheapest way to spell the first k characters: its cost, where its last piece
    // starts, and that piece's strokes
    let mut best: Vec<Option<(Cost, usize, Vec<Stroke>)>> = vec![None; at.len()];
    for end in 1..=last {
        let mut here: Option<(Cost, usize, Vec<Stroke>)> = None;
        for start in 0..end {
            let before: Cost = if start == 0 {
                (0, 0, 0)
            } else if let Some((cost, _, _)) = &best[start] {
                *cost
            } else {
                continue;
            };
            let piece = &word[at[start]..at[end]];
            let mut options: Vec<Vec<Stroke>> = Vec::new();
            if end - start >= 2
                && !(start == 0 && end == last)
                && let Some(outline) = get(piece)
                && outline.first().is_some_and(|s| s.0 & Stroke::SPELL == 0)
            {
                match (start, glue) {
                    (0, _) => options.push(outline),
                    (_, Glue::JoinKey) => {
                        let mut strokes = outline;
                        strokes[0].0 |= Stroke::JOIN;
                        options.push(strokes);
                    }
                    (_, Glue::Stroke(between)) => options.push([between, outline.as_slice()].concat()),
                    (_, Glue::Unavailable) => {}
                }
            }
            if start > 0 {
                options.extend(suffixes.iter().filter(|(text, _)| *text == piece).map(|(_, s)| s.clone()));
            }
            for strokes in options {
                let cost = (before.0 + strokes.len(), before.1 + crate::generate::keys(&strokes), before.2 + 1);
                if here.as_ref().is_none_or(|(held, _, _)| cost < *held) {
                    here = Some((cost, start, strokes));
                }
            }
        }
        best[end] = here;
    }
    best[last].as_ref()?;
    let (mut chunks, mut pieces, mut end) = (Vec::new(), Vec::new(), last);
    while end > 0 {
        let (_, start, piece) = best[end].as_ref()?;
        chunks.push(piece.clone());
        pieces.push(word[at[*start]..at[end]].to_owned());
        end = *start;
    }
    chunks.reverse();
    pieces.reverse();
    Some(Composition { strokes: chunks.concat(), pieces })
}

/// Whether `composed` costs no more than the rules' own `reading`: strokes first, then keys, a
/// tie going to the composition because every piece of it is an outline already in the dictionary.
#[must_use]
pub fn composition_wins(composed: &Composition, reading: Option<&str>) -> bool {
    let Some((strokes, keys)) = reading.and_then(crate::quote::cost) else { return true };
    (composed.strokes.len(), crate::generate::keys(&composed.strokes)) <= (strokes, keys)
}

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
    if looks_like_a_stroke
        && let Some(strokes) = stroke::parse_outline(trimmed) {
            return Query::Stroke(strokes);
        }
    Query::Text(trimmed.to_string())
}

/// One word's reverse lookup: lyk's own best candidate next to Plover's and Lapwing's shortest,
/// any of the three `None` when that theory (or `CMUdict`) has nothing for the word.
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
    /// The pieces `lyk` was composed from when it is not a dictionary entry of its own (see
    /// [`compose`]): `["everybody", "'s"]`. Never set for a deployed word.
    pub composed_of: Option<Vec<String>>,
    pub plover: Option<String>,
    /// The pieces `plover` was composed from when Plover has no entry of its own for the word
    pub plover_composed_of: Option<Vec<String>>,
    pub lapwing: Option<String>,
    /// The same for `lapwing`
    pub lapwing_composed_of: Option<Vec<String>>,
}

/// A word's real, settled outline - the same answer `resolve`, `quote`, and the deployed
/// dictionary itself would give, not an independently re-derived approximation - when `dictionary`
/// actually has an entry for it (`lyk_deployed: true`). Otherwise, falls back to the rules' own raw
/// candidates for a word `dictionary` has nothing for at all (one ranked past whatever `common` the
/// dictionary was built with, or not a real English word - `ander` has no dictionary entry of its
/// own, since nobody writes "ander", but the rules can still read it back once struck; see
/// `Rules::resolve`'s own stem-recognition fallback for that case instead), then to spelling mode
/// for a word with no pronunciation on record anywhere (`CMUdict`, nor MFA or `WikiPron`'s translated
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
    let mut composed_of = None;
    let lyk = deployed
        .or_else(|| {
            let reading = setup.candidates(&lower).into_iter().next();
            match compose(|w| dictionary.get(w).map(|o| o.to_vec()), &setup.rules.suffix_outlines(), Glue::JoinKey, &lower) {
                Some(composed) if composition_wins(&composed, reading.as_deref()) => {
                    let text = render_outline(&composed.strokes);
                    composed_of = Some(composed.pieces);
                    Some(text)
                }
                _ => reading,
            }
        })
        .or_else(|| setup.rules.spell_word(&lower).map(|o| render_outline(&o)));
    let (plover, lapwing) = setup.gold.shortest(&lower);
    let gold_composed = |theory| {
        setup.gold.compose(theory, &lower).map(|c| (render_outline(&c.strokes), c.pieces))
    };
    let (plover, plover_composed_of) =
        plover.map_or_else(|| gold_composed(crate::gold::Theory::Plover).unzip(), |own| (Some(own.to_owned()), None));
    let (lapwing, lapwing_composed_of) =
        lapwing.map_or_else(|| gold_composed(crate::gold::Theory::Lapwing).unzip(), |own| (Some(own.to_owned()), None));
    WordLookup {
        word: word.to_string(),
        lyk,
        lyk_deployed,
        composed_of,
        plover,
        plover_composed_of,
        lapwing,
        lapwing_composed_of,
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

    fn dict(entries: &[(&'static str, &str)]) -> HashMap<&'static str, Vec<Stroke>> {
        entries.iter().map(|(w, o)| (*w, parse_outline(o).unwrap())).collect()
    }

    fn composed(
        dictionary: &HashMap<&'static str, Vec<Stroke>>,
        suffixes: &[(&'static str, &str)],
        glue: Glue<'_>,
        word: &str,
    ) -> Option<(String, Vec<String>)> {
        let suffixes: Vec<(&str, Vec<Stroke>)> = suffixes.iter().map(|(t, o)| (*t, parse_outline(o).unwrap())).collect();
        compose(|w| dictionary.get(w).cloned(), &suffixes, glue, word)
            .map(|c| (render_outline(&c.strokes), c.pieces))
    }

    #[test]
    fn a_word_and_a_suffix_compose_with_the_join_key_written_out() {
        let dictionary = dict(&[("everybody", "EFB")]);
        let got = composed(&dictionary, &[("'s", "+-PBTS")], Glue::JoinKey, "everybody's").unwrap();
        assert_eq!(got.0, "EFB/+-PBTS");
        assert_eq!(got.1, ["everybody", "'s"]);
    }

    #[test]
    fn two_words_compose_with_the_join_key_on_the_second() {
        let dictionary = dict(&[("no", "TPHO"), ("one", "WUPB")]);
        let got = composed(&dictionary, &[], Glue::JoinKey, "noone").unwrap();
        assert_eq!(got.0, "TPHO/+WUPB");
        assert_eq!(got.1, ["no", "one"]);
    }

    #[test]
    fn plover_style_attach_stroke_goes_between_two_words_and_not_before_a_suffix() {
        let dictionary = dict(&[("no", "TPHO"), ("one", "WUPB"), ("dog", "TKOG")]);
        let glue = parse_outline("TK-LS").unwrap();
        let got = composed(&dictionary, &[("s", "-S")], Glue::Stroke(&glue), "noone").unwrap();
        assert_eq!(got.0, "TPHO/TK-LS/WUPB");
        let got = composed(&dictionary, &[("s", "-S")], Glue::Stroke(&glue), "dogs").unwrap();
        assert_eq!(got.0, "TKOG/-S");
    }

    #[test]
    fn without_an_attach_stroke_only_a_word_and_a_suffix_compose() {
        let dictionary = dict(&[("no", "TPHO"), ("one", "WUPB"), ("dog", "TKOG")]);
        assert!(composed(&dictionary, &[("s", "-S")], Glue::Unavailable, "noone").is_none());
        assert!(composed(&dictionary, &[("s", "-S")], Glue::Unavailable, "dogs").is_some());
    }

    #[test]
    fn a_word_is_never_composed_of_itself_or_of_single_letters() {
        let dictionary = dict(&[("cat", "KAT"), ("ca", "KA")]);
        assert!(composed(&dictionary, &[], Glue::JoinKey, "cat").is_none(), "no one-piece composition");
        assert!(composed(&dictionary, &[("t", "+T")], Glue::JoinKey, "cat").is_some(), "a suffix piece may be a letter");
        assert!(composed(&dictionary, &[], Glue::JoinKey, "ca").is_none());
    }

    #[test]
    fn the_fewest_strokes_win_then_the_fewest_keys() {
        // `every` + `body` is two strokes; `ever` + `y` + ... would not even be words; but
        // `everybody` + `'s` (one stroke each) beats `every` + `body` + `'s`.
        let dictionary = dict(&[("everybody", "EFB"), ("every", "EFR"), ("body", "PWOD")]);
        let got = composed(&dictionary, &[("'s", "+-PBTS")], Glue::JoinKey, "everybody's").unwrap();
        assert_eq!(got.0, "EFB/+-PBTS");
        let got = composed(&dictionary, &[], Glue::JoinKey, "everybody").unwrap();
        assert_eq!(got.0, "EFR/+PWOD", "a held word is not composed here, the caller asks only after a miss");
    }

    #[test]
    fn a_composition_wins_a_tie_with_the_rules_reading_and_loses_to_a_shorter_one() {
        let one = Composition { strokes: parse_outline("EFB/+-PBTS").unwrap(), pieces: vec![] };
        assert!(composition_wins(&one, Some("EF/RAOE/PWOD/AOEZ")), "fewer strokes");
        assert!(composition_wins(&one, Some("TPHO/WUPB")), "same strokes, same keys: the tie goes to the pieces");
        assert!(!composition_wins(&one, Some("EFB")), "one stroke beats two");
        assert!(composition_wins(&one, None), "no reading at all");
    }
}

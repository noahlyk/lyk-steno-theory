//! Joinless spellings: a `+` join written the way Plover writes a long word, as plain strokes with
//! no join key (`KOPL/PRO/PHAOEUZ` for *compromise*, the same strokes as `KOPL/+PR/+PHAOEUZ`).
//!
//! Struck one at a time, those strokes are three words (*column*, *prah*, *meyez*), so the multi-
//! stroke entry has to exist for the engine to read them as one. Every backup join gets its
//! joinless twin, derived here from the backups so the two can never drift apart. The generator's
//! one-word-per-outline guarantee is untouched: a twin never takes an outline anything else has,
//! and never goes to the engine as a word's primary.
//!
//! A run of strokes that are all common words is left out (*compress* `KUPL/PRES`: two ordinary
//! words struck one after the other must stay two words). See `docs/joins.md` and
//! `docs/decisions.md`.

use crate::analysis::Setup;
use crate::generate::{Generated, Origin, written};
use crate::stroke::{Stroke, render_outline};
use std::collections::{BTreeMap, HashMap, HashSet};

/// A stroke is a common word when a word ranked inside this holds it alone: a joinless spelling
/// made only of those would be what you type for those words one after the other.
pub const COMMON_RUN_TOP: usize = 3_000;

/// Every backup join as plain strokes: outline text to word text, the commoner word winning an
/// outline two joins share.
pub fn aliases(generated: &Generated, setup: &Setup) -> BTreeMap<String, String> {
    let rank = |word: &str| setup.rank.get(word).copied().unwrap_or(usize::MAX);
    let taken: HashSet<String> = generated.entries.iter().map(|e| render_outline(&e.outline)).collect();
    let common: HashSet<Stroke> = generated
        .entries
        .iter()
        .filter(|e| e.outline.len() == 1 && e.origin != Origin::Phrase && rank(&e.word) < COMMON_RUN_TOP)
        .map(|e| e.outline[0])
        .collect();
    let mut best: HashMap<String, (&str, usize)> = HashMap::new();
    for entry in generated.entries.iter().filter(|e| e.origin == Origin::Backup && e.outline.len() > 1) {
        if entry.outline.iter().any(|s| s.is_marker()) {
            continue;
        }
        let plain: Vec<Stroke> = entry.outline.iter().map(|s| Stroke(s.0 & !Stroke::JOIN)).collect();
        // A bare join stroke has nothing left once the key is off
        if plain == entry.outline || plain.contains(&Stroke(0)) || plain.iter().all(|s| common.contains(s)) {
            continue;
        }
        let text = render_outline(&plain);
        if taken.contains(&text) {
            continue;
        }
        let rank = rank(&entry.word);
        let slot = best.entry(text).or_insert((&entry.word, rank));
        if (rank, entry.word.as_str()) < (slot.1, slot.0) {
            *slot = (&entry.word, rank);
        }
    }
    best.into_iter().map(|(text, (word, _))| (text, written(word))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictionary_fixture;

    #[test]
    fn compromise_is_typable_syllable_by_syllable_with_any_break() {
        let aliases = aliases(dictionary_fixture::generated(), dictionary_fixture::setup());
        // Every break of the middle consonants is valid: com-pro-mise, comp-ro-mise, compro-mise
        for outline in ["KOPL/PRO/PHAOEUZ", "KOPL/PR/PHAOEUZ", "KOPL/PROPL/AOEUZ"] {
            assert_eq!(aliases.get(outline).map(String::as_str), Some("compromise"), "{outline}");
        }
    }

    #[test]
    fn no_alias_has_a_join_key_or_takes_an_outline_something_else_has() {
        let generated = dictionary_fixture::generated();
        let taken: HashSet<String> = generated.entries.iter().map(|e| render_outline(&e.outline)).collect();
        for text in aliases(generated, dictionary_fixture::setup()).keys() {
            assert!(!text.contains('+'), "{text}");
            assert!(!taken.contains(text), "{text} is already an outline");
        }
    }

    #[test]
    fn two_common_words_in_a_row_stay_two_words() {
        let setup = dictionary_fixture::setup();
        let generated = dictionary_fixture::generated();
        let common: HashSet<Stroke> = generated
            .entries
            .iter()
            .filter(|e| e.outline.len() == 1 && e.origin != Origin::Phrase && setup.rank.get(&e.word).is_some_and(|&r| r < COMMON_RUN_TOP))
            .map(|e| e.outline[0])
            .collect();
        // `com` is pinned on a number-bar stroke (`#KOPL`), which the parser does not read back
        for text in aliases(generated, setup).keys().filter(|t| !t.contains('#')) {
            let Some(strokes) = crate::stroke::parse_outline(text) else { panic!("{text} does not parse") };
            assert!(!strokes.iter().all(|s| common.contains(s)), "{text} is a run of common words");
        }
    }
}

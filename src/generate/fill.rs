//! The passes that only ever add entries once every primary is settled, each taking a text only
//! when nobody holds it: other pronunciations, bonus short forms, and last of all extra one-stroke
//! entries. Plus the stranded-word check, a regression guard over the finished dictionary.

use super::candidates::candidate_cost;
use super::settle::Pools;
use super::{Book, KEY_WEIGHT, Origin, cheapest, keys, typable};
use crate::analysis::Setup;
use crate::stroke::{Stroke, render_outline};
use std::collections::{HashMap, HashSet};

/// At most this many `Origin::Extra` entries per word, cheapest first.
const MAX_EXTRAS_PER_WORD: usize = 3;
/// An extra costs at most this many keys more than the word's own outline, so a learner meets
/// competitive alternatives (a phonetic full form, a Plover brief), not Plover's long oddities.
const EXTRA_KEY_MARGIN: u32 = 3;

/// Every other pronunciation's rules outline, where it clashes with nothing: never competes for
/// the primary, which is the first pronunciation's.
pub fn alternates(setup: &Setup, words: &[(&String, usize)], book: &mut Book) {
    for (word, _) in words {
        let mut seen: Vec<Vec<Stroke>> = Vec::new();
        for sounds in &setup.pron[word.as_str()] {
            let Some(outline) = setup.rules.outline(word, sounds) else { continue };
            if seen.contains(&outline) {
                continue;
            }
            seen.push(outline.clone());
            if seen.len() == 1 {
                continue;
            }
            let mut outline = outline;
            setup.rules.joined(&mut outline);
            if typable(&outline) {
                book.claim(word, outline, Origin::Alternate);
            }
        }
    }
}

/// Every word's `Origin::Short` candidates still free, beside its primary: the dictionary holds
/// every short form a learner might reach for, and `cheapest` picks whichever counts. After every
/// primary and every backup join, so a one-off short form never takes a text a primary or a
/// predictable join wanted (see `site/reference/brevity.json`).
pub fn short_forms(pools: &Pools, book: &mut Book) {
    for (p, (word, candidates)) in pools.iter().enumerate() {
        for (candidate, text) in candidates.iter().zip(&pools.texts[p]) {
            if matches!(candidate.origin, Origin::Short(_)) && !book.taken.contains_key(text) {
                book.place(word, candidate.outline.clone(), candidate.origin, text.clone());
            }
        }
    }
}

/// Extra one-stroke entries, last of all, in frequency order: a word's other one-stroke
/// candidates (its gold briefs, its plain reading, a spelling variant, an `-S` respelling, a family
/// shape) whose stroke nobody needed, up to `MAX_EXTRAS_PER_WORD` within `EXTRA_KEY_MARGIN` keys
/// of its outline. Star-ladder steps are collision fallbacks, not alternates, and are left out.
/// Returns how many were added.
pub fn extras(pools: &Pools, offers: &HashMap<String, Vec<Vec<Stroke>>>, book: &mut Book) -> usize {
    let primary_cost: HashMap<String, u32> =
        cheapest(&book.entries).into_iter().map(|(w, o)| (w.to_owned(), candidate_cost(o))).collect();
    let mut added = 0;
    for (word, candidates) in pools.iter() {
        let family = offers.get(word).into_iter().flatten().cloned();
        let own = candidates
            .iter()
            .filter(|c| matches!(c.origin, Origin::Brief | Origin::Rules | Origin::Spelling | Origin::Voicing))
            .map(|c| c.outline.clone());
        let limit = primary_cost.get(word).map_or(u32::MAX, |c| c.saturating_add(EXTRA_KEY_MARGIN * KEY_WEIGHT));
        let mut options: Vec<(u32, String, Vec<Stroke>)> = own
            .chain(family)
            .filter(|o| o.len() == 1)
            .map(|o| (candidate_cost(&o), render_outline(&o), o))
            .filter(|(cost, _, _)| *cost <= limit)
            .collect();
        options.sort();
        options.dedup_by(|a, b| a.1 == b.1);
        let mut here = 0;
        for (_, text, outline) in options {
            if here == MAX_EXTRAS_PER_WORD {
                break;
            }
            if !book.taken.contains_key(&text) {
                book.place(word, outline, Origin::Extra, text);
                here += 1;
            }
        }
        added += here;
    }
    added
}

/// Words whose plain rules outline would cost less than what they settled for while its text sits
/// unclaimed. A regression guard over the finished dictionary, expected to stay empty. A word on
/// its family's shape chose it over a cheaper reading on purpose, so it is never stranded.
pub fn stranded(setup: &Setup, book: &Book) -> Vec<String> {
    let family_shaped: HashSet<&str> =
        book.entries.iter().filter(|e| e.origin == Origin::Family).map(|e| e.word.as_str()).collect();
    let mut found: Vec<String> = cheapest(&book.entries)
        .into_iter()
        .filter(|(word, _)| !family_shaped.contains(word))
        .filter(|(word, outline)| {
            let Some(sounds) = setup.pron.get(*word).and_then(|p| p.first()) else { return false };
            let Some(mut plain) = setup.rules.outline(word, sounds) else { return false };
            setup.rules.joined(&mut plain);
            typable(&plain)
                && (plain.len(), keys(&plain)) < (outline.len(), keys(outline))
                && !book.taken.contains_key(&render_outline(&plain))
        })
        .map(|(word, _)| word.to_owned())
        .collect();
    found.sort();
    found
}

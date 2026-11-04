//! Phrase candidates: which word pairs could share one stroke, and the merged outline each would
//! get. Built only from settled primaries, never from a backup or short form, so a phrase rests on
//! the one invariant `generate` guarantees (see `docs/phrases.md`).

use crate::analysis::Setup;
use crate::generate::{Entry, Origin, entry_word, typable};
use crate::stroke::{Stroke, render_outline};
use std::collections::HashMap;

/// Each word's own settled *primary* outline, single-stroke only - exactly the entry `generate`'s
/// own peeling/solver/literal-override step assigned, never a bonus [`Origin::Short`],
/// [`Origin::Alternate`], or [`Origin::Backup`] entry. Feeds [`phrase_candidate`]: a backup entry
/// is itself contingent on composition-pass ordering and can change if the backup pass's own
/// internals change, so building a phrase on top of one would stack one best-effort pass on
/// another; a primary assignment is `generate`'s own invariant instead - one single-stroke outline
/// per word, stable and already heavily tested - and that is what phrase-briefing is built on. See
/// `docs/phrases.md`'s "why primary-only" section for the full reasoning.
pub fn primary_single_strokes(entries: &[Entry]) -> HashMap<&str, Stroke> {
    entries
        .iter()
        .filter(|e| matches!(e.origin, Origin::Brief | Origin::Rules | Origin::Spelling | Origin::Ladder(_) | Origin::Family))
        .filter_map(|e| match e.outline[..] {
            [stroke] => Some((e.word.as_str(), stroke)),
            _ => None,
        })
        .collect()
}

/// Each word's own settled *primary* outline, any length - the generalized counterpart of
/// [`primary_single_strokes`] used as the "host" side of a merge in [`phrase_candidate_outline`]:
/// the word whose stroke *sequence* beyond the merged first stroke is carried into a phrase entry
/// completely unchanged. Exactly the same `word_outline`/`word_outline[stem]` index the
/// backup-composition pass's own direction (b) already reads a stem's outline from regardless of
/// its length - that pass already merges a leading consonant onto `word_outline[stem][0]` and
/// leaves every later stroke (join bits included) untouched, real, shipped, tested code this
/// mirrors rather than a new risk. See `docs/phrases.md`.
pub fn primary_outlines(entries: &[Entry]) -> HashMap<&str, &[Stroke]> {
    entries
        .iter()
        .filter(|e| matches!(e.origin, Origin::Brief | Origin::Rules | Origin::Spelling | Origin::Ladder(_) | Origin::Family))
        .map(|e| (e.word.as_str(), e.outline.as_slice()))
        .collect()
}

/// One eligible phrase candidate: the two words' own primary strokes OR'd together, when
/// disjoint - `None` when either word has no single-stroke primary, or their keys overlap
/// (including the star bit). This is the only one of the two mechanisms this project considered
/// for a phrase entry's key that is a real compression rather than a no-op: naively concatenating
/// `w1`'s own strokes with `w2`'s own strokes costs exactly `strokes(w1) + strokes(w2)`, identical
/// to just striking the two words separately - nothing for such an entry to actually do. Merging
/// into one brand-new, previously-unclaimed stroke genuinely halves the stroke count instead. See
/// `docs/phrases.md`.
///
/// Kept, alongside [`phrase_candidate_outline`], as the original both-single-stroke-only check:
/// still exactly what every existing test and diagnostic in this module exercises directly. No
/// longer called from `generate` itself (superseded there by `phrase_candidate_outline`, which
/// reduces to this exact behavior when both words happen to be single-stroke) - hence
/// `#[allow(dead_code)]` outside tests, the same vestigial-but-still-meaningful pattern `Briefs`'s
/// own `count`/`default` fields already use above.
#[allow(dead_code)]
pub fn phrase_candidate(primary: &HashMap<&str, Stroke>, w1: &str, w2: &str) -> Option<Stroke> {
    let &a = primary.get(w1)?;
    let &b = primary.get(w2)?;
    (a.0 & b.0 == 0).then_some(Stroke(a.0 | b.0))
}

/// OR's `filler` (a single stroke) into `host`'s own first stroke, when the two are disjoint,
/// carrying every one of `host`'s later strokes through completely unchanged - the same
/// "disjoint-OR the first stroke, leave the rest alone" trick `compose_backup`'s own direction (b)
/// already uses on `word_outline[stem]`. `None` when `host` is empty (never true for a real
/// primary) or the two strokes overlap.
pub fn merge_into_host_first_stroke(filler: Stroke, host: &[Stroke]) -> Option<Vec<Stroke>> {
    let &first = host.first()?;
    if filler.0 & first.0 != 0 {
        return None;
    }
    let mut outline = vec![Stroke(filler.0 | first.0)];
    outline.extend_from_slice(&host[1..]);
    typable(&outline).then_some(outline)
}

/// The generalized form of [`phrase_candidate`]: eligible whenever *at least one* of `w1`/`w2` has
/// a single-stroke primary (`single`) and the other has a primary of *any* length, one or more
/// strokes (`any_len`) - the single-stroke side's whole primary is OR'd into the other's own first
/// stroke only, via [`merge_into_host_first_stroke`]. When both happen to be single-stroke, this
/// reduces to exactly [`phrase_candidate`]'s own original behavior (the "host"'s one stroke merges
/// with the filler, nothing left over to carry through). A pair where *neither* word has a
/// single-stroke primary - both multi-stroke - stays out of scope entirely (`None`): reaching into
/// *both* words' internal join-key structure at once is a different, bigger risk than folding one
/// whole single-stroke word onto one multi-stroke word's first stroke, and isn't attempted here.
/// See `docs/phrases.md`'s "What qualifies" and "Not in scope" sections.
pub fn phrase_candidate_outline(
    single: &HashMap<&str, Stroke>,
    any_len: &HashMap<&str, &[Stroke]>,
    w1: &str,
    w2: &str,
) -> Option<Vec<Stroke>> {
    if let (Some(&filler), Some(&host)) = (single.get(w1), any_len.get(w2)) {
        return merge_into_host_first_stroke(filler, host);
    }
    if let (Some(&host), Some(&filler)) = (any_len.get(w1), single.get(w2)) {
        return merge_into_host_first_stroke(filler, host);
    }
    None
}

pub fn enumerate_phrase_candidates(
    setup: &Setup,
    single: &HashMap<&str, Stroke>,
    any_len: &HashMap<&str, &[Stroke]>,
    top_n: usize,
) -> Vec<((String, String), Vec<Stroke>, u64)> {
    let mut by_count: Vec<(&(String, String), &u64)> = setup.bigrams.iter().collect();
    by_count.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    by_count
        .into_iter()
        .take(top_n)
        .filter_map(|((w1, w2), &count)| {
            phrase_candidate_outline(single, any_len, entry_word(w1), entry_word(w2)).map(|outline| ((w1.clone(), w2.clone()), outline, count))
        })
        .collect()
}

/// For one pass's full list of eligible phrase candidates (phrase words, merged outline, the
/// bigram's own raw count), which indices win their outline's text: mirrors
/// `arbitrate_backup_round`'s own tie-break rule exactly (see its doc) rather than a new formula,
/// adapted only for a phrase candidate's key being a *pair* of words rather than one, and its
/// weight already carried alongside each candidate (the bigram's own raw count) rather than looked
/// up by word. Grouped by the full rendered outline text - not just the merged first stroke - the
/// same text `taken` keys every other entry in the dictionary by, since that's what a genuine
/// collision actually means once an outline can be more than one stroke long. Two different
/// eligible bigrams can coincidentally land on the identical rendered text even though the words
/// differ (`setup.bigrams[(w1, w2)]`'s own bits happening to union the same way as some other
/// pair's) - the winner is whichever bigram was actually said more often, with a lexical tie-break
/// on the phrase text for determinism.
pub fn arbitrate_phrase_round(candidates: &[((String, String), Vec<Stroke>, u64)]) -> Vec<bool> {
    let mut by_text: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, (_, outline, _)) in candidates.iter().enumerate() {
        by_text.entry(render_outline(outline)).or_default().push(i);
    }
    let mut is_winner = vec![true; candidates.len()];
    for indices in by_text.values() {
        if indices.len() < 2 {
            continue;
        }
        let winner = *indices
            .iter()
            .max_by_key(|&&i| {
                let ((w1, w2), _, count) = &candidates[i];
                (*count, std::cmp::Reverse(format!("{w1} {w2}")))
            })
            .expect("by_text only ever groups indices that exist");
        for &i in indices {
            is_winner[i] = i == winner;
        }
    }
    is_winner
}

//! Phrase entries: two common words in one stroke, and which words give up a stroke for one.
//!
//! The merge itself (two primaries OR'd into a new outline) and the list of candidate bigrams live
//! in [`crate::generate`]; this module decides which candidates are committed. A candidate whose
//! merged stroke is free is committed as is. A candidate whose stroke a word already holds
//! *competes* for it: it takes the stroke when the pair is said often enough to be worth more than
//! the word loses by moving to its next-best outline. See `docs/phrases.md`, "Phrases compete for
//! strokes".

use crate::analysis::Setup;
use crate::generate::{Candidate, Entry, Origin, STROKE_WEIGHT, candidate_cost, entry_word, keys, typable};
use crate::stroke::{render_outline, Stroke};
use std::collections::{HashMap, HashSet};

/// How many of the most frequent bigrams are tried.
pub const PHRASE_TOP_N: usize = 20_000;

/// The most frequent words (by `Setup::count`) never lose their *primary* outline to a phrase. A
/// phrase may still take a short form or an alternate from them, since the word keeps its own
/// outline. Was 1,500: with that, 373 words had been pushed onto a star form by a phrase, 42 of
/// them ranked 1,500 to 5,000 (*twenty* `TW*ET` for "with text", *vincent* for "vicinity of").
pub const PROTECTED_WORDS: usize = 5_000;

/// Only this many of the most frequent eligible pairs may take a stroke a word holds; a rarer pair
/// is committed only into a free stroke. A pair past this point is one nobody will learn, so the
/// stroke it would save is not real, while the star it puts on a word is.
pub const TAKEOVER_TOP_N: usize = 1_000;

/// What the pass did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    /// Phrase entries committed
    pub added: usize,
    /// Candidates that lost their stroke to something that stayed
    pub skipped: usize,
    /// Of those committed, how many took the stroke from a word
    pub displaced: usize,
}

/// Whether an entry may be taken by a phrase. A suffix, a family member's shape (it is defined by
/// its stem), a ladder-settled clash, another phrase, and a multi-stroke backup (a stem plus an
/// ending the learner already knows, joined: the one multi-stroke way in that is predictable, and
/// `takeover` would price losing it at nothing, since the word's primary is cheaper) never are. A
/// one-stroke backup (a leading sound folded onto a shorter word, `sit` as `s` + `it` `ST`) is a
/// bonus like a short form, and a common pair may take it (`it is` is `ST`).
fn can_be_taken(entry: &Entry) -> bool {
    match entry.origin {
        Origin::Brief | Origin::Rules | Origin::Spelling | Origin::Voicing | Origin::Short(_) | Origin::Alternate => true,
        Origin::Backup => entry.outline.len() == 1,
        _ => false,
    }
}

/// Whether the entry is a primary outline worth protecting: a brief, the plain reading, a
/// spelling or voicing variant. A short-form primary (`antonio` `TPH-T`, a one-off the word was
/// given because it was free) is not: one of the commonest pairs may still take it, and the word
/// is rehomed to its next short form (`TPHO*EPBT`), see `unclaimed_candidate`.
const fn is_primary(origin: Origin) -> bool {
    matches!(origin, Origin::Brief | Origin::Rules | Origin::Spelling | Origin::Voicing)
}

/// `count` as a fraction of `total`.
#[allow(clippy::cast_precision_loss)]
fn share(count: u64, total: u64) -> f64 {
    count as f64 / total.max(1) as f64
}

/// What the word that holds a stroke does when a phrase wants it.
struct Takeover {
    /// Position in `entries` of the entry that is taken
    index: usize,
    /// A new primary for the word, when its next-best outline is one it does not have yet
    rehome: Option<(Vec<Stroke>, Origin)>,
}

/// The state of one pass.
struct Pass<'a> {
    entries: &'a mut Vec<Entry>,
    taken: &'a mut HashMap<String, String>,
    /// The position in `entries` of each outline's entry, by rendered text
    owner: HashMap<String, usize>,
    /// Positions of entries a phrase has taken
    removed: HashSet<usize>,
    /// Entries given to words that lost one
    rehomed: Vec<Entry>,
    /// Words a committed phrase is built from, and words that lost their primary outline: neither
    /// may take part in the other's story
    used: HashSet<String>,
    gone: HashSet<String>,
    protected: HashSet<&'a str>,
    stems: &'a HashSet<&'a str>,
    pools: &'a HashMap<&'a str, &'a [Candidate]>,
    natural: &'a dyn Fn(&str) -> Option<Vec<Stroke>>,
    literal: &'a dyn Fn(&str) -> bool,
    setup: &'a Setup,
    total_pairs: u64,
    total_words: u64,
}

impl Pass<'_> {
    /// The other outlines `word` has, apart from the entry at `index` and any alternate
    /// pronunciation (which never stands in for the word).
    fn others(&self, word: &str, index: usize) -> impl Iterator<Item = &Entry> {
        self.entries
            .iter()
            .enumerate()
            .filter(move |(j, e)| *j != index && !self.removed.contains(j) && e.word == word && e.origin != Origin::Alternate)
            .map(|(_, e)| e)
    }

    /// The best outline `word` has not got that nobody holds: any of its own candidates (a short
    /// form or a star rung is as good a primary as any other), or its plain-rules reading.
    fn unclaimed_candidate(&self, word: &str, but: &str) -> Option<(Vec<Stroke>, Origin)> {
        self.pools
            .get(word)
            .into_iter()
            .flat_map(|pool| pool.iter().map(|c| (c.outline.clone(), c.origin)))
            .chain((self.natural)(word).filter(|outline| typable(outline)).map(|outline| (outline, Origin::Rules)))
            .filter(|(outline, _)| {
                let text = render_outline(outline);
                text != but && !self.taken.contains_key(&text)
            })
            .min_by_key(|(outline, _)| (candidate_cost(outline), render_outline(outline)))
    }

    /// Whether a pair said `count` times is worth the stroke `text` that a word holds, and if so
    /// what happens to that word. `None` when it is not (or the stroke cannot be taken).
    fn takeover(&self, text: &str, count: u64) -> Option<Takeover> {
        let index = *self.owner.get(text)?;
        let victim = self.entries.get(index)?;
        if self.removed.contains(&index) || !can_be_taken(victim) {
            return None;
        }
        let word = victim.word.as_str();
        let primary = is_primary(victim.origin);
        if self.used.contains(word)
            || (self.literal)(word)
            || (primary && (self.stems.contains(word) || self.protected.contains(word)))
        {
            return None;
        }
        // The word falls back to another outline it already has, or to a better one from its own
        // candidates that nobody holds. "Better" is strokes, then keys: the order the
        // stranded-word check uses.
        let other = self.others(word, index).map(|e| (e.outline.len(), keys(&e.outline))).min();
        let rehome = self
            .unclaimed_candidate(word, text)
            .filter(|(outline, _)| other.is_none_or(|o| (outline.len(), keys(outline)) < o));
        let next_best = self
            .others(word, index)
            .map(|e| candidate_cost(&e.outline))
            .chain(rehome.as_ref().map(|(outline, _)| candidate_cost(outline)))
            .min()?;
        let gain = share(count, self.total_pairs) * f64::from(STROKE_WEIGHT);
        let word_count = self.setup.count.get(word).copied().unwrap_or(0);
        let loss = share(word_count, self.total_words) * f64::from(next_best.saturating_sub(candidate_cost(&victim.outline)));
        (gain > loss).then_some(Takeover { index, rehome })
    }

    fn take(&mut self, takeover: Takeover) {
        let victim = &self.entries[takeover.index];
        if is_primary(victim.origin) {
            self.gone.insert(victim.word.clone());
        }
        let lost_primary = is_primary(victim.origin);
        let word = victim.word.clone();
        if let Some((outline, origin)) = takeover.rehome {
            self.taken.insert(render_outline(&outline), word.clone());
            self.rehomed.push(Entry { word, outline, origin, lost_to: None });
        } else if lost_primary {
            // The word falls back to an outline it already has. When that is one of its own
            // syllable-path backups (the plain reading it would have been rehomed to, held already
            // as a way in), that entry becomes its primary: the word keeps a primary outline.
            let best = self
                .entries
                .iter()
                .enumerate()
                .filter(|(j, e)| *j != takeover.index && !self.removed.contains(j) && e.word == word && e.origin != Origin::Alternate)
                .min_by_key(|(_, e)| (e.outline.len(), keys(&e.outline), render_outline(&e.outline)))
                .map(|(j, e)| (j, e.origin));
            if let Some((j, Origin::Backup)) = best {
                self.entries[j].origin = Origin::Rules;
            }
        }
        self.removed.insert(takeover.index);
    }

    fn commit(&mut self, w1: &str, w2: &str, text: String, outline: Vec<Stroke>) {
        let phrase = format!("{w1} {w2}");
        self.used.insert(entry_word(w1).to_owned());
        self.used.insert(entry_word(w2).to_owned());
        self.taken.insert(text.clone(), phrase.clone());
        self.owner.insert(text, usize::MAX);
        self.entries.push(Entry { word: phrase, outline, origin: Origin::Phrase, lost_to: None });
    }
}

/// Commit phrase entries into `entries` and `taken`. `pools` holds each word's candidate
/// outlines and `natural` its plain-rules reading, used to give a word that loses its stroke
/// another one; `stems` are the words that have family members (their primary is not taken, since
/// their members start from its keys) and `literal` says which words have a hand-written outline
/// in `rules/briefs.toml` (never taken).
pub fn place(
    setup: &Setup,
    entries: &mut Vec<Entry>,
    taken: &mut HashMap<String, String>,
    pools: &HashMap<&str, &[Candidate]>,
    stems: &HashSet<&str>,
    natural: &dyn Fn(&str) -> Option<Vec<Stroke>>,
    literal: &dyn Fn(&str) -> bool,
) -> Placed {
    let candidates = {
        let single = primary_single_strokes(entries);
        let any_len = primary_outlines(entries);
        enumerate_phrase_candidates(setup, &single, &any_len, PHRASE_TOP_N)
    };
    let is_winner = arbitrate_phrase_round(&candidates);
    let protected: HashSet<&str> = {
        let mut ranked: Vec<(&String, &u64)> = setup.count.iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        ranked.into_iter().take(PROTECTED_WORDS).map(|(w, _)| w.as_str()).collect()
    };
    let owner = entries.iter().enumerate().map(|(i, e)| (render_outline(&e.outline), i)).collect();
    let mut pass = Pass {
        entries,
        taken,
        owner,
        removed: HashSet::new(),
        rehomed: Vec::new(),
        used: HashSet::new(),
        gone: HashSet::new(),
        protected,
        stems,
        pools,
        natural,
        literal,
        setup,
        total_pairs: setup.bigrams.values().sum(),
        total_words: setup.count.values().sum(),
    };

    let mut placed = Placed::default();
    for (i, ((w1, w2), outline, count)) in candidates.into_iter().enumerate() {
        if !is_winner[i] || pass.gone.contains(entry_word(&w1)) || pass.gone.contains(entry_word(&w2)) {
            placed.skipped += 1;
            continue;
        }
        let text = render_outline(&outline);
        if pass.taken.contains_key(&text) {
            // `candidates` is in count order, so `i` is the pair's rank among eligible pairs.
            let Some(takeover) = (i < TAKEOVER_TOP_N).then(|| pass.takeover(&text, count)).flatten() else {
                placed.skipped += 1;
                continue;
            };
            pass.take(takeover);
            placed.displaced += 1;
        }
        pass.commit(&w1, &w2, text, outline);
        placed.added += 1;
    }

    let Pass { entries, removed, rehomed, .. } = pass;
    if !removed.is_empty() {
        let mut position = 0;
        entries.retain(|_| {
            position += 1;
            !removed.contains(&(position - 1))
        });
    }
    entries.extend(rehomed);
    placed
}

/// Each word's own settled *primary* outline, single-stroke only - exactly the entry `generate`'s
/// own peeling/solver/literal-override step assigned, never a bonus [`Origin::Short`],
/// [`Origin::Alternate`], or [`Origin::Backup`] entry. Feeds [`phrase_candidate`]: a backup entry
/// is itself contingent on composition-pass ordering and can change if the backup pass's own
/// internals change, so building a phrase on top of one would stack one best-effort pass on
/// another; a primary assignment is `generate`'s own invariant instead - one single-stroke outline
/// per word, stable and already heavily tested - and that is what phrase-briefing is built on. See
/// `docs/phrases.md`'s "why primary-only" section for the full reasoning.
pub(crate) fn primary_single_strokes(entries: &[Entry]) -> HashMap<&str, Stroke> {
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
pub(crate) fn primary_outlines(entries: &[Entry]) -> HashMap<&str, &[Stroke]> {
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
fn phrase_candidate(primary: &HashMap<&str, Stroke>, w1: &str, w2: &str) -> Option<Stroke> {
    let &a = primary.get(w1)?;
    let &b = primary.get(w2)?;
    (a.0 & b.0 == 0).then_some(Stroke(a.0 | b.0))
}

/// OR's `filler` (a single stroke) into `host`'s own first stroke, when the two are disjoint,
/// carrying every one of `host`'s later strokes through completely unchanged - the same
/// "disjoint-OR the first stroke, leave the rest alone" trick `compose_backup`'s own direction (b)
/// already uses on `word_outline[stem]`. `None` when `host` is empty (never true for a real
/// primary) or the two strokes overlap.
fn merge_into_host_first_stroke(filler: Stroke, host: &[Stroke]) -> Option<Vec<Stroke>> {
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
fn phrase_candidate_outline(
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

pub(crate) fn enumerate_phrase_candidates(
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
pub(crate) fn arbitrate_phrase_round(candidates: &[((String, String), Vec<Stroke>, u64)]) -> Vec<bool> {
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


#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictionary_fixture;
    use crate::generate::reserved_outlines;
    use crate::stroke::{parse_outline, render_outline};

    fn outlines(word: &str) -> Vec<(String, Origin)> {
        dictionary_fixture::generated()
            .entries
            .iter()
            .filter(|e| e.word == word)
            .map(|e| (render_outline(&e.outline), e.origin))
            .collect()
    }

    #[test]
    fn a_phrase_takes_a_backup_stroke_and_the_word_keeps_its_own_outline() {
        // `sit`'s backup `ST` (`s` + `it`) is worth less than "it is" on the same stroke
        assert!(outlines("it is").contains(&("ST".to_owned(), Origin::Phrase)));
        let sit = outlines("sit");
        assert!(!sit.iter().any(|(o, _)| o == "ST"), "{sit:?}");
        assert!(sit.iter().any(|(_, origin)| matches!(origin, Origin::Rules | Origin::Brief | Origin::Spelling)), "{sit:?}");
    }

    #[test]
    fn a_rare_word_gives_up_its_stroke_and_is_written_another_way() {
        assert!(outlines("in the").contains(&("TPH-T".to_owned(), Origin::Phrase)));
        let antonio = outlines("antonio");
        assert!(!antonio.is_empty() && antonio.iter().all(|(o, _)| o != "TPH-T"), "{antonio:?}");
    }

    #[test]
    fn a_common_word_never_loses_its_primary_to_a_phrase() {
        // "with the" would be `W-T`, which is `want`'s own primary
        assert!(outlines("want").iter().any(|(o, origin)| o == "W-T" && *origin != Origin::Phrase));
        assert!(!outlines("with the").iter().any(|(o, _)| o == "W-T"));
    }

    #[test]
    fn no_outline_belongs_to_two_entries() {
        let mut seen = std::collections::HashMap::new();
        for entry in &dictionary_fixture::generated().entries {
            let text = render_outline(&entry.outline);
            if let Some(other) = seen.insert(text.clone(), &entry.word) {
                panic!("{text} is both {other:?} and {:?}", entry.word);
            }
        }
    }

    #[test]
    fn displacing_words_leaves_none_without_an_outline() {
        let generated = dictionary_fixture::generated();
        assert!(generated.entries.iter().filter(|e| e.origin == Origin::Phrase).count() > 5_000);
        let words: std::collections::HashSet<&str> =
            generated.entries.iter().filter(|e| e.origin != Origin::Phrase).map(|e| e.word.as_str()).collect();
        let setup = dictionary_fixture::setup();
        let missing: Vec<&String> = setup
            .count
            .iter()
            .filter(|(w, c)| **c > 50_000 && setup.pron.contains_key(w.as_str()) && w.len() > 1 && w.bytes().all(|b| b.is_ascii_lowercase()) && !words.contains(w.as_str()))
            .map(|(w, _)| w)
            .take(10)
            .collect();
        assert!(missing.is_empty(), "common words with no outline: {missing:?}");
    }

    #[test]
    fn phrase_candidate_merges_disjoint_primaries_and_rejects_everything_else() {
        let mut primary: HashMap<&str, Stroke> = HashMap::new();
        primary.insert("of", parse_outline("-F").unwrap()[0]);
        primary.insert("the", parse_outline("-T").unwrap()[0]);
        primary.insert("on", parse_outline("O").unwrap()[0]);
        // Shares no key with "of"'s "-F" or "the"'s "-T" in steno order, but does share the star
        // with a hand-built overlapping candidate below.
        primary.insert("starred", parse_outline("O*").unwrap()[0]);

        // A genuinely disjoint pair merges to the bitwise OR of both strokes.
        let merged = phrase_candidate(&primary, "of", "the").expect("of/the keys are disjoint");
        assert_eq!(merged, parse_outline("-FT").unwrap()[0]);

        // An overlapping pair - including an overlap on the star bit specifically, not just an
        // ordinary consonant - is never eligible, however common either word is on its own.
        assert_eq!(phrase_candidate(&primary, "on", "starred"), None, "O and *O share the O key");
        let mut star_primary = primary.clone();
        star_primary.insert("star1", parse_outline("*F").unwrap()[0]);
        star_primary.insert("star2", parse_outline("*T").unwrap()[0]);
        assert_eq!(
            phrase_candidate(&star_primary, "star1", "star2"),
            None,
            "both strokes carry the star bit - that overlap must be caught too, not just an \
             ordinary consonant key"
        );

        // Either word missing a single-stroke primary at all (never in the map) is never eligible.
        assert_eq!(phrase_candidate(&primary, "of", "nonexistent"), None);
        assert_eq!(phrase_candidate(&primary, "nonexistent", "the"), None);
    }

    #[test]
    fn a_few_real_high_value_bigrams_checked_by_hand_against_the_real_dictionary() {
        // Plan verification step 2: a hand-worked check against the real, settled dictionary for a
        // few of the highest-raw-count function-word bigrams - "of the", "in the", and "I am" were
        // named by the plan as the first candidates to check, with no assumption going in about
        // whether their key-bitmasks would actually turn out disjoint in practice. Reporting the
        // real outcome for each, honestly, including the two that do NOT end up as a committed
        // phrase entry and exactly why not:
        let setup = crate::dictionary_fixture::setup();
        let generated = crate::dictionary_fixture::generated();
        let primary = primary_single_strokes(&generated.entries);
        let lyk = crate::dictionary_fixture::cheapest();

        // "of the": both words' own real primaries are genuinely disjoint ("of" -> "-F", "the" ->
        // "-T"), the merged stroke "-FT" was genuinely free, and it is the highest-raw-count bigram
        // in the whole corpus (2,766,332,391) - so this is both the clearest and the single
        // highest-value real hit the whole mechanism produces.
        assert_eq!(primary.get("of").copied(), Some(parse_outline("-F").unwrap()[0]));
        assert_eq!(primary.get("the").copied(), Some(parse_outline("-T").unwrap()[0]));
        assert_eq!(
            phrase_candidate(&primary, "of", "the"),
            Some(parse_outline("-FT").unwrap()[0]),
            "\"of\" and \"the\"'s own real primaries are disjoint"
        );
        assert_eq!(lyk.get("of the").map(|o| render_outline(o)), Some("-FT".to_owned()));

        // "in the": "in"'s own real primary and "the"'s are disjoint, and the merged stroke,
        // "TPH-T", was not free: `antonio` (a proper noun whose plain-rules outline happens to
        // land there) held it. The pair is far more common than the word, so the phrase takes the
        // stroke and `antonio` is written another way (see `crate::phrases`).
        let in_the = phrase_candidate(&primary, "in", "the");
        assert_eq!(
            in_the,
            Some(parse_outline("TPH-T").unwrap()[0]),
            "\"in\" and \"the\"'s own real primaries are disjoint"
        );
        assert_eq!(
            lyk.get("in the").map(|o| render_outline(o)),
            in_the.map(|s| render_outline(&[s])),
            "\"in the\" is far more common than \"antonio\", so it takes the stroke"
        );
        assert_ne!(lyk.get("antonio").map(|o| render_outline(o)), Some("TPH-T".to_owned()));

        // "I am": the bigram corpus spells the pronoun lowercase, so a phrase looks it up through
        // `entry_word` - the pronoun's own entry, "I" on `EU`, not a bare letter. `EU` | `-PL`
        // ("am") is disjoint, but the merged `EUPL` is already a word ("im"), so nothing is
        // committed; what matters is that the lookup reaches the pronoun at all.
        assert!(setup.bigrams.contains_key(&("i".to_owned(), "am".to_owned())));
        assert!(!primary.contains_key("i"), "the bare letter \"i\" is not a dictionary word");
        assert_eq!(primary.get(entry_word("i")).copied(), Some(parse_outline("EU").unwrap()[0]));
        assert!(
            !generated.entries.iter().any(|e| e.origin == Origin::Phrase && e.word == "i am"),
            "\"i am\"'s merged stroke EUPL is already taken, so it must not have been committed"
        );
    }

    #[test]
    fn two_real_multi_stroke_host_bigrams_checked_by_hand_against_the_real_dictionary() {
        // Plan verification step 4's real-dictionary check for the multi-stroke-host update: two
        // real bigrams, one for each direction `phrase_candidate_outline` now supports, hand-worked
        // against the real, settled dictionary (`cargo run --release -- lookup "<phrase>"` confirms
        // each word's own real primary independently of this test).
        let setup = crate::dictionary_fixture::setup();
        let generated = crate::dictionary_fixture::generated();
        let single = primary_single_strokes(&generated.entries);
        let any_len = primary_outlines(&generated.entries);

        // Direction (B): "the" (single-stroke primary "-T") + "accuracy" (host, a real two-stroke
        // primary "ABG/+SEU") - a real bigram in `setup.bigrams`. The filler "-T" merges into
        // "accuracy"'s own first stroke "ABG" (disjoint: "-T" shares no key with "ABG"), and
        // "accuracy"'s own second stroke "+SEU" - join bit included - carries through unchanged.
        // (This used "a currency" until "currency" became the one-stroke `KURPBS`, and "a
        // working" before that, until "working" became `WOG`.)
        assert!(setup.bigrams.contains_key(&("the".to_owned(), "accuracy".to_owned())));
        assert_eq!(single.get("the").copied(), Some(parse_outline("-T").unwrap()[0]));
        assert_eq!(any_len.get("accuracy").copied(), Some(&parse_outline("ABG/+SEU").unwrap()[..]));
        let the_accuracy = phrase_candidate_outline(&single, &any_len, "the", "accuracy");
        assert_eq!(
            the_accuracy,
            Some(parse_outline("ABGT/+SEU").unwrap()),
            "\"the\"'s whole primary should merge into \"accuracy\"'s own first stroke only"
        );
        assert_eq!(
            generated
                .entries
                .iter()
                .find(|e| e.origin == Origin::Phrase && e.word == "the accuracy")
                .map(|e| e.outline.clone()),
            the_accuracy,
            "\"the accuracy\" should have actually committed with this exact two-stroke outline"
        );

        // Direction (C), the symmetric case: "accompanied" (host, a real two-stroke primary
        // "A/+K-PD") + "by" (single-stroke primary "PWEU") - also a real bigram. "by"'s whole
        // primary merges into "accompanied"'s own first stroke "A" (disjoint), and
        // "accompanied"'s own second stroke "+K-PD" carries through unchanged.
        assert!(setup.bigrams.contains_key(&("accompanied".to_owned(), "by".to_owned())));
        assert_eq!(any_len.get("accompanied").copied(), Some(&parse_outline("A/+K-PD").unwrap()[..]));
        assert_eq!(single.get("by").copied(), Some(parse_outline("PWEU").unwrap()[0]));
        let accompanied_by = phrase_candidate_outline(&single, &any_len, "accompanied", "by");
        assert_eq!(
            accompanied_by,
            Some(parse_outline("PWAEU/+K-PD").unwrap()),
            "\"by\"'s whole primary should merge into \"accompanied\"'s own first stroke only"
        );
        assert_eq!(
            generated
                .entries
                .iter()
                .find(|e| e.origin == Origin::Phrase && e.word == "accompanied by")
                .map(|e| e.outline.clone()),
            accompanied_by,
            "\"accompanied by\" should have actually committed with this exact two-stroke outline"
        );
    }

    #[test]
    fn arbitrate_phrase_round_picks_the_commoner_bigram_not_whoever_is_tried_first() {
        // Mirrors `backup_round_arbitration_picks_the_commoner_word_not_whoever_is_tried_first`
        // exactly, for phrase candidates: two different eligible bigrams that happen to OR to the
        // identical merged stroke are arbitrated by the bigram's own raw count, not insertion
        // order.
        let same_stroke = parse_outline("TK").unwrap();
        let forward = vec![
            (("rare".to_owned(), "pair".to_owned()), same_stroke.clone(), 5u64),
            (("common".to_owned(), "pair".to_owned()), same_stroke, 1_000u64),
        ];
        assert_eq!(
            arbitrate_phrase_round(&forward),
            vec![false, true],
            "the commoner bigram should win even when the rarer one is tried first"
        );

        let backward = vec![forward[1].clone(), forward[0].clone()];
        assert_eq!(
            arbitrate_phrase_round(&backward),
            vec![true, false],
            "the winner must not depend on which candidate was tried first"
        );
    }

    #[test]
    #[ignore = "diagnostic: checks a few specific high-value bigrams by hand, run with --ignored \
                --nocapture"]
    fn dump_specific_bigram_eligibility() {
        let setup = crate::dictionary_fixture::setup();
        let generated = crate::dictionary_fixture::generated();
        let primary = primary_single_strokes(&generated.entries);
        // Excludes the dictionary's own already-committed `Origin::Phrase` entries - see
        // `dump_phrase_eligible_set_sizes`'s identical note on why, otherwise a bigram the current
        // `PHRASE_TOP_N` already settled would show as "held by" itself.
        let index: HashMap<String, &str> = generated
            .entries
            .iter()
            .filter(|e| e.origin != Origin::Phrase)
            .map(|e| (render_outline(&e.outline), e.word.as_str()))
            .collect();
        for (w1, w2) in [
            ("of", "the"),
            ("in", "the"),
            ("to", "the"),
            ("i", "am"),
            ("i", "have"),
            ("it", "is"),
            ("i", "will"),
            ("i", "was"),
            ("for", "the"),
            ("on", "the"),
            ("and", "the"),
            ("is", "a"),
        ] {
            let count = setup.bigrams.get(&(w1.to_owned(), w2.to_owned())).copied();
            let p1 = primary.get(w1);
            let p2 = primary.get(w2);
            match phrase_candidate(&primary, w1, w2) {
                Some(stroke) => {
                    let text = render_outline(&[stroke]);
                    let holder = index.get(&text);
                    println!(
                        "{w1:?}+{w2:?} count={count:?} {w1}={p1:?} {w2}={p2:?} -> {text} \
                         (held by: {holder:?})"
                    );
                }
                None => println!("{w1:?}+{w2:?} count={count:?} {w1}={p1:?} {w2}={p2:?} -> NOT ELIGIBLE"),
            }
        }
    }

    #[test]
    #[ignore = "diagnostic: measures the real phrase-briefing eligible-set size at a few N values, \
                run with --ignored --nocapture"]
    fn dump_phrase_eligible_set_sizes() {
        // Plan open question #1: the real eligible-set size (both words single-stroke-primary,
        // keys disjoint, merged stroke still free) was unmeasured before this pass existed. This
        // scans the real, built dictionary's real bigram data and reports, for the raw-count-sorted
        // bigram list truncated to a few different N, how many of the first N are eligible at all,
        // and how many of those are still free once `taken` and within-scan collisions are
        // accounted for (first-come within this already-sorted-by-count scan, same outcome
        // `arbitrate_phrase_round` would reach for the winner).
        let setup = crate::dictionary_fixture::setup();
        let generated = crate::dictionary_fixture::generated();
        let primary = primary_single_strokes(&generated.entries);

        // Exclude the dictionary's own already-committed `Origin::Phrase` entries from the
        // baseline: `dictionary_fixture` builds the real, full dictionary, phrase pass included,
        // so without this exclusion a bigram this project's own current `PHRASE_TOP_N` already
        // settled would show up as "taken" by *itself*, undercounting what's genuinely free.
        let mut taken: HashSet<String> = reserved_outlines().unwrap().into_keys().collect();
        taken.extend(
            generated
                .entries
                .iter()
                .filter(|e| e.origin != Origin::Phrase)
                .map(|e| render_outline(&e.outline)),
        );

        let mut by_count: Vec<(&(String, String), &u64)> = setup.bigrams.iter().collect();
        by_count.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        println!("total vocabulary-filtered bigrams on record: {}", by_count.len());

        let checkpoints = [100usize, 300, 1_000, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000, 8_000, 9_000, 10_000, 15_000, 20_000, 30_000, 50_000, 100_000];
        let mut eligible = 0usize;
        let mut free = 0usize;
        let mut claimed_this_scan: HashSet<Stroke> = HashSet::new();
        let mut next_checkpoint = 0usize;
        for (i, &((w1, w2), &count)) in by_count.iter().enumerate() {
            if let Some(stroke) = phrase_candidate(&primary, w1, w2) {
                eligible += 1;
                let text = render_outline(&[stroke]);
                if !taken.contains(&text) && claimed_this_scan.insert(stroke) {
                    free += 1;
                    if i < 20 && next_checkpoint == 0 {
                        println!("  e.g. {w1:?} + {w2:?} (count {count}) -> {text}");
                    }
                }
            }
            while next_checkpoint < checkpoints.len() && i + 1 == checkpoints[next_checkpoint] {
                println!(
                    "top {} raw bigrams (count at cutoff: {}): {} eligible (disjoint primaries), \
                     {} still free (unclaimed, deduplicated)",
                    checkpoints[next_checkpoint], count, eligible, free
                );
                next_checkpoint += 1;
            }
        }
    }

    #[test]
    #[ignore = "diagnostic: measures real counts for the phrase-briefing-v2 plan's gate (Backup as \
                a phrase component), run with --ignored --nocapture"]
    fn dump_backup_phrase_component_gate() {
        const PHRASE_TOP_N: usize = 10_000;
        let setup = crate::dictionary_fixture::setup();
        let generated = crate::dictionary_fixture::generated();

        // Cheapest single-stroke entry per origin bucket, by word.
        let mut primary_single: HashMap<&str, Stroke> = HashMap::new();
        let mut backup_single: HashMap<&str, Stroke> = HashMap::new();
        let mut has_multistroke_primary: HashSet<&str> = HashSet::new();
        for e in &generated.entries {
            match e.origin {
                Origin::Brief | Origin::Rules | Origin::Spelling | Origin::Ladder(_) => match e.outline[..] {
                    [stroke] => {
                        primary_single
                            .entry(e.word.as_str())
                            .and_modify(|cur| if stroke.0.count_ones() < cur.0.count_ones() { *cur = stroke })
                            .or_insert(stroke);
                    }
                    _ => {
                        has_multistroke_primary.insert(e.word.as_str());
                    }
                },
                Origin::Backup => {
                    if let [stroke] = e.outline[..] {
                        backup_single
                            .entry(e.word.as_str())
                            .and_modify(|cur| if stroke.0.count_ones() < cur.0.count_ones() { *cur = stroke })
                            .or_insert(stroke);
                    }
                }
                _ => {}
            }
        }

        // (1) words with a multi-stroke primary but a single-stroke Backup entry - plan predicts ~0.
        let rescued_multistroke_primary = has_multistroke_primary
            .iter()
            .filter(|w| backup_single.contains_key(*w))
            .count();
        println!(
            "words with multi-stroke primary AND single-stroke Backup (plan predicts ~0): \
             {rescued_multistroke_primary}"
        );

        // (2) words with a single-stroke primary AND a cheaper single-stroke Backup (the sit/ST case).
        let mut cheaper_backup_examples: Vec<(&str, Stroke, Stroke)> = Vec::new();
        for (word, &p) in &primary_single {
            if let Some(&b) = backup_single.get(word)
                && b.0.count_ones() < p.0.count_ones()
            {
                cheaper_backup_examples.push((word, p, b));
            }
        }
        let cheaper_backup_count = cheaper_backup_examples.len();
        println!("words with single-stroke primary AND a cheaper single-stroke Backup: {cheaper_backup_count}");
        cheaper_backup_examples.sort_by_key(|(w, _, _)| *w);
        for (word, p, b) in cheaper_backup_examples.iter().take(15) {
            println!(
                "  e.g. {word:?}: primary={} ({} keys) backup={} ({} keys)",
                render_outline(&[*p]),
                p.0.count_ones(),
                render_outline(&[*b]),
                b.0.count_ones()
            );
        }

        // (3) delta in eligible/free phrase-candidate counts at PHRASE_TOP_N between today's
        // `primary_single_strokes` and a component map that also considers each word's cheapest
        // single-stroke Backup entry when it's cheaper than the primary.
        let primary = primary_single_strokes(&generated.entries);
        let mut component = primary.clone();
        for (word, &b) in &backup_single {
            component
                .entry(word)
                .and_modify(|cur| if b.0.count_ones() < cur.0.count_ones() { *cur = b })
                .or_insert(b);
        }

        let mut taken: HashSet<String> = reserved_outlines().unwrap().into_keys().collect();
        taken.extend(
            generated
                .entries
                .iter()
                .filter(|e| e.origin != Origin::Phrase)
                .map(|e| render_outline(&e.outline)),
        );

        let mut by_count: Vec<(&(String, String), &u64)> = setup.bigrams.iter().collect();
        by_count.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        let by_count: Vec<&(String, String)> = by_count.into_iter().take(PHRASE_TOP_N).map(|(k, _)| k).collect();

        let (eligible_before, free_before) = count_eligible_free(&by_count, &primary, &taken);
        let (eligible_after, free_after) = count_eligible_free(&by_count, &component, &taken);
        println!(
            "PHRASE_TOP_N={PHRASE_TOP_N}: primary_single_strokes -> {eligible_before} eligible, \
             {free_before} free"
        );
        println!(
            "PHRASE_TOP_N={PHRASE_TOP_N}: + cheapest single-stroke Backup -> {eligible_after} \
             eligible, {free_after} free"
        );
        let eligible_delta = eligible_after.abs_diff(eligible_before);
        let free_delta = free_after.abs_diff(free_before);
        println!("delta: {eligible_delta} more eligible, {free_delta} more free");
    }

    /// How many of `by_count`'s bigrams are eligible (disjoint primaries in `map`) and how many of
    /// those land on a stroke text not already in `taken` and not claimed earlier in this same scan.
    /// A helper for [`dump_backup_phrase_component_gate`] so it can run the identical count twice,
    /// once per candidate component map, without duplicating the scan itself.
    fn count_eligible_free(by_count: &[&(String, String)], map: &HashMap<&str, Stroke>, taken: &HashSet<String>) -> (usize, usize) {
        let mut eligible = 0usize;
        let mut free = 0usize;
        let mut claimed_this_scan: HashSet<Stroke> = HashSet::new();
        for (w1, w2) in by_count {
            if let Some(stroke) = phrase_candidate(map, w1, w2) {
                eligible += 1;
                let text = render_outline(&[stroke]);
                if !taken.contains(&text) && claimed_this_scan.insert(stroke) {
                    free += 1;
                }
            }
        }
        (eligible, free)
    }

    #[test]
    fn merge_into_host_first_stroke_preserves_every_later_stroke_untouched() {
        // Hand-worked synthetic case, step 2 of the multi-stroke-host plan: a small multi-stroke
        // word's own primary (`"ample"`, made up for this test as a two-stroke outline `APL/+PL`,
        // chosen only so its first stroke `APL` is disjoint from the filler below - no real
        // dictionary word needs to actually exist with this outline) and a real single-stroke
        // filler word's primary (`"of"`'s own real `-F`). Confirms the merge produces the right
        // combined outline, with the host's own second stroke - join bit included - carried
        // through completely unchanged, exactly the invariant `compose_backup`'s direction (b)
        // already guarantees for a leading consonant folded onto `word_outline[stem][0]`.
        let filler = parse_outline("-F").unwrap()[0];
        let host = parse_outline("APL/+PL").unwrap();
        assert_eq!(host.len(), 2, "test setup: host must be genuinely two strokes");
        assert_eq!(filler.0 & host[0].0, 0, "test setup: filler and the host's first stroke must be disjoint");

        let merged = merge_into_host_first_stroke(filler, &host).expect("disjoint filler and host first stroke");
        assert_eq!(merged.len(), 2, "the host's second stroke must be carried through, not dropped");
        assert_eq!(merged[0], Stroke(filler.0 | host[0].0), "the first stroke is the bitwise OR of filler and host[0]");
        assert_eq!(merged[1], host[1], "the host's later stroke(s) - join bit included - must be byte-for-byte unchanged");

        // An overlapping filler/host-first-stroke pair is rejected, exactly as `phrase_candidate`
        // already rejects an overlapping single+single pair.
        let overlapping_filler = parse_outline("A").unwrap()[0]; // shares the "A" vowel with host[0] ("APL")
        assert_eq!(merge_into_host_first_stroke(overlapping_filler, &host), None);

        // An empty host (never real, but checked for completeness) is also rejected rather than
        // panicking.
        assert_eq!(merge_into_host_first_stroke(filler, &[]), None);
    }

    #[test]
    fn phrase_candidate_outline_handles_both_host_directions_and_still_excludes_both_multi() {
        // `phrase_candidate_outline`'s own three cases, hand-built: (1) both single-stroke reduces
        // to exactly `phrase_candidate`'s old behavior; (2) w1 single + w2 a multi-stroke host; (3)
        // the symmetric w2 single + w1 a multi-stroke host; (4) both multi-stroke stays out of
        // scope entirely, `None`, regardless of disjointness.
        let mut single: HashMap<&str, Stroke> = HashMap::new();
        single.insert("of", parse_outline("-F").unwrap()[0]);
        single.insert("the", parse_outline("-T").unwrap()[0]);

        let ample_outline = parse_outline("APL/+PL").unwrap();
        let of_outline = parse_outline("-F").unwrap();
        let the_outline = parse_outline("-T").unwrap();
        let mut any_len: HashMap<&str, &[Stroke]> = HashMap::new();
        any_len.insert("of", &of_outline[..]);
        any_len.insert("the", &the_outline[..]);
        any_len.insert("ample", &ample_outline[..]);

        // (1) both single-stroke: identical to `phrase_candidate`'s own merged stroke, wrapped in a
        // one-element outline.
        assert_eq!(
            phrase_candidate_outline(&single, &any_len, "of", "the"),
            Some(vec![parse_outline("-FT").unwrap()[0]])
        );

        // (2) "of" (single) + "ample" (multi-stroke host): merges into "ample"'s own first stroke,
        // keeps "ample"'s own second stroke untouched.
        assert_eq!(
            phrase_candidate_outline(&single, &any_len, "of", "ample"),
            Some(vec![Stroke(single["of"].0 | ample_outline[0].0), ample_outline[1]])
        );

        // (3) the symmetric direction: "ample" (multi-stroke host) + "of" (single).
        assert_eq!(
            phrase_candidate_outline(&single, &any_len, "ample", "of"),
            Some(vec![Stroke(ample_outline[0].0 | single["of"].0), ample_outline[1]])
        );

        // (4) a pair where *neither* side has a single-stroke primary stays out of scope.
        let mut any_len_both_multi = any_len.clone();
        any_len_both_multi.insert("another", &ample_outline[..]);
        let no_single: HashMap<&str, Stroke> = HashMap::new();
        assert_eq!(phrase_candidate_outline(&no_single, &any_len_both_multi, "ample", "another"), None);
    }

    #[test]
    #[ignore = "diagnostic: measures the real yield of allowing a multi-stroke host word on either \
                side of a phrase bigram, run with --ignored --nocapture"]
    fn dump_multistroke_host_phrase_yield() {
        const PHRASE_TOP_N: usize = 10_000;
        let setup = crate::dictionary_fixture::setup();
        let generated = crate::dictionary_fixture::generated();

        let single = primary_single_strokes(&generated.entries);
        let any_len = primary_outlines(&generated.entries);

        // Baseline `taken`: every non-`Phrase` entry's own rendered outline, so a bigram this
        // project's own current phrase pass already settled doesn't show up as "taken by itself".
        let mut taken: HashSet<String> = reserved_outlines().unwrap().into_keys().collect();
        taken.extend(
            generated
                .entries
                .iter()
                .filter(|e| e.origin != Origin::Phrase)
                .map(|e| render_outline(&e.outline)),
        );

        let mut by_count: Vec<(&(String, String), &u64)> = setup.bigrams.iter().collect();
        by_count.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        let by_count: Vec<&(String, String)> = by_count.into_iter().take(PHRASE_TOP_N).map(|(k, _)| k).collect();

        // (A) today's both-single-stroke-only rule - the pre-existing baseline.
        let mut eligible_a = 0usize;
        let mut committed_a: HashSet<String> = HashSet::new();
        // (B) w1 single-stroke + w2 a *strictly* multi-stroke host (the new direction named first
        // in the plan).
        let mut eligible_b = 0usize;
        let mut committed_b: HashSet<String> = HashSet::new();
        // (C) the symmetric direction: w2 single-stroke + w1 a strictly multi-stroke host.
        let mut eligible_c = 0usize;
        let mut committed_c: HashSet<String> = HashSet::new();

        for (w1, w2) in &by_count {
            if let (Some(&a), Some(&b)) = (single.get(w1.as_str()), single.get(w2.as_str()))
                && a.0 & b.0 == 0
            {
                eligible_a += 1;
                let text = render_outline(&[Stroke(a.0 | b.0)]);
                if !taken.contains(&text) {
                    committed_a.insert(text);
                }
            }
            if let (Some(&filler), Some(&host)) = (single.get(w1.as_str()), any_len.get(w2.as_str()))
                && host.len() > 1
                && let Some(outline) = merge_into_host_first_stroke(filler, host)
            {
                eligible_b += 1;
                let text = render_outline(&outline);
                if !taken.contains(&text) {
                    committed_b.insert(text);
                }
            }
            if let (Some(&host), Some(&filler)) = (any_len.get(w1.as_str()), single.get(w2.as_str()))
                && host.len() > 1
                && let Some(outline) = merge_into_host_first_stroke(filler, host)
            {
                eligible_c += 1;
                let text = render_outline(&outline);
                if !taken.contains(&text) {
                    committed_c.insert(text);
                }
            }
        }

        println!("PHRASE_TOP_N={PHRASE_TOP_N}");
        println!(
            "  (A) both single-stroke only (today's rule): {eligible_a} eligible, {} distinct free texts",
            committed_a.len()
        );
        println!(
            "  (B) w1 single + w2 strictly-multi host:      {eligible_b} eligible, {} distinct free texts",
            committed_b.len()
        );
        println!(
            "  (C) w2 single + w1 strictly-multi host:      {eligible_c} eligible, {} distinct free texts",
            committed_c.len()
        );
        let b_only_new: HashSet<&String> = committed_b.difference(&committed_a).collect();
        let c_only_new: HashSet<&String> = committed_c.difference(&committed_a).collect();
        let union_new: HashSet<&String> = b_only_new.union(&c_only_new).copied().collect();
        println!(
            "  net new multi-stroke-host texts (B ∪ C, deduplicated, not already free under A): {}",
            union_new.len()
        );
    }
}

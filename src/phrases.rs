//! Phrase entries: two common words in one stroke, and which words give up a stroke for one.
//!
//! The merge itself (two primaries OR'd into a new outline) and the list of candidate bigrams live
//! in [`crate::generate`]; this module decides which candidates are committed. A candidate whose
//! merged stroke is free is committed as is. A candidate whose stroke a word already holds
//! *competes* for it: it takes the stroke when the pair is said often enough to be worth more than
//! the word loses by moving to its next-best outline. See `docs/phrases.md`, "Phrases compete for
//! strokes".

use crate::analysis::Setup;
use crate::generate::{
    arbitrate_phrase_round, candidate_cost, entry_word, enumerate_phrase_candidates, keys, primary_outlines,
    primary_single_strokes, typable, Candidate, Entry, Origin, STROKE_WEIGHT,
};
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

#[cfg(test)]
mod tests {
    use crate::dictionary_fixture;
    use crate::generate::Origin;
    use crate::stroke::render_outline;

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
}

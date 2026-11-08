//! What the lookup commands answer, as data: the rows a query produces.
//!
//! `lookup` (alias `keys`) and `resolve` (alias `stroke`) both end up here, whether they are given
//! a query on the command line or one is typed live ([`crate::interactive`]), so there is one
//! answer, drawn the same way ([`crate::view`]) every time. The structure is keymux's own
//! `steno/answer.rs`, and the one thing that differs is that a row has an outline per theory
//! (lyk, Plover, Lapwing) where keymux resolves its one engine setup.

use crate::analysis::Setup;
use crate::built::Backups;
use crate::generate::{self, Entry, Origin};
use crate::reverse::{self, Query, WordLookup};
use crate::stroke::{self, Stroke, render_outline};
use std::collections::HashMap;

/// The three theories, in the order a row holds them (and the order of the table's columns).
pub const THEORIES: [&str; 3] = ["lyk", "plover", "lapwing"];

/// Words that merely start with the query are only offered from this length on.
const MIN_PREFIX: usize = 2;

/// How a theory's outline for a word came to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum How {
    /// A dictionary entry
    Entry,
    /// A word with no entry, written from entries ("everybody", "'s")
    Composed(Vec<String>),
    /// lyk only: a word the built dictionary lacks, shown as the rules' own phonetic reading
    Reading,
    /// lyk only: a word with no pronunciation, spelled one letter a stroke (spelling mode)
    Spelled,
}

/// One theory's outline in a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub strokes: Vec<Stroke>,
    pub how: How,
}

/// What kind of line a row is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// A word's best outlines, or (empty `word`) another outline that types it properly
    Entry,
    /// A word that starts with the query, offered after the exact answers
    Completion,
    /// What a stroke types; the row's word is the text, or a note that it types nothing
    Forward,
    /// An entry one key or two from the stroke asked for
    Nearby,
    /// Nothing can type the word, in any theory
    Missing,
}

/// One line of an answer: a word and one outline per theory that types it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The word. Empty on the second and later outlines of a word.
    pub word: String,
    /// lyk, Plover, Lapwing; `None` where that theory has nothing. A stroke query fills lyk alone.
    pub cells: [Option<Cell>; 3],
    pub mark: Mark,
}

/// What a query found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Answer {
    pub rows: Vec<Row>,
    /// Set when the query could not be read at all (stroke mode, not stroke notation)
    pub problem: Option<String>,
}

/// The key totals of each word's best outline, lyk's own and paired with each other theory's.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Totals {
    /// lyk's strokes and keys over every word it writes
    pub strokes: usize,
    pub keys: u32,
    /// Keys over the words both lyk and the other theory write: (lyk, theirs)
    pub vs_plover: (u32, u32),
    pub vs_lapwing: (u32, u32),
}

impl Answer {
    /// The best outline of each word: the first row of every word, completions and nearby
    /// entries left out.
    #[must_use]
    pub fn totals(&self) -> Totals {
        let mut totals = Totals::default();
        for row in self.rows.iter().filter(|r| r.mark == Mark::Entry && !r.word.is_empty()) {
            let keys = |cell: &Option<Cell>| cell.as_ref().map(|c| generate::keys(&c.strokes));
            let ours = keys(&row.cells[0]);
            if let (Some(cell), Some(k)) = (&row.cells[0], ours) {
                totals.strokes += cell.strokes.len();
                totals.keys += k;
            }
            if let (Some(k), Some(p)) = (ours, keys(&row.cells[1])) {
                totals.vs_plover.0 += k;
                totals.vs_plover.1 += p;
            }
            if let (Some(k), Some(l)) = (ours, keys(&row.cells[2])) {
                totals.vs_lapwing.0 += k;
                totals.vs_lapwing.1 += l;
            }
        }
        totals
    }
}

/// How to answer.
#[derive(Debug, Clone, Copy)]
pub struct Ask {
    /// Only the fewest-stroke outline of each word: no other outlines, no completions
    pub fast: bool,
    /// Read the query as stroke notation whatever its case (`resolve`, `stroke`)
    pub stroke_mode: bool,
    /// Most rows that merely extend the answer: completions after a word, entries near a stroke
    pub extra: usize,
}

/// The built dictionary, indexed for lookups. Built once per command; cheap enough to ask per
/// keystroke.
pub struct Lookup<'a> {
    setup: &'a Setup,
    entries: &'a [Entry],
    dictionary: HashMap<&'a str, &'a [Stroke]>,
    by_outline: HashMap<String, &'a str>,
    stems: HashMap<Stroke, String>,
    /// Every outline that types each word properly (suffix entries, which glue to the word
    /// before them, left out)
    outlines: HashMap<&'a str, Vec<&'a [Stroke]>>,
    /// Every word with an outline
    words: Vec<&'a str>,
    /// The backups `built::load` leaves out, when the dictionary was read back from `out/lyk`
    backups: Option<&'a Backups>,
    /// The solver keymux runs on a miss (see `site/reference/fallback.json`), when one is built:
    /// a stroke with no entry reads as what it answers, the plain read-back otherwise. Loaded on
    /// the first miss that needs it, so a lookup that never misses never pays for it
    solver: std::sync::OnceLock<Option<crate::rules::solve::Model>>,
    load_solver: Option<fn() -> Option<crate::rules::solve::Model>>,
}

impl<'a> Lookup<'a> {
    pub fn new(setup: &'a Setup, entries: &'a [Entry], backups: Option<&'a Backups>) -> Self {
        let dictionary = generate::cheapest(entries);
        let mut outlines: HashMap<&str, Vec<&[Stroke]>> = HashMap::new();
        for entry in entries.iter().filter(|e| e.origin != Origin::Suffix) {
            outlines.entry(entry.word.as_str()).or_default().push(&entry.outline);
        }
        let mut words: Vec<&str> = dictionary.keys().copied().collect();
        words.sort_unstable_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
        Self {
            setup,
            entries,
            dictionary,
            by_outline: generate::index_of(entries),
            stems: generate::single_stroke_stems_of(entries),
            outlines,
            words,
            backups,
            solver: std::sync::OnceLock::new(),
            load_solver: None,
        }
    }

    /// The same lookup, reading a miss as the solver does: `load` reads the solver in when a miss
    /// first needs it.
    #[must_use]
    pub fn with_solver(mut self, load: fn() -> Option<crate::rules::solve::Model>) -> Self {
        self.load_solver = Some(load);
        self
    }

    fn solver(&self) -> Option<&crate::rules::solve::Model> {
        self.solver.get_or_init(|| self.load_solver.and_then(|load| load())).as_ref()
    }

    pub const fn dictionary(&self) -> &HashMap<&'a str, &'a [Stroke]> {
        &self.dictionary
    }

    /// The answer to `query`.
    ///
    /// Stroke notation (no lowercase letter, or any text in stroke mode) is resolved forward:
    /// what it types, then the entries nearest to it. Anything else is text: each word's
    /// outline in every theory, then every other outline that types the word properly; a word
    /// with no entry is composed from entries or spelled, and marked. A single word is followed
    /// by the words that start with it.
    #[must_use]
    pub fn answer(&self, query: &str, ask: Ask) -> Answer {
        let query = query.trim();
        if query.is_empty() {
            return Answer::default();
        }
        let parsed = if ask.stroke_mode {
            match stroke::parse_outline(&query.to_uppercase()) {
                Some(strokes) => Query::Stroke(strokes),
                None => {
                    return Answer { rows: Vec::new(), problem: Some(format!("\"{query}\" is not stroke notation")) };
                }
            }
        } else {
            reverse::classify(query)
        };
        match parsed {
            Query::Stroke(strokes) => self.forward(&strokes, ask),
            Query::Text(text) => self.reverse_rows(&text, ask),
        }
    }

    fn forward_row(&self, strokes: &[Stroke], mark: Mark) -> Row {
        // Not an entry: the rules' best reading is what the stroke reads back as
        let reading = || {
            let solved = self.solver().and_then(|solver| solver.answer(&render_outline(strokes)));
            solved.or_else(|| self.setup.rules.resolve(strokes, Some(&self.stems))).map_or_else(
                || ("(no translation)".to_owned(), How::Entry),
                |reading| (format!("{reading:?}"), How::Reading),
            )
        };
        let (word, how) = self
            .by_outline
            .get(&render_outline(strokes))
            .map_or_else(reading, |word| (format!("{word:?}"), How::Entry));
        Row { word, cells: [Some(Cell { strokes: strokes.to_vec(), how }), None, None], mark }
    }

    fn forward(&self, strokes: &[Stroke], ask: Ask) -> Answer {
        let mut rows = vec![self.forward_row(strokes, Mark::Forward)];
        if !ask.fast {
            for outline in self.nearest(strokes, ask.extra) {
                rows.push(self.forward_row(&outline, Mark::Nearby));
            }
        }
        Answer { rows, problem: None }
    }

    /// The `limit` outlines of as many strokes as `strokes` with the fewest keys different from
    /// it (never `strokes` itself), nearest first and then in key order.
    fn nearest(&self, strokes: &[Stroke], limit: usize) -> Vec<Vec<Stroke>> {
        let mut found: Vec<(u32, &[Stroke])> = self
            .entries
            .iter()
            .map(|e| e.outline.as_slice())
            .filter(|o| o.len() == strokes.len() && *o != strokes)
            .map(|o| (o.iter().zip(strokes).map(|(a, b)| (a.0 ^ b.0).count_ones()).sum(), o))
            .collect();
        found.sort_unstable();
        found.dedup();
        found.into_iter().take(limit).map(|(_, o)| o.to_vec()).collect()
    }

    /// A cell from the text of an outline `reverse` produced; `None` when it does not parse.
    fn cell(text: Option<&str>, how: How) -> Option<Cell> {
        Some(Cell { strokes: stroke::parse_outline(text?)?, how })
    }

    /// The three cells of a word's best outlines.
    fn best_cells(found: &WordLookup) -> [Option<Cell>; 3] {
        let ours = if found.lyk_deployed {
            How::Entry
        } else if let Some(pieces) = &found.composed_of {
            How::Composed(pieces.clone())
        } else if found.lyk.as_deref().is_some_and(|o| o.starts_with('~')) {
            How::Spelled
        } else {
            How::Reading
        };
        let gold = |pieces: &Option<Vec<String>>| pieces.clone().map_or(How::Entry, How::Composed);
        [
            Self::cell(found.lyk.as_deref(), ours),
            Self::cell(found.plover.as_deref(), gold(&found.plover_composed_of)),
            Self::cell(found.lapwing.as_deref(), gold(&found.lapwing_composed_of)),
        ]
    }

    /// Every other outline that types `word` properly, per theory, fewest strokes first.
    fn others(&self, word: &str, best: &[Option<Cell>; 3]) -> [Vec<Vec<Stroke>>; 3] {
        let skip = |cell: &Option<Cell>| cell.as_ref().filter(|c| c.how == How::Entry).map(|c| c.strokes.clone());
        let mut lyk: Vec<Vec<Stroke>> = if skip(&best[0]).is_some() {
            let mut all: Vec<Vec<Stroke>> =
                self.outlines.get(word).into_iter().flatten().map(|o| o.to_vec()).collect();
            all.extend(self.backups.map(|b| b.outlines(word)).unwrap_or_default());
            all
        } else {
            Vec::new()
        };
        lyk.sort_by_cached_key(|o| (o.len(), generate::keys(o), o.clone()));
        lyk.dedup();
        let gold = |all: &HashMap<String, Vec<String>>| -> Vec<Vec<Stroke>> {
            all.get(word).into_iter().flatten().filter_map(|o| stroke::parse_outline(o)).collect()
        };
        let mut lists = [lyk, gold(&self.setup.gold.plover), gold(&self.setup.gold.lapwing)];
        for (list, cell) in lists.iter_mut().zip(best) {
            match skip(cell) {
                Some(first) => list.retain(|o| *o != first),
                None => list.clear(),
            }
        }
        lists
    }

    fn reverse_rows(&self, text: &str, ask: Ask) -> Answer {
        let words: Vec<&str> = text.split_whitespace().collect();
        let mut rows = Vec::new();
        for found in reverse::reverse_phrase(self.setup, &self.dictionary, text) {
            let word = found.word.as_str();
            let cells = Self::best_cells(&found);
            if cells.iter().all(Option::is_none) {
                rows.push(Row { word: word.to_owned(), cells, mark: Mark::Missing });
                continue;
            }
            let others = if ask.fast { Default::default() } else { self.others(&word.to_lowercase(), &cells) };
            rows.push(Row { word: word.to_owned(), cells, mark: Mark::Entry });
            for n in 0..others.iter().map(Vec::len).max().unwrap_or(0) {
                let cell = |t: usize| others[t].get(n).map(|o| Cell { strokes: o.clone(), how: How::Entry });
                rows.push(Row { word: String::new(), cells: [cell(0), cell(1), cell(2)], mark: Mark::Entry });
            }
        }
        if let [word] = words.as_slice()
            && !ask.fast
            && word.chars().count() >= MIN_PREFIX
        {
            let lower = word.to_lowercase();
            let plain = |w: &str| w.chars().all(|c| c.is_alphanumeric() || c == '\'' || c == '-');
            for completion in self
                .words
                .iter()
                .filter(|w| w.len() > lower.len() && w.starts_with(lower.as_str()) && plain(w))
                .take(ask.extra)
            {
                let (plover, lapwing) = self.setup.gold.shortest(completion);
                let gold = |o: Option<&str>| Self::cell(o, How::Entry);
                rows.push(Row {
                    word: (*completion).to_owned(),
                    cells: [
                        Some(Cell { strokes: self.dictionary[completion].to_vec(), how: How::Entry }),
                        gold(plover),
                        gold(lapwing),
                    ],
                    mark: Mark::Completion,
                });
            }
        }
        Answer { rows, problem: None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: Ask = Ask { fast: false, stroke_mode: false, extra: 8 };

    fn lookup() -> Lookup<'static> {
        Lookup::new(crate::dictionary_fixture::setup(), &crate::dictionary_fixture::generated().entries, None)
    }

    fn text(cell: Option<&Cell>) -> String {
        cell.map_or_else(|| "-".to_owned(), |c| render_outline(&c.strokes))
    }

    #[test]
    fn a_word_has_an_outline_in_every_theory_and_a_row_per_other_outline() {
        let answer = lookup().answer("cat", ALL);
        let first = &answer.rows[0];
        assert_eq!((first.word.as_str(), first.mark), ("cat", Mark::Entry));
        assert!(first.cells[0].as_ref().is_some_and(|c| c.how == How::Entry));
        // only the first row carries the word; the others are further outlines of it
        for row in answer.rows.iter().filter(|r| r.mark == Mark::Entry).skip(1) {
            assert!(row.word.is_empty());
        }
    }

    #[test]
    fn fast_is_the_best_outline_alone() {
        let answer = lookup().answer("cat", Ask { fast: true, ..ALL });
        assert_eq!(answer.rows.len(), 1);
    }

    #[test]
    fn a_single_word_is_followed_by_longer_words_that_start_with_it() {
        let answer = lookup().answer("cat", ALL);
        let completions: Vec<&str> =
            answer.rows.iter().filter(|r| r.mark == Mark::Completion).map(|r| r.word.as_str()).collect();
        assert!(!completions.is_empty() && completions.len() <= 8, "{completions:?}");
        assert!(completions.iter().all(|w| w.starts_with("cat") && *w != "cat"));
        // shortest first, then alphabetical, as in keymux
        assert!(completions.windows(2).all(|p| (p[0].len(), p[0]) <= (p[1].len(), p[1])));
        // a phrase has none
        assert!(lookup().answer("tell me", ALL).rows.iter().all(|r| r.mark != Mark::Completion));
    }

    #[test]
    fn a_stroke_shows_what_it_types_then_the_entries_nearest_to_it() {
        let answer = lookup().answer("KAT", ALL);
        assert_eq!((answer.rows[0].word.as_str(), answer.rows[0].mark), ("\"cat\"", Mark::Forward));
        let nearby: Vec<&Row> = answer.rows.iter().filter(|r| r.mark == Mark::Nearby).collect();
        assert!(!nearby.is_empty() && nearby.len() <= 8);
        assert!(nearby.iter().all(|r| text(r.cells[0].as_ref()) != "KAT"));
        // only lyk's column is filled for a stroke
        assert!(answer.rows.iter().all(|r| r.cells[1].is_none() && r.cells[2].is_none()));
    }

    #[test]
    fn a_stroke_nothing_matches_is_no_translation_or_the_rules_reading() {
        let answer = lookup().answer("STKPWHRAO*EUFRPBLGTSDZ", ALL);
        assert!(answer.rows[0].word == "(no translation)" || answer.rows[0].cells[0].as_ref().unwrap().how == How::Reading);
    }

    #[test]
    fn stroke_mode_reads_lowercase_as_capitals_and_rejects_text() {
        let stroke_mode = Ask { stroke_mode: true, ..ALL };
        assert_eq!(lookup().answer("kat", stroke_mode).rows[0].word, "\"cat\"");
        let bad = lookup().answer("cat!", stroke_mode);
        assert_eq!(bad.problem.as_deref(), Some("\"cat!\" is not stroke notation"));
        assert!(bad.rows.is_empty());
    }

    #[test]
    fn totals_count_each_words_best_outline_only() {
        let answer = lookup().answer("tell me how", ALL);
        let totals = answer.totals();
        let best: u32 = answer
            .rows
            .iter()
            .filter(|r| r.mark == Mark::Entry && !r.word.is_empty())
            .filter_map(|r| r.cells[0].as_ref())
            .map(|c| generate::keys(&c.strokes))
            .sum();
        assert_eq!(totals.keys, best);
        assert!(totals.strokes >= 3);
    }

    #[test]
    fn nothing_typed_answers_nothing() {
        assert!(lookup().answer("   ", ALL).rows.is_empty());
    }
}

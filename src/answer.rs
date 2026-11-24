//! What the lookup commands answer, as data: the rows a query produces.
//!
//! `lookup` (alias `keys`) and `resolve` (alias `stroke`) both end up here, whether they are given
//! a query on the command line or one is typed live ([`crate::interactive`]), so there is one
//! answer, drawn the same way ([`crate::view`]) every time. The structure is keymux's own
//! `steno/answer.rs`, and the one thing that differs is that a row has an outline per theory
//! (lyk, Plover, Lapwing) where keymux resolves its one engine setup.

use crate::analysis::Setup;
use crate::built::Backups;
use crate::generate::{self, Entry, Origin, written};
use crate::phrases::grammar::{PartKind, Phrase, Phrasing};
use crate::reverse::{self, Query, WordLookup};
use crate::stroke::{self, Stroke, render_outline};
use std::collections::HashMap;

/// The three theories, in the order a row holds them (and the order of the table's columns).
pub const THEORIES: [&str; 3] = ["lyk", "plover", "lapwing"];

/// Words that merely start with the query are only offered from this length on.
const MIN_PREFIX: usize = 2;

/// A part of a non-word entry's text is only matched from this length on (`ca` is not every `{:case}`).
const MIN_PART: usize = 3;

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
    /// Not an entry of the deployed dictionary: keymux hands the strokes to the solver, and the
    /// lookup has run it and it reads exactly this word
    Solver,
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
    /// An entry that is not a word (command, suffix, prefix, punctuation, fingerspelling) whose
    /// text is or contains the query (`tape` finds `{:tape:toggle}`, `^ing` finds `{^ing}`), after
    /// the exact answers; only lyk's column is filled
    Command,
    /// A word that starts with the query, offered after the exact answers
    Completion,
    /// A phrase the phrasing grammar writes in one stroke (`I was`), ahead of its words; only lyk's
    /// column is filled and `detail` says which key group says what
    Phrasing,
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
    /// Lines printed under the table about this row (what the phrasing grammar says it is made of)
    pub detail: Vec<String>,
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
        // A phrase's own row (`to the`) is left out: the totals are of its words
        for row in self.rows.iter().filter(|r| r.mark == Mark::Entry && !r.word.is_empty() && !r.word.contains(' ')) {
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

/// An entry that is not a word: a command, suffix, prefix, punctuation mark, fingerspelled letter.
struct Special {
    /// As shown: the dictionary's text without the first-up (`{:firstup}`, `{:firstup:except=KEYS}`)
    /// and `{:transparent}` markers
    text: String,
    /// As the dictionary has it
    raw: String,
    /// `text` lowercased without braces and `^` (see `Lookup::bare`)
    norm: String,
    outline: Vec<Stroke>,
}

/// The built dictionary, indexed for lookups. Built once per command; cheap enough to ask per
/// keystroke.
pub struct Lookup<'a> {
    setup: &'a Setup,
    entries: &'a [Entry],
    dictionary: HashMap<&'a str, &'a [Stroke]>,
    /// What each outline of the *deployed* dictionary types: every entry but the backups, which
    /// `deploy` does not ship (keymux hands such an outline to the solver)
    by_outline: HashMap<&'a [Stroke], &'a str>,
    stems: HashMap<Stroke, String>,
    /// Every deployed entry that types each word properly (suffix entries, which glue to the
    /// word before them, and backups left out)
    outlines: HashMap<&'a str, Vec<&'a [Stroke]>>,
    /// The backups `built::load` kept as entries, by word (the rest are in `backups`)
    backup_entries: HashMap<&'a str, Vec<&'a [Stroke]>>,
    /// Every outline of the reserved tables (commands, punctuation, fingerspelling, shortcuts)
    reserved: std::collections::HashSet<Vec<Stroke>>,
    /// Every word with an outline
    words: Vec<&'a str>,
    /// Every entry that is not a word, reserved first, fewest strokes first
    specials: Vec<Special>,
    /// The backups `built::load` leaves out, when the dictionary was read back from `out/lyk`
    backups: Option<&'a Backups>,
    /// The solver keymux runs on a miss (see `site/reference/fallback.json`), when one is built:
    /// a stroke with no entry reads as what it answers, the plain read-back otherwise. Loaded on
    /// the first miss that needs it, so a lookup that never misses never pays for it
    solver: std::sync::OnceLock<Option<crate::rules::solve::Model>>,
    load_solver: Option<fn() -> Option<crate::rules::solve::Model>>,
    /// The phrasing grammar as this dictionary has it (`None` for one built before it existed)
    phrasing: Option<Phrasing>,
}

impl<'a> Lookup<'a> {
    pub fn new(setup: &'a Setup, entries: &'a [Entry], backups: Option<&'a Backups>) -> Self {
        Self::with_cheapest(setup, entries, backups, None)
    }

    /// The same lookup, given each word's cheapest entry (`generate::cheapest`) by its place in
    /// `entries` when it is already known (`crate::lookup_cache` keeps it).
    pub fn with_cheapest(
        setup: &'a Setup,
        entries: &'a [Entry],
        backups: Option<&'a Backups>,
        cheapest: Option<&[u32]>,
    ) -> Self {
        // Four indexes of the same entries, a few tens of milliseconds each: side by side
        let (dictionary, words, by_outline, stems, (outlines, backup_entries)) = std::thread::scope(|scope| {
            // `generate::index_of`, keyed by the strokes rather than their notation; the backups are
            // not in the dictionary keymux loads
            let by_outline = scope.spawn(|| {
                entries
                    .iter()
                    .filter(|e| e.origin != Origin::Backup)
                    .map(|e| (e.outline.as_slice(), e.word.as_str()))
                    .collect::<HashMap<_, _>>()
            });
            let stems = scope.spawn(|| generate::single_stroke_stems_of(entries));
            let outlines = scope.spawn(|| {
                let mut outlines: HashMap<&str, Vec<&[Stroke]>> = HashMap::new();
                let mut backups: HashMap<&str, Vec<&[Stroke]>> = HashMap::new();
                for entry in entries.iter().filter(|e| e.origin != Origin::Suffix) {
                    let into = if entry.origin == Origin::Backup { &mut backups } else { &mut outlines };
                    into.entry(entry.word.as_str()).or_default().push(&entry.outline);
                }
                (outlines, backups)
            });
            let dictionary: HashMap<&str, &[Stroke]> = cheapest.map_or_else(
                || generate::cheapest_deployed(entries),
                |cheapest| {
                    let entry = |at: u32| &entries[at as usize];
                    cheapest.iter().map(|&at| (entry(at).word.as_str(), entry(at).outline.as_slice())).collect()
                },
            );
            let mut words: Vec<&str> = dictionary.keys().copied().collect();
            words.sort_unstable_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
            (
                dictionary,
                words,
                by_outline.join().expect("outline index panicked"),
                stems.join().expect("stem index panicked"),
                outlines.join().expect("word index panicked"),
            )
        });
        Self {
            setup,
            entries,
            dictionary,
            by_outline,
            stems,
            outlines,
            backup_entries,
            reserved: generate::reserved_entries().unwrap_or_default().into_iter().map(|r| r.outline).collect(),
            words,
            specials: Self::specials(entries),
            backups,
            solver: std::sync::OnceLock::new(),
            load_solver: None,
            phrasing: Phrasing::read(entries),
        }
    }

    /// The same lookup, reading a miss as the solver does: `load` reads the solver in when a miss
    /// first needs it.
    #[must_use]
    pub fn with_solver(mut self, load: fn() -> Option<crate::rules::solve::Model>) -> Self {
        self.load_solver = Some(load);
        self
    }

    /// The same lookup, reading a miss with this solver model.
    #[must_use]
    pub fn with_model(self, model: crate::rules::solve::Model) -> Self {
        let _ = self.solver.set(Some(model));
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
            .get(strokes)
            .map_or_else(reading, |word| (format!("{word:?}"), How::Entry));
        Row { word, cells: [Some(Cell { strokes: strokes.to_vec(), how }), None, None], mark, detail: Vec::new() }
    }

    fn forward(&self, strokes: &[Stroke], ask: Ask) -> Answer {
        let mut first = self.forward_row(strokes, Mark::Forward);
        if let [stroke] = strokes
            && let Some(phrase) = self.phrasing.as_ref().and_then(|p| p.stroke(*stroke))
        {
            first.word = format!("{:?}", written(&phrase.text));
            first.detail = grammar_detail(phrase, None);
        }
        let mut rows = vec![first];
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

    /// Plover's and Lapwing's cells of a word's best outlines.
    fn gold_cells(found: &WordLookup) -> [Option<Cell>; 2] {
        let gold = |pieces: &Option<Vec<String>>| pieces.clone().map_or(How::Entry, How::Composed);
        [
            Self::cell(found.plover.as_deref(), gold(&found.plover_composed_of)),
            Self::cell(found.lapwing.as_deref(), gold(&found.lapwing_composed_of)),
        ]
    }

    /// Whether `strokes` is an entry of the deployed dictionary (or a reserved stroke): keymux
    /// types it without asking the solver.
    fn is_entry(&self, strokes: &[Stroke]) -> bool {
        self.by_outline.contains_key(strokes) || self.reserved.contains(strokes)
    }

    /// Of `outlines`, the ones the solver reads as exactly `word` (lowercase), in their order;
    /// `None` when there is no solver to ask. Several at once are read on every core.
    fn read_as(&self, word: &str, outlines: Vec<Vec<Stroke>>) -> Option<Vec<Vec<Stroke>>> {
        let model = self.solver()?;
        let reads = |sc: &mut crate::rules::solve::core::Scratch, o: &[Stroke]| {
            model.answer_with(sc, &render_outline(o)).is_some_and(|text| text.to_lowercase() == word)
        };
        let threads = std::thread::available_parallelism().map_or(1, std::num::NonZero::get).min(outlines.len() / 8 + 1);
        if threads <= 1 {
            let mut sc = model.scratch();
            return Some(outlines.into_iter().filter(|o| reads(&mut sc, o)).collect());
        }
        let chunk = outlines.len().div_ceil(threads);
        Some(std::thread::scope(|scope| {
            #[allow(clippy::needless_collect)]
            let handles: Vec<_> = outlines
                .chunks(chunk)
                .map(|part| {
                    scope.spawn(move || {
                        let mut sc = model.scratch();
                        part.iter().filter(|o| reads(&mut sc, o)).cloned().collect::<Vec<_>>()
                    })
                })
                .collect();
            handles.into_iter().flat_map(|h| h.join().expect("a solver thread panicked")).collect()
        }))
    }

    /// Every outline lyk lists for `word` (lowercase) that keymux types as it, best first.
    ///
    /// An outline is listed only when typing it gives the word: a deployed entry, or a backup
    /// (a way into the word the generator wrote, never shipped) that the solver reads as the word,
    /// which is run here exactly as the plugin runs it. Entries come before solver-read outlines.
    /// With no solver to ask, only entries are listed. `fast` stops at the best outline.
    fn lyk_listing(&self, found: &WordLookup, fast: bool) -> Vec<Cell> {
        let word = found.word.to_lowercase();
        let by_cost = |o: &Vec<Stroke>| (o.len(), generate::keys(o), o.clone());
        let mut entries: Vec<Vec<Stroke>> = self.outlines.get(word.as_str()).into_iter().flatten().map(|o| o.to_vec()).collect();
        entries.sort_by_cached_key(by_cost);
        entries.dedup();
        let best = found.lyk.as_deref().and_then(stroke::parse_outline);
        let cell = |strokes: Vec<Stroke>, how: How| Cell { strokes, how };
        // The settled outline is an entry: it leads, the other entries follow, then the backups
        if let Some(best) = best.as_ref().filter(|b| found.lyk_deployed && self.is_entry(b)) {
            let mut list = vec![cell(best.clone(), How::Entry)];
            list.extend(entries.into_iter().filter(|o| o != best).map(|o| cell(o, How::Entry)));
            if !fast {
                list.extend(self.solver_read(&word, best).into_iter().map(|o| cell(o, How::Solver)));
            }
            return list;
        }
        // The cheapest outline is a backup (or none is an entry): the cheapest of the entries and
        // of the backups the solver reads right leads
        let mut backups = self.backup_outlines(&word);
        let read = if fast {
            // only the first that reads right is wanted
            self.first_read(&word, std::mem::take(&mut backups)).into_iter().collect()
        } else {
            self.read_as(&word, backups).unwrap_or_default()
        };
        let lead_is_backup = match (entries.first(), read.first()) {
            (Some(e), Some(r)) => by_cost(r) < by_cost(e),
            (None, Some(_)) => true,
            _ => false,
        };
        if lead_is_backup || !entries.is_empty() {
            let mut list: Vec<Cell> = Vec::new();
            if lead_is_backup {
                list.push(cell(read[0].clone(), How::Solver));
            }
            list.extend(entries.into_iter().map(|o| cell(o, How::Entry)));
            list.extend(read.into_iter().skip(usize::from(lead_is_backup)).map(|o| cell(o, How::Solver)));
            return list;
        }
        // No entry, no backup that reads right: composed from entries, the rules' own reading, or
        // spelled letter by letter, whichever the solver reads as the word (all that can be
        // offered with no solver to ask)
        let how = if found.composed_of.is_some() {
            found.composed_of.clone().map(How::Composed)
        } else if found.lyk.as_deref().is_some_and(|o| o.starts_with('~')) {
            Some(How::Spelled)
        } else if found.lyk.is_some() && !found.lyk_deployed {
            Some(How::Reading)
        } else {
            None
        };
        let spelled = || self.setup.rules.spell_word(&word).map(|o| cell(o, How::Spelled));
        let own = best.zip(how).map(|(strokes, how)| cell(strokes, how));
        let ok = |c: &Cell| self.read_as(&word, vec![c.strokes.clone()]).is_none_or(|read| !read.is_empty());
        own.filter(ok).into_iter().chain(spelled().filter(ok)).take(1).collect()
    }

    /// The first of `outlines` the solver reads as `word` (lowercase), one at a time.
    fn first_read(&self, word: &str, outlines: Vec<Vec<Stroke>>) -> Option<Vec<Stroke>> {
        outlines.into_iter().find(|o| self.read_as(word, vec![o.clone()]).is_some_and(|ok| !ok.is_empty()))
    }

    /// The cell a completion shows for `word` (lowercase): its settled outline when that is an
    /// entry, else its cheapest backup the solver reads right; `None` when there is neither.
    fn completion_cell(&self, word: &str) -> Option<Cell> {
        let best = self.dictionary.get(word)?;
        if self.is_entry(best) {
            return Some(Cell { strokes: best.to_vec(), how: How::Entry });
        }
        let strokes = self.first_read(word, self.backup_outlines(word))?;
        Some(Cell { strokes, how: How::Solver })
    }

    /// The backups of `word` (lowercase): the ones kept as entries and the catalogue's, fewest
    /// strokes first, leaving out any outline an entry of the deployed dictionary holds.
    fn backup_outlines(&self, word: &str) -> Vec<Vec<Stroke>> {
        let mut all: Vec<Vec<Stroke>> = self.backup_entries.get(word).into_iter().flatten().map(|o| o.to_vec()).collect();
        all.extend(self.backups.map_or_default(|b| b.outlines(word)));
        all.retain(|o| !self.is_entry(o));
        all.sort_by_cached_key(|o| (o.len(), generate::keys(o), o.clone()));
        all.dedup();
        all
    }

    /// Every backup of `word` that the solver reads as the word, except `best`.
    fn solver_read(&self, word: &str, best: &[Stroke]) -> Vec<Vec<Stroke>> {
        let backups = self.backup_outlines(word).into_iter().filter(|o| o != best).collect();
        self.read_as(word, backups).unwrap_or_default()
    }

    /// Plover's and Lapwing's other outlines that type `word`, fewest strokes first.
    fn gold_others(&self, word: &str, best: &[Option<Cell>; 2]) -> [Vec<Vec<Stroke>>; 2] {
        let gold = |all: &HashMap<String, Vec<String>>| -> Vec<Vec<Stroke>> {
            all.get(word).into_iter().flatten().filter_map(|o| stroke::parse_outline(o)).collect()
        };
        let mut lists = [gold(&self.setup.gold.plover), gold(&self.setup.gold.lapwing)];
        for (list, cell) in lists.iter_mut().zip(best) {
            match cell.as_ref().filter(|c| c.how == How::Entry).map(|c| c.strokes.clone()) {
                Some(first) => list.retain(|o| *o != first),
                None => list.clear(),
            }
            list.sort_by_cached_key(|o| (o.len(), generate::keys(o), o.clone()));
            list.dedup();
        }
        lists
    }

    /// Every entry that is not a word: the reserved ones and the dictionary's own whose text has
    /// a suffix, or a brace, a `^` or any mark beyond letters, digits, `'` and `-` (suffixes, prefixes,
    /// `Jr.`), fewest strokes first.
    fn specials(entries: &[Entry]) -> Vec<Special> {
        let plain = |w: &str| w.chars().all(|c| c.is_alphanumeric() || c == '\'' || c == '-' || c == ' ');
        let mut found: Vec<Special> = generate::reserved_entries()
            .unwrap_or_default()
            .into_iter()
            .map(|r| Special { norm: Self::bare(&r.shown), text: r.shown, raw: r.text, outline: r.outline })
            .collect();
        let mut own: Vec<Special> = entries
            .iter()
            .filter(|e| e.origin == Origin::Suffix || !plain(&e.word))
            .map(|e| {
                // The generator keeps a suffix bare (`ing`); the dictionary writes it `{^}ing`
                let text = if e.origin == Origin::Suffix { format!("{{^}}{}", e.word) } else { e.word.clone() };
                Special { norm: Self::bare(&text), raw: text.clone(), text, outline: e.outline.clone() }
            })
            .collect();
        own.sort_by_cached_key(|s| (s.outline.len(), generate::keys(&s.outline), s.text.clone()));
        own.dedup_by(|a, b| a.text == b.text && a.outline == b.outline);
        found.extend(own);
        found
    }

    /// `text` lowercased without the marks that say how an entry joins or acts (`{`, `}`, `^`, `#`):
    /// `{^ing}` and `^ing` are both `ing`.
    fn bare(text: &str) -> String {
        text.chars().filter(|c| !matches!(c, '{' | '}' | '^' | '#')).collect::<String>().to_lowercase()
    }

    /// The non-word entries `query` names, best match first: rank 0 its exact text, 1 its text
    /// without braces and `^`, 2 a part of that text. At most `limit`, each with its rank.
    fn special_rows(&self, query: &str, limit: usize) -> Vec<(u8, Row)> {
        let bare = Self::bare(query);
        if bare.is_empty() {
            return Vec::new();
        }
        let mut found: Vec<(u8, &Special)> = self
            .specials
            .iter()
            .filter_map(|s| {
                let rank = if s.text.to_lowercase() == query || s.raw.to_lowercase() == query {
                    0
                } else if s.norm == bare {
                    1
                } else if bare.chars().count() >= MIN_PART && s.norm.contains(&bare) {
                    2
                } else {
                    return None;
                };
                Some((rank, s))
            })
            .collect();
        found.sort_by_key(|(rank, _)| *rank); // stable: fewest strokes first within a rank
        found
            .into_iter()
            .take(limit)
            .map(|(rank, s)| {
                let cell = Cell { strokes: s.outline.clone(), how: How::Entry };
                (rank, Row { word: s.text.clone(), cells: [Some(cell), None, None], mark: Mark::Command, detail: Vec::new() })
            })
            .collect()
    }

    /// The row for the longest start of the query that the phrasing grammar writes, and how many
    /// words it covers: `i was`, `They wouldn't!`, `she has been going` (covering `she has been`).
    fn grammar_row(&self, words: &[&str]) -> Option<(usize, Row)> {
        let phrasing = self.phrasing.as_ref()?;
        let tokens = grammar_tokens(words);
        let as_typed = phrasing.leading(&tokens);
        // `I do not know` is `I don't know`: the same words with the negative spelt out. Each
        // contracted token stands for the words it came from.
        let (contracted, spans) = contract_not(&tokens);
        let contracted = phrasing.leading(&contracted).map(|(n, phrase)| (spans[..n].iter().sum::<usize>(), phrase));
        let (covered, phrase) = match (as_typed, contracted) {
            (Some(a), Some(b)) => if b.0 > a.0 { b } else { a },
            (a, b) => a.or(b)?,
        };
        let partial = (covered < tokens.len()).then(|| (covered, tokens.len(), tokens[covered..].join(" ")));
        let row = Row {
            word: written(&phrase.text),
            cells: [Some(Cell { strokes: vec![phrase.stroke], how: How::Entry }), None, None],
            mark: Mark::Phrasing,
            detail: grammar_detail(phrase, partial),
        };
        Some((covered, row))
    }

    fn reverse_rows(&self, text: &str, ask: Ask) -> Answer {
        let words: Vec<&str> = text.split_whitespace().collect();
        let mut rows = Vec::new();
        // What the phrasing grammar writes of the query comes first (the whole of it, or its start)
        let grammar = self.grammar_row(&words);
        let whole = grammar.as_ref().is_some_and(|(covered, _)| *covered == words.len());
        // A query the grammar reads has its punctuation ignored by the per-word rows as well
        let cleaned: String;
        let text = if grammar.is_some() {
            cleaned = words.iter().map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'')).collect::<Vec<_>>().join(" ");
            cleaned.as_str()
        } else {
            text
        };
        rows.extend(grammar.map(|(_, row)| row));
        // A phrase that is an entry of its own (`to the`, one stroke) comes first, then each word
        if words.len() >= 2 && !whole {
            let phrase = words.join(" ").to_lowercase();
            if let Some(outline) = self.dictionary.get(phrase.as_str()) {
                let cell = Cell { strokes: outline.to_vec(), how: How::Entry };
                rows.push(Row { word: phrase, cells: [Some(cell), None, None], mark: Mark::Entry, detail: Vec::new() });
            }
        }
        for found in reverse::reverse_phrase(self.setup, &self.dictionary, text) {
            let word = found.word.as_str();
            let listing = self.lyk_listing(&found, ask.fast);
            let [plover, lapwing] = Self::gold_cells(&found);
            let cells = [listing.first().cloned(), plover, lapwing];
            if cells.iter().all(Option::is_none) {
                rows.push(Row { word: word.to_owned(), cells, mark: Mark::Missing, detail: Vec::new() });
                continue;
            }
            let gold = if ask.fast {
                Default::default()
            } else {
                self.gold_others(&word.to_lowercase(), &[cells[1].clone(), cells[2].clone()])
            };
            rows.push(Row { word: word.to_owned(), cells, mark: Mark::Entry, detail: Vec::new() });
            let ours = if ask.fast { &[][..] } else { listing.get(1..).unwrap_or_default() };
            for n in 0..ours.len().max(gold[0].len()).max(gold[1].len()) {
                let theirs = |t: usize| gold[t].get(n).map(|o| Cell { strokes: o.clone(), how: How::Entry });
                rows.push(Row { word: String::new(), cells: [ours.get(n).cloned(), theirs(0), theirs(1)], mark: Mark::Entry, detail: Vec::new() });
            }
        }
        if let [word] = words.as_slice()
            && !ask.fast
            && word.chars().count() >= MIN_PREFIX
        {
            let lower = word.to_lowercase();
            let found = self.special_rows(&lower, ask.extra);
            // A query that is an entry's own text (`{^ing}`, `^ing`) is not a missing word
            if found.iter().any(|(rank, _)| *rank <= 1) && rows.first().is_some_and(|r| r.mark == Mark::Missing) {
                rows.remove(0);
            }
            let commands: Vec<Row> = found.into_iter().map(|(_, row)| row).collect();
            // Right after the word's best outline: a word can have dozens of others, and a command
            // listed behind them would be off the screen
            let at = rows.len().min(1);
            rows.splice(at..at, commands);
            let plain = |w: &str| w.chars().all(|c| c.is_alphanumeric() || c == '\'' || c == '-');
            for (completion, cell) in self
                .words
                .iter()
                .filter(|w| w.len() > lower.len() && w.starts_with(lower.as_str()) && plain(w))
                .filter_map(|w| Some((w, self.completion_cell(w)?)))
                .take(ask.extra)
            {
                let (plover, lapwing) = self.setup.gold.shortest(completion);
                let gold = |o: Option<&str>| Self::cell(o, How::Entry);
                rows.push(Row {
                    word: (*completion).to_owned(),
                    cells: [Some(cell), gold(plover), gold(lapwing)],
                    mark: Mark::Completion,
                    detail: Vec::new(),
                });
            }
        }
        Answer { rows, problem: None }
    }
}

/// The words of a query as the grammar's texts have them: lowercase, a curly apostrophe straight,
/// punctuation gone, the contractions that are one word each (`I'm`, `they're`, `I've`, `I'll`)
/// spelt out.
fn grammar_tokens(words: &[&str]) -> Vec<String> {
    let mut tokens = Vec::new();
    for word in words {
        let clean: String = word
            .to_lowercase()
            .chars()
            .map(|c| if c == '\u{2019}' || c == '\u{2018}' { '\'' } else { c })
            .map(|c| if c.is_alphanumeric() || c == '\'' { c } else { ' ' })
            .collect();
        for token in clean.split_whitespace() {
            let token = token.trim_matches('\'');
            if token.is_empty() {
                continue;
            }
            let split = [("'m", "am"), ("'re", "are"), ("'ve", "have"), ("'ll", "will")]
                .iter()
                .find_map(|(tail, full)| token.strip_suffix(tail).filter(|head| !head.is_empty()).map(|head| (head, *full)));
            match split {
                Some((head, full)) => {
                    tokens.push(head.to_owned());
                    tokens.push(full.to_owned());
                }
                None => tokens.push(token.to_owned()),
            }
        }
    }
    tokens
}

/// `tokens` with each spelt-out negative contracted (`do not` is `don't`, `will not` is `won't`,
/// `can not` and `cannot` are `can't`; the grammar's own `shall not`, `may not`, `might not` and
/// `am not` stay), and how many of `tokens` each one stands for.
fn contract_not(tokens: &[String]) -> (Vec<String>, Vec<usize>) {
    let mut out: Vec<String> = Vec::new();
    let mut spans: Vec<usize> = Vec::new();
    for token in tokens {
        let joined = match (out.last().map(String::as_str), token.as_str()) {
            (Some("will"), "not") => Some("won't".to_owned()),
            (Some("can"), "not") => Some("can't".to_owned()),
            (Some(prev @ ("is" | "are" | "was" | "were" | "do" | "does" | "did" | "has" | "have" | "had" | "could" | "would" | "should" | "must")), "not") => {
                Some(format!("{prev}n't"))
            }
            _ => None,
        };
        match joined {
            Some(contracted) => {
                out.pop();
                out.push(contracted);
                if let Some(span) = spans.last_mut() {
                    *span += 1;
                }
            }
            None if token == "cannot" => {
                out.push("can't".to_owned());
                spans.push(1);
            }
            None => {
                out.push(token.clone());
                spans.push(1);
            }
        }
    }
    (out, spans)
}

/// The lines under a phrase's row: what each group of keys says, and for a query the grammar
/// covers only the start of (`(covered, words, rest)`), how much it covers.
fn grammar_detail(phrase: &Phrase, partial: Option<(usize, usize, String)>) -> Vec<String> {
    let parts: Vec<String> = phrase
        .parts
        .iter()
        .map(|p| if p.kind == PartKind::Not { format!("{} not", p.keys) } else { format!("{} {} ({})", p.keys, p.kind.name(), p.says) })
        .collect();
    let keys: Vec<&str> = phrase.parts.iter().map(|p| p.keys.as_str()).collect();
    let mut lines = vec![
        format!("  {}: one stroke of the phrasing grammar, {}", written(&phrase.text), keys.join(" + ")),
        format!("    {}", parts.join(" + ")),
    ];
    if let Some((covered, total, rest)) = partial {
        lines.push(format!(
            "  the grammar covers the first {covered} of {total} words ({}); the rest ({rest}) is per word below",
            written(&phrase.text)
        ));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: Ask = Ask { fast: false, stroke_mode: false, extra: 8 };

    #[test]
    fn a_phrase_that_is_an_entry_is_answered_as_one() {
        let answer = lookup().answer("of the", ALL);
        assert_eq!(answer.rows[0].word, "of the");
        assert_eq!(answer.rows[0].cells[0].as_ref().map(|c| render_outline(&c.strokes)).as_deref(), Some("-FT"));
        // and each word still follows
        assert!(answer.rows.iter().any(|r| r.word == "of") && answer.rows.iter().any(|r| r.word == "the"));
        // the totals count the words, not the phrase on top of them
        assert_eq!(answer.totals().strokes, 2);
    }

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
            assert!(row.word.is_empty(), "{:?}", row.word);
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

    // The entries are dictionary metas, not format strings.
    #[allow(clippy::literal_string_with_formatting_args)]
    #[test]
    fn a_command_is_found_by_the_name_in_it() {
        let answer = lookup().answer("tape", ALL);
        let commands: Vec<(&str, String)> = answer
            .rows
            .iter()
            .filter(|r| r.mark == Mark::Command)
            .map(|r| (r.word.as_str(), text(r.cells[0].as_ref())))
            .collect();
        assert!(commands.contains(&("{:tape:toggle}", "PHRO*T".to_owned())), "{commands:?}");
        assert!(commands.contains(&("{:tape:clear}", "PHRO*TSDZ".to_owned())), "{commands:?}");
        assert!(commands.iter().all(|(word, _)| !word.contains("firstup") && !word.contains("transparent")));
        // right after the word's best outline, ahead of its other outlines and its completions
        let marks: Vec<Mark> = answer.rows.iter().map(|r| r.mark).collect();
        assert_eq!(marks[0], Mark::Entry);
        assert_eq!(marks[1], Mark::Command);
        // the three that flip the overlay's parts are all findable
        for (name, outline) in [("overlay", "PHRO*L"), ("keyboard", "PHRO*B")] {
            let found = lookup().answer(name, ALL);
            assert!(
                found.rows.iter().any(|r| r.mark == Mark::Command && text(r.cells[0].as_ref()) == outline),
                "{name}"
            );
        }
        // fast mode and phrases skip them
        assert!(lookup().answer("tape", Ask { fast: true, ..ALL }).rows.iter().all(|r| r.mark != Mark::Command));
        assert!(lookup().answer("the tape", ALL).rows.iter().all(|r| r.mark != Mark::Command));
    }

    /// The `(text, outline)` of every non-word row of `query`'s answer.
    fn specials_of(query: &str) -> Vec<(String, String)> {
        lookup()
            .answer(query, ALL)
            .rows
            .iter()
            .filter(|r| r.mark == Mark::Command)
            .map(|r| (r.word.clone(), text(r.cells[0].as_ref())))
            .collect()
    }

    // The entries are dictionary metas, not format strings.
    #[allow(clippy::literal_string_with_formatting_args)]
    #[test]
    fn any_entry_is_found_by_its_text_with_or_without_its_marks() {
        // a suffix: its exact text, without the braces, without the `{}`, and as a bare word
        for query in ["{^ing}", "^ing", "ing"] {
            let found = specials_of(query);
            assert!(found.first().is_some_and(|(t, o)| t == "{^}ing" && o.starts_with('+') && o.ends_with('G')), "{query}: {found:?}");
        }
        // a query that is an entry's own text is not reported as a missing word
        let answer = lookup().answer("^ing", ALL);
        assert_eq!(answer.rows[0].mark, Mark::Command);
        assert!(answer.rows.iter().all(|r| r.mark != Mark::Missing));
        // exact text ranks ahead of a partial match, ignoring case
        assert_eq!(specials_of("{:TAPE:toggle}")[0].1, "PHRO*T");
        // punctuation and fingerspelling, not only commands
        let punctuation = specials_of("{>}{&e}");
        assert!(punctuation.iter().any(|(t, o)| t == "{>}{&e}" && o == "*E"), "{punctuation:?}");
        assert!(!specials_of("control").is_empty(), "control finds nothing");
        // fast mode, one-letter queries and marks alone find none
        assert!(lookup().answer("^ing", Ask { fast: true, ..ALL }).rows.iter().all(|r| r.mark != Mark::Command));
        assert!(specials_of("^").is_empty(), "{:?}", specials_of("^"));
        assert!(specials_of("{}").is_empty(), "{:?}", specials_of("{}"));
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
        assert!(bad.rows.is_empty(), "{:?}", bad.rows.len());
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
    fn a_phrase_the_grammar_writes_comes_first_with_its_parts() {
        let answer = lookup().answer("They wouldn't!", ALL);
        let first = &answer.rows[0];
        assert_eq!((first.word.as_str(), first.mark), ("they wouldn't", Mark::Phrasing));
        assert_eq!(text(first.cells[0].as_ref()), "STWHRO*E");
        assert_eq!(
            first.detail,
            [
                "  they wouldn't: one stroke of the phrasing grammar, STWHR + OE + *",
                "    STWHR starter (they) + OE auxiliary (would) + * not"
            ]
        );
        // each word still follows, the punctuation ignored, and the phrase is not counted in the totals
        let words: Vec<&str> = answer.rows.iter().filter(|r| r.mark == Mark::Entry && !r.word.is_empty()).map(|r| r.word.as_str()).collect();
        assert_eq!(words, ["They", "wouldn't"]);
        assert_eq!(answer.totals().strokes, 2);
        // the grammar's text is not also answered as a plain entry
        assert_eq!(answer.rows.iter().filter(|r| r.word.contains("wouldn't") && r.word.contains(' ')).count(), 1);
    }

    #[test]
    fn a_phrase_the_grammar_covers_only_at_its_start_says_how_much() {
        let answer = lookup().answer("she has been going", ALL);
        let first = &answer.rows[0];
        assert_eq!((first.word.as_str(), first.mark), ("she has been", Mark::Phrasing));
        assert_eq!(text(first.cells[0].as_ref()), "SKWH-FB");
        assert!(
            first.detail.iter().any(|l| l == "  the grammar covers the first 3 of 4 words (she has been); the rest (going) is per word below"),
            "{:?}",
            first.detail
        );
        assert!(answer.rows.iter().any(|r| r.word == "going" && r.mark == Mark::Entry));
    }

    #[test]
    fn a_spelt_out_not_and_a_contraction_read_as_the_grammars_text() {
        let lookup = lookup();
        let first = |query: &str| lookup.answer(query, ALL).rows.into_iter().next().unwrap();
        let spelt = first("I do not know");
        assert_eq!((spelt.word.as_str(), spelt.mark), ("I don't know", Mark::Phrasing));
        assert_eq!(text(spelt.cells[0].as_ref()), "SWR*PB");
        // all four words are covered: no "the rest" line
        assert!(spelt.detail.iter().all(|l| !l.contains("the rest")), "{:?}", spelt.detail);
        let contracted = first("I\u{2019}m going home");
        assert_eq!((contracted.word.as_str(), contracted.mark), ("I am", Mark::Phrasing));
        // a single word and a phrase the grammar does not write have none
        assert_eq!(first("cat").mark, Mark::Entry);
        assert!(lookup.answer("cats and dogs", ALL).rows.iter().all(|r| r.mark != Mark::Phrasing));
    }

    #[test]
    fn a_grammar_stroke_is_explained_part_by_part() {
        let answer = lookup().answer("SWRAB", ALL);
        let first = &answer.rows[0];
        assert_eq!((first.word.as_str(), first.mark), ("\"I can be\"", Mark::Forward));
        assert_eq!(first.detail[1], "    SWR starter (I) + A auxiliary (can) + -B verb (be)");
        // a stroke that is not the grammar's has no such lines
        assert!(lookup().answer("KAT", ALL).rows[0].detail.is_empty(), "KAT is not a grammar stroke");
    }

    #[test]
    fn nothing_typed_answers_nothing() {
        assert!(lookup().answer("   ", ALL).rows.is_empty(), "blank query answered");
    }
}

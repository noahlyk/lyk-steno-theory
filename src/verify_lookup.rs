//! `verify-lookup`: does every outline `lookup` lists type the word it was listed for?
//!
//! The lookup lists outlines from the catalogue (`out/lyk`), but keymux types from what `deploy`
//! ships: the dictionary files, with no backups, and the solver plugin for everything they miss.
//! This simulates keymux on every outline a lookup lists, in the order keymux asks:
//!
//! 1. the outline is an entry of the deployed files (the generated roles except the backups, the
//!    name lane's files and the reserved tables): it types that entry's text;
//! 2. otherwise the solver reads it, exactly as the plugin does (`Model::answer`, no word before
//!    the run, not the name lane).
//!
//! and compares what it types to the word the lookup listed it for. See
//! `site/reference/fallback.json` ("What lookup guarantees").

use crate::answer::{Ask, Cell, How, Lookup, Mark, Row};
use crate::analysis::Setup;
use crate::generate::{self, Entry, Origin};
use crate::output::Role;
use crate::rules::solve::Model;
use crate::stroke::{Stroke, parse_outline, render_outline};
use anyhow::{Context, Result, bail};
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::Path;

/// Every outline keymux holds after `deploy`, with the text it types (the entry's text).
pub struct Deployed(HashMap<Vec<Stroke>, String>);

impl Deployed {
    /// The files `deploy` ships from `dir` (`out/lyk`): every generated role but the backups, the
    /// name lane's two files, and the reserved tables.
    pub fn load(dir: &Path) -> Result<Self> {
        let mut map: HashMap<Vec<Stroke>, String> = HashMap::new();
        for reserved in generate::reserved_entries()? {
            map.insert(reserved.outline, reserved.text);
        }
        let files = Role::ALL
            .into_iter()
            .filter(|r| *r != Role::Backups)
            .map(Role::file_name)
            .chain([crate::names::FILE, crate::names::BORROWED_FILE]);
        for name in files {
            let path = dir.join(name);
            // the name lane's files only exist once `build` has written them
            let Ok(text) = std::fs::read_to_string(&path) else {
                if name == crate::names::FILE || name == crate::names::BORROWED_FILE {
                    continue;
                }
                bail!("cannot read {}", path.display());
            };
            let table: BTreeMap<String, String> =
                serde_json::from_str(&text).with_context(|| path.display().to_string())?;
            for (outline, text) in table {
                if let Some(strokes) = parse_outline(&outline) {
                    map.entry(strokes).or_insert(text);
                }
            }
        }
        Ok(Self(map))
    }

    #[must_use]
    pub fn get(&self, outline: &[Stroke]) -> Option<&str> {
        self.0.get(outline).map(String::as_str)
    }
}

/// What keymux types for an outline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Typed {
    Entry(String),
    Solver(String),
    Nothing,
}

/// Simulate keymux on `outline`: the deployed entry, else the solver.
pub fn types(deployed: &Deployed, model: &Model, scratch: &mut crate::rules::solve::core::Scratch, outline: &[Stroke]) -> Typed {
    if let Some(text) = deployed.get(outline) {
        return Typed::Entry(text.to_owned());
    }
    model.answer_with(scratch, &render_outline(outline)).map_or(Typed::Nothing, Typed::Solver)
}

/// How an outline of a row was listed, for the table: where it stood and what `How` said.
fn group_of(place: &str, cell: &Cell, deployed: &Deployed) -> String {
    let how = match &cell.how {
        How::Entry if deployed.get(&cell.strokes).is_some() => "entry",
        How::Entry => "backup listed as an entry",
        How::Solver => "solver-verified",
        How::Composed(_) => "composed",
        How::Reading => "reading",
        How::Spelled => "spelled",
    };
    format!("{place}: {how}")
}

/// What became of one listed outline.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Outcome {
    /// A deployed entry that types the word
    Entry,
    /// Not an entry; the solver reads the word
    SolverRight,
    /// A deployed entry for a different text (it shadows the listing)
    EntryWrong,
    /// Not an entry; the solver reads something else
    SolverWrong,
    /// Not an entry and the solver reads nothing
    Miss,
}

impl Outcome {
    pub const fn is_mismatch(&self) -> bool {
        matches!(self, Self::EntryWrong | Self::SolverWrong | Self::Miss)
    }
}

/// One listed outline that does not type its word.
#[derive(Debug, Clone)]
pub struct Sample {
    pub word: String,
    pub outline: String,
    pub typed: String,
}

const SAMPLE_CAP: usize = 4;

/// The counts of a run, by listing group and outcome; the causes of the solver's wrong readings;
/// a few samples of each mismatch.
#[derive(Debug, Default, Clone)]
pub struct Tally {
    pub counts: BTreeMap<(String, Outcome), usize>,
    pub causes: BTreeMap<(String, &'static str), usize>,
    pub samples: BTreeMap<(String, &'static str), Vec<Sample>>,
    pub queries: usize,
    pub unlisted: usize,
    /// Words whose best outline is spelled letter by letter although they have a pronunciation
    /// (no entry and no backup the solver reads right)
    pub spelled_though_pronounced: usize,
}

impl Tally {
    fn merge(&mut self, other: Self) {
        for (key, n) in other.counts {
            *self.counts.entry(key).or_default() += n;
        }
        for (key, n) in other.causes {
            *self.causes.entry(key).or_default() += n;
        }
        for (key, mut found) in other.samples {
            let held = self.samples.entry(key).or_default();
            found.truncate(SAMPLE_CAP.saturating_sub(held.len()));
            held.extend(found);
        }
        self.queries += other.queries;
        self.unlisted += other.unlisted;
        self.spelled_though_pronounced += other.spelled_though_pronounced;
    }

    #[must_use]
    pub fn total(&self) -> usize {
        self.counts.values().sum()
    }

    #[must_use]
    pub fn mismatches(&self) -> usize {
        self.counts.iter().filter(|((_, o), _)| o.is_mismatch()).map(|(_, n)| n).sum()
    }

    /// The table, causes and samples as text.
    #[must_use]
    pub fn report(&self) -> String {
        let mut out = String::new();
        let groups: Vec<&String> = {
            let mut g: Vec<&String> = self.counts.keys().map(|(g, _)| g).collect();
            g.dedup();
            g
        };
        let _ = writeln!(out, "{} queries, {} listed outlines, {} not typed as listed ({:.2}%)", self.queries, self.total(), self.mismatches(), pct(self.mismatches(), self.total()));
        let _ = writeln!(out, "{:<34} {:>9} {:>9} {:>9} {:>9} {:>9} {:>9}", "listed as", "outlines", "entry", "solver ok", "wrong", "no read", "entry!=");
        for group in groups {
            let n = |o: Outcome| self.counts.get(&(group.clone(), o)).copied().unwrap_or(0);
            let total: usize = Outcome::ALL.iter().map(|o| n(o.clone())).sum();
            let _ = writeln!(
                out,
                "{group:<34} {total:>9} {:>9} {:>9} {:>9} {:>9} {:>9}",
                n(Outcome::Entry),
                n(Outcome::SolverRight),
                n(Outcome::SolverWrong),
                n(Outcome::Miss),
                n(Outcome::EntryWrong)
            );
        }
        let _ = writeln!(out, "{} words are spelled letter by letter although they have a pronunciation (no entry, no backup the solver reads right)", self.spelled_though_pronounced);
        if !self.causes.is_empty() {
            let _ = writeln!(out, "\ncauses of wrong readings:");
            let mut causes: Vec<(&(String, &str), &usize)> = self.causes.iter().collect();
            causes.sort_by(|a, b| b.1.cmp(a.1));
            for ((group, cause), n) in causes {
                let _ = writeln!(out, "  {n:>8}  {group}: {cause}");
            }
        }
        for ((group, cause), found) in &self.samples {
            let _ = writeln!(out, "\n{group} / {cause}:");
            for s in found {
                let _ = writeln!(out, "  {:<22} {:<34} types {:?}", s.word, crate::notation::shorten(&s.outline), s.typed);
            }
        }
        out
    }
}

impl Outcome {
    const ALL: [Self; 5] = [Self::Entry, Self::SolverRight, Self::SolverWrong, Self::Miss, Self::EntryWrong];
}

fn pct(n: usize, of: usize) -> f64 {
    if of == 0 { 0.0 } else { 100.0 * f64::from(u32::try_from(n).unwrap_or(u32::MAX)) / f64::from(u32::try_from(of).unwrap_or(u32::MAX)) }
}

/// Why the solver read `typed` for an outline listed for `expected`.
fn cause(setup: &Setup, expected: &str, typed: &str) -> &'static str {
    let (e, t) = (expected.to_lowercase(), typed.to_lowercase());
    let word = |w: &str| setup.rank.contains_key(w) || setup.pron.contains_key(w);
    let rank = |w: &str| setup.rank.get(w).copied().unwrap_or(usize::MAX);
    if !word(&t) && !t.contains(' ') && !t.contains('\'') {
        return if t.chars().all(|c| c.is_ascii_lowercase()) { "types a non-word (pieces or read-back)" } else { "types punctuation or marks" };
    }
    if let (Some(a), Some(b)) = (setup.pron.get(e.as_str()), setup.pron.get(t.as_str()))
        && a.iter().any(|p| b.contains(p))
    {
        return "homophone";
    }
    if (t.starts_with(&e) || e.starts_with(&t) || t.ends_with(&e) || e.ends_with(&t)) && e.len().abs_diff(t.len()) <= 4 {
        return if rank(&t) < rank(&e) { "inflection neighbour, commoner word" } else { "inflection neighbour, rarer word" };
    }
    if rank(&t) < rank(&e) {
        "a different, commoner word"
    } else {
        "a different, rarer word"
    }
}

/// Check one query: every outline of lyk's column against what keymux would type.
fn check_query(
    lookup: &Lookup<'_>,
    setup: &Setup,
    deployed: &Deployed,
    model: &Model,
    scratch: &mut crate::rules::solve::core::Scratch,
    query: &str,
    tally: &mut Tally,
) {
    tally.queries += 1;
    let answer = lookup.answer(query, Ask { fast: false, stroke_mode: false, extra: 4 });
    let mut word = String::new();
    let mut best_seen = false;
    for Row { word: row_word, cells, mark, .. } in &answer.rows {
        if !matches!(mark, Mark::Entry | Mark::Missing | Mark::Completion) {
            continue;
        }
        if !row_word.is_empty() {
            word.clone_from(row_word);
            best_seen = false;
        }
        let Some(cell) = &cells[0] else {
            tally.unlisted += 1;
            continue;
        };
        let is_best = !best_seen;
        best_seen = true;
        let place = if *mark == Mark::Completion { group_of("completion", cell, deployed) } else { group_of(if is_best { "best" } else { "other" }, cell, deployed) };
        let typed = types(deployed, model, scratch, &cell.strokes);
        let same = |text: &str| text.to_lowercase() == word.to_lowercase();
        let (outcome, shown) = match &typed {
            Typed::Entry(text) if same(text) => (Outcome::Entry, String::new()),
            Typed::Entry(text) => (Outcome::EntryWrong, text.clone()),
            Typed::Solver(text) if same(text) => (Outcome::SolverRight, String::new()),
            Typed::Solver(text) => (Outcome::SolverWrong, text.clone()),
            Typed::Nothing => (Outcome::Miss, "(nothing)".to_owned()),
        };
        let kind = match outcome {
            Outcome::SolverWrong => Some(cause(setup, &word, &shown)),
            Outcome::EntryWrong => Some("shadowed by an entry of another word"),
            Outcome::Miss => Some("solver reads nothing"),
            _ => None,
        };
        if is_best && cell.how == How::Spelled && setup.pron.contains_key(word.to_lowercase().as_str()) {
            tally.spelled_though_pronounced += 1;
        }
        *tally.counts.entry((place.clone(), outcome)).or_default() += 1;
        if let Some(kind) = kind {
            *tally.causes.entry((place.clone(), kind)).or_default() += 1;
            let held = tally.samples.entry((place, kind)).or_default();
            if held.len() < SAMPLE_CAP {
                held.push(Sample { word: word.clone(), outline: render_outline(&cell.strokes), typed: shown });
            }
        }
    }
}

/// A small deterministic generator, so a sample is the same every run for a seed.
struct Rng(u64);

impl Rng {
    const fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// How much of the vocabulary to check.
#[derive(Debug, Clone, Copy)]
pub struct Sampling {
    /// The commonest words, all of them
    pub common: usize,
    /// Random words beyond them (`usize::MAX`: all)
    pub rest: usize,
    /// Phrase entries (`usize::MAX`: all)
    pub phrases: usize,
    /// Names (`usize::MAX`: all)
    pub names: usize,
    pub seed: u64,
}

impl Sampling {
    /// The fast sample (a minute or so on a dozen cores, run by `cargo test` at a smaller size).
    pub const FAST: Self = Self { common: 400, rest: 300, phrases: 60, names: 60, seed: 1 };
    /// Every word of the dictionary, every phrase, every name.
    pub const FULL: Self = Self { common: usize::MAX, rest: usize::MAX, phrases: usize::MAX, names: usize::MAX, seed: 1 };
}

/// The queries of a sampling: common words by rank, a seeded random pick of the rest, phrases, names.
pub fn queries(setup: &Setup, entries: &[Entry], words: &[&str], sampling: Sampling) -> Vec<String> {
    let mut ranked: Vec<&str> = words.to_vec();
    ranked.sort_unstable_by_key(|w| (setup.rank.get(*w).copied().unwrap_or(usize::MAX), *w));
    let common = sampling.common.min(ranked.len());
    let mut out: Vec<String> = ranked[..common].iter().map(|w| (*w).to_owned()).collect();
    let mut rng = Rng(sampling.seed.max(1) ^ 0x9E37_79B9_7F4A_7C15);
    let pick = |pool: &[&str], n: usize, rng: &mut Rng| -> Vec<String> {
        if n >= pool.len() {
            return pool.iter().map(|w| (*w).to_owned()).collect();
        }
        let mut pool = pool.to_vec();
        for i in 0..n {
            let j = i + usize::try_from(rng.next() % (pool.len() - i) as u64).unwrap_or(0);
            pool.swap(i, j);
        }
        pool[..n].iter().map(|w| (*w).to_owned()).collect()
    };
    out.extend(pick(&ranked[common..], sampling.rest, &mut rng));
    let mut phrases: Vec<&str> =
        entries.iter().filter(|e| e.origin == Origin::Phrase).map(|e| e.word.as_str()).collect();
    phrases.sort_unstable();
    phrases.dedup();
    out.extend(pick(&phrases, sampling.phrases, &mut rng));
    let names: Vec<String> = crate::names::collect(setup).into_iter().map(|n| n.key).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    out.extend(pick(&names, sampling.names, &mut rng));
    out
}

/// Check every query, across the machine's cores.
pub fn check(lookup: &Lookup<'_>, setup: &Setup, deployed: &Deployed, model: &Model, queries: &[String]) -> Tally {
    let threads = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
    let chunk = queries.len().div_ceil(threads).max(1);
    let parts: Vec<Tally> = std::thread::scope(|scope| {
        #[allow(clippy::needless_collect)]
        let handles: Vec<_> = queries
            .chunks(chunk)
            .map(|part| {
                scope.spawn(move || {
                    let mut scratch = model.scratch();
                    let mut tally = Tally::default();
                    for query in part {
                        check_query(lookup, setup, deployed, model, &mut scratch, query, &mut tally);
                    }
                    tally
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("a verify thread panicked")).collect()
    });
    let mut all = Tally::default();
    for part in parts {
        all.merge(part);
    }
    all
}

/// The `verify-lookup` command.
pub fn run(setup: &Setup, sources: (&Path, Option<&Path>), dir: &Path, common: usize, sampling: Sampling) -> Result<()> {
    let loaded = crate::lookup_cache::load(sources.0, sources.1, common, dir, &crate::paths::cache_dir())?;
    let blob = std::fs::read(dir.join(crate::rules::solve::plugin::BLOB_FILE))
        .with_context(|| format!("the solver in {} (run build)", dir.display()))?;
    let model = Model::load(&blob).context("the solver blob does not load")?;
    let for_lookup = Model::load(&blob).context("the solver blob does not load")?;
    let deployed = Deployed::load(dir)?;
    let lookup = Lookup::with_cheapest(&loaded.setup, &loaded.entries, loaded.backups.as_ref(), loaded.cheapest.as_deref());
    let lookup = lookup.with_model(for_lookup);
    let words: Vec<&str> = lookup.dictionary().keys().copied().collect();
    let queries = queries(setup, &loaded.entries, &words, sampling);
    let started = std::time::Instant::now();
    let tally = check(&lookup, setup, &deployed, &model, &queries);
    print!("{}", tally.report());
    println!("\n{} queries in {:.1?}", queries.len(), started.elapsed());
    if tally.mismatches() > 0 {
        bail!("{} listed outlines do not type their word", tally.mismatches());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictionary_fixture;
    use crate::rules::solve::{self, Lexicon};

    /// The solver `build` ships, from the full fixture dictionary: packed once, loaded per use.
    fn model() -> Model {
        static BLOB: std::sync::LazyLock<Vec<u8>> = std::sync::LazyLock::new(|| {
            let setup = dictionary_fixture::setup();
            let entries = &dictionary_fixture::generated().entries;
            let lexicon = Lexicon { pron: &setup.pron, rank: &setup.rank, spellings: &setup.spellings, bigrams: &setup.web_bigrams, common: 100_000 };
            solve::pack(&setup.rules, &lexicon, &solve::stems(entries), Some(&crate::names::collect(setup)))
        });
        Model::load(&BLOB).expect("the packed blob loads")
    }

    /// The deployed files of the fixture dictionary, as `build` writes them (written once).
    fn deployed() -> &'static Deployed {
        static DEPLOYED: std::sync::LazyLock<Deployed> = std::sync::LazyLock::new(|| {
            let dir = crate::test_dir::TestDir::new("lyk-verify-lookup");
            crate::output::write_dictionary(dictionary_fixture::generated(), &dir).expect("the dictionary is written");
            Deployed::load(&dir).expect("the deployed files load")
        });
        &DEPLOYED
    }

    /// The guarantee: every outline `lookup` lists for a word, phrase or name types exactly that
    /// text in keymux, through a deployed entry or the solver. A sample sized for a test run;
    /// `lyk verify-lookup --full` runs every word.
    #[test]
    fn every_outline_the_lookup_lists_types_its_word() {
        let setup = dictionary_fixture::setup();
        let entries = &dictionary_fixture::generated().entries;
        let lookup = Lookup::new(setup, entries, None).with_model(model());
        let words: Vec<&str> = lookup.dictionary().keys().copied().collect();
        let sampling = Sampling { common: 150, rest: 150, phrases: 30, names: 30, seed: 7 };
        let queries = queries(setup, entries, &words, sampling);
        let tally = check(&lookup, setup, deployed(), &model(), &queries);
        assert!(tally.total() > 1_000, "only {} outlines were checked", tally.total());
        assert!(tally.mismatches() == 0, "{}", tally.report());
    }

    /// The harness itself: an outline the dictionary lacks and the solver reads as another word
    /// is a mismatch, and a deployed entry is not (the numbers in `site/reference/fallback.json`
    /// come from this being able to see them).
    #[test]
    fn the_harness_sees_a_wrong_reading() {
        let entries = &dictionary_fixture::generated().entries;
        let model = model();
        let deployed = deployed();
        let mut scratch = model.scratch();
        let outline = |text: &str| parse_outline(text).expect("an outline");
        let cat = types(deployed, &model, &mut scratch, &outline("KAT"));
        assert_eq!(cat, Typed::Entry("cat".into()));
        // a backup the dictionary does not ship is the solver's to read
        let backup = entries.iter().find(|e| e.origin == Origin::Backup && e.outline.len() > 1).expect("a backup");
        assert!(matches!(types(deployed, &model, &mut scratch, &backup.outline), Typed::Solver(_)));
    }

    /// A backup is never a word's best outline while the word has an entry (keymux does not load
    /// backups: `bit` once listed the backup `TPW` over its entry, and typing it gave "to").
    #[test]
    fn a_backup_never_outranks_an_entry() {
        let entries = &dictionary_fixture::generated().entries;
        let best = generate::cheapest_deployed(entries);
        let has_entry: std::collections::HashSet<&str> =
            entries.iter().filter(|e| !matches!(e.origin, Origin::Backup | Origin::Suffix)).map(|e| e.word.as_str()).collect();
        let backups: std::collections::HashSet<&[Stroke]> =
            entries.iter().filter(|e| e.origin == Origin::Backup).map(|e| e.outline.as_slice()).collect();
        let wrong: Vec<&str> =
            best.iter().filter(|(w, o)| backups.contains(**o) && has_entry.contains(**w)).map(|(w, _)| *w).collect();
        assert!(wrong.is_empty(), "{} words rank a backup over an entry, e.g. {:?}", wrong.len(), &wrong[..wrong.len().min(8)]);
    }
}

//! `solve`: the solver's own command. With an outline, every reading of it, cheapest first; with
//! `--eval`, how well it recovers the words of the dictionary's own backups (outline -> word, a
//! labelled test set millions strong) and of the tape's misses.

use crate::analysis::Setup;
use crate::num::ToF64;
use crate::rules::solve::{self, Lexicon, Model};
use crate::stroke::{Stroke, parse_outline, render_outline};
use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::Path;
use std::time::{Duration, Instant};

pub struct Options<'a> {
    pub outline: Option<&'a str>,
    pub eval: Option<usize>,
    pub tape: Option<&'a Path>,
    pub misses: Option<&'a Path>,
    pub dir: &'a Path,
    pub common: usize,
    /// Write the plugin (source and blob) here and stop
    pub plugin: Option<&'a Path>,
    /// Print `outline<TAB>answer` for every outline in this file (one a line) and stop
    pub answers: Option<&'a Path>,
}

/// The model `build` would ship, from the dictionary already in `dir`.
pub fn model(setup: &Setup, dir: &Path, common: usize) -> Result<(Model, usize)> {
    let entries = crate::built::load(dir).with_context(|| format!("reading the dictionary in {}", dir.display()))?;
    let stems = solve::stems(&entries);
    let lexicon = Lexicon { pron: &setup.pron, rank: &setup.rank, common };
    let blob = solve::pack(&setup.rules, &lexicon, &stems);
    let size = blob.len();
    Ok((Model::load(&blob).context("the packed blob does not load")?, size))
}

pub fn run(setup: &Setup, options: &Options) -> Result<()> {
    if let Some(to) = options.plugin {
        let entries = crate::built::load(options.dir)?;
        let lexicon = Lexicon { pron: &setup.pron, rank: &setup.rank, common: options.common };
        std::fs::create_dir_all(to)?;
        let size = solve::write_plugin(&setup.rules, &lexicon, &entries, to)?;
        println!("wrote {} and {} ({:.1} MB) to {}", solve::plugin::SOURCE_FILE, solve::plugin::BLOB_FILE, size.to_f64() / 1e6, to.display());
        return Ok(());
    }
    let started = Instant::now();
    let (model, size) = model(setup, options.dir, options.common)?;
    println!(
        "model: {} words, {} trie nodes, {} edges, {:.1} MB packed, built in {:.1?}",
        model.word_count(),
        model.node_count(),
        model.edge_count(),
        size.to_f64() / 1e6,
        started.elapsed()
    );
    if let Some(path) = options.answers {
        let text = std::fs::read_to_string(path).with_context(|| path.display().to_string())?;
        for outline in text.lines().filter(|l| !l.is_empty()) {
            println!("{outline}\t{}", model.answer(outline).unwrap_or_else(|| "-".into()));
        }
        return Ok(());
    }
    if let Some(text) = options.outline {
        let run: Vec<u32> = parse_outline(text).context("not a stroke outline")?.iter().map(|s| s.0).collect();
        if std::env::var_os("LYK_SOLVE_DEBUG").is_some() {
            for &stroke in &run {
                println!("{}", model.describe(stroke).join(" | "));
            }
        }
        let t = Instant::now();
        let readings = model.solve(&run, 8);
        let took = t.elapsed();
        for r in &readings {
            println!("{:>4}  {:<24} {}", r.cost, r.text, r.pieces.join(" + "));
        }
        println!("answer: {:?}  ({took:.1?})", model.answer(text));
        return Ok(());
    }
    if let Some(step) = options.eval {
        eval_backups(&model, options.dir, step, options.misses, &setup.rank)?;
    }
    if let Some(tape) = options.tape {
        eval_tape(&model, tape)?;
    }
    Ok(())
}

#[derive(Default, Clone, Copy)]
struct Tally {
    n: usize,
    top1: usize,
    top5: usize,
}

impl Tally {
    fn add(&mut self, top1: bool, top5: bool) {
        self.n += 1;
        self.top1 += usize::from(top1);
        self.top5 += usize::from(top5);
    }
    fn line(&self) -> String {
        format!("{:>7}  top1 {:>5.1}%  top5 {:>5.1}%", self.n, pct(self.top1, self.n), pct(self.top5, self.n))
    }
}

fn pct(a: usize, b: usize) -> f64 {
    100.0 * a.to_f64() / b.max(1).to_f64()
}

/// Every `step`th backup: does the solver read its outline as its word?
fn eval_backups(model: &Model, dir: &Path, step: usize, misses: Option<&Path>, rank: &HashMap<String, usize>) -> Result<()> {
    let path = dir.join("02-lyk-backups.json");
    let text = std::fs::read_to_string(&path).with_context(|| path.display().to_string())?;
    let rows: Vec<(&str, String)> = text
        .lines()
        .filter_map(|l| {
            let l = l.trim().trim_end_matches(',').strip_prefix('"')?;
            let at = l.find("\": \"")?;
            Some((&l[..at], l[at + 4..].strip_suffix('"')?.to_lowercase()))
        })
        .collect();
    let mut all = Tally::default();
    let mut by_len: Vec<Tally> = vec![Tally::default(); 9];
    let bands = [2_000, 10_000, 30_000, usize::MAX];
    let mut by_rank = [Tally::default(); 4];
    let mut times: Vec<Duration> = Vec::new();
    let mut fell_back = 0usize;
    let mut answered_right = 0usize;
    let mut wrong: Vec<String> = Vec::new();
    let mut weighted = (0.0f64, 0.0f64);
    // A backup whose outline another file also holds is shadowed: keymux types that file's word
    let shadowed: HashSet<String> = crate::built::load(dir)?
        .iter()
        .filter(|e| e.origin != crate::generate::Origin::Backup)
        .map(|e| render_outline(&e.outline))
        .collect();
    let mut skipped = 0usize;
    let sample: Vec<(&str, &String, Vec<u32>)> = rows
        .iter()
        .step_by(step.max(1))
        .filter_map(|(outline, word)| {
            let strokes = parse_outline(outline)?;
            if shadowed.contains(*outline) {
                skipped += 1;
                return None;
            }
            Some((*outline, word, strokes.iter().map(|s: &Stroke| s.0).collect()))
        })
        .collect();
    let solved = solve_all(model, &sample);
    for (&(outline, word, ref run), (took, got, answer)) in sample.iter().zip(solved) {
        times.push(took);
        let top1 = got.first().is_some_and(|r| r.text.to_lowercase() == *word);
        let top5 = got.iter().any(|r| r.text.to_lowercase() == *word);
        all.add(top1, top5);
        by_len[run.len().min(8)].add(top1, top5);
        let r = rank.get(word).copied().unwrap_or(usize::MAX);
        let weight = 1.0 / (r.min(1_000_000).to_f64() + 100.0);
        weighted.0 += weight;
        if top1 {
            weighted.1 += weight;
        }
        by_rank[bands.iter().position(|&b| r < b).unwrap_or(3)].add(top1, top5);
        if answer.as_deref().map(str::to_lowercase).as_deref() == Some(word.as_str()) {
            answered_right += 1;
        } else if got.first().is_none_or(|b| answer.as_deref() != Some(b.text.as_str())) {
            fell_back += 1;
        }
        if !top1 {
            wrong.push(miss_line(outline, word, &got));
        }
    }
    times.sort();
    let at = |percent: usize| times.get(times.len().saturating_sub(1) * percent / 100).copied().unwrap_or_default();
    let total: Duration = times.iter().sum();
    println!("backups, every {step}th:     {}   ({skipped} shadowed by another file, skipped)", all.line());
    println!("  weighted by use (1/rank):  top1 {:>5.1}%", 100.0 * weighted.1 / weighted.0.max(1e-12));
    for (len, t) in by_len.iter().enumerate().filter(|(_, t)| t.n > 0) {
        println!("  {len} strokes:            {}", t.line());
    }
    for (band, t) in ["rank < 2k", "rank < 10k", "rank < 30k", "rarer"].iter().zip(by_rank) {
        println!("  {band:<20} {}", t.line());
    }
    println!(
        "answer() exactly right {:.1}%, fell back to the read-back {:.1}%",
        pct(answered_right, all.n),
        pct(fell_back, all.n)
    );
    println!(
        "time per run: mean {:.0?}  p50 {:.0?}  p99 {:.0?}  max {:.0?}",
        total / u32::try_from(times.len().max(1)).unwrap_or(u32::MAX),
        at(50),
        at(99),
        times.last().copied().unwrap_or_default()
    );
    for line in wrong.iter().take(30) {
        println!("  miss  {line}");
    }
    if let Some(path) = misses {
        std::fs::write(path, wrong.join("\n") + "\n").with_context(|| path.display().to_string())?;
        println!("{} misses written to {}", wrong.len(), path.display());
    }
    Ok(())
}

/// Every run of `sample` solved (its five best readings, the plugin's answer, the time the
/// readings took), on every core, each thread with its own working memory.
/// One line of the misses file: the outline, its word, what the solver said and at what cost and
/// from which pieces, then where the word itself came and at what cost (`2:7`), or `-` past the
/// top five: a near tie, or far behind.
fn miss_line(outline: &str, word: &str, got: &[solve::Reading]) -> String {
    let said = got.first().map_or("-", |r| r.text.as_str());
    let mut line = format!("{outline}\t{word}\t{said}");
    if let Some(best) = got.first() {
        let _ = write!(line, "\t{}\t{}", best.cost, best.pieces.join("+"));
    }
    match got.iter().position(|r| r.text.to_lowercase() == word) {
        Some(at) => {
            let _ = write!(line, "\t{}:{}", at + 1, got[at].cost);
        }
        None => line.push_str("\t-"),
    }
    line
}

fn solve_all(model: &Model, sample: &[(&str, &String, Vec<u32>)]) -> Vec<(Duration, Vec<solve::Reading>, Option<String>)> {
    let threads = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
    let chunk = sample.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        // Every thread is spawned before any is joined: joining lazily would run them one at a time
        #[allow(clippy::needless_collect)]
        let handles: Vec<_> = sample
            .chunks(chunk)
            .map(|part| {
                scope.spawn(move || {
                    let mut sc = model.scratch();
                    part.iter()
                        .map(|(outline, _, run)| {
                            let t = Instant::now();
                            let got = model.solve_with(&mut sc, run, 5);
                            let took = t.elapsed();
                            (took, got, model.answer_with(&mut sc, outline))
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().expect("an eval thread panicked")).collect()
    })
}

/// The tape's misses (`?` lines), each against what was struck next once the miss was undone: the
/// first later stroke that typed a word (a weak label: usually the correction). A miss one key
/// from that stroke was a misstroke, not a word the dictionary lacked.
fn eval_tape(model: &Model, tape: &Path) -> Result<()> {
    let text = std::fs::read_to_string(tape).with_context(|| tape.display().to_string())?;
    let lines: Vec<&str> = text.lines().collect();
    let (mut n, mut labelled, mut right) = (0usize, 0usize, 0usize);
    let (mut slips, mut slips_right) = (0usize, 0usize);
    let mut shown = 0;
    for (i, line) in lines.iter().enumerate() {
        let mut parts = line.split_whitespace();
        let (Some(_), Some(stroke), Some("?")) = (parts.next(), parts.next(), parts.next()) else { continue };
        n += 1;
        // The next stroke within a few lines that typed a word, past undos and repeated misses
        let Some((next_stroke, word)) = lines[i + 1..lines.len().min(i + 8)].iter().find_map(|l| {
            let mut p = l.split_whitespace();
            let (_, s) = (p.next()?, p.next()?);
            let out = p.collect::<Vec<_>>().join(" ");
            let word = out.strip_prefix('"')?.strip_suffix('"')?.trim().to_lowercase();
            (s != "*" && !word.is_empty() && word.chars().all(|c| c.is_alphabetic() || c == '\'')).then(|| (s.to_owned(), word))
        }) else {
            continue;
        };
        labelled += 1;
        let answer = model.answer(stroke).unwrap_or_else(|| "-".into()).to_lowercase();
        let ok = answer == word;
        right += usize::from(ok);
        let one_key = match (Stroke::parse(stroke), Stroke::parse(&next_stroke)) {
            (Some(a), Some(b)) => (a.0 ^ b.0).is_power_of_two(),
            _ => false,
        };
        if one_key {
            slips += 1;
            slips_right += usize::from(ok);
        }
        if shown < 60 && !ok {
            shown += 1;
            let note = if one_key { "  (one key off)" } else { "" };
            println!("  {stroke:<12} -> {answer:<16} then {next_stroke} {word:?}{note}");
        }
    }
    println!(
        "{n} misses on the tape, {labelled} with a word struck next: the solver types that word for {right} ({:.1}%); \
         {slips} were one key from it ({slips_right} right), {} were not ({} right)",
        pct(right, labelled),
        labelled - slips,
        right - slips_right
    );
    Ok(())
}

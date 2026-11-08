//! `solve`: the solver's own command. With an outline, every reading of it, cheapest first; with
//! `--eval`, how well it recovers the words of the dictionary's own backups (outline -> word, a
//! labelled test set millions strong) and of the tape's misses.

use crate::analysis::Setup;
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
        println!("wrote {} and {} ({:.1} MB) to {}", solve::plugin::SOURCE_FILE, solve::plugin::BLOB_FILE, size as f64 / 1e6, to.display());
        return Ok(());
    }
    let started = Instant::now();
    let (model, size) = model(setup, options.dir, options.common)?;
    println!(
        "model: {} words, {} trie nodes, {} edges, {:.1} MB packed, built in {:.1?}",
        model.word_count(),
        model.node_count(),
        model.edge_count(),
        size as f64 / 1e6,
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
    100.0 * a as f64 / b.max(1) as f64
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
    for (outline, word) in rows.iter().step_by(step.max(1)) {
        let Some(strokes) = parse_outline(outline) else { continue };
        if shadowed.contains(*outline) {
            skipped += 1;
            continue;
        }
        let run: Vec<u32> = strokes.iter().map(|s: &Stroke| s.0).collect();
        let t = Instant::now();
        let got = model.solve(&run, 5);
        times.push(t.elapsed());
        let top1 = got.first().is_some_and(|r| r.text.to_lowercase() == *word);
        let top5 = got.iter().any(|r| r.text.to_lowercase() == *word);
        all.add(top1, top5);
        by_len[run.len().min(8)].add(top1, top5);
        let r = rank.get(word).copied().unwrap_or(usize::MAX);
        let weight = 1.0 / (r.min(1_000_000) as f64 + 100.0);
        weighted.0 += weight;
        if top1 {
            weighted.1 += weight;
        }
        by_rank[bands.iter().position(|&b| r < b).unwrap_or(3)].add(top1, top5);
        let answer = model.answer(outline);
        if answer.as_deref().map(str::to_lowercase).as_deref() == Some(word.as_str()) {
            answered_right += 1;
        } else if got.first().is_none_or(|b| answer.as_deref() != Some(b.text.as_str())) {
            fell_back += 1;
        }
        if !top1 {
            let said = got.first().map_or("-", |r| r.text.as_str());
            let mut line = format!("{outline}\t{word}\t{said}");
            if let Some(best) = got.first() {
                let _ = write!(line, "\t{}\t{}", best.cost, best.pieces.join("+"));
            }
            wrong.push(line);
        }
    }
    times.sort();
    let at = |q: f64| times.get(((times.len() as f64 - 1.0) * q) as usize).copied().unwrap_or_default();
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
        total / times.len().max(1) as u32,
        at(0.5),
        at(0.99),
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

/// The tape's misses (`?` lines), each beside the next stroke struck (usually its correction).
fn eval_tape(model: &Model, tape: &Path) -> Result<()> {
    let text = std::fs::read_to_string(tape).with_context(|| tape.display().to_string())?;
    let lines: Vec<&str> = text.lines().collect();
    let mut shown = 0;
    let mut n = 0;
    for (i, line) in lines.iter().enumerate() {
        let mut parts = line.split_whitespace();
        let (Some(_), Some(stroke), Some(out)) = (parts.next(), parts.next(), parts.next()) else { continue };
        if out != "?" {
            continue;
        }
        n += 1;
        if shown >= 200 {
            continue;
        }
        let then = lines[i + 1..]
            .iter()
            .filter_map(|l| {
                let mut p = l.split_whitespace();
                let (_, s, rest) = (p.next()?, p.next()?, p.collect::<Vec<_>>().join(" "));
                (s != "*").then(|| format!("{s} {rest}"))
            })
            .next()
            .unwrap_or_default();
        let answer = model.answer(stroke).unwrap_or_else(|| "-".into());
        println!("  {stroke:<12} -> {answer:<16} then {then}");
        shown += 1;
    }
    println!("{n} misses on the tape");
    Ok(())
}

/// The thinned backups file `deploy` ships: every backup of the built dictionary that the solver
/// does not already read as its word (exactly, case and all), so keymux types every outline it
/// typed before, with the backups the solver gets right left to it. Cached beside the dictionary
/// under a hash of the solver's blob and the full backups file, since reading millions of
/// outlines takes minutes even on every core. Not named `02-lyk-*`, so `build` leaves it be.
pub const THIN_FILE: &str = "backups-thin.json";
const THIN_STAMP: &str = "backups-thin.stamp";

pub struct Thinned {
    pub path: std::path::PathBuf,
    pub kept: usize,
    pub dropped: usize,
}

pub fn thin_backups(dir: &Path) -> Result<Thinned> {
    use std::hash::{Hash, Hasher};
    let blob_path = dir.join(solve::plugin::BLOB_FILE);
    let blob = std::fs::read(&blob_path).with_context(|| blob_path.display().to_string())?;
    let full_path = dir.join("02-lyk-backups.json");
    let full = std::fs::read_to_string(&full_path).with_context(|| full_path.display().to_string())?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    blob.hash(&mut hasher);
    full.hash(&mut hasher);
    let stamp = format!("{:016x}", hasher.finish());
    let path = dir.join(THIN_FILE);
    let stamp_path = dir.join(THIN_STAMP);
    if let (Ok(old), Ok(thin)) = (std::fs::read_to_string(&stamp_path), std::fs::read_to_string(&path)) {
        let mut parts = old.split_whitespace();
        if parts.next() == Some(stamp.as_str()) {
            let dropped = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
            return Ok(Thinned { path, kept: thin.lines().count().saturating_sub(2), dropped });
        }
    }
    let model = Model::load(&blob).context("the solver's blob does not load")?;
    // Each line as written, its outline, and its word
    let rows: Vec<(&str, &str, String)> = full
        .lines()
        .filter_map(|line| {
            let l = line.trim().trim_end_matches(',').strip_prefix('"')?;
            let at = l.find("\": \"")?;
            let raw = l[at + 4..].strip_suffix('"')?;
            let word = if raw.contains('\\') { serde_json::from_str::<String>(&format!("\"{raw}\"")).ok()? } else { raw.to_owned() };
            Some((line.trim().trim_end_matches(','), &l[..at], word))
        })
        .collect();
    let threads = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
    let chunk = rows.len().div_ceil(threads).max(1);
    let started = Instant::now();
    let keep: Vec<bool> = std::thread::scope(|scope| {
        let handles: Vec<_> = rows
            .chunks(chunk)
            .map(|part| {
                let model = &model;
                scope.spawn(move || {
                    let mut sc = model.scratch();
                    part.iter()
                        .map(|(_, outline, word)| model.answer_with(&mut sc, outline).as_deref() != Some(word.as_str()))
                        .collect::<Vec<bool>>()
                })
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().expect("a thinning thread panicked")).collect()
    });
    let kept: Vec<&str> = rows.iter().zip(&keep).filter(|(_, k)| **k).map(|(r, _)| r.0).collect();
    let dropped = rows.len() - kept.len();
    let mut out = String::from("{\n");
    for (i, line) in kept.iter().enumerate() {
        out.push_str("  ");
        out.push_str(line);
        out.push_str(if i + 1 < kept.len() { ",\n" } else { "\n" });
    }
    out.push_str("}\n");
    std::fs::write(&path, out).with_context(|| path.display().to_string())?;
    std::fs::write(&stamp_path, format!("{stamp} {dropped}\n")).with_context(|| stamp_path.display().to_string())?;
    println!(
        "thinned the backups in {:.0?}: {} of {} kept (the solver reads the other {dropped} as their word)",
        started.elapsed(),
        kept.len(),
        rows.len()
    );
    Ok(Thinned { path, kept: kept.len(), dropped })
}

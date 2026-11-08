//! `solve`: the solver's own command. With an outline, every reading of it, cheapest first; with
//! `--eval`, how well it recovers the words of the dictionary's own backups (outline -> word, a
//! labelled test set millions strong) and of the tape's misses.

use crate::analysis::Setup;
use crate::rules::solve::{self, Lexicon, Model};
use crate::stroke::{Stroke, parse_outline};
use anyhow::{Context, Result};
use std::collections::HashMap;
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
    for (outline, word) in rows.iter().step_by(step.max(1)) {
        let Some(strokes) = parse_outline(outline) else { continue };
        let run: Vec<u32> = strokes.iter().map(|s: &Stroke| s.0).collect();
        let t = Instant::now();
        let got = model.solve(&run, 5);
        times.push(t.elapsed());
        let top1 = got.first().is_some_and(|r| r.text.to_lowercase() == *word);
        let top5 = got.iter().any(|r| r.text.to_lowercase() == *word);
        all.add(top1, top5);
        by_len[run.len().min(8)].add(top1, top5);
        let r = rank.get(word).copied().unwrap_or(usize::MAX);
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
    println!("backups, every {step}th:     {}", all.line());
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

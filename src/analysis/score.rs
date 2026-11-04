//! `score` and `score --check-readme`: how often the dictionary writes a word the way Plover or
//! Lapwing do, and how many keys a stroke takes, by syllable count; the README's tables are
//! rendered from the same tally and kept in sync between their markers.

use super::Setup;
use crate::generate;
use crate::num::ToF64;
use anyhow::{Context, Result};
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::Path;

/// Strokes and keys summed over some words.
#[derive(Default, Clone, Copy)]
struct Sum {
    strokes: f64,
    keys: f64,
}

impl Sum {
    fn add(&mut self, (strokes, keys): (usize, u32), weight: f64) {
        self.strokes = weight.mul_add(strokes.to_f64(), self.strokes);
        self.keys = weight.mul_add(f64::from(keys), self.keys);
    }

    fn per_stroke(self) -> f64 {
        self.keys / self.strokes.max(1.0)
    }
}

/// The theories compared, in table order.
const LYK: usize = 0;
const PLOVER: usize = 1;
const LAPWING: usize = 2;

/// One row of the tally: a syllable count, or every word.
#[derive(Default, Clone, Copy)]
struct Tally {
    words: usize,
    written: usize,
    plover: usize,
    lapwing: usize,
    either: usize,
    /// Each theory's strokes and keys, every word counted once
    plain: [Sum; 3],
    /// The same, each word weighted by how often it is said (`Setup::count`): closer to what
    /// typing each theory feels like than the per-word average
    weighted: [Sum; 3],
    /// How often this row's words are said, in all
    usage_weight: f64,
}

/// The rows: one per syllable count (4 and up together, "4+"), then "all".
type Rows = Vec<(String, Tally)>;

/// `score`'s and `check_readme`'s shared tally, over the outline `build` would write for each
/// word (the generated dictionary's cheapest), not the bare rules.
fn tallies(setup: &Setup, briefs: &generate::Briefs, common: usize) -> Result<Rows> {
    let generated = generate::generate(setup, briefs, common)?;
    let ours: HashMap<&str, (usize, u32)> = generate::cheapest(&generated.entries)
        .into_iter()
        .map(|(word, outline)| (word, (outline.len(), generate::keys(outline))))
        .collect();
    let gold_cost = |outlines: Option<&Vec<String>>| {
        let outline = generate::shortest(outlines?)?;
        Some((outline.len(), generate::keys(&outline)))
    };
    let mut by_size: BTreeMap<usize, Tally> = BTreeMap::new();
    let mut total = Tally::default();
    for word in setup.words().into_iter().filter(|w| setup.rank.get(*w).copied().unwrap_or(usize::MAX) < common) {
        let candidates = setup.candidates(word);
        let (plover, lapwing) =
            candidates.iter().map(|o| setup.gold.writes(word, o)).fold((false, false), |(p, l), (a, b)| (p || a, l || b));
        let ours = ours.get(word).copied();
        let costs = [
            ours,
            ours.and_then(|_| gold_cost(setup.gold.plover.get(word))),
            ours.and_then(|_| gold_cost(setup.gold.lapwing.get(word))),
        ];
        let weight = setup.count.get(word).copied().unwrap_or(0).to_f64();
        for tally in [by_size.entry(setup.syllable_count(word).min(4)).or_default(), &mut total] {
            tally.words += 1;
            tally.written += usize::from(!candidates.is_empty());
            tally.plover += usize::from(plover);
            tally.lapwing += usize::from(lapwing);
            tally.either += usize::from(plover || lapwing);
            tally.usage_weight += weight;
            // Lyk's own cost counts where Plover's is known too, as before: the columns compare
            // the same words
            if costs[PLOVER].is_some() {
                tally.plain[LYK].add(costs[LYK].expect("ours is known"), 1.0);
                tally.weighted[LYK].add(costs[LYK].expect("ours is known"), weight);
            }
            for theory in [PLOVER, LAPWING] {
                if let Some(cost) = costs[theory] {
                    tally.plain[theory].add(cost, 1.0);
                    tally.weighted[theory].add(cost, weight);
                }
            }
        }
    }
    let mut rows: Rows =
        by_size.into_iter().map(|(size, t)| (if size == 4 { "4+".to_owned() } else { size.to_string() }, t)).collect();
    rows.push(("all".to_owned(), total));
    Ok(rows)
}

/// How many of the words the rules write the way Plover and Lapwing do, by syllables, and how
/// many keys each syllable count takes per stroke, Lyk next to Plover and Lapwing.
pub fn score(setup: &Setup, briefs: &generate::Briefs, common: usize) -> Result<()> {
    let rows = tallies(setup, briefs, common)?;
    println!("the {common} most common words that Plover or Lapwing write and CMUdict pronounces\n");
    println!(
        "syllables   words  written   plover  lapwing   either  keys/stroke: lyk  plover  \
         lapwing  by frequency: lyk  plover  lapwing"
    );
    for (label, t) in rows {
        let pct = |n: usize| format!("{:>5.1}%", 100.0 * n.to_f64() / t.words.max(1).to_f64());
        let [lyk, plover, lapwing] = t.plain.map(Sum::per_stroke);
        let [lyk_said, plover_said, lapwing_said] = t.weighted.map(Sum::per_stroke);
        println!(
            "{label:<9} {:>7} {:>8} {:>8} {:>8} {:>8}  {lyk:>16.3} {plover:>7.3} {lapwing:>7.3}  {lyk_said:>13.3} {plover_said:>7.3} {lapwing_said:>7.3}",
            t.words,
            pct(t.written),
            pct(t.plover),
            pct(t.lapwing),
            pct(t.either),
        );
    }
    Ok(())
}

/// A markdown table: a header row, its separator, then each row.
fn table(header: &[&str], rows: impl IntoIterator<Item = Vec<String>>) -> String {
    let line = |cells: &[String]| format!("| {} |\n", cells.join(" | "));
    let mut out = line(&header.iter().map(|h| (*h).to_owned()).collect::<Vec<_>>());
    let _ = writeln!(out, "|{}", "---|".repeat(header.len()));
    for row in rows {
        out.push_str(&line(&row));
    }
    out
}

/// The real syllable-count rows: "0" (a few pronunciation-lookup artifacts with no syllable) and
/// the "all" summary are left out of every README table.
fn buckets(rows: &Rows) -> impl Iterator<Item = &(String, Tally)> {
    rows.iter().filter(|(label, _)| label != "all" && label != "0")
}

/// The "all" row, which `tallies` always ends with.
fn all(rows: &Rows) -> &Tally {
    rows.iter().find(|(label, _)| label == "all").map(|(_, t)| t).expect("tallies() always produces an \"all\" row")
}

fn two(x: f64) -> String {
    format!("{x:.2}")
}

fn percent(n: f64, of: f64) -> String {
    format!("{:.0}%", 100.0 * n / of.max(1.0))
}

/// How often the rules match Plover/Lapwing, and keys per stroke, every distinct word once.
fn score_markdown(rows: &Rows) -> String {
    table(
        &["Syllables", "Same as Plover or Lapwing", "Plover", "Lapwing", "Keys/Stroke (Lyk)", "Keys/Stroke (Plover)", "Keys/Stroke (Lapwing)"],
        buckets(rows).map(|(label, t)| {
            let words = t.words.to_f64();
            let mut row = vec![
                label.clone(),
                percent(t.either.to_f64(), words),
                percent(t.plover.to_f64(), words),
                percent(t.lapwing.to_f64(), words),
            ];
            row.extend(t.plain.map(|s| two(s.per_stroke())));
            row
        }),
    )
}

/// Keys per stroke with each word counted as often as it is said: a few very common words
/// dominate, a long tail of rare ones barely moves it.
fn usage_markdown(rows: &Rows) -> String {
    table(
        &["Syllables", "Keys/Stroke (Lyk, by frequency)", "Keys/Stroke (Plover, by frequency)", "Keys/Stroke (Lapwing, by frequency)"],
        buckets(rows).map(|(label, t)| std::iter::once(label.clone()).chain(t.weighted.map(|s| two(s.per_stroke()))).collect()),
    )
}

/// How common each syllable count is, every distinct word once.
fn syllable_words_markdown(rows: &Rows) -> String {
    let words = all(rows).words.to_f64();
    table(
        &["Syllables", "Share of distinct words"],
        buckets(rows).map(|(label, t)| vec![label.clone(), percent(t.words.to_f64(), words)]),
    )
}

/// How much of everything said falls into each syllable count.
fn syllable_usage_markdown(rows: &Rows) -> String {
    let weight = all(rows).usage_weight;
    table(
        &["Syllables", "Share of words actually said"],
        buckets(rows).map(|(label, t)| vec![label.clone(), percent(t.usage_weight, weight)]),
    )
}

/// Keys per stroke across every word at once, weighted by how often each is said: weighted by
/// usage directly, strokes included, not an average of the per-syllable ratios.
fn overall_markdown(rows: &Rows) -> String {
    let t = all(rows);
    table(
        &["Theory", "Keys/Stroke, weighted by how often each word is said"],
        ["Lyk", "Plover", "Lapwing"].into_iter().zip(t.weighted).map(|(name, s)| vec![name.to_owned(), two(s.per_stroke())]),
    )
}

type TableFn = fn(&Rows) -> String;

/// Every table the README keeps in sync with `tallies`: its name, the marker it sits between
/// (`<!-- lyk:{marker}:start/end -->`), and what renders it.
const TABLES: &[(&str, &str, TableFn)] = &[
    ("score", "score-table", score_markdown),
    ("frequency-weighted", "usage-table", usage_markdown),
    ("syllables by distinct word", "syllable-words-table", syllable_words_markdown),
    ("syllables by usage", "syllable-usage-table", syllable_usage_markdown),
    ("overall", "overall-table", overall_markdown),
];

/// Whether the README's tables still match what `score` computes today. Prints each stale table
/// and fails, so a habit of running this before a commit catches drift; only the text between a
/// table's markers is compared, so the prose around it is never touched. With `write`, stale
/// tables are replaced in place instead and the check succeeds. The dictionary is generated once.
pub fn check_readme(setup: &Setup, briefs: &generate::Briefs, common: usize, readme: &Path, write: bool) -> Result<()> {
    let mut text = std::fs::read_to_string(readme).with_context(|| readme.display().to_string())?;
    let rows = tallies(setup, briefs, common)?;
    let mut stale = 0usize;
    for (name, marker, render) in TABLES {
        let (start, end) = (format!("<!-- lyk:{marker}:start -->"), format!("<!-- lyk:{marker}:end -->"));
        let fresh = render(&rows);
        let from = text.find(&start).with_context(|| format!("{start} not found in {}", readme.display()))? + start.len();
        let to = from + text[from..].find(&end).with_context(|| format!("{end} not found in {}", readme.display()))?;
        if text[from..to].trim() == fresh.trim() {
            continue;
        }
        stale += 1;
        if write {
            text.replace_range(from..to, &format!("\n{fresh}"));
            println!("rewrote the {name} table in {}", readme.display());
        } else {
            println!("{} has a stale {name} table between {start} and {end}. it should read:\n", readme.display());
            println!("{fresh}");
        }
    }
    if stale == 0 {
        println!("every table matches: {}", readme.display());
        return Ok(());
    }
    if write {
        return std::fs::write(readme, &text).with_context(|| readme.display().to_string());
    }
    anyhow::bail!(
        "{} has a stale table — rerun with --write-readme, or paste the table(s) above over what's there now",
        readme.display()
    );
}

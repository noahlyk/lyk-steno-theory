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
    /// Print `line<TAB>answer` for every line in this file (an outline, or an outline, a tab and
    /// the text typed before it) and stop
    pub answers: Option<&'a Path>,
    /// Test against the outlines the rules teach for this many of the commonest words
    pub taught: Option<usize>,
    /// Test against compounds past the lexicon struck as two words, at most this many
    pub compounds: Option<usize>,
    /// Test against the giant words of this word list that morphemes build, at most `giant_count`
    pub giant: Option<&'a Path>,
    pub giant_count: usize,
    /// Test the context rule on every this-many-th word of running text with backups
    pub context: Option<usize>,
    /// Test the name lane on every this-many-th name with a pronunciation
    pub names: Option<usize>,
    /// Where the running text is (`data/corpus`, books)
    pub corpus: &'a Path,
    /// With `outline`: the text typed before it
    pub after: &'a str,
    /// With `outline`: read it in the name lane
    pub name: bool,
}

/// The model `build` would ship, from the dictionary already in `dir`.
pub fn model(setup: &Setup, dir: &Path, common: usize) -> Result<(Model, usize)> {
    let entries = crate::built::load(dir).with_context(|| format!("reading the dictionary in {}", dir.display()))?;
    let stems = solve::stems(&entries);
    let lexicon = Lexicon { pron: &setup.pron, rank: &setup.rank, spellings: &setup.spellings, bigrams: &setup.web_bigrams, common };
    let blob = solve::pack(&setup.rules, &lexicon, &stems, Some(&crate::names::collect(setup)));
    let size = blob.len();
    Ok((Model::load(&blob).context("the packed blob does not load")?, size))
}

pub fn run(setup: &Setup, options: &Options) -> Result<()> {
    if let Some(to) = options.plugin {
        let entries = crate::built::load(options.dir)?;
        let lexicon = Lexicon { pron: &setup.pron, rank: &setup.rank, spellings: &setup.spellings, bigrams: &setup.web_bigrams, common: options.common };
        std::fs::create_dir_all(to)?;
        let size = solve::write_plugin(&setup.rules, &lexicon, &entries, &crate::names::collect(setup), to)?;
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
        // A line is an outline, or an outline, a tab and the text typed before it, or those and a
        // third field, `lane`, when the run was struck after a lone `#` stroke (the name lane)
        for line in text.lines().filter(|l| !l.is_empty()) {
            let mut fields = line.split('\t');
            let outline = fields.next().unwrap_or_default();
            let before = fields.next().unwrap_or_default();
            let lane = fields.next() == Some("lane");
            println!("{line}\t{}", model.answer_after(outline, before, lane).unwrap_or_else(|| "-".into()));
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
        let readings = model.solve(&run, 8, options.after, options.name);
        let took = t.elapsed();
        for r in &readings {
            let context = if r.context == 0 { String::new() } else { format!("  (context {:+.1})", f64::from(r.context) / 10.0) };
            println!("{:>4}  {:<24} {}{context}", r.cost, r.text, r.pieces.join(" + "));
        }
        if !options.after.is_empty() {
            println!("after: {:?}", solve::core::previous_word(options.after));
        }
        println!("answer: {:?}  ({took:.1?})", model.answer_after(text, options.after, options.name));
        return Ok(());
    }
    if let Some(step) = options.eval {
        eval_backups(&model, options.dir, step, options.misses, &setup.rank)?;
    }
    if let Some(tape) = options.tape {
        eval_tape(&model, tape)?;
    }
    if let Some(n) = options.compounds {
        eval_compounds(setup, &model, n, options.misses)?;
    }
    if let Some(path) = options.giant {
        eval_giant(setup, &model, path, options.giant_count, options.misses)?;
    }
    if let Some(n) = options.taught {
        eval_taught(setup, &model, options.dir, n, options.misses)?;
    }
    if let Some(step) = options.context {
        eval_context(&model, options.dir, options.corpus, step, options.misses)?;
    }
    if let Some(step) = options.names {
        eval_names(setup, &model, step, options.misses)?;
    }
    Ok(())
}

/// One kind of name test: how many strikes, how many the solver put first, how many in its five.
#[derive(Default)]
struct NameTally {
    cases: usize,
    first: usize,
    five: usize,
}

impl NameTally {
    fn add(&mut self, got: &[solve::Reading], name: &str) {
        self.cases += 1;
        self.first += usize::from(got.first().is_some_and(|r| r.text == name));
        self.five += usize::from(got.iter().any(|r| r.text == name));
    }
    fn line(&self, what: &str) -> String {
        format!("  {what:<44} {:>6} strikes  top-1 {:>5.1}%  top-5 {:>5.1}%", self.cases, pct(self.first, self.cases), pct(self.five, self.cases))
    }
}

/// The outline (join keys set) as the solver is asked it: the strokes after the lone `#`, which
/// the plugin reads from keymux's stroke history and the solver is told as `lane`.
fn lane_run(outline: &[Stroke]) -> Vec<u32> {
    outline.iter().map(|s| s.0).collect()
}

/// The name lane: every `step`th name that has a pronunciation, struck in the lane (after a lone `#`) in four ways: as
/// the rules strike its sounds, as the cheapest lattice paths strike them, and (the pronunciation
/// kept, then withheld) as its spelling is struck. The model whose pronunciations are withheld is
/// the one a name with none is read by.
fn eval_names(setup: &Setup, model: &Model, step: usize, misses: Option<&Path>) -> Result<()> {
    let names = crate::names::collect(setup);
    let sample: Vec<&crate::names::Name> =
        names.iter().filter(|n| setup.pron.contains_key(&n.key) && crate::names::spellable(&n.key)).step_by(step.max(1)).collect();
    let held: HashSet<&str> = sample.iter().map(|n| n.key.as_str()).collect();
    let mut withheld = setup.pron.clone();
    withheld.retain(|k, _| !held.contains(k.as_str()));
    let blind = solve::pack_names(&setup.rules, &withheld, &names);
    let blind = Model::load(&blind).context("the names model without pronunciations does not load")?;
    let mut tally: [NameTally; 5] = Default::default();
    let mut lost = String::new();
    let mut sc = blind.scratch();
    for name in &sample {
        let rules = &setup.rules;
        let mut strikes: Vec<(usize, Vec<Stroke>)> = Vec::new();
        for sounds in &setup.pron[&name.key] {
            if let Some(mut outline) = rules.outline(&name.key, sounds) {
                rules.joined(&mut outline);
                strikes.push((0, outline.clone()));
                strikes.push((3, outline));
            }
            for (_, mut outline) in rules.lattice_paths(&name.key, sounds, &[3, 2, 1, 0, 0]) {
                rules.joined(&mut outline);
                strikes.push((1, outline));
            }
        }
        for (_, mut outline) in rules.orthographic_paths(&name.key, &crate::names::SPELLING_QUOTAS).into_iter().chain(rules.spelled_remainder(&name.key, &crate::names::SPELLING_QUOTAS)) {
            rules.joined(&mut outline);
            strikes.push((2, outline.clone()));
            strikes.push((4, outline));
        }
        for (kind, outline) in strikes {
            let run = lane_run(&outline);
            // Kinds 3 and 4 are read by the model that was never told how the name sounds
            let got = if kind >= 3 { blind.solve_with(&mut sc, &run, 5).into_iter().map(|mut r| { r.text = capitalized(&r.text, name); r }).collect() } else { model.solve(&run, 5, "", true) };
            tally[kind].add(&got, &name.text);
            if kind == 0 && got.first().is_none_or(|r| r.text != name.text) {
                let _ = writeln!(lost, "{}\t{}\t{}", render_outline(&outline), name.text, got.first().map_or("-", |r| r.text.as_str()));
            }
        }
    }
    println!("names: {} of {} names with a pronunciation, every {step}th", sample.len(), names.iter().filter(|n| setup.pron.contains_key(&n.key)).count());
    for (kind, what) in ["the rules' outline of its sounds", "lattice paths of its sounds", "its spelling, pronunciation kept", "the rules' outline, pronunciation withheld", "its spelling, pronunciation withheld"].iter().enumerate() {
        println!("{}", tally[kind].line(what));
    }
    if let Some(path) = misses {
        std::fs::write(path, lost)?;
    }
    Ok(())
}

/// A reading's text as the name is written: the name's own text when it is the same word.
fn capitalized(text: &str, name: &crate::names::Name) -> String {
    if text.to_lowercase() == name.key { name.text.clone() } else { text.to_owned() }
}

/// The backups by word: each word's outlines, but those another file holds (keymux types that
/// file's word, the solver never sees them).
fn backups_by_word(dir: &Path) -> Result<HashMap<String, Vec<String>>> {
    let path = dir.join("02-lyk-backups.json");
    let text = std::fs::read_to_string(&path).with_context(|| path.display().to_string())?;
    let shadowed: HashSet<String> = crate::built::load(dir)?
        .iter()
        .filter(|e| e.origin != crate::generate::Origin::Backup)
        .map(|e| render_outline(&e.outline))
        .collect();
    let mut by_word: HashMap<String, Vec<String>> = HashMap::new();
    for line in text.lines() {
        let Some(l) = line.trim().trim_end_matches(',').strip_prefix('"') else { continue };
        let Some(at) = l.find("\": \"") else { continue };
        let Some(word) = l[at + 4..].strip_suffix('"') else { continue };
        let outline = &l[..at];
        if parse_outline(outline).is_some() && !shadowed.contains(outline) {
            by_word.entry(word.to_lowercase()).or_default().push(outline.to_owned());
        }
    }
    Ok(by_word)
}

/// The books of running text in `corpus` (its `.txt` files), in name order.
fn books(corpus: &Path) -> Result<Vec<String>> {
    let mut books: Vec<std::path::PathBuf> = std::fs::read_dir(corpus)
        .with_context(|| corpus.display().to_string())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "txt"))
        .collect();
    books.sort();
    Ok(books.iter().map(std::fs::read_to_string).collect::<std::io::Result<_>>()?)
}

/// A word of running text to solve: an outline, its word, the text before it, and whether it is in
/// its book's second half.
type ContextCase<'a> = (&'a str, String, &'a str, bool);

/// Every `step`th word of `texts` with backups, one of its backup outlines picked by where the
/// word sits, after its last [`solve::core::CONTEXT_BYTES`] of text; and how many words had backups.
fn context_sample<'a>(texts: &'a [String], by_word: &'a HashMap<String, Vec<String>>, step: usize) -> (Vec<ContextCase<'a>>, usize) {
    let mut sample: Vec<ContextCase> = Vec::new();
    let mut eligible = 0usize;
    for text in texts {
        let bytes = text.as_bytes();
        let mut at = 0;
        while at < bytes.len() {
            if !bytes[at].is_ascii_alphabetic() {
                at += 1;
                continue;
            }
            let start = at;
            while at < bytes.len() && (bytes[at].is_ascii_alphabetic() || bytes[at] == b'\'') {
                at += 1;
            }
            let word = text[start..at].trim_end_matches('\'').to_lowercase();
            let Some(outlines) = by_word.get(&word) else { continue };
            eligible += 1;
            if !eligible.is_multiple_of(step.max(1)) {
                continue;
            }
            let mut from = start.saturating_sub(solve::core::CONTEXT_BYTES);
            while !text.is_char_boundary(from) {
                from += 1;
            }
            let outline = outlines[start % outlines.len()].as_str();
            sample.push((outline, word, &text[from..start], start >= text.len() / 2));
        }
    }
    (sample, eligible)
}

/// One word of running text, solved: right at the top without context and with it, in the top
/// five without and with, and a line saying what each read.
type ContextResult = (bool, bool, bool, bool, String);

/// The context rule on running text: every `step`th word of the books in `corpus` that has backups,
/// one of its backup outlines (picked by where the word sits), solved with no context and after the
/// text before it (its last 256 bytes, as keymux sends). The first half of each book tunes the
/// rule's weight and cap, the second half is the test.
fn eval_context(model: &Model, dir: &Path, corpus: &Path, step: usize, misses: Option<&Path>) -> Result<()> {
    let by_word = backups_by_word(dir)?;
    let texts = books(corpus)?;
    let (mut sample, eligible) = context_sample(&texts, &by_word, step);
    // By outline, so each one's beam is read once and every context after it only re-ranks
    sample.sort_by(|a, b| a.0.cmp(b.0));
    let threads = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
    let chunk = sample.len().div_ceil(threads).max(1);
    let solved: Vec<ContextResult> = std::thread::scope(|scope| {
        // Every thread is spawned before any is joined: joining lazily would run them one at a time
        #[allow(clippy::needless_collect)]
        let handles: Vec<_> = sample
            .chunks(chunk)
            .map(|part| {
                scope.spawn(move || {
                    let mut sc = model.scratch();
                    let mut last: Option<(&str, Vec<solve::Reading>)> = None;
                    part.iter()
                        .map(|&(outline, ref word, before, _)| {
                            let run: Vec<u32> = parse_outline(outline).map(|s| s.iter().map(|s| s.0).collect()).unwrap_or_default();
                            if last.as_ref().is_none_or(|l| l.0 != outline) {
                                last = Some((outline, model.solve_with(&mut sc, &run, 5)));
                            }
                            let plain = &last.as_ref().expect("solved just now").1;
                            let after = model.solve_in(&mut sc, &run, 5, before, false);
                            let right = |got: &[solve::Reading]| got.first().is_some_and(|r| r.text.to_lowercase() == *word);
                            let in5 = |got: &[solve::Reading]| got.iter().any(|r| r.text.to_lowercase() == *word);
                            let note = format!(
                                "{outline}\t{word}\t{}\t{}\t{}",
                                plain.first().map_or("-", |r| r.text.as_str()),
                                after.first().map_or("-", |r| r.text.as_str()),
                                solve::core::previous_word(before).unwrap_or_else(|| "-".into())
                            );
                            (right(plain), right(&after), in5(plain), in5(&after), note)
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().expect("an eval thread panicked")).collect()
    });
    let mut lines = Vec::new();
    for (half, name) in [(false, "tune (first half of each book)"), (true, "test (second half)")] {
        let (mut plain, mut after) = (Tally::default(), Tally::default());
        let (mut wins, mut losses) = (0usize, 0usize);
        for ((_, _, _, second), (p, a, p5, a5, note)) in sample.iter().zip(&solved) {
            if *second != half {
                continue;
            }
            plain.add(*p, *p5);
            after.add(*a, *a5);
            if p != a {
                if *a {
                    wins += 1;
                } else {
                    losses += 1;
                }
            }
            match (p, a) {
                (false, true) => lines.push(format!("win\t{note}")),
                (true, false) => lines.push(format!("loss\t{note}")),
                (false, false) => lines.push(format!("miss\t{note}")),
                (true, true) => {}
            }
        }
        println!("context, {name}: no context {}  ->  after the text before it {}   ({wins} won, {losses} lost)", plain.line(), after.line());
    }
    println!("  ({eligible} words of running text with backups, every {step}th tested)");
    if let Some(path) = misses {
        std::fs::write(path, lines.join("\n") + "\n")?;
        println!("{} changes and misses written to {}", lines.len(), path.display());
    }
    Ok(())
}


/// How well the solver reads compounds past its lexicon (`Setup::spellings`) struck as two words:
/// each word that splits into two lexicon words of three letters or more, both pieces struck as
/// the rules teach them (`HRAOBG/+UP` *lookup*). At most `n` of them.
fn eval_compounds(setup: &Setup, model: &Model, n: usize, misses: Option<&Path>) -> Result<()> {
    let lexicon = |w: &str| setup.rank.get(w).is_some_and(|&r| r < 100_000) && setup.pron.contains_key(w);
    let first_outline = |w: &str| setup.candidates(w).into_iter().next();
    let mut sample: Vec<(String, &String, Vec<u32>)> = Vec::new();
    for (word, _) in &setup.spellings {
        if sample.len() >= n || !word.bytes().all(|c| c.is_ascii_lowercase()) {
            continue;
        }
        let split = (3..word.len().saturating_sub(2)).find(|&at| lexicon(&word[..at]) && lexicon(&word[at..]));
        let Some(at) = split else { continue };
        let (Some(a), Some(b)) = (first_outline(&word[..at]), first_outline(&word[at..])) else { continue };
        let outline = format!("{a}/+{b}");
        let Some(strokes) = parse_outline(&outline) else { continue };
        sample.push((outline, word, strokes.iter().map(|s| s.0).collect()));
    }
    let borrowed: Vec<(&str, &String, Vec<u32>)> = sample.iter().map(|(o, w, r)| (o.as_str(), *w, r.clone())).collect();
    let solved = solve_all(model, &borrowed);
    let mut all = Tally::default();
    let mut wrong = Vec::new();
    for ((outline, word, _), (_, got, _)) in borrowed.iter().zip(&solved) {
        let top1 = got.first().is_some_and(|r| r.text.to_lowercase() == **word);
        all.add(top1, got.iter().any(|r| r.text.to_lowercase() == **word));
        if !top1 {
            wrong.push(miss_line(outline, word, got));
        }
    }
    println!("compounds past the lexicon, struck as two words: {}", all.line());
    if let Some(path) = misses {
        std::fs::write(path, wrong.join("\n") + "\n")?;
    }
    Ok(())
}

/// A word past the lexicon split as `rules/morphemes.toml` builds one: prefixes, one lexicon word of
/// three letters or more, suffixes, each joined as written (no spelling change at the seams), the
/// fewest pieces that do it, at most three of each kind.
fn giant_pieces(word: &str, lexicon: &dyn Fn(&str) -> bool, table: &[solve::Morpheme]) -> Option<Vec<String>> {
    fn suffixes(rest: &str, table: &[solve::Morpheme], depth: usize) -> Option<Vec<String>> {
        if rest.is_empty() {
            return Some(Vec::new());
        }
        if depth == 3 {
            return None;
        }
        let tries = table.iter().filter(|m| !m.prefix && rest.starts_with(m.text.as_str()));
        tries.filter_map(|m| suffixes(&rest[m.text.len()..], table, depth + 1).map(|mut s| (s.insert(0, m.text.clone()), s).1)).min_by_key(Vec::len)
    }
    fn go(rest: &str, lexicon: &dyn Fn(&str) -> bool, table: &[solve::Morpheme], depth: usize) -> Option<Vec<String>> {
        let mut best: Option<Vec<String>> = None;
        let mut keep = |found: Vec<String>| {
            if best.as_ref().is_none_or(|b| found.len() < b.len()) {
                best = Some(found);
            }
        };
        for end in (3..=rest.len()).rev() {
            if lexicon(&rest[..end])
                && let Some(mut tail) = suffixes(&rest[end..], table, 0)
            {
                tail.insert(0, rest[..end].to_owned());
                keep(tail);
            }
        }
        if depth < 3 {
            for m in table.iter().filter(|m| m.prefix && rest.starts_with(m.text.as_str())) {
                if let Some(mut tail) = go(&rest[m.text.len()..], lexicon, table, depth + 1) {
                    tail.insert(0, m.text.clone());
                    keep(tail);
                }
            }
        }
        best
    }
    go(word, lexicon, table, 0).filter(|p| p.len() > 1)
}

/// How well the solver reads giant words past its lexicon (one a line in `path`, such as Monkeytype's
/// `english_450k`) that split as prefixes, a word and suffixes ([`giant_pieces`]), each piece struck
/// as the rules teach it (the word as `lookup` gives it, a morpheme by its first sounds), every
/// stroke after the first joined. At most `n` of them.
fn eval_giant(setup: &Setup, model: &Model, path: &Path, n: usize, misses: Option<&Path>) -> Result<()> {
    let lexicon = |w: &str| setup.rank.get(w).is_some_and(|&r| r < 100_000) && setup.pron.contains_key(w);
    let table = solve::morphemes();
    let text = std::fs::read_to_string(path).with_context(|| path.display().to_string())?;
    let mut sample: Vec<(String, String, Vec<u32>)> = Vec::new();
    let mut words = 0;
    for word in text.lines().map(str::trim).filter(|w| w.len() >= 8 && w.bytes().all(|c| c.is_ascii_lowercase()) && !lexicon(w)) {
        words += 1;
        if sample.len() >= n {
            continue;
        }
        let Some(pieces) = giant_pieces(word, &lexicon, &table) else { continue };
        let outlines: Option<Vec<Vec<Stroke>>> = pieces
            .iter()
            .map(|piece| {
                table.iter().find(|m| m.text == *piece).map_or_else(
                    || setup.candidates(piece).into_iter().next().and_then(|o| parse_outline(&o)),
                    |m| setup.rules.outline(piece, &m.sounds[0].split_whitespace().map(str::to_owned).collect::<Vec<_>>()),
                )
            })
            .collect();
        let Some(outlines) = outlines else { continue };
        let mut strokes: Vec<Stroke> = outlines.concat();
        setup.rules.joined(&mut strokes);
        sample.push((render_outline(&strokes), word.to_owned(), strokes.iter().map(|s| s.0).collect()));
    }
    let borrowed: Vec<(&str, &String, Vec<u32>)> = sample.iter().map(|(o, w, r)| (o.as_str(), w, r.clone())).collect();
    let solved = solve_all(model, &borrowed);
    let mut all = Tally::default();
    let mut wrong = Vec::new();
    for ((outline, word, _), (_, got, answer)) in borrowed.iter().zip(&solved) {
        let top1 = answer.as_deref().is_some_and(|a| a.to_lowercase() == **word);
        all.add(top1, got.iter().any(|r| r.text.to_lowercase() == **word));
        if !top1 {
            wrong.push(miss_line(outline, word, got));
        }
    }
    println!("giant words past the lexicon ({words} of 8 letters or more), split as prefixes + a word + suffixes and struck piece by piece: {}", all.line());
    if let Some(path) = misses {
        std::fs::write(path, wrong.join("\n") + "\n")?;
    }
    Ok(())
}

/// How well the solver reads the outlines the rules teach (`explain`'s "ours") for the `n`
/// commonest words: the runs a writer strikes syllable by syllable, as the lessons have them. An
/// outline a dictionary entry holds is skipped (keymux types the entry, the solver never sees it),
/// and so is one of a single stroke.
fn eval_taught(setup: &Setup, model: &Model, dir: &Path, n: usize, misses: Option<&Path>) -> Result<()> {
    let held: HashSet<String> = crate::built::load(dir)?
        .iter()
        .filter(|e| e.origin != crate::generate::Origin::Backup)
        .map(|e| render_outline(&e.outline))
        .collect();
    let mut words: Vec<(&String, usize)> = setup.rank.iter().map(|(w, r)| (w, *r)).filter(|(_, r)| *r < n).collect();
    words.sort_by(|a, b| (a.1, a.0).cmp(&(b.1, b.0)));
    let mut sample: Vec<(String, &String, Vec<u32>)> = Vec::new();
    for (word, _) in words {
        for outline in setup.candidates(word) {
            let Some(strokes) = parse_outline(&outline) else { continue };
            if strokes.len() < 2 || held.contains(&outline) {
                continue;
            }
            sample.push((outline, word, strokes.iter().map(|s| s.0).collect()));
        }
    }
    let borrowed: Vec<(&str, &String, Vec<u32>)> = sample.iter().map(|(o, w, r)| (o.as_str(), *w, r.clone())).collect();
    let solved = solve_all(model, &borrowed);
    let mut all = Tally::default();
    let mut wrong = Vec::new();
    for ((outline, word, _), (_, got, _)) in borrowed.iter().zip(&solved) {
        let top1 = got.first().is_some_and(|r| r.text.to_lowercase() == **word);
        all.add(top1, got.iter().any(|r| r.text.to_lowercase() == **word));
        if !top1 {
            wrong.push(miss_line(outline, word, got));
        }
    }
    println!("taught outlines, {n} commonest words: {}", all.line());
    if let Some(path) = misses {
        std::fs::write(path, wrong.join("\n") + "\n")?;
        println!("{} misses written to {}", wrong.len(), path.display());
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

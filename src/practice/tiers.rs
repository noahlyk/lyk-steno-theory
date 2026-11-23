//! Levels: the pool cut into tiers by the list a word first appears in, and how much of each the
//! typist knows.
//!
//! The tiers, in order: Monkeytype's `english` (the 200 commonest words), `english_1k`,
//! `english_5k`, `english_10k` and `english_25k`, each holding only the words an earlier list does
//! not (so a tier is the words *added* by that list); then `lyk top 20k`, the practice words of
//! lyk's frequency list up to rank 20,000 that no Monkeytype list has; with `--deep`, last, `deep`,
//! the rest of the 450k dictionary. Only practice words count (the pool's predicate), and hidden
//! words (`lyk vocab clean`) are left out.
//!
//! A word is *known* when it is in review or retired (learning counts 0). A tier is complete when
//! at least [`COMPLETE`] of its words are known; the typist's *level* is the first tier that is not
//! complete (1-based). A *goal* (`lyk vocab goal`) is a tier, or a number of words, and its bar is
//! the known share of everything up to it.
//!
//! Pure: the tiers are built from word lists, the numbers from a [`Vocab`].

// Counts become shares: the casts are exact at these sizes.
#![allow(clippy::cast_precision_loss)]

use super::pool::{list_path, parse_monkeytype, usable};
use super::sources;
use super::words::{LYK_RANK_LIMIT, WordFilter};
use crate::vocab::{Status, Vocab};
use std::collections::HashSet;
use std::path::Path;

/// The known share at which a tier is complete.
pub const COMPLETE: f64 = 0.90;

/// The Monkeytype tiers, in order: list file name, label.
const MONKEYTYPE: [(&str, &str); 5] = [
    ("english", "monkeytype 200"),
    ("english_1k", "monkeytype 1k"),
    ("english_5k", "monkeytype 5k"),
    ("english_10k", "monkeytype 10k"),
    ("english_25k", "monkeytype 25k"),
];
const LYK_KEY: &str = "lyk";
const LYK_LABEL: &str = "lyk top 20k";
const DEEP_KEY: &str = "deep";
const DEEP_LABEL: &str = "deep";
/// A goal of the first N words of the tiers, in tier order.
const WORDS_PREFIX: &str = "words:";

/// One tier: the words a list adds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tier {
    /// The key a goal is stored under (`english_1k`, `lyk`, `deep`)
    pub key: String,
    pub label: String,
    pub words: Vec<String>,
}

/// The tiers from the lists, each word in the first tier that has it. `lists` are the usable words
/// of each Monkeytype list by file name (a missing list is simply absent), `lyk` the usable words
/// of lyk's list up to its limit in rank order, `deep` the usable words of the 450k dictionary (or
/// none). An empty tier is dropped.
#[must_use]
pub fn build(lists: &[(String, Vec<String>)], lyk: &[String], deep: &[String]) -> Vec<Tier> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut take = |key: &str, label: &str, words: &[String]| {
        let words: Vec<String> = words.iter().filter(|w| seen.insert((*w).clone())).cloned().collect();
        (!words.is_empty()).then(|| Tier { key: key.to_owned(), label: label.to_owned(), words })
    };
    let mut tiers = Vec::new();
    for (key, label) in MONKEYTYPE {
        if let Some((_, words)) = lists.iter().find(|(n, _)| n == key) {
            tiers.extend(take(key, label, words));
        }
    }
    tiers.extend(take(LYK_KEY, LYK_LABEL, lyk));
    tiers.extend(take(DEEP_KEY, DEEP_LABEL, deep));
    tiers
}

/// Reads the tiers from the data folder, as the pool reads its lists: a list that is not
/// downloaded is a missing tier, never an error.
#[must_use]
pub fn load(data: &Path, filter: &WordFilter, skip: &HashSet<String>, deep: bool) -> Vec<Tier> {
    let read = |name: &str| std::fs::read_to_string(list_path(data, name)).ok().and_then(|t| parse_monkeytype(&t).ok()).map(|w| usable(w, filter));
    let lists: Vec<(String, Vec<String>)> = MONKEYTYPE.iter().filter_map(|(k, _)| read(k).map(|w| ((*k).to_owned(), w))).collect();
    let lyk = sources::common(&data.join("frequency/en_full.txt"), LYK_RANK_LIMIT, filter.allow_names, skip).map_or_else(|_| Vec::new(), |w| usable(w, filter));
    let wide = if deep { read("english_450k").unwrap_or_default() } else { Vec::new() };
    build(&lists, &lyk, &wide)
}

/// One tier's numbers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TierStat {
    pub key: String,
    pub label: String,
    pub size: usize,
    pub known: usize,
}

impl TierStat {
    /// Known share, 0 to 1 (1 for an empty tier).
    #[must_use]
    pub fn share(&self) -> f64 {
        if self.size == 0 { 1.0 } else { self.known as f64 / self.size as f64 }
    }

    #[must_use]
    pub fn complete(&self) -> bool {
        self.share() >= COMPLETE
    }
}

/// Whether the typist knows `word`: review or retired.
fn is_known(vocab: &Vocab, word: &str) -> bool {
    vocab.words.get(word).is_some_and(|w| matches!(w.status, Status::Review | Status::Retired))
}

/// The words of a tier that count: not hidden.
fn counted<'a>(vocab: &'a Vocab, tier: &'a Tier) -> impl Iterator<Item = &'a String> {
    tier.words.iter().filter(|w| !vocab.hidden.contains(*w))
}

/// Size and known count of each tier.
#[must_use]
pub fn stats(tiers: &[Tier], vocab: &Vocab) -> Vec<TierStat> {
    tiers
        .iter()
        .map(|t| {
            let (size, known) = counted(vocab, t).fold((0, 0), |(s, k), w| (s + 1, k + usize::from(is_known(vocab, w))));
            TierStat { key: t.key.clone(), label: t.label.clone(), size, known }
        })
        .collect()
}

/// The index of the typist's tier: the first that is not complete; `stats.len()` when all are.
#[must_use]
pub fn level_index(stats: &[TierStat]) -> usize {
    stats.iter().position(|s| !s.complete()).unwrap_or(stats.len())
}

/// A goal as stored in `vocab.json`: a tier key or `words:N`. `None` when the text names neither.
///
/// Accepted: a tier by size (`200`, `1k`, `5k`, `10k`, `25k`, `lyk` or `20k`, `deep`), optionally
/// with a `monkeytype-` or `english_` prefix (`monkeytype-1k`, `english_5k`), and a plain number of
/// words (`3000`: the first 3000 words of the tiers).
#[must_use]
pub fn parse_goal(text: &str) -> Option<String> {
    let t = text.trim().to_lowercase().replace([' ', '_'], "-");
    let (t, a) = t.strip_prefix("monkeytype").map_or((t.as_str(), false), |r| (r, true));
    let (t, b) = t.strip_prefix("english").map_or((t, false), |r| (r, true));
    let t = if a || b { t.trim_start_matches('-') } else { t };
    let t = t.trim_end_matches('-');
    let tier = match t {
        "" | "200" => "english",
        "1k" | "1000" => "english_1k",
        "5k" | "5000" => "english_5k",
        "10k" | "10000" => "english_10k",
        "25k" | "25000" => "english_25k",
        "lyk" | "20k" | "20000" | "lyk-20k" | "lyk-top-20k" => LYK_KEY,
        "deep" => DEEP_KEY,
        n => return n.parse::<usize>().ok().filter(|n| *n > 0).map(|n| format!("{WORDS_PREFIX}{n}")),
    };
    Some(tier.to_owned())
}

/// What a stored goal is called.
#[must_use]
pub fn goal_label(goal: &str) -> String {
    if let Some(n) = goal.strip_prefix(WORDS_PREFIX) {
        return format!("the first {n} words");
    }
    match goal {
        LYK_KEY => LYK_LABEL.to_owned(),
        other => MONKEYTYPE.iter().find(|(k, _)| *k == other).map_or_else(|| other.to_owned(), |(_, l)| (*l).to_owned()),
    }
}

/// The known share toward a goal: everything in the tiers up to and including it (or the first
/// N words), as `(known, size)`. `None` when the goal's tier is not among the tiers (its list is
/// not downloaded, or `deep` is off).
#[must_use]
pub fn goal_progress(tiers: &[Tier], vocab: &Vocab, goal: &str) -> Option<(usize, usize)> {
    let upto = |n: usize| tiers.iter().take(n).flat_map(|t| counted(vocab, t)).collect::<Vec<&String>>();
    let words = if let Some(n) = goal.strip_prefix(WORDS_PREFIX).and_then(|n| n.parse::<usize>().ok()) {
        upto(tiers.len()).into_iter().take(n).collect()
    } else {
        upto(tiers.iter().position(|t| t.key == goal)? + 1)
    };
    (!words.is_empty()).then(|| (words.iter().filter(|w| is_known(vocab, w)).count(), words.len()))
}

/// The progress bar line's numbers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Progress {
    /// 1-based level (the last tier's, when every tier is complete)
    pub level: usize,
    pub label: String,
    pub known: usize,
    pub size: usize,
    /// Every tier is complete
    pub all_complete: bool,
    /// The label of the tier after this one
    pub next: Option<String>,
    /// The goal: its label, known and size
    pub goal: Option<(String, usize, usize)>,
}

impl Progress {
    #[must_use]
    pub const fn percent(&self) -> usize {
        percent(self.known, self.size)
    }
}

/// `known` over `size` as a whole percent (0 for none).
#[must_use]
pub const fn percent(known: usize, size: usize) -> usize {
    match (known * 100 + size / 2).checked_div(size) {
        Some(p) => p,
        None => 0,
    }
}

/// The level, its bar and the goal's, for a vocabulary. `None` with no tiers (no lists on file).
#[must_use]
pub fn progress(tiers: &[Tier], vocab: &Vocab) -> Option<Progress> {
    let stats = stats(tiers, vocab);
    let last = stats.len().checked_sub(1)?;
    let at = level_index(&stats);
    let shown = &stats[at.min(last)];
    Some(Progress {
        level: at.min(last) + 1,
        label: shown.label.clone(),
        known: shown.known,
        size: shown.size,
        all_complete: at > last,
        next: stats.get(at + 1).map(|s| s.label.clone()),
        goal: vocab.goal.as_deref().and_then(|g| goal_progress(tiers, vocab, g).map(|(k, s)| (goal_label(g), k, s))),
    })
}

/// `[█████░░░░░]` for a share, `width` cells wide.
#[must_use]
pub fn bar(known: usize, size: usize, width: usize) -> String {
    let filled = (known * width).checked_div(size).unwrap_or(0).min(width);
    format!("[{}{}]", "█".repeat(filled), "░".repeat(width - filled))
}

/// The tier table of `lyk vocab`: one row a tier with a level marker, then the goal.
#[must_use]
pub fn table(tiers: &[Tier], vocab: &Vocab) -> String {
    use std::fmt::Write as _;
    let stats = stats(tiers, vocab);
    if stats.is_empty() {
        return "tiers: no Monkeytype lists or frequency list on file (run `lyk fetch`)\n".to_owned();
    }
    let at = level_index(&stats);
    let mut out = format!("tiers (a tier is complete at {:.0}% known; level {}):\n", COMPLETE * 100.0, at.min(stats.len() - 1) + 1);
    let _ = writeln!(out, "  {:<3} {:<16} {:>7} {:>7} {:>5}", "lv", "tier", "words", "known", "%");
    for (i, s) in stats.iter().enumerate() {
        let mark = if i == at { "   <- you are here" } else { "" };
        let _ = writeln!(out, "  {:<3} {:<16} {:>7} {:>7} {:>4}%{mark}", i + 1, s.label, s.size, s.known, percent(s.known, s.size));
    }
    match vocab.goal.as_deref() {
        Some(goal) => match goal_progress(tiers, vocab, goal) {
            Some((k, s)) => {
                let _ = writeln!(out, "goal: {}  {} {}%  {k} of {s} words known", goal_label(goal), bar(k, s, 10), percent(k, s));
            }
            None => {
                let _ = writeln!(out, "goal: {} (not among the tiers on file)", goal_label(goal));
            }
        },
        None => out.push_str("goal: none (lyk vocab goal 1k)\n"),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vocab::Word;

    fn w(words: &[&str]) -> Vec<String> {
        words.iter().map(|x| (*x).to_owned()).collect()
    }

    fn lists() -> Vec<(String, Vec<String>)> {
        vec![
            ("english".into(), w(&["a", "b", "c", "d"])),
            ("english_1k".into(), w(&["a", "b", "c", "d", "e", "f", "g", "h"])),
            ("english_5k".into(), w(&["e", "i", "j", "k"])),
        ]
    }

    fn vocab_with(known: &[&str], learning: &[&str]) -> Vocab {
        let mut v = Vocab::default();
        for k in known {
            v.words.insert((*k).to_owned(), Word { status: Status::Review, ..Word::default() });
        }
        for l in learning {
            v.words.insert((*l).to_owned(), Word { status: Status::Learning, ..Word::default() });
        }
        v
    }

    #[test]
    fn a_word_is_in_the_first_tier_that_has_it_and_empty_tiers_are_dropped() {
        let tiers = build(&lists(), &w(&["a", "x", "y", "k", "z"]), &w(&["x", "deepword"]));
        let got: Vec<(&str, Vec<&str>)> = tiers.iter().map(|t| (t.key.as_str(), t.words.iter().map(String::as_str).collect())).collect();
        assert_eq!(
            got,
            vec![
                ("english", vec!["a", "b", "c", "d"]),
                ("english_1k", vec!["e", "f", "g", "h"]),
                ("english_5k", vec!["i", "j", "k"]),
                ("lyk", vec!["x", "y", "z"]),
                ("deep", vec!["deepword"]),
            ]
        );
        // a list that is not on file is no tier, and the labels name the tiers
        let tiers = build(&lists()[..1], &[], &[]);
        assert_eq!(tiers.len(), 1);
        assert_eq!(tiers[0].label, "monkeytype 200");
    }

    #[test]
    fn known_counts_review_and_retired_not_learning_and_hidden_words_are_left_out() {
        let tiers = build(&lists(), &[], &[]);
        let mut v = vocab_with(&["a", "b"], &["c"]);
        v.words.insert("d".into(), Word { status: Status::Retired, ..Word::default() });
        let s = stats(&tiers, &v);
        assert_eq!((s[0].size, s[0].known), (4, 3), "{s:?}");
        assert_eq!((s[1].size, s[1].known), (4, 0));
        v.hidden.insert("c".into());
        v.hidden.insert("f".into());
        let s = stats(&tiers, &v);
        assert_eq!((s[0].size, s[0].known), (3, 3));
        assert_eq!(s[1].size, 3, "a hidden word is not counted");
        assert_eq!(percent(3, 4), 75);
        assert_eq!(percent(1, 3), 33);
        assert_eq!(percent(0, 0), 0);
    }

    #[test]
    fn the_level_is_the_first_tier_under_the_completion_bar() {
        let tiers = build(&lists(), &[], &[]);
        // tier 1 has 4 words: 3 known is 75%, under 90%
        let v = vocab_with(&["a", "b", "c"], &[]);
        assert_eq!(level_index(&stats(&tiers, &v)), 0);
        let p = progress(&tiers, &v).expect("tiers");
        assert_eq!((p.level, p.label.as_str(), p.known, p.size, p.percent(), p.all_complete), (1, "monkeytype 200", 3, 4, 75, false));
        assert_eq!(p.next.as_deref(), Some("monkeytype 1k"));
        // all four known: tier 1 is complete, level 2
        let v = vocab_with(&["a", "b", "c", "d", "e"], &[]);
        let p = progress(&tiers, &v).expect("tiers");
        assert_eq!((p.level, p.label.as_str(), p.known, p.size), (2, "monkeytype 1k", 1, 4));
        // a tier of 10 is complete at 9 known, not at 8
        let big = build(&[("english".into(), (0..10).map(|i| format!("w{i}")).collect())], &[], &[]);
        let nine: Vec<String> = (0..9).map(|i| format!("w{i}")).collect();
        let v = vocab_with(&nine.iter().map(String::as_str).collect::<Vec<_>>(), &[]);
        assert!(stats(&big, &v)[0].complete());
        let eight: Vec<String> = (0..8).map(|i| format!("w{i}")).collect();
        let v = vocab_with(&eight.iter().map(String::as_str).collect::<Vec<_>>(), &[]);
        assert!(!stats(&big, &v)[0].complete());
        // everything complete: the last tier, flagged
        let all = vocab_with(&["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k"], &[]);
        let p = progress(&tiers, &all).expect("tiers");
        assert!(p.all_complete && p.level == 3 && p.next.is_none(), "{p:?}");
        assert!(progress(&[], &all).is_none());
    }

    #[test]
    fn goals_parse_to_a_tier_or_a_number_of_words() {
        for (text, key) in [
            ("200", "english"),
            ("1k", "english_1k"),
            ("1K", "english_1k"),
            ("monkeytype-1k", "english_1k"),
            ("monkeytype 1k", "english_1k"),
            ("english_5k", "english_5k"),
            ("5k", "english_5k"),
            ("10000", "english_10k"),
            ("25k", "english_25k"),
            ("lyk", "lyk"),
            ("20k", "lyk"),
            ("deep", "deep"),
            ("monkeytype-200", "english"),
            ("english", "english"),
            ("3000", "words:3000"),
        ] {
            assert_eq!(parse_goal(text).as_deref(), Some(key), "{text}");
        }
        for bad in ["banana", "0", "-5", "1kk"] {
            assert_eq!(parse_goal(bad), None, "{bad}");
        }
        assert_eq!(goal_label("english_1k"), "monkeytype 1k");
        assert_eq!(goal_label("words:3000"), "the first 3000 words");
        assert_eq!(goal_label("lyk"), "lyk top 20k");
    }

    #[test]
    fn the_goal_bar_is_cumulative_up_to_the_goal_tier() {
        let tiers = build(&lists(), &w(&["x", "y"]), &[]);
        let v = vocab_with(&["a", "b", "e"], &["c"]);
        // up to 1k: tiers 200 and 1k, 8 words, 3 known
        assert_eq!(goal_progress(&tiers, &v, "english_1k"), Some((3, 8)));
        assert_eq!(goal_progress(&tiers, &v, "english"), Some((2, 4)));
        assert_eq!(goal_progress(&tiers, &v, "lyk"), Some((3, 13)));
        // the first N words, in tier order
        assert_eq!(goal_progress(&tiers, &v, "words:5"), Some((3, 5)));
        assert_eq!(goal_progress(&tiers, &v, "words:500"), Some((3, 13)), "capped at what there is");
        // a tier that is not there
        assert_eq!(goal_progress(&tiers, &v, "english_25k"), None);
        assert_eq!(goal_progress(&tiers, &v, "deep"), None);
        // the bar line's goal
        let mut v = v;
        v.goal = Some("english_1k".into());
        let p = progress(&tiers, &v).expect("tiers");
        assert_eq!(p.goal, Some(("monkeytype 1k".to_owned(), 3, 8)));
        let t = table(&tiers, &v);
        assert!(t.contains("goal: monkeytype 1k") && t.contains("3 of 8 words known") && t.contains("<- you are here"), "{t}");
    }

    #[test]
    fn the_bar_fills_by_share() {
        assert_eq!(bar(0, 10, 10), "[░░░░░░░░░░]");
        assert_eq!(bar(5, 10, 10), "[█████░░░░░]");
        assert_eq!(bar(10, 10, 10), "[██████████]");
        assert_eq!(bar(104, 200, 10), "[█████░░░░░]");
        assert_eq!(bar(0, 0, 4), "[░░░░]");
        assert_eq!(bar(99, 10, 10), "[██████████]", "never past the end");
    }

    #[test]
    fn the_table_lists_every_tier_and_marks_the_level() {
        let tiers = build(&lists(), &[], &[]);
        let v = vocab_with(&["a", "b", "c", "d", "e"], &[]);
        let t = table(&tiers, &v);
        assert!(t.contains("monkeytype 200") && t.contains("monkeytype 5k") && t.contains("level 2"), "{t}");
        let marked: Vec<&str> = t.lines().filter(|l| l.contains("<- you are here")).collect();
        assert_eq!(marked.len(), 1, "{t}");
        assert!(marked[0].contains("monkeytype 1k"), "{t}");
        assert!(t.contains("goal: none"), "{t}");
        assert!(table(&[], &v).contains("no Monkeytype lists"));
    }
}

//! What the results screen says: the numbers of the test, what it changed in the vocabulary
//! (compared between two snapshots of `vocab.json`'s contents), and the text version printed after
//! the screen closes. Pure: no terminal, no clock.

// Counts become shares: the casts are exact at these sizes.
#![allow(clippy::cast_precision_loss)]

use super::engine::{Chars, Results, Series};
use super::tiers::{self, Progress};
use crate::vocab::{Status, Vocab, WORD_WINDOW};
use std::collections::HashMap;

/// How near a learning word is to mastery, by its last ten attempts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Heat {
    /// Under 30% clean
    Red,
    /// In between
    Yellow,
    /// At the mastery threshold (9 of 10)
    Green,
}

/// A word being learned: `word 7/10`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chip {
    pub word: String,
    /// Clean among the last ten attempts
    pub clean: usize,
    pub heat: Heat,
}

/// What a test changed, for the panels under the numbers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Extra {
    /// Words that were wrong or needed a correction, with how many times, in order of appearance
    pub stumbled: Vec<(String, u32)>,
    /// Words that left the learning set for review in this test
    pub mastered: Vec<String>,
    /// Words that became retired in this test
    pub retired: Vec<String>,
    /// Words that entered learning in this test
    pub new: Vec<String>,
    /// Words that are stuck now
    pub stuck: Vec<String>,
    /// The learning set now (stuck words aside), the nearest to mastery first
    pub chips: Vec<Chip>,
}

/// The heat of a learning word with `clean` of its last ten right, against the mastery share.
#[must_use]
pub fn heat(clean: usize, mastery: f64) -> Heat {
    let share = clean as f64 / WORD_WINDOW as f64;
    if share >= mastery {
        Heat::Green
    } else if share < 0.30 {
        Heat::Red
    } else {
        Heat::Yellow
    }
}

fn status_of(v: &Vocab, w: &str) -> Option<Status> {
    v.words.get(w).map(|s| s.status)
}

/// What changed between the vocabulary before a test and after it, with the test's `stumbled`
/// words. Deterministic: lists are sorted, chips by closeness to mastery.
#[must_use]
pub fn compose(before: &Vocab, after: &Vocab, stumbled: Vec<(String, u32)>, mastery: f64) -> Extra {
    let mut extra = Extra { stumbled, ..Extra::default() };
    for (word, now) in &after.words {
        let was = status_of(before, word);
        match (was, now.status) {
            (Some(Status::Learning), Status::Review) => extra.mastered.push(word.clone()),
            (was, Status::Retired) if was != Some(Status::Retired) => extra.retired.push(word.clone()),
            (None | Some(Status::New), Status::Learning) => extra.new.push(word.clone()),
            _ => {}
        }
        if now.status == Status::Learning {
            if now.stuck {
                extra.stuck.push(word.clone());
            } else {
                let clean = now.window().0;
                extra.chips.push(Chip { word: word.clone(), clean, heat: heat(clean, mastery) });
            }
        }
    }
    // BTreeMap order is alphabetical already; chips go by closeness
    extra.chips.sort_by(|a, b| b.clean.cmp(&a.clean).then(a.word.cmp(&b.word)));
    extra
}

/// Everything the results screen shows.
#[derive(Clone, Debug)]
pub struct Screen {
    pub wpm: f64,
    pub raw: f64,
    pub accuracy: f64,
    /// Share of committed words that were right
    pub word_accuracy: f64,
    pub consistency: Option<f64>,
    pub secs: f64,
    /// `25 words  monkeytype 200`
    pub test: String,
    pub chars: Chars,
    pub series: Series,
    pub extra: Extra,
    pub progress: Option<Progress>,
    /// The lyk outline of a word, when `--outlines` found one
    pub outlines: HashMap<String, String>,
    /// What the vocabulary update says (`+2 new  1 mastered  3 due`), for the printed summary
    pub vocab_line: String,
}

impl Screen {
    #[must_use]
    pub const fn new(r: &Results, test: String, chars: Chars, series: Series, extra: Extra, progress: Option<Progress>, outlines: HashMap<String, String>) -> Self {
        Self { wpm: r.wpm, raw: r.raw, accuracy: r.accuracy, word_accuracy: r.word_accuracy, consistency: r.consistency, secs: r.secs, test, chars, series, extra, progress, outlines, vocab_line: String::new() }
    }

    /// `+1` style outline text of a word: ` (OUTLINE)`, or nothing.
    #[must_use]
    pub fn outline(&self, word: &str) -> String {
        self.outlines.get(word).map_or_else(String::new, |o| format!(" ({o})"))
    }
}

/// `1:05` or `12s`.
#[must_use]
pub fn clock(secs: f64) -> String {
    let s = secs.round() as u64;
    if s >= 60 { format!("{}:{:02}", s / 60, s % 60) } else { format!("{s}s") }
}

/// `word` or `word×3`.
#[must_use]
pub fn counted(word: &str, n: u32) -> String {
    if n > 1 { format!("{word}×{n}") } else { word.to_owned() }
}

/// The progress line: `lv1  monkeytype 200  [█████░░░░░] 52%  104/200 known   next: monkeytype 1k`.
#[must_use]
pub fn progress_line(p: &Progress, bar: usize) -> String {
    let next = if p.all_complete { "   every tier complete".to_owned() } else { p.next.as_ref().map_or_else(String::new, |n| format!("   next: {n}")) };
    format!("lv{}  {}  {} {}%  {}/{} known{next}", p.level, p.label, tiers::bar(p.known, p.size, bar), p.percent(), p.known, p.size)
}

/// The goal line: `goal  monkeytype 1k  [██░░░░░░░░] 12%  120/1008 known`.
#[must_use]
pub fn goal_line(goal: &(String, usize, usize), bar: usize) -> String {
    let (label, known, size) = goal;
    format!("goal  {label}  {} {}%  {known}/{size} known", tiers::bar(*known, *size, bar), tiers::percent(*known, *size))
}

/// The first of the progress line's shorter forms that fits `width`: the full line with a 10-cell
/// bar, a 6-cell bar, without `next:`, and last the plain numbers.
#[must_use]
pub fn progress_line_fit(p: &Progress, width: usize) -> String {
    let short = Progress { next: None, all_complete: false, ..p.clone() };
    let plain = format!("lv{}  {}  {}%  {}/{}", p.level, p.label, p.percent(), p.known, p.size);
    [progress_line(p, 10), progress_line(p, 6), progress_line(&short, 6), plain.clone()].into_iter().find(|l| l.chars().count() <= width).unwrap_or(plain)
}

/// The goal line in the first form that fits `width`.
#[must_use]
pub fn goal_line_fit(goal: &(String, usize, usize), width: usize) -> String {
    let (label, known, size) = goal;
    let plain = format!("goal  {label}  {}%  {known}/{size}", tiers::percent(*known, *size));
    [goal_line(goal, 10), goal_line(goal, 6), plain.clone()].into_iter().find(|l| l.chars().count() <= width).unwrap_or(plain)
}

/// The results as lines of text, printed after the screen closes: the numbers, the panels as one
/// line each, the progress. No chart.
#[must_use]
pub fn text_lines(s: &Screen) -> Vec<String> {
    let join = |words: &[String]| words.join(" ");
    let mut out = vec![
        format!("{:.0} wpm   raw {:.0}   acc {:.0}%   words {:.0}%", s.wpm, s.raw, s.accuracy, s.word_accuracy),
        format!(
            "test {}   characters {}/{}/{}/{}   consistency {}   time {}",
            s.test,
            s.chars.correct,
            s.chars.incorrect,
            s.chars.extra,
            s.chars.missed,
            s.consistency.map_or_else(|| "n/a".to_owned(), |c| format!("{c:.0}%")),
            clock(s.secs)
        ),
    ];
    let e = &s.extra;
    if !e.stumbled.is_empty() {
        let items: Vec<String> = e.stumbled.iter().map(|(w, n)| format!("{}{}", counted(w, *n), s.outline(w))).collect();
        out.push(format!("stumbled  {}", join(&items)));
    }
    let done: Vec<String> = e.mastered.iter().chain(&e.retired).cloned().collect();
    if !done.is_empty() {
        out.push(format!("mastered  {}", join(&done)));
    }
    if !e.new.is_empty() {
        out.push(format!("new       {}", join(&e.new)));
    }
    if !e.stuck.is_empty() {
        out.push(format!("stuck     {}", join(&e.stuck)));
    }
    if !e.chips.is_empty() {
        let chips: Vec<String> = e.chips.iter().map(|c| format!("{} {}/{WORD_WINDOW}", c.word, c.clean)).collect();
        out.push(format!("wordset   {}", join(&chips)));
    }
    if let Some(p) = &s.progress {
        out.push(progress_line(p, 10));
        if let Some(g) = &p.goal {
            out.push(goal_line(g, 10));
        }
    }
    if !s.vocab_line.is_empty() {
        out.push(format!("vocab  {}", s.vocab_line));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vocab::Word;

    fn word(status: Status, recent: &str, stuck: bool) -> Word {
        Word { status, recent: recent.to_owned(), stuck, ..Word::default() }
    }

    fn vocab(words: &[(&str, Word)]) -> Vocab {
        let mut v = Vocab::default();
        for (w, s) in words {
            v.words.insert((*w).to_owned(), s.clone());
        }
        v
    }

    #[test]
    fn compose_names_what_changed_between_two_snapshots() {
        let before = vocab(&[
            ("cat", word(Status::Learning, "111111111", false)),
            ("dog", word(Status::Learning, "0101010101", false)),
            ("owl", word(Status::Learning, "1111111111", false)),
            ("elk", word(Status::New, "", false)),
            ("fox", word(Status::Review, "", false)),
            ("gnu", word(Status::Learning, "0000000000", false)),
        ]);
        let after = vocab(&[
            ("cat", word(Status::Review, "1111111111", false)),
            ("dog", word(Status::Learning, "1010101011", false)),
            ("owl", word(Status::Retired, "1111111111", false)),
            ("elk", word(Status::Learning, "0", false)),
            ("fox", word(Status::Review, "", false)),
            ("gnu", word(Status::Learning, "0000000000", true)),
            ("hen", word(Status::Learning, "1", false)),
        ]);
        let e = compose(&before, &after, vec![("dog".into(), 2), ("elk".into(), 1)], 0.9);
        assert_eq!(e.mastered, ["cat"], "learning to review");
        assert_eq!(e.retired, ["owl"]);
        assert_eq!(e.new, ["elk", "hen"], "new on file and absent both count");
        assert_eq!(e.stuck, ["gnu"]);
        assert_eq!(e.stumbled, [("dog".to_owned(), 2), ("elk".to_owned(), 1)]);
        // the learning set without the stuck word, nearest to mastery first
        let chips: Vec<(&str, usize, Heat)> = e.chips.iter().map(|c| (c.word.as_str(), c.clean, c.heat)).collect();
        assert_eq!(chips, [("dog", 6, Heat::Yellow), ("hen", 1, Heat::Red), ("elk", 0, Heat::Red)]);
        // deterministic, and nothing changes between equal snapshots
        assert_eq!(e, compose(&before, &after, vec![("dog".into(), 2), ("elk".into(), 1)], 0.9));
        let quiet = compose(&after, &after, vec![], 0.9);
        assert!(quiet.mastered.is_empty() && quiet.new.is_empty() && quiet.retired.is_empty(), "{quiet:?}");
        assert_eq!(quiet.stuck, ["gnu"], "a stuck word is listed while it is stuck");
    }

    #[test]
    fn heat_runs_red_yellow_green_at_the_mastery_share() {
        assert_eq!(heat(0, 0.9), Heat::Red);
        assert_eq!(heat(2, 0.9), Heat::Red);
        assert_eq!(heat(3, 0.9), Heat::Yellow);
        assert_eq!(heat(8, 0.9), Heat::Yellow);
        assert_eq!(heat(9, 0.9), Heat::Green);
        assert_eq!(heat(10, 0.9), Heat::Green);
        assert_eq!(heat(8, 0.8), Heat::Green, "the share is the configured one");
    }

    #[test]
    fn small_pieces_of_text() {
        assert_eq!(clock(12.4), "12s");
        assert_eq!(clock(65.0), "1:05");
        assert_eq!(counted("find", 1), "find");
        assert_eq!(counted("find", 3), "find×3");
    }

    #[test]
    fn the_progress_lines_read_as_the_typist_asked() {
        let p = Progress { level: 1, label: "monkeytype 200".into(), known: 104, size: 200, all_complete: false, next: Some("monkeytype 1k".into()), goal: Some(("monkeytype 5k".into(), 120, 1000)) };
        assert_eq!(progress_line(&p, 10), "lv1  monkeytype 200  [█████░░░░░] 52%  104/200 known   next: monkeytype 1k");
        assert_eq!(goal_line(p.goal.as_ref().unwrap(), 10), "goal  monkeytype 5k  [█░░░░░░░░░] 12%  120/1000 known");
        let done = Progress { all_complete: true, next: None, ..p };
        assert!(progress_line(&done, 10).ends_with("every tier complete"));
    }

    #[test]
    fn the_text_summary_has_the_numbers_the_panels_and_the_bars() {
        let r = super::super::engine::Session::new(vec!["a".into()], false).results(1.0);
        let extra = Extra {
            stumbled: vec![("find".into(), 3), ("cat".into(), 1)],
            mastered: vec!["dog".into()],
            new: vec!["elk".into()],
            chips: vec![Chip { word: "hen".into(), clean: 7, heat: Heat::Yellow }],
            ..Extra::default()
        };
        let p = Progress { level: 1, label: "monkeytype 200".into(), known: 1, size: 2, all_complete: false, next: None, goal: None };
        let s = Screen::new(&r, "25 words  monkeytype 200".into(), Chars { correct: 118, incorrect: 3, extra: 0, missed: 0 }, Series::default(), extra, Some(p), HashMap::from([("find".to_owned(), "TPAOEUPB".to_owned())]));
        let text = text_lines(&s).join("\n");
        assert!(text.contains("characters 118/3/0/0") && text.contains("test 25 words  monkeytype 200"), "{text}");
        assert!(text.contains("stumbled  find×3 (TPAOEUPB) cat"), "{text}");
        assert!(text.contains("mastered  dog") && text.contains("new       elk") && text.contains("wordset   hen 7/10"), "{text}");
        assert!(text.contains("lv1  monkeytype 200") && !text.contains("stuck"), "{text}");
    }
}

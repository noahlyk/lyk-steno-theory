//! The run history: one JSON line per finished run in `practice.jsonl`.
//!
//! A line holds the numbers and the feed words that were missed, never anything typed that is
//! not a word of the feed.

use super::engine::Results;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Record {
    /// Seconds since the Unix epoch when the run ended
    pub timestamp: u64,
    pub source: String,
    /// Words typed (committed)
    pub count: usize,
    pub wpm: f64,
    pub raw: f64,
    pub accuracy: f64,
    pub errors: usize,
    pub secs: f64,
    /// The feed words that were missed, ordinary words only
    pub missed: Vec<String>,
    /// False when the run was quit early
    pub complete: bool,
}

impl Record {
    #[must_use]
    pub fn from_results(r: &Results, source: &str, timestamp: u64, complete: bool) -> Self {
        let round = |x: f64| (x * 100.0).round() / 100.0;
        Self {
            timestamp,
            source: source.to_owned(),
            count: r.words,
            wpm: round(r.wpm),
            raw: round(r.raw),
            accuracy: round(r.accuracy),
            errors: r.errors,
            secs: round(r.secs),
            missed: r.misses.iter().map(|m| m.word.clone()).filter(|w| super::feed::is_plain_word(w, true)).collect(),
            complete,
        }
    }
}

pub fn append(path: &Path, record: &Record) -> Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).with_context(|| dir.display().to_string())?;
    }
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).with_context(|| path.display().to_string())?;
    writeln!(file, "{}", serde_json::to_string(record)?)?;
    Ok(())
}

/// Every readable line, oldest first; a line that does not parse is skipped.
#[must_use]
pub fn read(path: &Path) -> Vec<Record> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

/// `YYYY-MM-DD HH:MM` (UTC) of a Unix time.
#[must_use]
pub fn stamp(t: u64) -> String {
    let (days, rest) = (t / 86_400, t % 86_400);
    // Howard Hinnant's civil-from-days
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {:02}:{:02}", rest / 3600, rest % 3600 / 60)
}

/// The recent runs and the trend, as text.
#[must_use]
pub fn report(records: &[Record], recent: usize) -> String {
    if records.is_empty() {
        return "no practice runs yet\n".to_owned();
    }
    let mut text = String::from("date (UTC)        source    words   wpm   raw   acc  err  time\n");
    for r in records.iter().skip(records.len().saturating_sub(recent)) {
        let _ = writeln!(
            text,
            "{}  {:<8}  {:>5}  {:>4.0}  {:>4.0}  {:>3.0}%  {:>3}  {:>3.0}s{}",
            stamp(r.timestamp),
            r.source,
            r.count,
            r.wpm,
            r.raw,
            r.accuracy,
            r.errors,
            r.secs,
            if r.complete { "" } else { "  (quit early)" }
        );
    }
    let done: Vec<&Record> = records.iter().filter(|r| r.complete && r.count > 0).collect();
    let mean = |runs: &[&Record]| runs.iter().map(|r| r.wpm).sum::<f64>() / runs.len() as f64;
    let _ = writeln!(text, "{} runs, {} complete", records.len(), done.len());
    if let Some(best) = done.iter().map(|r| r.wpm).max_by(f64::total_cmp) {
        let _ = writeln!(text, "best {best:.0} wpm");
    }
    if done.len() >= 4 {
        let n = (done.len() / 2).min(5);
        let (last, before) = (&done[done.len() - n..], &done[done.len() - 2 * n..done.len() - n]);
        let _ = writeln!(text, "last {n} runs {:.1} wpm, the {n} before {:.1}: {:+.1}", mean(last), mean(before), mean(last) - mean(before));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(t: u64, wpm: f64) -> Record {
        Record { timestamp: t, source: "common".into(), count: 50, wpm, raw: wpm + 2.0, accuracy: 97.0, errors: 1, secs: 40.0, missed: vec!["the".into()], complete: true }
    }

    #[test]
    fn dates() {
        assert_eq!(stamp(0), "1970-01-01 00:00");
        assert_eq!(stamp(1_700_000_000), "2023-11-14 22:13");
        assert_eq!(stamp(1_709_164_800 + 3600 * 5 + 60 * 7), "2024-02-29 05:07");
    }

    #[test]
    fn a_record_round_trips_through_a_file() {
        let dir = std::env::temp_dir().join(format!("lyk-practice-hist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("nested").join("practice.jsonl");
        append(&path, &rec(1, 50.0)).unwrap();
        append(&path, &rec(2, 60.0)).unwrap();
        std::fs::write(&path, format!("{}\nnot json\n", std::fs::read_to_string(&path).unwrap())).unwrap();
        assert_eq!(read(&path), vec![rec(1, 50.0), rec(2, 60.0)]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_ordinary_words_are_saved_as_missed() {
        let r = Results {
            secs: 10.0,
            wpm: 30.0,
            raw: 31.0,
            accuracy: 90.0,
            word_accuracy: 80.0,
            errors: 2,
            words: 10,
            consistency: None,
            slow: vec![],
            misses: vec![
                super::super::engine::Miss { word: "the".into(), typed: "teh".into() },
                super::super::engine::Miss { word: "it does, not.".into(), typed: "x".into() },
            ],
        };
        let rec = Record::from_results(&r, "phrases", 5, true);
        assert_eq!(rec.missed, ["the"]);
        assert!(!serde_json::to_string(&rec).unwrap().contains("teh"));
    }

    #[test]
    fn the_report_shows_recent_runs_and_a_trend() {
        let runs: Vec<Record> = (0..8).map(|i| rec(1_700_000_000 + i * 86_400, f64::from(u32::try_from(i).unwrap()).mul_add(2.0, 40.0))).collect();
        let text = report(&runs, 3);
        assert_eq!(text.lines().filter(|l| l.starts_with("2023-")).count(), 3, "{text}");
        assert!(text.contains("best 54 wpm"), "{text}");
        assert!(text.contains("+8.0"), "{text}");
        assert_eq!(report(&[], 5), "no practice runs yet\n");
    }
}

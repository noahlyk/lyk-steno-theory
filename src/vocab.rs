//! `vocab`: what the adaptive practice source knows about each word, saved in `vocab.json`.
//!
//! The file holds ordinary words with counts, a short window of right/wrong marks, a speed and
//! day numbers (days since 1970-01-01): nothing typed that was not a word of a feed, no tape text.
//! The rules that read and change it are in `practice::adaptive`; this is the data, its file, the
//! seeding from the typist's history and the `lyk vocab` command.

// Counts become shares and days: the casts are exact at these sizes.
#![allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use crate::mistakes::TapeWord;
use crate::paths;
use crate::practice::adaptive::{Config, accuracy_ok, due_reviews, window_accuracy};
use crate::practice::pool::{self, Candidates, Pool};
use anyhow::{Context, Result, bail};
use clap::{Args as ClapArgs, Subcommand};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Version of the file's format.
pub const VERSION: u32 = 1;
/// How many of the last results of a word are kept.
pub const WORD_WINDOW: usize = 10;
/// How many of the last results over all words are kept.
pub const GLOBAL_WINDOW: usize = 50;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Known to the file (counts from the past) but not introduced yet
    #[default]
    New,
    Learning,
    Review,
    /// Marked known or skipped: never shown
    Retired,
}

impl Status {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Learning => "learning",
            Self::Review => "review",
            Self::Retired => "retired",
        }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize, Default)]
pub struct Word {
    pub status: Status,
    pub attempts: u32,
    pub hits: u32,
    /// The last results, oldest first: `1` right, `0` wrong
    #[serde(default)]
    pub recent: String,
    /// Moving average of the time per word in milliseconds, over right answers
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed_ms: Option<f64>,
    #[serde(default)]
    pub last_seen: u32,
    /// The day a review word is next due
    #[serde(default)]
    pub due: u32,
    /// Review level, 1 to 6 (an index into the intervals); 0 outside review
    #[serde(default)]
    pub level: u8,
    /// Distinct days practised since it entered learning
    #[serde(default)]
    pub days: u32,
    #[serde(default)]
    pub promotions: u32,
    #[serde(default)]
    pub lapses: u32,
    /// Wrong answers in a row
    #[serde(default)]
    pub streak: u32,
    /// The day it was last mastered (entered review), when it has been
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mastered: Option<u32>,
    /// Corrections made while typing it, and attempts committed wrong, in all
    #[serde(default)]
    pub stumbles: u32,
    /// The test (its number in `sessions`) that last had it stumble or miss; 0 never
    #[serde(default)]
    pub stumbled_in: u32,
    /// Stumbles in that test
    #[serde(default)]
    pub stumbled_n: u32,
    /// Stumbles by day over the last week: `[day, count]`
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stumble_days: Vec<[u32; 2]>,
}

impl Word {
    /// Right answers in the window, and its length.
    #[must_use]
    pub fn window(&self) -> (usize, usize) {
        (self.recent.bytes().filter(|b| *b == b'1').count(), self.recent.len())
    }

    /// Share of wrong answers in the window (0 with none).
    #[must_use]
    pub fn error_rate(&self) -> f64 {
        let (ok, n) = self.window();
        if n == 0 { 0.0 } else { (n - ok) as f64 / n as f64 }
    }

    pub fn push_result(&mut self, ok: bool) {
        self.recent.push(if ok { '1' } else { '0' });
        if self.recent.len() > WORD_WINDOW {
            self.recent.remove(0);
        }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Vocab {
    pub version: u32,
    /// Whether the history has been read into it (an empty file does not re-seed)
    pub seeded: bool,
    /// Complete tests taken
    #[serde(default)]
    pub sessions: u32,
    /// The last results over all words, oldest first
    #[serde(default)]
    pub recent: String,
    /// The accuracy gate for new words as a share (0 to 1), when the typist set one
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<f64>,
    /// The day a new word last entered (0: not known yet)
    #[serde(default)]
    pub last_new: u32,
    pub words: BTreeMap<String, Word>,
}

impl Default for Vocab {
    fn default() -> Self {
        Self { version: VERSION, seeded: false, sessions: 0, recent: String::new(), gate: None, last_new: 0, words: BTreeMap::new() }
    }
}

impl Vocab {
    pub fn push_global(&mut self, ok: bool) {
        self.recent.push(if ok { '1' } else { '0' });
        if self.recent.len() > GLOBAL_WINDOW {
            self.recent.remove(0);
        }
    }

    #[must_use]
    pub fn count(&self, status: Status) -> usize {
        self.words.values().filter(|w| w.status == status).count()
    }

    /// Words the typist can type: learning, in review or retired (known).
    #[must_use]
    pub fn size(&self) -> usize {
        self.words.values().filter(|w| matches!(w.status, Status::Learning | Status::Review | Status::Retired)).count()
    }

    /// Reads the file; a missing file is an empty, unseeded vocabulary.
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let v: Self = serde_json::from_str(&text).with_context(|| format!("{}: not a vocab file", path.display()))?;
                if v.version > VERSION {
                    bail!("{}: written by a newer lyk (format {}, this reads {VERSION})", path.display(), v.version);
                }
                Ok(v)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| path.display().to_string()),
        }
    }

    /// Writes the file whole, through a temporary name, so a crash never leaves half of it.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).with_context(|| dir.display().to_string())?;
        }
        let part = path.with_extension("json.part");
        std::fs::write(&part, serde_json::to_string(self)?).with_context(|| part.display().to_string())?;
        std::fs::rename(&part, path).with_context(|| path.display().to_string())
    }
}

// ---------------------------------------------------------------------------------------------
// Days

/// The local time's offset from UTC in seconds, from `date +%z`; 0 when that cannot be read.
#[must_use]
pub fn local_offset() -> i64 {
    let Ok(out) = std::process::Command::new("date").arg("+%z").output() else { return 0 };
    let text = String::from_utf8_lossy(&out.stdout);
    let t = text.trim();
    if t.len() != 5 {
        return 0;
    }
    let sign = if t.starts_with('-') { -1 } else { 1 };
    let (h, m) = (t[1..3].parse::<i64>().unwrap_or(0), t[3..5].parse::<i64>().unwrap_or(0));
    sign * (h * 3600 + m * 60)
}

/// The day number (days since 1970-01-01) of a Unix time in the local zone, `offset` seconds from UTC.
#[must_use]
pub fn day_of(unix: u64, offset: i64) -> u32 {
    (i64::try_from(unix).unwrap_or(0) + offset).div_euclid(86_400).max(0) as u32
}

/// `YYYY-MM-DD` of a day number.
#[must_use]
pub fn date(day: u32) -> String {
    // Howard Hinnant's civil-from-days
    let z = i64::from(day) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{m:02}-{d:02}", yoe + era * 400 + i64::from(m <= 2))
}

// ---------------------------------------------------------------------------------------------
// Seeding

/// Fewest tape attempts for a word to start in review or to count towards the median error rate.
pub const SEED_REVIEW_ATTEMPTS: usize = 20;
/// A word typed this cleanly (error share at most this) starts at review level 2.
pub const SEED_CLEAN_ERRORS: f64 = 0.20;
/// The words typed most on the tape (this many) that are also clean start at review level 3.
pub const SEED_TOP_WORDS: usize = 200;
/// Fewest tape attempts for a word to start in learning.
pub const SEED_LEARNING_ATTEMPTS: usize = 5;
/// The median error share is capped here, so a very rough tape does not make every word known.
pub const SEED_MEDIAN_CAP: f64 = 0.5;

fn recent_from(attempts: usize, errors: usize) -> String {
    let n = attempts.min(WORD_WINDOW);
    let bad = ((errors as f64 / attempts.max(1) as f64) * n as f64).round() as usize;
    let mut s = "1".repeat(n - bad.min(n));
    s.push_str(&"0".repeat(bad.min(n)));
    s
}

/// The typist's median error share over the tape words with 20 or more attempts (their overall
/// share when there are none), capped at `SEED_MEDIAN_CAP`.
fn median_error_rate(tape: &[TapeWord]) -> f64 {
    let mut rates: Vec<f64> = tape.iter().filter(|t| t.attempts >= SEED_REVIEW_ATTEMPTS).map(|t| t.errors as f64 / t.attempts as f64).collect();
    rates.sort_by(f64::total_cmp);
    let median = if rates.is_empty() {
        let (a, e) = tape.iter().fold((0, 0), |(a, e), t| (a + t.attempts, e + t.errors));
        if a == 0 { 0.0 } else { e as f64 / a as f64 }
    } else {
        rates[(rates.len() - 1) / 2]
    };
    median.min(SEED_MEDIAN_CAP)
}

/// A vocabulary seeded from what the typist has done: the tape's per-word counts (`tape`) and the
/// words missed in past practice runs (`missed`, one entry per miss). The tape's error share is
/// the typist's own baseline (corrections show as undos), so the thresholds are relative to it:
///
/// - 20 or more attempts and an error share at or below the typist's median: review, level 1; at
///   most 20% wrong: level 2; among the 200 most typed words and at most 20% wrong: level 3.
///   Due one interval after the last use.
/// - 5 or more attempts and an error share above the median, and not in review: the
///   `cfg.learning_cap` with the most errors start in learning.
/// - Every other word with a history is kept as new with its counts, so it is introduced in pool
///   order.
#[must_use]
pub fn seed(tape: &[TapeWord], missed: &[String], cfg: &Config, today: u32) -> Vocab {
    let mut v = Vocab { seeded: true, ..Vocab::default() };
    let day_of_tape = |t: &TapeWord| t.last_day.map_or(today, |d| u32::try_from(d.max(0)).unwrap_or(today).min(today));
    let median = median_error_rate(tape);
    let mut by_use: Vec<&TapeWord> = tape.iter().filter(|t| t.attempts > 0).collect();
    by_use.sort_by(|a, b| b.attempts.cmp(&a.attempts).then(a.word.cmp(&b.word)));
    let top: std::collections::HashSet<&str> = by_use.iter().take(SEED_TOP_WORDS).map(|t| t.word.as_str()).collect();
    let mut learning: Vec<&TapeWord> = Vec::new();
    for t in tape.iter().filter(|t| t.attempts > 0) {
        let rate = t.errors as f64 / t.attempts as f64;
        let last = day_of_tape(t);
        let hits = (t.attempts - t.errors.min(t.attempts)) as u32;
        let mut w = Word { attempts: t.attempts as u32, hits, last_seen: last, days: t.days as u32, recent: recent_from(t.attempts, t.errors), ..Word::default() };
        if t.attempts >= SEED_REVIEW_ATTEMPTS && rate <= median {
            w.status = Status::Review;
            w.level = if rate <= SEED_CLEAN_ERRORS { if top.contains(t.word.as_str()) { 3 } else { 2 } } else { 1 };
            w.due = last + cfg.intervals[usize::from(w.level) - 1];
        } else if t.attempts >= SEED_LEARNING_ATTEMPTS && rate > median {
            learning.push(t);
        }
        v.words.insert(t.word.clone(), w);
    }
    learning.sort_by(|a, b| b.errors.cmp(&a.errors).then(a.word.cmp(&b.word)));
    for t in learning.into_iter().take(cfg.learning_cap) {
        if let Some(w) = v.words.get_mut(&t.word) {
            w.status = Status::Learning;
        }
    }
    // A word missed in a past run and not on the tape: its misses are all that is known.
    let on_tape: std::collections::HashSet<&str> = tape.iter().map(|t| t.word.as_str()).collect();
    for m in missed.iter().filter(|m| !on_tape.contains(m.as_str())) {
        let w = v.words.entry(m.clone()).or_default();
        w.attempts += 1;
        w.push_result(false);
        w.last_seen = today;
    }
    v.last_new = today;
    v
}

/// `base` with the accuracy gate from `flag` (percent) or else the one saved in the vocabulary.
#[must_use]
pub fn with_gate(base: &Config, vocab: &Vocab, flag: Option<f64>) -> Config {
    let gate = flag.map(|p| (p / 100.0).clamp(0.0, 1.0)).or(vocab.gate);
    Config { accuracy_threshold: gate.unwrap_or(base.accuracy_threshold), ..base.clone() }
}

// ---------------------------------------------------------------------------------------------
// The command

#[derive(ClapArgs, Debug)]
pub struct Args {
    #[command(subcommand)]
    action: Option<Action>,
    /// Print the summary as JSON
    #[arg(long, global = true)]
    json: bool,
    /// Which word lists the frontier is measured in (as `practice --wordlist`)
    #[arg(long, value_enum, default_value = "both", global = true)]
    wordlist: pool::Wordlist,
    /// Weight of Monkeytype's lists (as `practice --monkeytype-weight`)
    #[arg(long, default_value_t = pool::DEFAULT_WEIGHT, global = true)]
    monkeytype_weight: f64,
    /// Count names as words
    #[arg(long, global = true)]
    names: bool,
}

#[derive(Subcommand, Debug)]
enum Action {
    /// Mark words as known: never shown, never introduced again
    Know { words: Vec<String> },
    /// Skip words: never shown, never introduced again
    Skip { words: Vec<String> },
    /// Remove words from the file (they can be introduced again)
    Forget { words: Vec<String> },
    /// Empty the file: every word is new again (and the history is not read again)
    Reset {
        /// Do it without asking
        #[arg(long)]
        yes: bool,
    },
    /// Set the accuracy gate for new words: the clean share (percent, 0 to 100) of the last 50
    /// attempts needed to earn one, or `default`
    Gate {
        /// A percent such as 70, or `default`
        percent: String,
    },
    /// Read the practice history and the steno tape into the words not in the file yet
    Seed {
        /// The steno tape
        #[arg(long, default_value = "~/.local/state/keymux/steno-tape.txt")]
        tape: PathBuf,
        /// The built dictionary (only its words count as ordinary)
        #[arg(long, default_value = paths::built_dir_text())]
        dict: PathBuf,
    },
}

/// `vocab.json` in the state folder.
#[must_use]
pub fn file() -> PathBuf {
    paths::vocab_file()
}

/// Missed words of the complete runs in the practice history, one entry per miss.
#[must_use]
pub fn history_misses(path: &Path) -> Vec<String> {
    crate::practice::history_misses(path)
}

/// Seeds `vocab` from the history and tape when it has not been seeded; a missing tape or
/// dictionary only means less is read. Returns the number of words added.
pub fn ensure_seeded(vocab: &mut Vocab, tape: &Path, dict: &Path, cfg: &Config, today: u32) -> usize {
    if vocab.seeded {
        return 0;
    }
    let before = vocab.words.len();
    merge_seed(vocab, tape, dict, cfg, today);
    vocab.words.len() - before
}

fn merge_seed(vocab: &mut Vocab, tape: &Path, dict: &Path, cfg: &Config, today: u32) {
    let stats = crate::mistakes::word_stats(tape, dict).unwrap_or_default();
    let seeded = seed(&stats, &history_misses(&paths::practice_history()), cfg, today);
    for (word, state) in seeded.words {
        vocab.words.entry(word).or_insert(state);
    }
    vocab.seeded = true;
}

fn expand_home(path: &Path) -> PathBuf {
    match (path.strip_prefix("~"), std::env::var_os("HOME")) {
        (Ok(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => path.to_path_buf(),
    }
}

pub fn run(args: &Args) -> Result<()> {
    let path = file();
    let today = day_of(now(), local_offset());
    let mut vocab = Vocab::load(&path)?;
    let cfg = with_gate(&Config::default(), &vocab, None);
    let lower = |words: &[String]| words.iter().map(|w| w.to_lowercase()).collect::<Vec<_>>();
    match &args.action {
        None => {
            print!("{}", summary(&vocab, &cfg, today, &frontier(args)?, args.json)?);
            return Ok(());
        }
        Some(Action::Know { words } | Action::Skip { words }) => {
            let retired = lower(words);
            for w in &retired {
                let e = vocab.words.entry(w.clone()).or_default();
                e.status = Status::Retired;
                e.level = 0;
                e.due = 0;
                e.mastered = None;
            }
            println!("{} retired: {}", retired.len(), retired.join(" "));
        }
        Some(Action::Forget { words }) => {
            let gone = lower(words).into_iter().filter(|w| vocab.words.remove(w).is_some()).count();
            println!("{gone} forgotten");
        }
        Some(Action::Reset { yes }) => {
            if !yes {
                bail!("`lyk vocab reset` empties {} ({} words); run it again with --yes to do it", path.display(), vocab.words.len());
            }
            vocab = Vocab { seeded: true, ..Vocab::default() };
            println!("{} emptied", path.display());
        }
        Some(Action::Gate { percent }) => {
            if percent == "default" {
                vocab.gate = None;
                println!("gate back to the default, {:.0}%", Config::default().accuracy_threshold * 100.0);
            } else {
                let p: f64 = percent.trim_end_matches('%').parse().ok().filter(|p| (0.0..=100.0).contains(p)).with_context(|| format!("{percent:?} is not a percent from 0 to 100 (or `default`)"))?;
                vocab.gate = Some(p / 100.0);
                println!("gate set to {p:.0}%: new words need that share of clean answers over the last {GLOBAL_WINDOW} attempts");
            }
        }
        Some(Action::Seed { tape, dict }) => {
            let before = vocab.words.len();
            merge_seed(&mut vocab, &expand_home(tape), dict, &cfg, today);
            println!("{} words added from the history and the tape", vocab.words.len() - before);
        }
    }
    vocab.save(&path)
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The pool's next un-introduced word and its rank.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Frontier {
    pub rank: usize,
    pub next: Option<String>,
}

/// The first pool word that is absent from `vocab` or still new, and its 1-based rank.
pub fn find_frontier(vocab: &Vocab, pool: &mut dyn Candidates) -> Frontier {
    let mut i = 0;
    while let Some(w) = pool.get(i) {
        if vocab.words.get(w).is_none_or(|s| s.status == Status::New) {
            return Frontier { rank: i + 1, next: Some(w.to_owned()) };
        }
        i += 1;
    }
    Frontier { rank: i + 1, next: None }
}

fn frontier(args: &Args) -> Result<Frontier> {
    let data = paths::data_dir();
    let opts = pool::Opts { wordlist: args.wordlist, weight: args.monkeytype_weight, names: args.names };
    let Ok(mut p) = Pool::open(&data, &opts) else { return Ok(Frontier::default()) };
    let vocab = Vocab::load(&file())?;
    Ok(find_frontier(&vocab, &mut p))
}

const LIST: usize = 15;

/// Days without a new word before `lyk vocab` says why.
pub const STUCK_DAYS: u32 = 14;

/// The words with stumbles over the last 7 days, most first.
fn stumbled_this_week(vocab: &Vocab, today: u32) -> Vec<(&String, u32)> {
    let mut v: Vec<(&String, u32)> = vocab
        .words
        .iter()
        .map(|(w, s)| (w, s.stumble_days.iter().filter(|d| d[0] + 7 > today && d[0] <= today).map(|d| d[1]).sum::<u32>()))
        .filter(|(_, n)| *n > 0)
        .collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    v
}

/// Why no new word has entered for more than `STUCK_DAYS` days, when that is so.
fn stuck_reason(vocab: &Vocab, cfg: &Config, today: u32) -> Option<String> {
    if vocab.sessions == 0 || vocab.last_new == 0 || today.saturating_sub(vocab.last_new) <= STUCK_DAYS {
        return None;
    }
    let learning = vocab.count(Status::Learning);
    if learning >= cfg.learning_cap {
        return Some(format!("the learning pool is full ({learning} of {}); master them or `lyk vocab know` the ones you do not want", cfg.learning_cap));
    }
    if !accuracy_ok(vocab, cfg) {
        return Some(format!(
            "clean accuracy {:.0}% is under the gate, {:.0}%, over the last {GLOBAL_WINDOW}; `lyk vocab gate N` sets a lower one",
            window_accuracy(vocab).unwrap_or(0.0) * 100.0,
            cfg.accuracy_threshold * 100.0
        ));
    }
    Some("the gate is met and the pool has room: run a test (the next new word comes in it)".to_owned())
}

/// The summary: counts, size, frontier, the learning pool, the next due reviews, the recently
/// mastered words and the lapses, as text or JSON.
pub fn summary(vocab: &Vocab, cfg: &Config, today: u32, frontier: &Frontier, json: bool) -> Result<String> {
    let learning: Vec<(&String, &Word)> = vocab.words.iter().filter(|(_, w)| w.status == Status::Learning).collect();
    let mut reviews: Vec<(&String, &Word)> = vocab.words.iter().filter(|(_, w)| w.status == Status::Review).collect();
    reviews.sort_by(|a, b| a.1.due.cmp(&b.1.due).then(a.0.cmp(b.0)));
    let mut mastered: Vec<(&String, &Word)> = vocab.words.iter().filter(|(_, w)| w.mastered.is_some() && w.status != Status::Retired).collect();
    mastered.sort_by(|a, b| b.1.mastered.cmp(&a.1.mastered).then(a.0.cmp(b.0)));
    let mut lapsed: Vec<(&String, &Word)> = vocab.words.iter().filter(|(_, w)| w.lapses > 0).collect();
    lapsed.sort_by(|a, b| b.1.lapses.cmp(&a.1.lapses).then(a.0.cmp(b.0)));
    let due = due_reviews(vocab, today).len();
    let accuracy = window_accuracy(vocab);
    let stumble_rate = accuracy.map(|a| 1.0 - a);
    let week = stumbled_this_week(vocab, today);
    let stuck = stuck_reason(vocab, cfg, today);
    if json {
        let item = |(w, s): &(&String, &Word)| serde_json::json!({"word": w, "level": s.level, "due": (s.status == Status::Review).then(|| date(s.due)), "accuracy": 1.0 - s.error_rate(), "lapses": s.lapses});
        let value = serde_json::json!({
            "new": vocab.count(Status::New), "learning": vocab.count(Status::Learning), "review": vocab.count(Status::Review),
            "retired": vocab.count(Status::Retired), "vocabulary": vocab.size(), "sessions": vocab.sessions,
            "frontier_rank": frontier.rank, "next_word": frontier.next, "recent_accuracy": accuracy, "due_now": due,
            "learning_cap": cfg.learning_cap, "gate": cfg.accuracy_threshold, "stumble_rate": stumble_rate,
            "stumbled_this_week": week.iter().take(LIST).map(|(w, n)| serde_json::json!({"word": w, "stumbles": n})).collect::<Vec<_>>(),
            "no_new_words": stuck,
            "learning_pool": learning.iter().map(item).collect::<Vec<_>>(),
            "next_due": reviews.iter().take(LIST).map(item).collect::<Vec<_>>(),
            "recently_mastered": mastered.iter().take(LIST).map(|(w, s)| serde_json::json!({"word": w, "date": s.mastered.map(date)})).collect::<Vec<_>>(),
            "lapses": lapsed.iter().take(LIST).map(item).collect::<Vec<_>>(),
        });
        return Ok(serde_json::to_string_pretty(&value)? + "\n");
    }
    let mut out = String::new();
    let _ = writeln!(
        out,
        "vocabulary {} words (learning {} + review {} + retired {});  {} new on file;  {} tests",
        vocab.size(),
        vocab.count(Status::Learning),
        vocab.count(Status::Review),
        vocab.count(Status::Retired),
        vocab.count(Status::New),
        vocab.sessions
    );
    let _ = match &frontier.next {
        Some(w) => writeln!(out, "frontier: rank {} is next ({w}); every word above it is learning, in review, retired or on file", frontier.rank),
        None => writeln!(out, "frontier: no word list loaded or every word introduced"),
    };
    let _ = writeln!(
        out,
        "recent clean accuracy {}  (new words need {:.0}% over the last {};  stumble rate {})",
        accuracy.map_or_else(|| "none yet".to_owned(), |a| format!("{:.0}%", a * 100.0)),
        cfg.accuracy_threshold * 100.0,
        crate::vocab::GLOBAL_WINDOW,
        stumble_rate.map_or_else(|| "none yet".to_owned(), |r| format!("{:.0}%", r * 100.0))
    );
    if let Some(why) = &stuck {
        let _ = writeln!(out, "no new word for {} days: {why}", today.saturating_sub(vocab.last_new));
    }
    let _ = writeln!(out, "\nlearning ({} of {}):", learning.len(), cfg.learning_cap);
    for (w, s) in &learning {
        let (ok, n) = s.window();
        let _ = writeln!(out, "  {w:<16} {ok}/{n} recent   {} days   {} attempts", s.days, s.attempts);
    }
    let _ = writeln!(out, "\nreview: {due} due now; next due:");
    for (w, s) in reviews.iter().take(LIST) {
        let _ = writeln!(out, "  {w:<16} level {}   {}", s.level, date(s.due));
    }
    if !mastered.is_empty() {
        let _ = writeln!(out, "\nrecently mastered:");
        for (w, s) in mastered.iter().take(LIST) {
            let _ = writeln!(out, "  {w:<16} {}", s.mastered.map_or_else(String::new, date));
        }
    }
    if !week.is_empty() {
        let _ = writeln!(out, "\nstumbled most this week (corrections before the commit):");
        for (w, n) in week.iter().take(LIST) {
            let _ = writeln!(out, "  {w:<16} {n}");
        }
    }
    if !lapsed.is_empty() {
        let _ = writeln!(out, "\nlapses:");
        for (w, s) in lapsed.iter().take(LIST) {
            let _ = writeln!(out, "  {w:<16} {}   now {}", s.lapses, s.status.name());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tw(word: &str, attempts: usize, errors: usize, days: usize, last: i64) -> TapeWord {
        TapeWord { word: word.into(), attempts, errors, days, last_day: Some(last) }
    }

    #[test]
    fn days_and_dates() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(19_723), "2024-01-01");
        assert_eq!(day_of(86_400 * 3 + 5, 0), 3);
        assert_eq!(day_of(86_400 * 3 + 5, -3600 * 8), 2, "evening in the west is still the day before in UTC terms");
        assert_eq!(day_of(0, -3600), 0, "never below day 0");
    }

    #[test]
    fn the_file_round_trips_and_a_missing_one_is_empty() {
        let dir = std::env::temp_dir().join(format!("lyk-vocab-file-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("a").join("vocab.json");
        assert_eq!(Vocab::load(&path).unwrap(), Vocab::default());
        let mut v = Vocab { seeded: true, ..Vocab::default() };
        v.words.insert("cat".into(), Word { status: Status::Review, level: 2, due: 9, attempts: 3, hits: 3, recent: "111".into(), speed_ms: Some(812.5), mastered: Some(4), ..Word::default() });
        v.push_global(true);
        v.save(&path).unwrap();
        assert_eq!(Vocab::load(&path).unwrap(), v);
        assert!(!dir.join("a").join("vocab.json.part").exists());
        std::fs::write(&path, r#"{"version":99,"seeded":true,"words":{}}"#).unwrap();
        assert!(Vocab::load(&path).unwrap_err().to_string().contains("newer lyk"));
        std::fs::write(&path, "nope").unwrap();
        assert!(Vocab::load(&path).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn windows_are_capped() {
        let mut v = Vocab::default();
        let mut w = Word::default();
        for i in 0..30 {
            w.push_result(i % 2 == 0);
            v.push_global(true);
        }
        assert_eq!(w.recent.len(), WORD_WINDOW);
        assert_eq!(v.recent.len(), 30);
        for _ in 0..40 {
            v.push_global(false);
        }
        assert_eq!(v.recent.len(), GLOBAL_WINDOW);
        assert!((w.error_rate() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn seeding_sorts_tape_words_by_their_numbers_relative_to_the_typists_median() {
        let cfg = Config::default();
        let tape = [
            tw("the", 400, 4, 30, 100), // 1%: among the most typed and clean: review level 3
            tw("cat", 20, 1, 5, 100),   // 5%: review
            tw("late", 30, 6, 9, 100),  // 20%: the median of the five words with 20 attempts: review
            tw("mid", 40, 12, 9, 100),  // 30%: above the median: learning
            tw("bad", 25, 15, 9, 100),  // 60%: learning
            tw("dog", 19, 0, 5, 100),   // too few attempts for review: new with counts
            tw("lead", 8, 5, 3, 100),   // 62% on 8 attempts: learning
            tw("rare", 3, 1, 1, 100),   // few attempts: new
            tw("zero", 0, 0, 0, 100),   // nothing: not on file
        ];
        let v = seed(&tape, &["bad".into(), "lead".into(), "ghost".into(), "ghost".into()], &cfg, 105);
        assert!(v.seeded);
        assert_eq!(v.last_new, 105, "the gate's clock starts at the seeding");
        let the = &v.words["the"];
        assert_eq!((the.status, the.level, the.due, the.last_seen, the.days), (Status::Review, 3, 107, 100, 30), "level 3 = the 7-day interval after the last use");
        assert_eq!((v.words["cat"].status, v.words["cat"].level), (Status::Review, 3), "with under 200 words every clean word is among the most typed");
        assert_eq!(v.words["late"].status, Status::Review);
        assert_eq!(v.words["mid"].status, Status::Learning);
        assert_eq!(v.words["bad"].status, Status::Learning);
        assert_eq!(v.words["lead"].status, Status::Learning);
        assert_eq!(v.words["dog"].status, Status::New);
        assert_eq!((v.words["dog"].attempts, v.words["dog"].hits), (19, 19));
        assert_eq!(v.words["rare"].status, Status::New);
        assert!(!v.words.contains_key("zero"));
        assert_eq!(v.words["bad"].attempts, 25, "a missed word already on the tape is not counted twice");
        let ghost = &v.words["ghost"];
        assert_eq!((ghost.status, ghost.attempts, ghost.hits, ghost.recent.as_str()), (Status::New, 2, 0, "00"));
    }

    #[test]
    fn seeded_review_levels_follow_the_error_share_and_the_typing_frequency() {
        let cfg = Config::default();
        // 210 constantly typed words at 30% wrong make the median 30%
        let mut tape: Vec<TapeWord> = (0..210).map(|i| tw(&format!("f{i:03}"), 500, 150, 20, 100)).collect();
        tape.push(tw("cat", 20, 1, 5, 100)); // 5%, rarely typed: level 2
        tape.push(tw("sure", 20, 5, 5, 100)); // 25%, below the median: level 1
        tape.push(tw("hard", 20, 8, 5, 100)); // 40%, above it: learning
        tape.push(tw("easy", 900, 90, 20, 100)); // 10% and typed most: level 3
        let v = seed(&tape, &[], &cfg, 105);
        let lv = |w: &str| (v.words[w].status, v.words[w].level);
        assert_eq!(lv("cat"), (Status::Review, 2));
        assert_eq!(lv("sure"), (Status::Review, 1));
        assert_eq!(lv("easy"), (Status::Review, 3));
        assert_eq!(lv("f000"), (Status::Review, 1), "at the median counts as typical");
        assert_eq!(lv("hard"), (Status::Learning, 0));
        assert_eq!(v.count(Status::Learning), 1);
    }

    #[test]
    fn seeded_learning_obeys_the_cap_worst_first() {
        let cfg = Config { learning_cap: 2, ..Config::default() };
        let tape = [tw("a1", 10, 5, 3, 1), tw("b1", 10, 9, 3, 1), tw("c1", 10, 7, 3, 1), tw("d1", 10, 6, 3, 1)];
        let v = seed(&tape, &[], &cfg, 5);
        assert_eq!(v.count(Status::Learning), 2);
        assert_eq!(v.words["b1"].status, Status::Learning);
        assert_eq!(v.words["c1"].status, Status::Learning);
        assert_eq!(v.words["d1"].status, Status::New);
        assert_eq!(v.words["a1"].status, Status::New, "at the median (capped at 50%) is not worse than typical");
        // the default pool is 10
        let many: Vec<TapeWord> = (0..40).map(|i| tw(&format!("w{i:02}"), 10, 6 + i % 4, 3, 1)).collect();
        assert_eq!(seed(&many, &[], &Config::default(), 5).count(Status::Learning), 10);
        assert!(seed(&[], &[], &Config::default(), 5).words.is_empty());
    }

    #[test]
    fn an_undatable_tape_counts_from_today_and_never_the_future() {
        let cfg = Config::default();
        let mut undated = tw("the", 50, 0, 3, 0);
        undated.last_day = None;
        let future = tw("of", 50, 0, 3, 9999);
        let v = seed(&[undated, future], &[], &cfg, 100);
        assert_eq!(v.words["the"].last_seen, 100);
        assert_eq!(v.words["of"].last_seen, 100);
    }

    #[test]
    fn seeded_files_hold_words_and_numbers_only() {
        let cfg = Config::default();
        let v = seed(&[tw("the", 50, 1, 3, 10)], &["cat".into()], &cfg, 20);
        let text = serde_json::to_string(&v).unwrap();
        // keys of the object are the words; every value is a number, a short mark string or a status
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        for (_, w) in value["words"].as_object().unwrap() {
            for (k, x) in w.as_object().unwrap() {
                assert!(
                    x.is_number() || (k == "status" && x.is_string()) || (k == "recent" && x.as_str().is_some_and(|s| s.chars().all(|c| c == '0' || c == '1'))),
                    "{k}: {x}"
                );
            }
        }
    }

    #[test]
    fn ensure_seeded_reads_once_and_a_missing_tape_leaves_it_empty() {
        let mut v = Vocab::default();
        let n = ensure_seeded(&mut v, Path::new("/no/such/tape"), Path::new("/no/dict"), &Config::default(), 10);
        assert!(v.seeded);
        assert_eq!(n, v.words.len());
        v.words.insert("kept".into(), Word::default());
        assert_eq!(ensure_seeded(&mut v, Path::new("/no/such/tape"), Path::new("/no/dict"), &Config::default(), 10), 0, "already seeded");
    }

    #[test]
    fn the_summary_lists_the_pool_the_due_words_and_the_lapses() {
        let cfg = Config::default();
        let mut v = Vocab::default();
        v.words.insert("cat".into(), Word { status: Status::Learning, attempts: 4, hits: 3, recent: "1101".into(), days: 2, ..Word::default() });
        v.words.insert("dog".into(), Word { status: Status::Review, level: 3, due: 50, mastered: Some(40), lapses: 1, ..Word::default() });
        v.words.insert("owl".into(), Word { status: Status::Review, level: 1, due: 60, mastered: Some(45), ..Word::default() });
        v.words.insert("zed".into(), Word { status: Status::Retired, ..Word::default() });
        let f = Frontier { rank: 12, next: Some("horse".into()) };
        let text = summary(&v, &cfg, 55, &f, false).unwrap();
        assert!(text.contains("vocabulary 4 words"), "{text}");
        assert!(text.contains("rank 12 is next (horse)"), "{text}");
        assert!(text.contains("learning (1 of 10)") && text.contains("cat"), "{text}");
        assert!(text.contains("1 due now"), "{text}");
        assert!(text.contains("lapses:") && text.contains("dog"), "{text}");
        let json: serde_json::Value = serde_json::from_str(&summary(&v, &cfg, 55, &f, true).unwrap()).unwrap();
        assert_eq!(json["vocabulary"], 4);
        assert_eq!(json["frontier_rank"], 12);
        assert_eq!(json["next_due"][0]["word"], "dog");
        assert_eq!(json["due_now"], 1);
    }

    #[test]
    fn the_gate_comes_from_the_flag_then_the_file_then_the_default() {
        let base = Config::default();
        let mut v = Vocab::default();
        assert!((with_gate(&base, &v, None).accuracy_threshold - 0.75).abs() < 1e-9);
        v.gate = Some(0.6);
        assert!((with_gate(&base, &v, None).accuracy_threshold - 0.6).abs() < 1e-9);
        assert!((with_gate(&base, &v, Some(90.0)).accuracy_threshold - 0.9).abs() < 1e-9, "the flag wins");
        assert!((with_gate(&base, &v, Some(250.0)).accuracy_threshold - 1.0).abs() < 1e-9, "clamped");
        // and it is kept in the file
        let text = serde_json::to_string(&v).unwrap();
        assert_eq!(serde_json::from_str::<Vocab>(&text).unwrap().gate, Some(0.6));
        assert!(!serde_json::to_string(&Vocab::default()).unwrap().contains("gate"));
    }

    #[test]
    fn a_long_wait_for_a_new_word_says_why() {
        let cfg = Config::default();
        let mut v = Vocab { sessions: 12, last_new: 100, recent: "1".repeat(30) + &"0".repeat(20), ..Vocab::default() };
        assert!(stuck_reason(&v, &cfg, 110).is_none(), "ten days is not long");
        let why = stuck_reason(&v, &cfg, 120).expect("20 days with 60% against a 75% gate");
        assert!(why.contains("60%") && why.contains("75%") && why.contains("lyk vocab gate"), "{why}");
        let f = Frontier { rank: 4, next: Some("x".into()) };
        let text = summary(&v, &cfg, 120, &f, false).unwrap();
        assert!(text.contains("no new word for 20 days") && text.contains("lyk vocab gate"), "{text}");
        // a lower gate lets it through
        v.gate = Some(0.5);
        let low = with_gate(&cfg, &v, None);
        assert!(stuck_reason(&v, &low, 120).is_some_and(|w| w.contains("gate is met")));
        // a full pool is another reason
        for i in 0..10 {
            v.words.insert(format!("l{i}"), Word { status: Status::Learning, ..Word::default() });
        }
        assert!(stuck_reason(&v, &low, 120).is_some_and(|w| w.contains("pool is full")));
        // never a test taken, or a new word lately: nothing to say
        assert!(stuck_reason(&Vocab::default(), &cfg, 500).is_none());
        let json: serde_json::Value = serde_json::from_str(&summary(&v, &low, 120, &f, true).unwrap()).unwrap();
        assert!(json["no_new_words"].is_string());
    }

    #[test]
    fn the_summary_shows_the_stumble_rate_and_the_words_stumbled_most_this_week() {
        let cfg = Config::default();
        let mut v = Vocab { sessions: 3, recent: "1110".into(), last_new: 100, ..Vocab::default() };
        v.words.insert("cat".into(), Word { status: Status::Learning, stumbles: 5, stumble_days: vec![[98, 1], [100, 3]], ..Word::default() });
        v.words.insert("dog".into(), Word { status: Status::Learning, stumbles: 1, stumble_days: vec![[100, 1]], ..Word::default() });
        v.words.insert("old".into(), Word { status: Status::Learning, stumbles: 9, stumble_days: vec![[90, 9]], ..Word::default() });
        let f = Frontier::default();
        let text = summary(&v, &cfg, 101, &f, false).unwrap();
        assert!(text.contains("stumble rate 25%"), "{text}");
        let week = text.split("stumbled most this week").nth(1).expect("the list is there");
        assert!(week.find("cat").unwrap() < week.find("dog").unwrap() && !week.contains("old"), "{text}");
        let json: serde_json::Value = serde_json::from_str(&summary(&v, &cfg, 101, &f, true).unwrap()).unwrap();
        assert_eq!(json["stumbled_this_week"][0]["word"], "cat");
        assert_eq!(json["stumbled_this_week"][0]["stumbles"], 4);
        assert!((json["stumble_rate"].as_f64().unwrap() - 0.25).abs() < 1e-9);
    }

    #[test]
    fn the_frontier_is_the_first_word_not_yet_introduced() {
        let mut v = Vocab::default();
        for (w, s) in [("a1", Status::Review), ("b1", Status::Retired), ("d1", Status::Learning), ("c1", Status::New)] {
            v.words.insert(w.into(), Word { status: s, ..Word::default() });
        }
        let mut pool: Vec<String> = ["a1", "b1", "c1", "d1", "e1"].map(String::from).to_vec();
        assert_eq!(find_frontier(&v, &mut pool), Frontier { rank: 3, next: Some("c1".into()) }, "a word on file as new is not behind the frontier");
        pool.truncate(2);
        assert_eq!(find_frontier(&v, &mut pool), Frontier { rank: 3, next: None });
    }
}

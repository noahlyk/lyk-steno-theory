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
use crate::practice::adaptive::{Config, due_reviews, window_accuracy};
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
    pub words: BTreeMap<String, Word>,
}

impl Default for Vocab {
    fn default() -> Self {
        Self { version: VERSION, seeded: false, sessions: 0, recent: String::new(), words: BTreeMap::new() }
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

/// Words typed at least this often on the tape, with at most `SEED_REVIEW_ERRORS` wrong, start in review.
pub const SEED_REVIEW_ATTEMPTS: usize = 20;
/// Highest error share of a tape word that starts in review level 2.
pub const SEED_REVIEW_ERRORS: f64 = 0.05;
/// Fewest tape attempts and lowest error share for a word to start in learning.
pub const SEED_LEARNING_ATTEMPTS: usize = 5;
pub const SEED_LEARNING_ERRORS: f64 = 0.10;
/// The level seeded review words start at.
pub const SEED_LEVEL: u8 = 2;

fn recent_from(attempts: usize, errors: usize) -> String {
    let n = attempts.min(WORD_WINDOW);
    let bad = ((errors as f64 / attempts.max(1) as f64) * n as f64).round() as usize;
    let mut s = "1".repeat(n - bad.min(n));
    s.push_str(&"0".repeat(bad.min(n)));
    s
}

/// A vocabulary seeded from what the typist has done: the tape's per-word counts (`tape`) and the
/// words missed in past practice runs (`missed`, one entry per miss). Words typed 20 or more times
/// at 95% or better start in review at level 2, due one interval after their last use; words with
/// 5 or more attempts and 10% or more wrong start in learning (the `cfg.learning_cap` worst);
/// every other word with a history is kept as new with its counts, so it is introduced in pool order.
#[must_use]
pub fn seed(tape: &[TapeWord], missed: &[String], cfg: &Config, today: u32) -> Vocab {
    let mut v = Vocab { seeded: true, ..Vocab::default() };
    let day_of_tape = |t: &TapeWord| t.last_day.map_or(today, |d| u32::try_from(d.max(0)).unwrap_or(today).min(today));
    let mut learning: Vec<&TapeWord> = Vec::new();
    for t in tape.iter().filter(|t| t.attempts > 0) {
        let rate = t.errors as f64 / t.attempts as f64;
        let last = day_of_tape(t);
        let hits = (t.attempts - t.errors.min(t.attempts)) as u32;
        let mut w = Word { attempts: t.attempts as u32, hits, last_seen: last, days: t.days as u32, recent: recent_from(t.attempts, t.errors), ..Word::default() };
        if t.attempts >= SEED_REVIEW_ATTEMPTS && rate <= SEED_REVIEW_ERRORS {
            w.status = Status::Review;
            w.level = SEED_LEVEL;
            w.due = last + cfg.intervals[usize::from(SEED_LEVEL) - 1];
        } else if t.attempts >= SEED_LEARNING_ATTEMPTS && rate >= SEED_LEARNING_ERRORS {
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
    v
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
    let cfg = Config::default();
    let today = day_of(now(), local_offset());
    let mut vocab = Vocab::load(&path)?;
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
    if json {
        let item = |(w, s): &(&String, &Word)| serde_json::json!({"word": w, "level": s.level, "due": (s.status == Status::Review).then(|| date(s.due)), "accuracy": 1.0 - s.error_rate(), "lapses": s.lapses});
        let value = serde_json::json!({
            "new": vocab.count(Status::New), "learning": vocab.count(Status::Learning), "review": vocab.count(Status::Review),
            "retired": vocab.count(Status::Retired), "vocabulary": vocab.size(), "sessions": vocab.sessions,
            "frontier_rank": frontier.rank, "next_word": frontier.next, "recent_accuracy": accuracy, "due_now": due,
            "learning_cap": cfg.learning_cap,
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
        "recent accuracy {}  (new words need {:.0}% over the last {})",
        accuracy.map_or_else(|| "none yet".to_owned(), |a| format!("{:.0}%", a * 100.0)),
        cfg.accuracy_threshold * 100.0,
        crate::vocab::GLOBAL_WINDOW
    );
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
    fn seeding_sorts_tape_words_by_their_numbers() {
        let cfg = Config::default();
        let tape = [
            tw("the", 400, 4, 30, 100),    // 1%: review level 2
            tw("cat", 20, 1, 5, 100),      // 5%: review (at the limit)
            tw("dog", 19, 0, 5, 100),      // too few attempts: new with counts
            tw("late", 30, 6, 9, 100),     // 20% wrong: learning
            tw("lead", 8, 1, 3, 100),      // 12.5% wrong, 8 attempts: learning
            tw("rare", 3, 1, 1, 100),      // few attempts: new
            tw("zero", 0, 0, 0, 100),      // nothing: not on file
        ];
        let v = seed(&tape, &["late".into(), "lead".into(), "ghost".into(), "ghost".into()], &cfg, 105);
        assert!(v.seeded);
        let the = &v.words["the"];
        assert_eq!((the.status, the.level, the.due, the.last_seen, the.days), (Status::Review, 2, 103, 100, 30), "level 2 = the 3-day interval after the last use");
        assert_eq!(v.words["cat"].status, Status::Review);
        assert_eq!(v.words["dog"].status, Status::New);
        assert_eq!((v.words["dog"].attempts, v.words["dog"].hits), (19, 19));
        assert_eq!(v.words["late"].status, Status::Learning);
        assert_eq!(v.words["lead"].status, Status::Learning);
        assert_eq!(v.words["rare"].status, Status::New);
        assert!(!v.words.contains_key("zero"));
        assert_eq!(v.words["late"].attempts, 30, "a missed word already on the tape is not counted twice");
        let ghost = &v.words["ghost"];
        assert_eq!((ghost.status, ghost.attempts, ghost.hits, ghost.recent.as_str()), (Status::New, 2, 0, "00"));
    }

    #[test]
    fn seeded_learning_obeys_the_cap_worst_first() {
        let cfg = Config { learning_cap: 2, ..Config::default() };
        let tape = [tw("a1", 10, 5, 3, 1), tw("b1", 10, 9, 3, 1), tw("c1", 10, 7, 3, 1), tw("d1", 10, 6, 3, 1)];
        let v = seed(&tape, &[], &cfg, 5);
        assert_eq!(v.count(Status::Learning), 2);
        assert_eq!(v.words["b1"].status, Status::Learning);
        assert_eq!(v.words["c1"].status, Status::Learning);
        assert_eq!(v.words["a1"].status, Status::New);
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
        assert!(text.contains("learning (1 of 15)") && text.contains("cat"), "{text}");
        assert!(text.contains("1 due now"), "{text}");
        assert!(text.contains("lapses:") && text.contains("dog"), "{text}");
        let json: serde_json::Value = serde_json::from_str(&summary(&v, &cfg, 55, &f, true).unwrap()).unwrap();
        assert_eq!(json["vocabulary"], 4);
        assert_eq!(json["frontier_rank"], 12);
        assert_eq!(json["next_due"][0]["word"], "dog");
        assert_eq!(json["due_now"], 1);
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

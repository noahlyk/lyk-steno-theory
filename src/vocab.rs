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
use crate::practice::adaptive::{Config, accuracy_ok, base_accuracy, due_reviews, learning_accuracy, recent_tests_accuracy, window_accuracy};
use crate::practice::pool::{self, Candidates, Pool};
use crate::practice::words::{FILTER_VERSION, WordFilter};
use anyhow::{Context, Result, bail};
use clap::{Args as ClapArgs, Subcommand};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Version of the file's format.
pub const VERSION: u32 = 1;
/// How many of the last results of a word are kept.
pub const WORD_WINDOW: usize = 10;
/// How many of the last results over all words are kept.
pub const GLOBAL_WINDOW: usize = 50;
/// How many tests' easy-base results are kept for the second gate.
pub const BASE_TESTS: usize = 3;

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
    /// Attempts typed in practice since the seeding (every status): 0 for a word never practised
    #[serde(default)]
    pub practiced: u32,
    /// Attempts typed in practice since it last entered learning, and the clean ones
    #[serde(default)]
    pub learn_n: u32,
    #[serde(default)]
    pub learn_hits: u32,
    /// A learning word that stays unclean: out of the learning cap, drilled lightly
    #[serde(default, skip_serializing_if = "is_false")]
    pub stuck: bool,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_false(b: &bool) -> bool {
    !*b
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
    /// The first attempts of review words in tests (the easy base, not the drilled learning
    /// words), oldest first, the last `GLOBAL_WINDOW`: the signal the gate reads
    #[serde(default)]
    pub base_recent: String,
    /// The easy base of the last `BASE_TESTS` tests: `[clean, attempts]` each
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub base_tests: Vec<[u32; 2]>,
    /// The accuracy gate for new words as a share (0 to 1), when the typist set one
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<f64>,
    /// The day a new word last entered (0: not known yet)
    #[serde(default)]
    pub last_new: u32,
    /// The version of the practice-word filter (`practice::words`) this file was last cleaned with;
    /// 0 for a file written before there was one
    #[serde(default)]
    pub filter_version: u32,
    /// Words `lyk vocab clean` removed: never seeded or introduced again (`lyk vocab unhide`)
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub hidden: BTreeSet<String>,
    pub words: BTreeMap<String, Word>,
}

impl Default for Vocab {
    fn default() -> Self {
        Self { version: VERSION, seeded: false, sessions: 0, recent: String::new(), base_recent: String::new(), base_tests: Vec::new(), gate: None, last_new: 0, filter_version: FILTER_VERSION, hidden: BTreeSet::new(), words: BTreeMap::new() }
    }
}

impl Vocab {
    pub fn push_global(&mut self, ok: bool) {
        self.recent.push(if ok { '1' } else { '0' });
        if self.recent.len() > GLOBAL_WINDOW {
            self.recent.remove(0);
        }
    }

    /// Records one first attempt of a review word (the easy base).
    pub fn push_base(&mut self, ok: bool) {
        self.base_recent.push(if ok { '1' } else { '0' });
        if self.base_recent.len() > GLOBAL_WINDOW {
            self.base_recent.remove(0);
        }
    }

    /// Records a test's easy base: `clean` of `n` first attempts.
    pub fn push_base_test(&mut self, clean: u32, n: u32) {
        self.base_tests.push([clean, n]);
        if self.base_tests.len() > BASE_TESTS {
            self.base_tests.remove(0);
        }
    }

    #[must_use]
    pub fn count(&self, status: Status) -> usize {
        self.words.values().filter(|w| w.status == status).count()
    }

    /// Learning words that count against the learning cap: not the stuck ones.
    #[must_use]
    pub fn active_learning(&self) -> usize {
        self.words.values().filter(|w| w.status == Status::Learning && !w.stuck).count()
    }

    /// Learning words marked stuck.
    #[must_use]
    pub fn stuck_count(&self) -> usize {
        self.words.values().filter(|w| w.status == Status::Learning && w.stuck).count()
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

/// Most seeded reviews due on one day.
pub const SEED_MAX_PER_DAY: usize = 15;
/// Days out a seeded review may first be due, by level 1 to 3: `(first, last)`.
pub const SEED_WINDOWS: [(u32, u32); 3] = [(1, 14), (3, 21), (7, 30)];

/// FNV-1a: a fixed hash, so a spread repeats exactly.
fn hash(word: &str) -> u64 {
    word.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// Due days for `words` (word, level), counted from `today`: a day inside the level's window picked
/// by the word's hash, moved on to the next day with room while `load` (reviews due per day, which
/// this adds to) holds `SEED_MAX_PER_DAY`. Deterministic.
pub fn spread_due(words: &[(String, u8)], today: u32, load: &mut BTreeMap<u32, usize>) -> Vec<(String, u32)> {
    let mut order: Vec<&(String, u8)> = words.iter().collect();
    order.sort_by(|a, b| hash(&a.0).cmp(&hash(&b.0)).then(a.0.cmp(&b.0)));
    order
        .into_iter()
        .map(|(w, level)| {
            let (lo, hi) = SEED_WINDOWS[usize::from(*level).clamp(1, SEED_WINDOWS.len()) - 1];
            let mut day = today + lo + (hash(w) % u64::from(hi - lo + 1)) as u32;
            while load.get(&day).copied().unwrap_or(0) >= SEED_MAX_PER_DAY {
                day += 1;
            }
            *load.entry(day).or_default() += 1;
            (w.clone(), day)
        })
        .collect()
}

/// Whether a review word has been left as the seeding made it: never practised, missed, passed or
/// lapsed (so moving its due day loses nothing).
fn untouched(w: &Word) -> bool {
    w.status == Status::Review && w.practiced == 0 && w.stumbles == 0 && w.promotions == 0 && w.lapses == 0
}

/// Re-spreads the due days of untouched reviews that pile up on one day (more than
/// `SEED_MAX_PER_DAY` of them): each goes to a day in its level's window from `today`, as seeding
/// does. Idempotent. Returns how many words moved.
pub fn spread(vocab: &mut Vocab, today: u32) -> usize {
    let mut by_day: BTreeMap<u32, Vec<&String>> = BTreeMap::new();
    for (word, w) in vocab.words.iter().filter(|(_, w)| untouched(w)) {
        by_day.entry(w.due).or_default().push(word);
    }
    let moving: Vec<(String, u8)> = by_day.values().filter(|ws| ws.len() > SEED_MAX_PER_DAY).flatten().map(|w| ((*w).clone(), vocab.words[*w].level)).collect();
    if moving.is_empty() {
        return 0;
    }
    let names: std::collections::HashSet<&str> = moving.iter().map(|(w, _)| w.as_str()).collect();
    let mut load: BTreeMap<u32, usize> = BTreeMap::new();
    for w in vocab.words.iter().filter(|(word, w)| w.status == Status::Review && !names.contains(word.as_str())).map(|(_, w)| w) {
        *load.entry(w.due).or_default() += 1;
    }
    let moved = spread_due(&moving, today, &mut load);
    let n = moved.len();
    for (word, due) in moved {
        if let Some(w) = vocab.words.get_mut(&word) {
            w.due = due;
        }
    }
    n
}

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
///   Due dates are spread over a window by level (1: 1 to 14 days out, 2: 3 to 21, 3: 7 to 30,
///   chosen by a hash of the word) with at most `SEED_MAX_PER_DAY` due on a day, so the first
///   weeks are not one wall of reviews.
/// - 5 or more attempts and an error share above the median, and not in review: the
///   `cfg.learning_cap` with the most errors start in learning.
/// - Every other word with a history is kept as new with its counts, so it is introduced in pool
///   order.
///
/// Only words `accept` (the practice-word predicate) are read, from the tape and from the missed
/// words alike: a fragment, a name or a rare word the typist's strokes happened to type is not a
/// word they chose to practise.
#[must_use]
pub fn seed(tape: &[TapeWord], missed: &[String], cfg: &Config, today: u32, accept: &dyn Fn(&str) -> bool) -> Vocab {
    let mut v = Vocab { seeded: true, ..Vocab::default() };
    let tape: Vec<TapeWord> = tape.iter().filter(|t| accept(&t.word)).cloned().collect();
    let tape = tape.as_slice();
    let day_of_tape = |t: &TapeWord| t.last_day.map_or(today, |d| u32::try_from(d.max(0)).unwrap_or(today).min(today));
    let median = median_error_rate(tape);
    let mut by_use: Vec<&TapeWord> = tape.iter().filter(|t| t.attempts > 0).collect();
    by_use.sort_by(|a, b| b.attempts.cmp(&a.attempts).then(a.word.cmp(&b.word)));
    let top: std::collections::HashSet<&str> = by_use.iter().take(SEED_TOP_WORDS).map(|t| t.word.as_str()).collect();
    let mut learning: Vec<&TapeWord> = Vec::new();
    let mut reviews: Vec<(String, u8)> = Vec::new();
    for t in tape.iter().filter(|t| t.attempts > 0) {
        let rate = t.errors as f64 / t.attempts as f64;
        let last = day_of_tape(t);
        let hits = (t.attempts - t.errors.min(t.attempts)) as u32;
        let mut w = Word { attempts: t.attempts as u32, hits, last_seen: last, days: t.days as u32, recent: recent_from(t.attempts, t.errors), ..Word::default() };
        if t.attempts >= SEED_REVIEW_ATTEMPTS && rate <= median {
            w.status = Status::Review;
            w.level = if rate <= SEED_CLEAN_ERRORS { if top.contains(t.word.as_str()) { 3 } else { 2 } } else { 1 };
            reviews.push((t.word.clone(), w.level));
        } else if t.attempts >= SEED_LEARNING_ATTEMPTS && rate > median {
            learning.push(t);
        }
        v.words.insert(t.word.clone(), w);
    }
    for (word, due) in spread_due(&reviews, today, &mut BTreeMap::new()) {
        if let Some(w) = v.words.get_mut(&word) {
            w.due = due;
        }
    }
    learning.sort_by(|a, b| b.errors.cmp(&a.errors).then(a.word.cmp(&b.word)));
    for t in learning.into_iter().take(cfg.learning_cap) {
        if let Some(w) = v.words.get_mut(&t.word) {
            w.status = Status::Learning;
        }
    }
    // A word missed in a past run and not on the tape: its misses are all that is known.
    let on_tape: std::collections::HashSet<&str> = tape.iter().map(|t| t.word.as_str()).collect();
    for m in missed.iter().filter(|m| !on_tape.contains(m.as_str()) && accept(m)) {
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
    /// The dictionary's whole tail counts as practice words (as `practice --deep`)
    #[arg(long, global = true)]
    deep: bool,
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
    /// Set the gate for new words: the clean share (percent, 0 to 100) of the easy base (the first
    /// attempts of review words, last 50) needed to earn one, or `default`
    Gate {
        /// A percent such as 70, or `default`
        percent: String,
    },
    /// Learning words that stay unclean (see `lyk vocab stuck --json`): they leave the learning
    /// cap and are drilled lightly
    Stuck {
        /// The steno tape (for the most common wrong word)
        #[arg(long, default_value = "~/.local/state/keymux/steno-tape.txt")]
        tape: PathBuf,
        /// The built dictionary (for the outline)
        #[arg(long, default_value = paths::built_dir_text())]
        dict: PathBuf,
    },
    /// Remove the words that are not practice words (fragments such as `er`, names, rare words)
    /// from every status and hide them for good; writes `vocab.json.bak` first
    Clean {
        /// Print what would go, write nothing
        #[arg(long)]
        dry_run: bool,
    },
    /// Let hidden words be introduced again (if they are practice words)
    Unhide { words: Vec<String> },
    /// Spread the due days of reviews that all fall on one day (never practised since the seeding);
    /// writes `vocab.json.bak` first
    Spread,
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
pub fn ensure_seeded(vocab: &mut Vocab, tape: &Path, dict: &Path, cfg: &Config, today: u32, filter: &WordFilter) -> usize {
    if vocab.seeded {
        return 0;
    }
    let before = vocab.words.len();
    merge_seed(vocab, tape, dict, cfg, today, filter);
    vocab.words.len() - before
}

fn merge_seed(vocab: &mut Vocab, tape: &Path, dict: &Path, cfg: &Config, today: u32, filter: &WordFilter) {
    let stats = crate::mistakes::word_stats(tape, dict).unwrap_or_default();
    let seeded = seed(&stats, &history_misses(&paths::practice_history()), cfg, today, &|w| filter.accepts(w));
    for (word, state) in seeded.words.into_iter().filter(|(w, _)| !vocab.hidden.contains(w)) {
        vocab.words.entry(word).or_insert(state);
    }
    vocab.seeded = true;
    vocab.filter_version = FILTER_VERSION;
}

/// What `clean` took out: the word, its status and its attempts, most-used reviews first.
pub type Removed = Vec<(String, Status, u32)>;

/// Removes every word that is not a practice word under `filter` (any status) and records it in
/// `hidden`, so it is never seeded or introduced again. Marks the vocabulary as cleaned with this
/// filter version. Idempotent: a second run finds nothing.
pub fn clean(vocab: &mut Vocab, filter: &WordFilter) -> Removed {
    let mut removed: Removed = Vec::new();
    for (w, s) in std::mem::take(&mut vocab.words) {
        if filter.accepts(&w) {
            vocab.words.insert(w, s);
        } else {
            removed.push((w, s.status, s.attempts));
        }
    }
    removed.sort_by(|a, b| status_rank(a.1).cmp(&status_rank(b.1)).then(b.2.cmp(&a.2)).then(a.0.cmp(&b.0)));
    vocab.hidden.extend(removed.iter().map(|(w, ..)| w.clone()));
    vocab.filter_version = FILTER_VERSION;
    removed
}

const fn status_rank(s: Status) -> u8 {
    match s {
        Status::Learning => 0,
        Status::Review => 1,
        Status::Retired => 2,
        Status::New => 3,
    }
}

/// Counts of a removal by status: (learning, review, retired, new).
#[must_use]
pub fn removed_counts(removed: &Removed) -> [usize; 4] {
    let mut n = [0; 4];
    for (_, s, _) in removed {
        n[usize::from(status_rank(*s))] += 1;
    }
    n
}

/// What `lyk vocab clean` prints: counts by status and the first `shown` words.
#[must_use]
pub fn clean_report(removed: &Removed, kept: usize, dry_run: bool, shown: usize) -> String {
    let [learning, review, retired, new] = removed_counts(removed);
    let mut out = format!(
        "{} {} not practice words: {learning} learning, {review} review, {retired} retired, {new} new on file; {kept} words stay\n",
        if dry_run { "would remove" } else { "removed" },
        removed.len()
    );
    let list: Vec<String> = removed.iter().take(shown).map(|(w, s, a)| format!("{w} ({}, {a})", s.name())).collect();
    if !list.is_empty() {
        let _ = writeln!(out, "first {}: {}", list.len(), list.join(", "));
    }
    out
}

/// Cleans a vocabulary written by an older filter, once: when `filter_version` is below the
/// current one. Returns the words removed, or `None` when nothing was due.
pub fn migrate(vocab: &mut Vocab, filter: &WordFilter) -> Option<Removed> {
    (vocab.filter_version < FILTER_VERSION).then(|| clean(vocab, filter))
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
                println!("gate set to {p:.0}%: new words need that share of clean first attempts of review words over the last {GLOBAL_WINDOW} (or over the last 3 tests)");
            }
        }
        Some(Action::Stuck { tape, dict }) => {
            let extras = stuck_extras(&vocab, &expand_home(tape), dict);
            print!("{}", stuck_report(&vocab, &extras, args.json)?);
            return Ok(());
        }
        Some(Action::Clean { dry_run }) => {
            let filter = WordFilter::load(&paths::data_dir(), args.deep, args.names);
            let mut cleaned = vocab.clone();
            let removed = clean(&mut cleaned, &filter);
            print!("{}", clean_report(&removed, cleaned.words.len(), *dry_run, CLEAN_SHOWN));
            if *dry_run || cleaned == vocab {
                return Ok(());
            }
            if path.exists() {
                let bak = path.with_extension("json.bak");
                std::fs::copy(&path, &bak).with_context(|| format!("cannot back up {} to {}", path.display(), bak.display()))?;
                println!("the old file is {}", bak.display());
            }
            vocab = cleaned;
        }
        Some(Action::Unhide { words }) => {
            let filter = WordFilter::load(&paths::data_dir(), args.deep, args.names);
            for w in lower(words) {
                let note = if !vocab.hidden.remove(&w) {
                    "was not hidden"
                } else if filter.accepts(&w) {
                    "can be introduced again"
                } else {
                    "unhidden, but it is still not a practice word, so it stays out of the feed"
                };
                println!("{w}: {note}");
            }
        }
        Some(Action::Spread) => {
            let mut spread_vocab = vocab.clone();
            let moved = spread(&mut spread_vocab, today);
            if moved > 0 {
                let bak = path.with_extension("json.bak");
                std::fs::copy(&path, &bak).with_context(|| format!("cannot back up {} to {}", path.display(), bak.display()))?;
                spread_vocab.save(&path)?;
                println!("{moved} reviews re-spread over their level's window (at most {SEED_MAX_PER_DAY} due a day); the old file is {}", bak.display());
            } else {
                println!("nothing to spread: no day has more than {SEED_MAX_PER_DAY} untouched reviews due");
            }
            return Ok(());
        }
        Some(Action::Seed { tape, dict }) => {
            let before = vocab.words.len();
            merge_seed(&mut vocab, &expand_home(tape), dict, &cfg, today, &WordFilter::load(&paths::data_dir(), args.deep, args.names));
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
    let opts = pool::Opts { wordlist: args.wordlist, weight: args.monkeytype_weight, names: args.names, deep: args.deep };
    let Ok(mut p) = Pool::open(&data, &opts) else { return Ok(Frontier::default()) };
    let vocab = Vocab::load(&file())?;
    Ok(find_frontier(&vocab, &mut p))
}

const LIST: usize = 15;
/// Words `lyk vocab clean` lists.
const CLEAN_SHOWN: usize = 30;

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
    let learning = vocab.active_learning();
    if learning >= cfg.learning_cap {
        return Some(format!("the learning pool is full ({learning} of {}); master them, wait for the stuck ones (`lyk vocab stuck`) or `lyk vocab know` the ones you do not want", cfg.learning_cap));
    }
    if !accuracy_ok(vocab, cfg) {
        return Some(format!(
            "filler clean {:.0}% is under the gate, {:.0}%, over the last {GLOBAL_WINDOW} first attempts of review words; `lyk vocab gate N` sets a lower one",
            base_accuracy(vocab).unwrap_or(0.0) * 100.0,
            cfg.accuracy_threshold * 100.0
        ));
    }
    Some("the gate is met and the pool has room: run a test (the next new word comes in it)".to_owned())
}

/// What `lyk vocab stuck` adds to a stuck word from outside the file.
#[derive(Clone, Debug, Default, Serialize)]
pub struct StuckExtra {
    /// The best outline in the built dictionary, its strokes, keys and ergonomic cost
    pub outline: Option<String>,
    pub strokes: Option<usize>,
    pub keys: Option<u32>,
    pub cost: Option<u32>,
    /// The ordinary word the tape most often typed when this one was meant, and how often
    pub wrong: Option<String>,
    pub wrong_times: Option<usize>,
}

/// The stuck words, worst (fewest clean since entering learning) first.
#[must_use]
pub fn stuck_words(vocab: &Vocab) -> Vec<(&String, &Word)> {
    let rate = |w: &Word| if w.learn_n == 0 { 0.0 } else { f64::from(w.learn_hits) / f64::from(w.learn_n) };
    let mut v: Vec<(&String, &Word)> = vocab.words.iter().filter(|(_, w)| w.status == Status::Learning && w.stuck).collect();
    v.sort_by(|a, b| rate(a.1).total_cmp(&rate(b.1)).then(b.1.stumbles.cmp(&a.1.stumbles)).then(a.0.cmp(b.0)));
    v
}

/// The outline, its cost and the usual wrong word of each stuck word, from the built dictionary
/// and the tape (nothing when either is missing).
fn stuck_extras(vocab: &Vocab, tape: &Path, dict: &Path) -> BTreeMap<String, StuckExtra> {
    let words: std::collections::HashSet<String> = stuck_words(vocab).iter().map(|(w, _)| (*w).clone()).collect();
    let wanted: std::collections::HashSet<&str> = words.iter().map(String::as_str).collect();
    let outlines = crate::practice::best_outlines(&wanted, dict);
    let wrong = crate::mistakes::wrong_words(tape, dict, &words);
    words
        .iter()
        .map(|w| {
            let mut e = StuckExtra::default();
            if let Some(o) = outlines.get(w) {
                e.outline = Some(o.clone());
                if let Some(strokes) = crate::stroke::parse_outline(o) {
                    e.strokes = Some(strokes.len());
                    e.keys = Some(strokes.iter().map(|s| (s.0 & !(crate::stroke::Stroke::JOIN | crate::stroke::Stroke::SPELL)).count_ones()).sum());
                    e.cost = Some(crate::ergonomics::weighted_outline(&strokes));
                }
            }
            if let Some((typed, n)) = wrong.get(w) {
                e.wrong = Some(typed.clone());
                e.wrong_times = Some(*n);
            }
            (w.clone(), e)
        })
        .collect()
}

/// `lyk vocab stuck`: each stuck word with its recent clean rate, stumbles, outline and cost and
/// the usual wrong word; as text or JSON.
pub fn stuck_report(vocab: &Vocab, extras: &BTreeMap<String, StuckExtra>, json: bool) -> Result<String> {
    let words = stuck_words(vocab);
    let none = StuckExtra::default();
    if json {
        let items: Vec<serde_json::Value> = words
            .iter()
            .map(|(w, s)| {
                let (ok, n) = s.window();
                let e = extras.get(*w).unwrap_or(&none);
                serde_json::json!({
                    "word": w, "recent_clean": ok, "recent_attempts": n, "since_attempts": s.learn_n,
                    "since_clean": if s.learn_n == 0 { 0.0 } else { f64::from(s.learn_hits) / f64::from(s.learn_n) },
                    "stumbles": s.stumbles, "days": s.days, "outline": e.outline, "strokes": e.strokes, "keys": e.keys, "cost": e.cost,
                    "wrong": e.wrong, "wrong_times": e.wrong_times,
                })
            })
            .collect();
        return Ok(serde_json::to_string_pretty(&serde_json::json!({ "stuck": items.len(), "words": items }))? + "\n");
    }
    if words.is_empty() {
        return Ok("no stuck words\n".to_owned());
    }
    let mut out = format!("{} stuck words (out of the learning cap, drilled lightly; they leave this list at 9 of 10 clean):\n", words.len());
    for (w, s) in words {
        let (ok, n) = s.window();
        let e = extras.get(w).unwrap_or(&none);
        let since = if s.learn_n == 0 { 0.0 } else { 100.0 * f64::from(s.learn_hits) / f64::from(s.learn_n) };
        let _ = write!(out, "  {w:<14} recent {ok}/{n} clean, {since:.0}% of {} since learning, {} stumbles", s.learn_n, s.stumbles);
        if let (Some(o), Some(k), Some(c)) = (&e.outline, e.keys, e.cost) {
            let _ = write!(out, ";  {o} ({k} keys, cost {c})");
        }
        if let (Some(t), Some(n)) = (&e.wrong, e.wrong_times) {
            let _ = write!(out, ";  often types {t} ({n}x)");
        }
        out.push('\n');
    }
    Ok(out)
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
    let (filler, learning_clean, last3) = (base_accuracy(vocab), learning_accuracy(vocab), recent_tests_accuracy(vocab));
    let stumble_rate = accuracy.map(|a| 1.0 - a);
    let week = stumbled_this_week(vocab, today);
    let stuck = stuck_reason(vocab, cfg, today);
    if json {
        let item = |(w, s): &(&String, &Word)| serde_json::json!({"word": w, "level": s.level, "due": (s.status == Status::Review).then(|| date(s.due)), "accuracy": 1.0 - s.error_rate(), "lapses": s.lapses, "stuck": s.stuck});
        let value = serde_json::json!({
            "new": vocab.count(Status::New), "learning": vocab.count(Status::Learning), "review": vocab.count(Status::Review),
            "retired": vocab.count(Status::Retired), "vocabulary": vocab.size(), "sessions": vocab.sessions,
            "frontier_rank": frontier.rank, "next_word": frontier.next, "recent_accuracy": accuracy, "due_now": due,
            "learning_cap": cfg.learning_cap, "gate": cfg.accuracy_threshold, "stumble_rate": stumble_rate,
            "filler_accuracy": filler, "filler_accuracy_last_3_tests": last3, "learning_accuracy": learning_clean,
            "learning_active": vocab.active_learning(), "stuck": vocab.stuck_count(),
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
    let pct = |x: Option<f64>| x.map_or_else(|| "none yet".to_owned(), |a| format!("{:.0}%", a * 100.0));
    let _ = writeln!(
        out,
        "filler clean {} (gate {:.0}%, over the last {GLOBAL_WINDOW} first attempts of review words; last 3 tests {}), learning clean {};  stumble rate over all words {}",
        pct(filler),
        cfg.accuracy_threshold * 100.0,
        pct(last3),
        pct(learning_clean),
        pct(stumble_rate)
    );
    if let Some(why) = &stuck {
        let _ = writeln!(out, "no new word for {} days: {why}", today.saturating_sub(vocab.last_new));
    }
    let _ = writeln!(out, "\nlearning ({} of {}{}):", vocab.active_learning(), cfg.learning_cap, if vocab.stuck_count() > 0 { format!(", {} stuck: lyk vocab stuck", vocab.stuck_count()) } else { String::new() });
    for (w, s) in &learning {
        let (ok, n) = s.window();
        let _ = writeln!(out, "  {w:<16} {ok}/{n} recent   {} days   {} attempts{}", s.days, s.attempts, if s.stuck { "   stuck" } else { "" });
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

    fn live(attempts: u32) -> Word {
        Word { status: Status::Review, attempts, hits: attempts, ..Word::default() }
    }

    fn test_filter() -> WordFilter {
        WordFilter::from_parts(&["the", "cat", "dog", "it's"], &[], &[], &[], &["ben"], false)
    }

    #[test]
    fn seeding_reads_only_practice_words() {
        let tape = [tw("the", 50, 1, 3, 10), tw("er", 112, 30, 3, 10), tw("tash", 72, 0, 3, 10), tw("cat", 30, 1, 5, 10)];
        let v = seed(&tape, &["er".into(), "dog".into(), "zzzz".into()], &Config::default(), 20, &|w| test_filter().accepts(w));
        let mut words: Vec<&str> = v.words.keys().map(String::as_str).collect();
        words.sort_unstable();
        assert_eq!(words, ["cat", "dog", "the"], "fragments, rare words and unknown missed words stay out");
    }

    #[test]
    fn clean_removes_every_non_practice_word_hides_it_and_is_idempotent() {
        let mut v = Vocab { filter_version: 0, ..Vocab::default() };
        for (w, status) in [("the", Status::Review), ("er", Status::Review), ("tash", Status::Learning), ("abb", Status::New), ("ing", Status::Retired), ("cat", Status::New)] {
            v.words.insert(w.into(), Word { status, ..live(10) });
        }
        let removed = clean(&mut v, &test_filter());
        assert_eq!(removed.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["tash", "er", "ing", "abb"], "learning, review, retired, new; most used first");
        assert_eq!(removed_counts(&removed), [1, 1, 1, 1]);
        assert_eq!(v.words.keys().map(String::as_str).collect::<Vec<_>>(), ["cat", "the"]);
        assert_eq!(v.hidden.iter().map(String::as_str).collect::<Vec<_>>(), ["abb", "er", "ing", "tash"]);
        assert_eq!(v.filter_version, FILTER_VERSION);
        let again = clean(&mut v, &test_filter());
        assert!(again.is_empty(), "a second clean finds nothing: {again:?}");
        assert_eq!(v.hidden.len(), 4, "and hides nothing more");
        let report = clean_report(&removed, v.words.len(), true, 2);
        assert!(report.starts_with("would remove 4 not practice words: 1 learning, 1 review, 1 retired, 1 new on file; 2 words stay"), "{report}");
        assert!(report.contains("first 2: tash (learning, 10), er (review, 10)"), "{report}");
    }

    #[test]
    fn a_hidden_word_is_not_seeded_again_and_the_file_round_trips() {
        let dir = std::env::temp_dir().join(format!("lyk-vocab-clean-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("vocab.json");
        let mut v = Vocab::default();
        v.hidden.insert("cat".into());
        v.save(&path).unwrap();
        let loaded = Vocab::load(&path).unwrap();
        assert!(loaded.hidden.contains("cat") && loaded.filter_version == FILTER_VERSION);
        let mut seeded = loaded;
        merge_seed(&mut seeded, Path::new("/no/such/tape"), Path::new("/no/dict"), &Config::default(), 10, &test_filter());
        assert!(!seeded.words.contains_key("cat"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_of_an_older_filter_version_is_cleaned_once() {
        // a file written before `filter_version` and `hidden` existed
        let old = r#"{"version":1,"seeded":true,"words":{"cat":{"status":"review","attempts":5,"hits":5},"er":{"status":"review","attempts":50,"hits":40}}}"#;
        let mut v: Vocab = serde_json::from_str(old).unwrap();
        assert_eq!(v.filter_version, 0);
        assert!(v.hidden.is_empty());
        let removed = migrate(&mut v, &test_filter()).expect("older version: cleaned");
        assert_eq!(removed.len(), 1);
        assert_eq!(v.filter_version, FILTER_VERSION);
        assert!(migrate(&mut v, &test_filter()).is_none(), "current version: nothing to do");
        assert!(v.words.contains_key("cat") && v.hidden.contains("er"));
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
        let v = seed(&tape, &["bad".into(), "lead".into(), "ghost".into(), "ghost".into()], &cfg, 105, &|_| true);
        assert!(v.seeded);
        assert_eq!(v.last_new, 105, "the gate's clock starts at the seeding");
        let the = &v.words["the"];
        assert_eq!((the.status, the.level, the.last_seen, the.days), (Status::Review, 3, 100, 30));
        assert!((112..=135).contains(&the.due), "level 3 is due 7 to 30 days from today (105): {}", the.due);
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
        let v = seed(&tape, &[], &cfg, 105, &|_| true);
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
        let v = seed(&tape, &[], &cfg, 5, &|_| true);
        assert_eq!(v.count(Status::Learning), 2);
        assert_eq!(v.words["b1"].status, Status::Learning);
        assert_eq!(v.words["c1"].status, Status::Learning);
        assert_eq!(v.words["d1"].status, Status::New);
        assert_eq!(v.words["a1"].status, Status::New, "at the median (capped at 50%) is not worse than typical");
        // the default pool is 10
        let many: Vec<TapeWord> = (0..40).map(|i| tw(&format!("w{i:02}"), 10, 6 + i % 4, 3, 1)).collect();
        assert_eq!(seed(&many, &[], &Config::default(), 5, &|_| true).count(Status::Learning), 10);
        assert!(seed(&[], &[], &Config::default(), 5, &|_| true).words.is_empty());
    }

    #[test]
    fn an_undatable_tape_counts_from_today_and_never_the_future() {
        let cfg = Config::default();
        let mut undated = tw("the", 50, 0, 3, 0);
        undated.last_day = None;
        let future = tw("of", 50, 0, 3, 9999);
        let v = seed(&[undated, future], &[], &cfg, 100, &|_| true);
        assert_eq!(v.words["the"].last_seen, 100);
        assert_eq!(v.words["of"].last_seen, 100);
    }

    #[test]
    fn seeded_files_hold_words_and_numbers_only() {
        let cfg = Config::default();
        let v = seed(&[tw("the", 50, 1, 3, 10)], &["cat".into()], &cfg, 20, &|_| true);
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
        let n = ensure_seeded(&mut v, Path::new("/no/such/tape"), Path::new("/no/dict"), &Config::default(), 10, &WordFilter::default());
        assert!(v.seeded);
        assert_eq!(n, v.words.len());
        v.words.insert("kept".into(), Word::default());
        assert_eq!(ensure_seeded(&mut v, Path::new("/no/such/tape"), Path::new("/no/dict"), &Config::default(), 10, &WordFilter::default()), 0, "already seeded");
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
        assert!((with_gate(&base, &v, None).accuracy_threshold - 0.70).abs() < 1e-9);
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
        let mut v = Vocab { sessions: 12, last_new: 100, base_recent: "1".repeat(30) + &"0".repeat(20), ..Vocab::default() };
        assert!(stuck_reason(&v, &cfg, 110).is_none(), "ten days is not long");
        let why = stuck_reason(&v, &cfg, 120).expect("20 days with 60% against a 70% gate");
        assert!(why.contains("filler clean 60%") && why.contains("70%") && why.contains("lyk vocab gate"), "{why}");
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
        assert!(text.contains("stumble rate over all words 25%"), "{text}");
        let week = text.split("stumbled most this week").nth(1).expect("the list is there");
        assert!(week.find("cat").unwrap() < week.find("dog").unwrap() && !week.contains("old"), "{text}");
        let json: serde_json::Value = serde_json::from_str(&summary(&v, &cfg, 101, &f, true).unwrap()).unwrap();
        assert_eq!(json["stumbled_this_week"][0]["word"], "cat");
        assert_eq!(json["stumbled_this_week"][0]["stumbles"], 4);
        assert!((json["stumble_rate"].as_f64().unwrap() - 0.25).abs() < 1e-9);
    }

    #[test]
    fn seeded_due_dates_are_spread_over_each_levels_window_and_never_a_wall() {
        let cfg = Config::default();
        // 300 words typed cleanly, two thirds of them at 10% wrong: levels 2 and 3, the rest level 1
        let tape: Vec<TapeWord> = (0..300).map(|i| tw(&format!("w{i:03}"), 400 - i, if i % 3 == 0 { 120 } else { 40 }, 9, 100)).collect();
        let v = seed(&tape, &[], &cfg, 200, &|_| true);
        let mut per_day: BTreeMap<u32, usize> = BTreeMap::new();
        for w in v.words.values().filter(|w| w.status == Status::Review) {
            let (lo, hi) = SEED_WINDOWS[usize::from(w.level).clamp(1, 3) - 1];
            *per_day.entry(w.due).or_default() += 1;
            assert!(w.due >= 200 + lo, "level {} due {}", w.level, w.due);
            let _ = hi;
        }
        assert!(per_day.values().all(|n| *n <= SEED_MAX_PER_DAY), "{per_day:?}");
        assert!(per_day.len() >= 20, "spread over {} days", per_day.len());
        assert_eq!(per_day.values().sum::<usize>(), v.count(Status::Review));
        // reproducible
        assert_eq!(seed(&tape, &[], &cfg, 200, &|_| true), v);
        // a level inside its window when there is room: a few words only
        let few = seed(&[tw("cat", 30, 1, 5, 100), tw("dog", 30, 1, 5, 100)], &[], &cfg, 200, &|_| true);
        for w in few.words.values() {
            let (lo, hi) = SEED_WINDOWS[usize::from(w.level).clamp(1, 3) - 1];
            assert!((200 + lo..=200 + hi).contains(&w.due), "level {} due {}", w.level, w.due);
        }
    }

    #[test]
    fn spread_moves_only_a_wall_of_untouched_reviews_and_is_idempotent() {
        let mut v = Vocab::default();
        for i in 0..60 {
            v.words.insert(format!("a{i:02}"), Word { status: Status::Review, level: 1, due: 101, ..Word::default() });
        }
        for i in 0..5 {
            v.words.insert(format!("b{i}"), Word { status: Status::Review, level: 1, due: 101, practiced: 3, ..Word::default() });
        }
        v.words.insert("c".into(), Word { status: Status::Review, level: 2, due: 140, ..Word::default() });
        v.words.insert("d".into(), Word { status: Status::Learning, due: 101, ..Word::default() });
        let before = v.clone();
        let moved = spread(&mut v, 100);
        assert_eq!(moved, 60, "the untouched words of the day with more than {SEED_MAX_PER_DAY}");
        let mut per_day: BTreeMap<u32, usize> = BTreeMap::new();
        for w in v.words.values().filter(|w| w.status == Status::Review) {
            *per_day.entry(w.due).or_default() += 1;
        }
        assert!(per_day.values().all(|n| *n <= SEED_MAX_PER_DAY), "{per_day:?}");
        assert!(v.words.keys().filter(|k| k.starts_with('a')).all(|k| (101..=114 + 4).contains(&v.words[k].due)), "level 1 spreads over 1 to 14 days out (a little past it when full)");
        for k in ["b0", "b4", "c", "d"] {
            assert_eq!(v.words[k], before.words[k], "{k} is left as it was");
        }
        let again = v.clone();
        assert_eq!(spread(&mut v, 100), 0, "a second time changes nothing");
        assert_eq!(v, again);
        // a small pile is not a wall
        let mut small = Vocab::default();
        for i in 0..SEED_MAX_PER_DAY {
            small.words.insert(format!("a{i:02}"), Word { status: Status::Review, level: 1, due: 101, ..Word::default() });
        }
        assert_eq!(spread(&mut small, 100), 0);
    }

    #[test]
    fn stuck_words_are_listed_worst_first_with_their_outline_and_the_usual_wrong_word() {
        let mut v = Vocab::default();
        v.words.insert("to".into(), Word { status: Status::Learning, stuck: true, learn_n: 800, learn_hits: 160, stumbles: 69, days: 33, recent: "0000000110".into(), ..Word::default() });
        v.words.insert("of".into(), Word { status: Status::Learning, stuck: true, learn_n: 40, learn_hits: 4, stumbles: 20, days: 12, recent: "0000000000".into(), ..Word::default() });
        v.words.insert("the".into(), Word { status: Status::Learning, learn_n: 40, ..Word::default() });
        assert_eq!(stuck_words(&v).iter().map(|(w, _)| w.as_str()).collect::<Vec<_>>(), ["of", "to"], "worst first, not-stuck words left out");
        let mut extras = BTreeMap::new();
        extras.insert("to".to_owned(), StuckExtra { outline: Some("TO".into()), strokes: Some(1), keys: Some(2), cost: Some(7), wrong: Some("too".into()), wrong_times: Some(5) });
        let text = stuck_report(&v, &extras, false).unwrap();
        assert!(text.starts_with("2 stuck words"), "{text}");
        assert!(text.contains("recent 2/10 clean, 20% of 800 since learning, 69 stumbles") && text.contains("TO (2 keys, cost 7)") && text.contains("often types too (5x)"), "{text}");
        assert!(text.find("of ").unwrap() < text.find("to ").unwrap(), "{text}");
        let json: serde_json::Value = serde_json::from_str(&stuck_report(&v, &extras, true).unwrap()).unwrap();
        assert_eq!(json["stuck"], 2);
        assert_eq!(json["words"][1]["word"], "to");
        assert_eq!(json["words"][1]["outline"], "TO");
        assert_eq!(json["words"][1]["recent_clean"], 2);
        assert_eq!(json["words"][1]["stumbles"], 69);
        assert!(json["words"][0]["outline"].is_null());
        assert_eq!(stuck_report(&Vocab::default(), &BTreeMap::new(), false).unwrap(), "no stuck words\n");
    }

    #[test]
    fn the_summary_shows_the_filler_and_learning_clean_numbers_and_the_stuck_count() {
        let cfg = Config::default();
        let mut v = Vocab { sessions: 4, recent: "0".repeat(44) + "111111", base_recent: "1".repeat(17) + "000", ..Vocab::default() };
        v.words.insert("cat".into(), Word { status: Status::Learning, recent: "0000000001".into(), ..Word::default() });
        v.words.insert("dog".into(), Word { status: Status::Learning, stuck: true, recent: "0000000000".into(), ..Word::default() });
        let f = Frontier::default();
        let text = summary(&v, &cfg, 120, &f, false).unwrap();
        assert!(text.contains("filler clean 85% (gate 70%") && text.contains("learning clean 5%") && text.contains("stumble rate over all words 88%"), "{text}");
        assert!(text.contains("learning (1 of 10, 1 stuck: lyk vocab stuck)") && text.contains("stuck"), "{text}");
        let json: serde_json::Value = serde_json::from_str(&summary(&v, &cfg, 120, &f, true).unwrap()).unwrap();
        assert!((json["filler_accuracy"].as_f64().unwrap() - 0.85).abs() < 1e-9);
        assert!((json["learning_accuracy"].as_f64().unwrap() - 0.05).abs() < 1e-9);
        assert_eq!((json["stuck"].as_u64(), json["learning_active"].as_u64()), (Some(1), Some(1)));
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

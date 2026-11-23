//! The adaptive engine: which words a test holds, and what a finished test teaches the vocabulary.
//!
//! Pure: it reads and changes a [`Vocab`] and walks a [`Candidates`] list; no terminal, no clock
//! (the day is passed in), no files. Every tunable is a field of [`Config`], in one place.
//!
//! ## The rules
//!
//! A word is `new` (on file with past counts only, or not on file), `learning`, `review` or
//! `retired`. A learning word may be `stuck`: it then does not count against the learning cap.
//!
//! - **Clean attempts.** An attempt is a `hit` only when the word committed right *and* had no
//!   correction while it was typed (a stumble: a backspace run or word delete). A typist whose
//!   corrections precede the commit would otherwise score every word clean.
//! - **Earning a new word.** A new word enters when (a) fewer than `learning_cap` (10) words are
//!   learning (stuck ones not counted), (b) the *easy base* is clean enough: the clean share of the
//!   first attempts of review words (not the drilled learning words) over the last `GLOBAL_WINDOW`
//!   (50) such attempts is at least `accuracy_threshold` (70%, or the gate `lyk vocab gate` set), or
//!   the last `BASE_TESTS` (3) tests' easy base together reach it (with fewer than
//!   `accuracy_min_attempts` (20) base attempts on record it is not held against the typist), and
//!   (c) fewer than `new_per_session` (2) new words have entered this test. The word is the next
//!   one in pool order that is not already learning, in review or retired.
//! - **Mastery.** A learning word becomes review (level 1) when at least `mastery_accuracy` (90%:
//!   9 of 10) of its last 10 attempts are clean and its speed is within `speed_factor` (1.5x) of
//!   the median speed of the words with a measured speed (the check is skipped while fewer than 10
//!   words have one). There is no day count: every attempt in a test counts, a second copy of a
//!   word in the same test included, so a word can master within one day once it has 10 attempts.
//! - **Stuck.** A learning word with at least `stuck_attempts` (30) practice attempts since it
//!   entered learning, under `stuck_clean` (30%) of them clean, and practised on at least
//!   `stuck_days` (7) days is marked stuck: it leaves the learning cap (so a new word can be
//!   earned) and is drilled lightly (at most `stuck_per_test` (2) of them a test, once each, the
//!   longest unseen first). It is unstuck when it reaches mastery, or 9 of 10 clean with a free
//!   slot.
//! - **Review.** Intervals by level: 1, 3, 7, 21, 60, 180 days. A due review word is in the next
//!   test; passed (every attempt right) it moves up a level and is due one interval later; a miss
//!   moves it down one level (never below 1) and makes it due tomorrow; two misses in a row
//!   send it back to learning. A stumble is a miss.
//! - **A test** has exactly `count` words. Learning words (new and stuck included) fill at most
//!   `learning_share` (40%) of it: stuck words, then the new words (twice each), then the words that
//!   stumbled or missed in the last `stumble_sessions` (2) tests (once each, the most stumbles
//!   first), then the other learning words by recent error rate (once each, in the weighted
//!   order), then a second copy for the `second_copies` (3) with the most recent stumbles if room
//!   is left. Due reviews fill at most `review_share` (30%), longest overdue first. The rest is an
//!   easy base: review words not yet due, the ones with the lowest recent stumble rate. Shuffled
//!   with the seed, no word twice in a row. In the typing session a word that stumbled or was
//!   committed wrong comes back `respace_gap` (3) words later in place of a later base word (never
//!   a due review or a learning word), so the test keeps its length; at most 2 copies of a word in
//!   a test, 3 once it has been unclean twice. That in-test replacement is off by default: the feed
//!   is fixed when the test starts (`lyk practice --requeue` turns it on); a stumbled word is still
//!   drilled first in the *next* tests.

use super::feed::Rng;
use super::pool::Candidates;
use crate::vocab::{BASE_TESTS, Status, Vocab, WORD_WINDOW, Word};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Every tunable of the engine.
#[derive(Clone, Debug)]
pub struct Config {
    /// Most new words introduced in one test
    pub new_per_session: usize,
    /// Most words learning at once (stuck words not counted)
    pub learning_cap: usize,
    /// Share of clean first attempts of review words that earns new words
    pub accuracy_threshold: f64,
    /// Fewest base attempts on record before the accuracy gate applies
    pub accuracy_min_attempts: usize,
    /// Share of the last `WORD_WINDOW` attempts that must be clean for mastery
    pub mastery_accuracy: f64,
    /// A word's speed may be this many times the median speed
    pub speed_factor: f64,
    /// Words with a measured speed needed before the speed check applies
    pub speed_min_words: usize,
    /// Weight of a new right answer in the speed average
    pub speed_alpha: f64,
    /// Days until a review word is due again, by level 1 to 6
    pub intervals: [u32; 6],
    /// Most of a test given to due reviews
    pub review_share: f64,
    /// Most of a test given to learning words (new and stuck included)
    pub learning_share: f64,
    /// Times a new word is in its first test
    pub new_reps: usize,
    /// How many of the learning words with the most recent stumbles get a second copy
    pub second_copies: usize,
    /// Most times a word is repeated to fill a test the vocabulary is too small to fill
    pub fallback_copies: usize,
    /// A missed or stumbled word comes back this many words later in the same test
    pub respace_gap: usize,
    /// A learning word that stumbled or missed within this many tests is drilled first
    pub stumble_sessions: u32,
    /// Practice attempts since entering learning before a word can be stuck
    pub stuck_attempts: u32,
    /// A word under this share clean over those attempts is stuck
    pub stuck_clean: f64,
    /// Distinct days practised before a word can be stuck
    pub stuck_days: u32,
    /// Most stuck words in one test
    pub stuck_per_test: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            new_per_session: 2,
            learning_cap: 10,
            accuracy_threshold: 0.70,
            accuracy_min_attempts: 20,
            mastery_accuracy: 0.90,
            speed_factor: 1.5,
            speed_min_words: 10,
            speed_alpha: 0.3,
            intervals: [1, 3, 7, 21, 60, 180],
            review_share: 0.30,
            learning_share: 0.40,
            new_reps: 2,
            second_copies: 3,
            fallback_copies: 5,
            respace_gap: 3,
            stumble_sessions: 2,
            stuck_attempts: 30,
            stuck_clean: 0.30,
            stuck_days: 7,
            stuck_per_test: 2,
        }
    }
}

fn share_of(marks: &str) -> Option<f64> {
    (!marks.is_empty()).then(|| marks.bytes().filter(|b| *b == b'1').count() as f64 / marks.len() as f64)
}

/// The clean share of the last `GLOBAL_WINDOW` first attempts of review words in tests (the easy
/// base); none before any.
#[must_use]
pub fn base_accuracy(vocab: &Vocab) -> Option<f64> {
    share_of(&vocab.base_recent)
}

/// The clean share of the easy base over the last `BASE_TESTS` tests; none until that many tests
/// have one.
#[must_use]
pub fn recent_tests_accuracy(vocab: &Vocab) -> Option<f64> {
    if vocab.base_tests.len() < BASE_TESTS {
        return None;
    }
    let (clean, n) = vocab.base_tests.iter().fold((0, 0), |(c, n), t| (c + t[0], n + t[1]));
    (n > 0).then(|| f64::from(clean) / f64::from(n))
}

/// The clean share of the learning words' own last-10 windows, stuck ones included; none with no
/// learning word that has a result.
#[must_use]
pub fn learning_accuracy(vocab: &Vocab) -> Option<f64> {
    let (ok, n) = vocab.words.values().filter(|w| w.status == Status::Learning).map(Word::window).fold((0, 0), |(a, b), (o, n)| (a + o, b + n));
    (n > 0).then(|| ok as f64 / n as f64)
}

/// The share of right answers over the last `GLOBAL_WINDOW` attempts of every word; none before
/// any.
#[must_use]
pub fn window_accuracy(vocab: &Vocab) -> Option<f64> {
    share_of(&vocab.recent)
}

/// Whether the easy base earns new words: the last 50 base attempts, or the last 3 tests, reach the
/// gate (or too few attempts are on record to say).
#[must_use]
pub fn accuracy_ok(vocab: &Vocab, cfg: &Config) -> bool {
    vocab.base_recent.len() < cfg.accuracy_min_attempts
        || base_accuracy(vocab).is_none_or(|a| a >= cfg.accuracy_threshold)
        || recent_tests_accuracy(vocab).is_some_and(|a| a >= cfg.accuracy_threshold)
}

/// Review words due on or before `day`, the longest overdue first.
#[must_use]
pub fn due_reviews(vocab: &Vocab, day: u32) -> Vec<(&String, &Word)> {
    let mut due: Vec<(&String, &Word)> = vocab.words.iter().filter(|(_, w)| w.status == Status::Review && w.due <= day).collect();
    due.sort_by(|a, b| a.1.due.cmp(&b.1.due).then(a.0.cmp(b.0)));
    due
}

/// A test's words, and which of them are new.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub words: Vec<String>,
    /// The words entering learning in this test
    pub new_words: Vec<String>,
    /// How many due reviews are in it
    pub due: usize,
    /// The easy-base words in it (reviews not yet due): the ones a missed word may replace
    pub fillers: Vec<String>,
}

/// Picks among `items` (word, weight, times used) by weight, those under `cap` uses and, with
/// `unused_first`, among the unused ones while any are left; `n` picks at most.
fn draw(items: &mut [(&str, f64, usize)], n: usize, cap: usize, unused_first: bool, rng: &mut Rng, out: &mut Vec<String>) {
    for _ in 0..n {
        let any_unused = unused_first && items.iter().any(|i| i.2 == 0);
        let live = |i: &(&str, f64, usize)| i.2 < cap && (!any_unused || i.2 == 0);
        let total: f64 = items.iter().filter(|i| live(i)).map(|i| i.1).sum();
        if total <= 0.0 {
            return;
        }
        let mut at = (rng.next_u64() >> 11) as f64 / (1_u64 << 53) as f64 * total;
        let Some(pick) = items.iter().position(|i| {
            if !live(i) {
                return false;
            }
            at -= i.1;
            at < 0.0
        }) else {
            // rounding at the very end: the last live item
            let Some(last) = items.iter().rposition(live) else { return };
            items[last].2 += 1;
            out.push(items[last].0.to_owned());
            continue;
        };
        items[pick].2 += 1;
        out.push(items[pick].0.to_owned());
    }
}

/// Whether a word stumbled or missed in one of the last `cfg.stumble_sessions` tests.
const fn recently_stumbled(w: &Word, sessions: u32, cfg: &Config) -> bool {
    w.stumbled_in > 0 && w.stumbled_in + cfg.stumble_sessions > sessions
}

/// A learning word's weight: more for a word with recent errors, a little more for one with few
/// attempts on record, and more again for one that stumbled lately (the more, the more it did).
fn weight(w: &Word, recent: bool) -> f64 {
    let stumble = if recent { 2.0 + f64::from(w.stumbled_n.saturating_sub(1).min(3)) } else { 0.0 };
    4.0_f64.mul_add(w.error_rate(), 1.0) + if w.recent.len() < WORD_WINDOW { 1.0 } else { 0.0 } + stumble
}

/// Makes no word follow itself where the words allow it.
fn separate(words: &mut [String]) {
    let touches = |w: &[String], p: usize| (p > 0 && w[p] == w[p - 1]) || (p + 1 < w.len() && w[p] == w[p + 1]);
    for _ in 0..2 {
        for i in 1..words.len() {
            if words[i] != words[i - 1] {
                continue;
            }
            for j in 0..words.len() {
                if j == i || words[j] == words[i] {
                    continue;
                }
                words.swap(i, j);
                if !touches(words, i) && !touches(words, j) {
                    break;
                }
                words.swap(i, j);
            }
        }
    }
}

/// The words of one test of `count` words on day `day`. `keep` filters words (length, case); with
/// `allow_new` false no new word enters.
#[allow(clippy::too_many_arguments)]
pub fn plan(vocab: &Vocab, cands: &mut dyn Candidates, cfg: &Config, day: u32, count: usize, seed: u64, keep: &dyn Fn(&str) -> bool, allow_new: bool) -> Plan {
    let mut rng = Rng::new(seed);
    let share = |f: f64| ((count as f64 * f).floor() as usize).max(usize::from(count >= 1));
    let (due_max, learn_max) = (share(cfg.review_share), share(cfg.learning_share));
    let mut words: Vec<String> = Vec::new();
    // 1. due reviews, longest overdue first, up to their share
    let due: Vec<&String> = due_reviews(vocab, day).into_iter().map(|(w, _)| w).filter(|w| keep(w)).take(due_max).collect();
    words.extend(due.iter().map(|w| (*w).clone()));
    let due_n = words.len();
    // 2. the learning share
    let mut learn: Vec<String> = Vec::new();
    let room = |learn: &Vec<String>| learn_max.min(count.saturating_sub(due_n)).saturating_sub(learn.len());
    let recent = |s: &Word| recently_stumbled(s, vocab.sessions, cfg);
    // 2a. stuck words, lightly: the longest unseen first
    let mut stuck: Vec<(&String, &Word)> = vocab.words.iter().filter(|(w, s)| s.status == Status::Learning && s.stuck && keep(w)).collect();
    stuck.sort_by(|a, b| a.1.last_seen.cmp(&b.1.last_seen).then(a.0.cmp(b.0)));
    learn.extend(stuck.into_iter().take(cfg.stuck_per_test.min(room(&learn))).map(|(w, _)| w.clone()));
    // 2b. new words, earned, each new_reps times
    let mut new_words: Vec<String> = Vec::new();
    let slots = cfg.new_per_session.min(cfg.learning_cap.saturating_sub(vocab.active_learning()));
    if allow_new && slots > 0 && accuracy_ok(vocab, cfg) {
        let mut i = 0;
        while new_words.len() < slots {
            let Some(w) = cands.get(i) else { break };
            i += 1;
            if vocab.words.get(w).is_none_or(|s| s.status == Status::New) && keep(w) {
                new_words.push(w.to_owned());
            }
        }
        for w in &new_words {
            for _ in 0..cfg.new_reps.min(room(&learn)) {
                learn.push(w.clone());
            }
        }
    }
    // 2c. the words being learned: the ones that stumbled lately first (the most stumbles first),
    // then the rest by weight, once each
    let learning: Vec<(&String, &Word)> = vocab.words.iter().filter(|(w, s)| s.status == Status::Learning && !s.stuck && keep(w)).collect();
    let mut items: Vec<(&str, f64, usize)> = learning.iter().map(|(w, s)| (w.as_str(), weight(s, recent(s)), 0)).collect();
    let mut first: Vec<usize> = (0..learning.len()).filter(|i| recent(learning[*i].1)).collect();
    first.sort_by(|a, b| learning[*b].1.stumbled_n.cmp(&learning[*a].1.stumbled_n).then(learning[*a].0.cmp(learning[*b].0)));
    for i in first {
        if room(&learn) > 0 {
            learn.push(items[i].0.to_owned());
            items[i].2 += 1;
        }
    }
    let n = room(&learn);
    draw(&mut items, n, 1, false, &mut rng, &mut learn);
    // 2d. a second copy for the few with the most recent stumbles
    let mut worst: Vec<&(&String, &Word)> = learning.iter().filter(|(w, s)| learn.contains(w) && (s.error_rate() > 0.0 || recent(s))).collect();
    worst.sort_by(|a, b| b.1.error_rate().total_cmp(&a.1.error_rate()).then(b.1.stumbled_n.cmp(&a.1.stumbled_n)).then(a.0.cmp(b.0)));
    for (w, _) in worst.into_iter().take(cfg.second_copies) {
        if room(&learn) > 0 {
            learn.push((*w).clone());
        }
    }
    words.extend(learn);
    // 3. the easy base: review words not yet due, the cleanest first (lowest recent stumble
    // rate, then the higher level, then the more typed), drawn from the best few
    let mut fillers: Vec<String> = Vec::new();
    let need = count.saturating_sub(words.len());
    if need > 0 {
        let mut later: Vec<(&String, &Word)> = vocab.words.iter().filter(|(w, s)| s.status == Status::Review && s.due > day && keep(w)).collect();
        later.sort_by(|a, b| a.1.error_rate().total_cmp(&b.1.error_rate()).then(b.1.level.cmp(&a.1.level)).then(b.1.attempts.cmp(&a.1.attempts)).then(a.0.cmp(b.0)));
        later.truncate((3 * need).max(30));
        let mut pool: Vec<(&str, f64, usize)> = later.iter().map(|(w, s)| (w.as_str(), { let clean = 1.0 - s.error_rate(); clean.mul_add(clean, 0.05) }, 0)).collect();
        draw(&mut pool, need, 1, false, &mut rng, &mut fillers);
        words.extend(fillers.iter().cloned());
    }
    // 4. still short (a vocabulary too small to fill a test): more due reviews, then repeats
    if words.len() < count {
        let have: HashSet<&String> = words.iter().collect();
        let more: Vec<String> = due_reviews(vocab, day).into_iter().map(|(w, _)| w).filter(|w| keep(w) && !have.contains(w)).take(count - words.len()).cloned().collect();
        words.extend(more);
    }
    if words.len() < count {
        let mut used: HashMap<&str, usize> = HashMap::new();
        for w in &words {
            *used.entry(w.as_str()).or_default() += 1;
        }
        let mut extra: Vec<(&str, f64, usize)> = vocab
            .words
            .iter()
            .filter(|(w, s)| s.status == Status::Learning && keep(w))
            .map(|(w, s)| (w.as_str(), weight(s, recent(s)), used.get(w.as_str()).copied().unwrap_or(0)))
            .chain(new_words.iter().map(|w| (w.as_str(), 1.0, used.get(w.as_str()).copied().unwrap_or(0))))
            .collect();
        let room = count - words.len();
        draw(&mut extra, room, cfg.fallback_copies, false, &mut rng, &mut words);
    }
    words.truncate(count);
    rng.shuffle(&mut words);
    separate(&mut words);
    Plan { words, new_words, due: due_n, fillers }
}

/// A feed of `count` words: one test, and more rounds of the same words (never new ones) when a
/// timed run needs more than one test holds.
#[allow(clippy::too_many_arguments)]
pub fn plan_feed(vocab: &Vocab, cands: &mut dyn Candidates, cfg: &Config, day: u32, count: usize, seed: u64, keep: &dyn Fn(&str) -> bool) -> Plan {
    let mut plan = plan(vocab, cands, cfg, day, count, seed, keep, true);
    let mut round = 1;
    while plan.words.len() < count && round < 8 && !plan.words.is_empty() {
        // the words that just entered are learning words for the rounds that follow
        let mut with_new = vocab.clone();
        for w in &plan.new_words {
            with_new.words.insert(w.clone(), Word { status: Status::Learning, ..Word::default() });
        }
        let more = self::plan(&with_new, cands, cfg, day, count - plan.words.len(), seed.wrapping_add(round), keep, false);
        if more.words.is_empty() {
            break;
        }
        plan.words.extend(more.words);
        plan.fillers.extend(more.fillers);
        round += 1;
    }
    if round > 1 {
        separate(&mut plan.words);
    }
    plan
}

/// One typed word of a finished test.
#[derive(Clone, Debug, PartialEq)]
pub struct Attempt {
    pub word: String,
    /// Clean: committed right with no correction on the way
    pub hit: bool,
    /// Corrections made while typing it
    pub stumbles: u32,
    /// Seconds since the word before, when that carries a recall time (not the first word)
    pub secs: Option<f64>,
}

/// What a test changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub new: Vec<String>,
    pub mastered: Vec<String>,
    pub promoted: Vec<String>,
    pub demoted: Vec<String>,
    pub relearn: Vec<String>,
    /// Review words due now that are still waiting after the test
    pub due: usize,
    /// Learning words that became stuck, and stuck words that came back
    pub stuck: Vec<String>,
    pub unstuck: Vec<String>,
}

impl Summary {
    /// `+2 new  1 mastered  3 due`, empty parts left out.
    #[must_use]
    pub fn line(&self) -> String {
        let mut parts = Vec::new();
        if !self.new.is_empty() {
            parts.push(format!("+{} new", self.new.len()));
        }
        if !self.mastered.is_empty() {
            parts.push(format!("{} mastered", self.mastered.len()));
        }
        if !self.promoted.is_empty() {
            parts.push(format!("{} passed", self.promoted.len()));
        }
        if !self.demoted.is_empty() || !self.relearn.is_empty() {
            parts.push(format!("{} slipped", self.demoted.len().max(self.relearn.len())));
        }
        if !self.stuck.is_empty() {
            parts.push(format!("{} stuck", self.stuck.len()));
        }
        if self.due > 0 {
            parts.push(format!("{} due", self.due));
        }
        parts.join("  ")
    }
}

fn median_speed(vocab: &Vocab, cfg: &Config) -> Option<f64> {
    let mut speeds: Vec<f64> = vocab.words.values().filter(|w| w.status != Status::Retired && w.attempts >= 3).filter_map(|w| w.speed_ms).collect();
    if speeds.len() < cfg.speed_min_words {
        return None;
    }
    speeds.sort_by(f64::total_cmp);
    Some(speeds[speeds.len() / 2])
}

fn mastered(w: &Word, median: Option<f64>, cfg: &Config) -> bool {
    let (ok, n) = w.window();
    n >= WORD_WINDOW
        && ok as f64 / n as f64 >= cfg.mastery_accuracy
        && match (w.speed_ms, median) {
            (Some(s), Some(m)) => s <= cfg.speed_factor * m,
            _ => true,
        }
}

/// Whether a learning word is stuck by the numbers: enough practice attempts since it entered
/// learning, over enough days, and still mostly unclean.
#[must_use]
pub fn is_stuck(w: &Word, cfg: &Config) -> bool {
    w.status == Status::Learning && w.learn_n >= cfg.stuck_attempts && w.days >= cfg.stuck_days && f64::from(w.learn_hits) / f64::from(w.learn_n) < cfg.stuck_clean
}

/// Notes an unclean attempt on a word: `stumbles` corrections (a wrong commit counts as one) in
/// test number `id` on `day`.
fn record_stumble(w: &mut Word, stumbles: u32, id: u32, day: u32) {
    let n = stumbles.max(1);
    w.stumbles += n;
    if w.stumbled_in != id {
        w.stumbled_n = 0;
    }
    w.stumbled_in = id;
    w.stumbled_n += n;
    if stumbles > 0 {
        w.stumble_days.retain(|d| d[0] + 7 > day);
        match w.stumble_days.iter_mut().find(|d| d[0] == day) {
            Some(d) => d[1] += stumbles,
            None => w.stumble_days.push([day, stumbles]),
        }
    }
}

#[derive(Default)]
struct Touch {
    missed: bool,
    demoted: bool,
    relearned: bool,
}

/// Teaches the vocabulary what a finished test showed: each attempt in order, then the words
/// that are now mastered or passed. Retired words are left alone.
#[allow(clippy::too_many_lines)]
pub fn apply(vocab: &mut Vocab, attempts: &[Attempt], cfg: &Config, day: u32) -> Summary {
    let median = median_speed(vocab, cfg);
    let id = vocab.sessions + 1;
    let mut summary = Summary::default();
    let mut touched: BTreeMap<String, Touch> = BTreeMap::new();
    let mut seen_new: HashSet<String> = HashSet::new();
    // the words entering learning in this test hold their places before any lapse takes one
    let entering: HashSet<&str> = attempts.iter().filter(|a| vocab.words.get(&a.word).is_none_or(|w| w.status == Status::New)).map(|a| a.word.as_str()).collect();
    let mut learning = vocab.active_learning() + entering.len();
    let mut globals: Vec<bool> = Vec::new();
    // the easy base: the first attempt of each word that was a review word before the test
    let was_review: HashSet<&str> = attempts.iter().filter(|a| vocab.words.get(&a.word).is_some_and(|w| w.status == Status::Review)).map(|a| a.word.as_str()).collect();
    let mut seen_base: HashSet<&str> = HashSet::new();
    let mut base: Vec<bool> = Vec::new();
    for a in attempts {
        if was_review.contains(a.word.as_str()) && seen_base.insert(a.word.as_str()) {
            base.push(a.hit);
        }
        let w = vocab.words.entry(a.word.clone()).or_default();
        if w.status == Status::Retired {
            continue;
        }
        if w.status == Status::New {
            w.status = Status::Learning;
            w.days = 0;
            w.level = 0;
            w.learn_n = 0;
            w.learn_hits = 0;
            w.stuck = false;
            if seen_new.insert(a.word.clone()) {
                summary.new.push(a.word.clone());
            }
        }
        w.attempts += 1;
        w.practiced += 1;
        if w.status == Status::Learning {
            w.learn_n += 1;
            w.learn_hits += u32::from(a.hit);
        }
        w.hits += u32::from(a.hit);
        w.push_result(a.hit);
        if !a.hit {
            record_stumble(w, a.stumbles, id, day);
        }
        globals.push(a.hit);
        if a.hit {
            w.streak = 0;
            if let Some(secs) = a.secs {
                let ms = secs * 1000.0;
                w.speed_ms = Some(w.speed_ms.map_or(ms, |p| cfg.speed_alpha.mul_add(ms - p, p)));
            }
        } else {
            w.streak += 1;
        }
        if w.last_seen != day {
            w.days += 1;
            w.last_seen = day;
        }
        let t = touched.entry(a.word.clone()).or_default();
        if !a.hit {
            t.missed = true;
            if w.status == Status::Review {
                if !t.demoted {
                    t.demoted = true;
                    w.level = w.level.saturating_sub(1).max(1);
                    w.due = day + 1;
                    summary.demoted.push(a.word.clone());
                }
                if w.streak >= 2 && !t.relearned {
                    // back to learning if there is a slot; else it stays a level-1 review, due tomorrow
                    w.lapses += 1;
                    t.relearned = true;
                    if learning < cfg.learning_cap {
                        learning += 1;
                        w.status = Status::Learning;
                        w.level = 0;
                        w.days = 1;
                        w.learn_n = 0;
                        w.learn_hits = 0;
                        w.stuck = false;
                        "00".clone_into(&mut w.recent);
                        w.mastered = None;
                        summary.relearn.push(a.word.clone());
                    }
                }
            }
        }
    }
    for hit in globals {
        vocab.push_global(hit);
    }
    for hit in &base {
        vocab.push_base(*hit);
    }
    if !base.is_empty() {
        vocab.push_base_test(base.iter().filter(|h| **h).count() as u32, base.len() as u32);
    }
    for (word, t) in &touched {
        let Some(w) = vocab.words.get_mut(word) else { continue };
        match w.status {
            Status::Learning if mastered(w, median, cfg) => {
                w.stuck = false;
                w.status = Status::Review;
                w.level = 1;
                w.due = day + cfg.intervals[0];
                w.mastered = Some(day);
                w.promotions += 1;
                w.streak = 0;
                summary.mastered.push(word.clone());
            }
            Status::Review if !t.missed && w.due <= day => {
                w.level = (w.level + 1).min(6);
                w.due = day + cfg.intervals[usize::from(w.level) - 1];
                w.promotions += 1;
                summary.promoted.push(word.clone());
            }
            _ => {}
        }
    }
    // stuck words leave the cap; a stuck word that gets clean comes back when there is room
    let mut active = vocab.active_learning();
    for word in touched.keys() {
        let Some(w) = vocab.words.get_mut(word) else { continue };
        if w.status != Status::Learning {
            continue;
        }
        if w.stuck {
            let (ok, n) = w.window();
            if n >= WORD_WINDOW && ok as f64 / n as f64 >= cfg.mastery_accuracy && active < cfg.learning_cap {
                w.stuck = false;
                active += 1;
                summary.unstuck.push(word.clone());
            }
        } else if is_stuck(w, cfg) {
            w.stuck = true;
            active = active.saturating_sub(1);
            summary.stuck.push(word.clone());
        }
    }
    vocab.sessions += 1;
    if vocab.last_new == 0 || !summary.new.is_empty() {
        vocab.last_new = day;
    }
    summary.due = due_reviews(vocab, day).len();
    summary
}

#[cfg(test)]
mod tests {
    use crate::mistakes::TapeWord;
    use super::super::attempts_of;
    use super::super::engine::{Input, Session};
    use super::*;
    use std::collections::HashMap;

    fn none<T: std::fmt::Debug>(items: &[T]) {
        assert!(items.is_empty(), "expected none, got {items:?}");
    }

    fn names(n: usize) -> Vec<String> {
        (1..=n).map(|i| format!("w{i:04}")).collect()
    }

    fn cfg() -> Config {
        Config::default()
    }

    fn all(_: &str) -> bool {
        true
    }

    fn word(status: Status) -> Word {
        Word { status, ..Word::default() }
    }

    fn learning(recent: &str, days: u32) -> Word {
        Word { status: Status::Learning, recent: recent.into(), attempts: recent.len() as u32, hits: recent.bytes().filter(|b| *b == b'1').count() as u32, days, ..Word::default() }
    }

    fn hit(w: &str) -> Attempt {
        Attempt { word: w.into(), hit: true, stumbles: 0, secs: Some(1.0) }
    }

    /// Right in the end, after `n` corrections: not clean.
    fn stumble(w: &str, n: u32) -> Attempt {
        Attempt { word: w.into(), hit: false, stumbles: n, secs: Some(2.0) }
    }

    fn miss(w: &str) -> Attempt {
        Attempt { word: w.into(), hit: false, stumbles: 0, secs: None }
    }

    fn p(v: &Vocab, pool: &mut Vec<String>, count: usize, seed: u64, day: u32) -> Plan {
        plan(v, pool, &cfg(), day, count, seed, &all, true)
    }

    // ---- earning new words ----

    #[test]
    fn an_empty_vocabulary_gets_the_first_words_in_pool_order() {
        let mut pool = names(50);
        let plan = p(&Vocab::default(), &mut pool, 25, 1, 100);
        assert_eq!(plan.new_words, ["w0001", "w0002"], "two new words a test");
        // nothing else to show: each new word twice, then topped up to five times
        assert_eq!(plan.words.len(), 10, "{:?}", plan.words);
        assert!(plan.new_words.iter().all(|n| plan.words.iter().filter(|w| *w == n).count() == 5), "{:?}", plan.words);
    }

    #[test]
    fn new_words_skip_the_ones_already_known_and_wait_for_a_free_slot() {
        let mut v = Vocab::default();
        v.words.insert("w0001".into(), word(Status::Retired));
        v.words.insert("w0002".into(), word(Status::Review));
        v.words.insert("w0003".into(), word(Status::New));
        let mut pool = names(50);
        assert_eq!(p(&v, &mut pool, 25, 1, 100).new_words, ["w0003", "w0004"], "a word on file as new is still new");
        // a full learning pool lets nothing in
        for i in 10..20 {
            v.words.insert(format!("w{i:04}"), learning("1", 1));
        }
        assert_eq!(v.count(Status::Learning), 10);
        none(&p(&v, &mut pool, 25, 1, 100).new_words);
        // one free slot: one word
        v.words.insert("w0010".into(), word(Status::Review));
        assert_eq!(p(&v, &mut pool, 25, 1, 100).new_words, ["w0003"]);
        // a stuck word does not hold a slot
        v.words.insert("w0011".into(), Word { stuck: true, ..learning("0000000000", 9) });
        assert_eq!(p(&v, &mut pool, 25, 1, 100).new_words, ["w0003", "w0004"], "{}", v.active_learning());
    }

    #[test]
    fn at_most_k_new_words_a_test() {
        let mut pool = names(50);
        let c = Config { new_per_session: 1, ..cfg() };
        assert_eq!(plan(&Vocab::default(), &mut pool, &c, 1, 25, 1, &all, true).new_words.len(), 1);
        assert!(plan(&Vocab::default(), &mut pool, &c, 1, 25, 1, &all, false).new_words.is_empty(), "allow_new false");
    }

    #[test]
    fn a_low_easy_base_earns_no_new_words_and_the_drilled_words_do_not_count() {
        let mut v = Vocab::default();
        let mut pool = names(50);
        v.base_recent = "1".repeat(13) + "0000000"; // 65%
        none(&p(&v, &mut pool, 25, 1, 5).new_words);
        v.base_recent = "1".repeat(14) + "000000"; // 70%: enough
        assert_eq!(p(&v, &mut pool, 25, 1, 5).new_words.len(), 2);
        v.base_recent = "0".repeat(19); // fewer than 20 attempts: not held against the typist
        assert_eq!(p(&v, &mut pool, 25, 1, 5).new_words.len(), 2);
        v.base_recent = "0".repeat(20);
        none(&p(&v, &mut pool, 25, 1, 5).new_words);
        // the last three tests together can open the gate again
        v.base_tests = vec![[9, 10], [8, 10], [9, 10]]; // 26 of 30: 87%
        assert_eq!(p(&v, &mut pool, 25, 1, 5).new_words.len(), 2);
        v.base_tests = vec![[9, 10], [8, 10]];
        none(&p(&v, &mut pool, 25, 1, 5).new_words);
        v.base_tests = vec![[2, 10], [8, 10], [9, 10]]; // 63%
        none(&p(&v, &mut pool, 25, 1, 5).new_words);
        // what the learning words did (88% stumbles over all attempts) is not the gate
        v.base_recent = "1".repeat(18) + "00";
        v.recent = "0".repeat(44) + "111111";
        assert_eq!(p(&v, &mut pool, 25, 1, 5).new_words.len(), 2);
        assert!((base_accuracy(&v).unwrap() - 0.9).abs() < 1e-9 && (window_accuracy(&v).unwrap() - 0.12).abs() < 1e-9);
    }

    #[test]
    fn filters_apply_to_every_word() {
        let mut pool = vec!["a".to_owned(), "bb".into(), "ccc".into(), "dddd".into()];
        let mut v = Vocab::default();
        v.words.insert("xx".into(), learning("1", 1));
        v.words.insert("yyyyyy".into(), learning("1", 1));
        let plan = plan(&v, &mut pool, &cfg(), 1, 10, 1, &|w: &str| w.len() >= 2 && w.len() <= 4, true);
        assert!(plan.words.iter().all(|w| (2..=4).contains(&w.len()) && w != "yyyyyy"), "{:?}", plan.words);
        assert_eq!(plan.new_words, ["bb", "ccc"]);
    }

    // ---- composition ----

    #[test]
    fn due_reviews_come_first_up_to_their_share() {
        let mut v = Vocab::default();
        for i in 0..20 {
            v.words.insert(format!("r{i:02}"), Word { status: Status::Review, level: 2, due: 50 + i, ..Word::default() });
        }
        for i in 0..30 {
            v.words.insert(format!("f{i:02}"), Word { status: Status::Review, level: 2, due: 200, ..Word::default() });
        }
        let mut pool = names(20);
        let plan = p(&v, &mut pool, 25, 3, 100);
        assert_eq!(plan.due, 7, "30% of 25, rounded down");
        let reviews: Vec<&String> = plan.words.iter().filter(|w| w.starts_with('r')).collect();
        assert_eq!(reviews.len(), 7, "{:?}", plan.words);
        // the longest overdue are the ones that are in it
        let want: HashSet<String> = (0..7).map(|i| format!("r{i:02}")).collect();
        assert_eq!(reviews.iter().map(|w| (*w).clone()).collect::<HashSet<_>>(), want);
        assert_eq!(plan.words.len(), 25);
        // with nothing else to fill the test, more due reviews do
        let mut small = Vocab::default();
        for i in 0..20 {
            small.words.insert(format!("r{i:02}"), Word { status: Status::Review, level: 2, due: 50 + i, ..Word::default() });
        }
        let plan = self::plan(&small, &mut pool, &Config { new_per_session: 0, ..cfg() }, 100, 25, 3, &all, true);
        assert_eq!((plan.due, plan.words.len()), (7, 20), "{:?}", plan.words);
    }

    #[test]
    fn learning_words_are_in_once_with_a_second_copy_for_the_worst_and_never_over_their_share() {
        let mut v = Vocab::default();
        for i in 0..6 {
            v.words.insert(format!("l{i}"), learning("1111111111", 3));
        }
        v.words.insert("hard".into(), learning("0001000101", 3));
        for i in 0..40 {
            v.words.insert(format!("f{i:02}"), Word { status: Status::Review, level: 2, due: 900, recent: "1111111111".into(), ..Word::default() });
        }
        let mut pool = names(20);
        let c = Config { new_per_session: 0, ..cfg() };
        let mut counts: HashMap<String, usize> = HashMap::new();
        for seed in 0..200 {
            let plan = plan(&v, &mut pool, &c, 1, 25, seed, &all, true);
            let n = |w: &str| plan.words.iter().filter(|x| *x == w).count();
            assert_eq!(n("hard"), 2, "the worst gets a second copy: {:?}", plan.words);
            for i in 0..6 {
                assert_eq!(n(&format!("l{i}")), 1, "everyone else once: {:?}", plan.words);
            }
            assert_eq!(plan.words.len(), 25);
            for w in &plan.words {
                *counts.entry(w.clone()).or_default() += 1;
            }
        }
        assert!(counts["hard"] > counts["l0"], "the word with errors is seen more: {counts:?}");
        // more learning words than the 40% share (10 of 25): only 10 copies, the weighted ones
        let mut crowded = Vocab::default();
        for i in 0..14 {
            crowded.words.insert(format!("l{i:02}"), learning(if i < 4 { "0101010101" } else { "1111111111" }, 3));
        }
        for i in 0..40 {
            crowded.words.insert(format!("f{i:02}"), Word { status: Status::Review, level: 2, due: 900, recent: "1111111111".into(), ..Word::default() });
        }
        let mut seen: HashMap<String, usize> = HashMap::new();
        for seed in 0..200 {
            let plan = plan(&crowded, &mut pool, &c, 1, 25, seed, &all, true);
            assert_eq!(plan.words.iter().filter(|w| w.starts_with('l')).count(), 10, "{:?}", plan.words);
            for w in plan.words.iter().filter(|w| w.starts_with('l')) {
                *seen.entry(w.clone()).or_default() += 1;
            }
        }
        assert!(seen["l00"] > seen["l13"], "the ones with errors come more often: {seen:?}");
        assert!(seen.len() == 14, "everyone gets a turn: {seen:?}");
    }

    #[test]
    fn the_easy_base_is_the_cleanest_reviews_not_yet_due() {
        let mut v = Vocab::default();
        let review = |recent: &str, level: u8| Word { status: Status::Review, level, due: 120, recent: recent.into(), attempts: 10, ..Word::default() };
        v.words.insert("c1".into(), review("1111111111", 3));
        v.words.insert("c2".into(), review("1111111111", 2));
        for (i, r) in ["1111101111", "1011101111", "0001000101", "0000000011"].iter().enumerate() {
            v.words.insert(format!("p{i}"), review(r, 1));
        }
        v.words.insert("due".into(), Word { status: Status::Review, due: 5, ..Word::default() });
        let mut pool: Vec<String> = Vec::new();
        let mut got: HashMap<String, usize> = HashMap::new();
        for seed in 0..100 {
            let plan = plan(&v, &mut pool, &cfg(), 100, 3, seed, &all, true);
            assert!(plan.words.contains(&"due".to_owned()), "{:?}", plan.words);
            assert_eq!(plan.fillers.len(), 2, "{:?}", plan.fillers);
            for w in &plan.fillers {
                *got.entry(w.clone()).or_default() += 1;
            }
        }
        let n = |w: &str| got.get(w).copied().unwrap_or(0);
        assert!(n("c1") + n("c2") > 4 * (n("p2") + n("p3")), "the worst are rarely the base: {got:?}");
        assert!(n("c1") + n("c2") > n("p0") + n("p1") - 20, "{got:?}");
    }

    #[test]
    fn retired_words_never_appear_and_an_empty_vocabulary_makes_an_empty_test() {
        let mut v = Vocab::default();
        v.words.insert("gone".into(), word(Status::Retired));
        let mut pool = vec!["gone".to_owned()];
        let plan = p(&v, &mut pool, 25, 1, 100);
        assert!(plan.words.is_empty(), "{:?}", plan.words);
    }

    #[test]
    fn no_word_twice_in_a_row_and_the_seed_decides_the_order() {
        let mut v = Vocab::default();
        for i in 0..4 {
            v.words.insert(format!("l{i}"), learning("1111", 2));
        }
        let mut pool = names(20);
        for seed in 0..100 {
            let plan = p(&v, &mut pool, 25, seed, 1);
            assert!(plan.words.windows(2).all(|w| w[0] != w[1]), "seed {seed}: {:?}", plan.words);
        }
        assert_eq!(p(&v, &mut pool, 25, 9, 1), p(&v, &mut pool, 25, 9, 1));
        assert_ne!(p(&v, &mut pool, 25, 9, 1).words, p(&v, &mut pool, 25, 10, 1).words);
    }

    #[test]
    fn a_timed_feed_repeats_rounds_without_new_words() {
        let mut v = Vocab::default();
        for i in 0..6 {
            v.words.insert(format!("l{i}"), learning("11111", 2));
        }
        let mut pool = names(40);
        let feed = plan_feed(&v, &mut pool, &cfg(), 1, 200, 5, &all);
        assert_eq!(feed.words.len(), 200);
        assert_eq!(feed.new_words.len(), 2, "only the first round brings new words");
        assert!(feed.words.windows(2).all(|w| w[0] != w[1]));
        let (e, e2) = (Vocab::default(), Vocab::default());
        let mut empty: Vec<String> = Vec::new();
        none(&plan_feed(&e, &mut empty, &cfg(), 1, 50, 5, &all).words);
        assert_eq!(e, e2);
    }

    // ---- what a test teaches ----

    #[test]
    fn a_new_word_that_is_typed_becomes_learning_and_the_windows_move() {
        let mut v = Vocab::default();
        let s = apply(&mut v, &[hit("cat"), miss("cat"), hit("dog")], &cfg(), 50);
        assert_eq!(s.new, ["cat", "dog"]);
        let cat = &v.words["cat"];
        assert_eq!((cat.status, cat.attempts, cat.hits, cat.recent.as_str(), cat.days, cat.last_seen, cat.streak), (Status::Learning, 2, 1, "10", 1, 50, 1));
        assert_eq!(v.recent, "101");
        assert_eq!(v.sessions, 1);
        let dog = &v.words["dog"];
        assert_eq!(dog.speed_ms, Some(1000.0));
        let again = apply(&mut v, &[Attempt { word: "dog".into(), hit: true, stumbles: 0, secs: Some(2.0) }], &cfg(), 50);
        none(&again.new);
        assert!((v.words["dog"].speed_ms.unwrap() - 1300.0).abs() < 1e-9, "0.3 of the way to the new time");
        assert_eq!(v.words["dog"].days, 1, "the same day is one day");
        apply(&mut v, &[hit("dog")], &cfg(), 51);
        assert_eq!(v.words["dog"].days, 2);
    }

    #[test]
    fn retired_words_are_untouched_by_a_test() {
        let mut v = Vocab::default();
        v.words.insert("gone".into(), word(Status::Retired));
        let s = apply(&mut v, &[hit("gone")], &cfg(), 5);
        assert_eq!(v.words["gone"], word(Status::Retired));
        none(&s.new);
        none(&s.mastered);
        assert_eq!(v.recent, "");
    }

    #[test]
    fn mastery_needs_nine_of_ten_clean_and_a_speed_near_the_median_and_no_days() {
        let mut v = Vocab::default();
        v.words.insert("cat".into(), learning("111111111", 3));
        // ten right: mastered, review level 1, due tomorrow
        let s = apply(&mut v, &[hit("cat")], &cfg(), 60);
        assert_eq!(s.mastered, ["cat"]);
        let cat = &v.words["cat"];
        assert_eq!((cat.status, cat.level, cat.due, cat.mastered, cat.promotions), (Status::Review, 1, 61, Some(60), 1));
        // 9 of 10 clean is enough; 8 of 10 is not
        let mut v = Vocab::default();
        v.words.insert("cat".into(), learning("111101111", 3));
        assert_eq!(apply(&mut v, &[hit("cat")], &cfg(), 60).mastered, ["cat"]);
        let mut v = Vocab::default();
        v.words.insert("cat".into(), learning("110101111", 3));
        none(&apply(&mut v, &[hit("cat")], &cfg(), 60).mastered);
        // a stumble is not clean: the tenth attempt right but corrected leaves 8 of 10
        let mut v = Vocab::default();
        v.words.insert("cat".into(), learning("111101111", 3));
        none(&apply(&mut v, &[stumble("cat", 1)], &cfg(), 60).mastered);
        // no day count: a word on its first day with ten clean attempts is mastered
        let mut v = Vocab::default();
        v.words.insert("cat".into(), Word { last_seen: 60, ..learning("111111111", 1) });
        assert_eq!(apply(&mut v, &[hit("cat")], &cfg(), 60).mastered, ["cat"], "one day is enough");
        // too few results
        let mut v = Vocab::default();
        v.words.insert("cat".into(), learning("11111111", 5));
        none(&apply(&mut v, &[hit("cat")], &cfg(), 60).mastered);
    }

    #[test]
    fn a_slow_word_is_not_mastered_while_the_median_says_it_is_slow() {
        let mut v = Vocab::default();
        for i in 0..12 {
            v.words.insert(format!("f{i}"), Word { status: Status::Review, level: 2, due: 999, attempts: 20, speed_ms: Some(1000.0), ..Word::default() });
        }
        v.words.insert("slow".into(), Word { speed_ms: Some(2400.0), attempts: 9, last_seen: 1, ..learning("111111111", 4) });
        v.words.insert("ok".into(), Word { speed_ms: Some(1400.0), attempts: 9, last_seen: 1, ..learning("111111111", 4) });
        let s = apply(&mut v, &[Attempt { word: "slow".into(), hit: true, stumbles: 0, secs: Some(2.4) }, Attempt { word: "ok".into(), hit: true, stumbles: 0, secs: Some(1.4) }], &cfg(), 5);
        assert_eq!(s.mastered, ["ok"], "1.4x the median passes, 2.4x does not");
    }

    #[test]
    fn a_due_review_passed_moves_up_and_waits_longer() {
        let c = cfg();
        let mut v = Vocab::default();
        v.words.insert("cat".into(), Word { status: Status::Review, level: 1, due: 60, last_seen: 59, ..Word::default() });
        let mut day = 60;
        let mut gaps = Vec::new();
        for _ in 0..7 {
            let s = apply(&mut v, &[hit("cat")], &c, day);
            assert_eq!(s.promoted, ["cat"], "day {day}");
            let w = &v.words["cat"];
            gaps.push(w.due - day);
            day = w.due;
        }
        assert_eq!(gaps, [3, 7, 21, 60, 180, 180, 180], "levels 2 to 6, then it stays at the top");
        assert_eq!(v.words["cat"].level, 6);
        assert_eq!(v.words["cat"].promotions, 7);
    }

    #[test]
    fn a_review_word_that_is_not_due_does_not_move_when_passed() {
        let mut v = Vocab::default();
        v.words.insert("cat".into(), Word { status: Status::Review, level: 2, due: 90, ..Word::default() });
        let s = apply(&mut v, &[hit("cat")], &cfg(), 60);
        none(&s.promoted);
        assert_eq!((v.words["cat"].level, v.words["cat"].due), (2, 90));
    }

    #[test]
    fn a_miss_demotes_one_level_and_makes_it_due_tomorrow() {
        let mut v = Vocab::default();
        v.words.insert("cat".into(), Word { status: Status::Review, level: 4, due: 60, ..Word::default() });
        // a miss then a right retry in the same test: demoted once, not promoted
        let s = apply(&mut v, &[miss("cat"), hit("cat")], &cfg(), 60);
        assert_eq!(s.demoted, ["cat"]);
        none(&s.promoted);
        let w = &v.words["cat"];
        assert_eq!((w.status, w.level, w.due, w.streak), (Status::Review, 3, 61, 0));
        // never below level 1
        let mut v = Vocab::default();
        v.words.insert("cat".into(), Word { status: Status::Review, level: 1, due: 60, ..Word::default() });
        apply(&mut v, &[miss("cat")], &cfg(), 60);
        assert_eq!(v.words["cat"].level, 1);
        assert_eq!(v.words["cat"].status, Status::Review);
    }

    #[test]
    fn missed_twice_in_a_row_goes_back_to_learning() {
        let mut v = Vocab::default();
        v.words.insert("cat".into(), Word { status: Status::Review, level: 3, due: 60, mastered: Some(40), ..Word::default() });
        let s = apply(&mut v, &[miss("cat"), miss("cat")], &cfg(), 60);
        assert_eq!(s.relearn, ["cat"]);
        let w = &v.words["cat"];
        assert_eq!((w.status, w.level, w.lapses, w.mastered), (Status::Learning, 0, 1, None));
        assert_eq!(w.recent, "00", "the window starts over: mastery has to be earned again");
        // a miss, a right answer in another test, a miss again: not in a row
        let mut v = Vocab::default();
        v.words.insert("cat".into(), Word { status: Status::Review, level: 3, due: 60, ..Word::default() });
        apply(&mut v, &[miss("cat")], &cfg(), 60);
        apply(&mut v, &[hit("cat")], &cfg(), 61);
        apply(&mut v, &[miss("cat")], &cfg(), 62);
        assert_eq!(v.words["cat"].status, Status::Review);
    }

    #[test]
    fn a_lapse_with_the_learning_pool_full_stays_a_level_one_review() {
        let mut v = Vocab::default();
        for i in 0..10 {
            v.words.insert(format!("l{i:02}"), learning("1", 1));
        }
        v.words.insert("cat".into(), Word { status: Status::Review, level: 3, due: 60, ..Word::default() });
        let s = apply(&mut v, &[miss("cat"), miss("cat")], &cfg(), 60);
        none(&s.relearn);
        let cat = &v.words["cat"];
        assert_eq!((cat.status, cat.level, cat.due, cat.lapses), (Status::Review, 2, 61, 1), "one level down, due tomorrow, the lapse counted");
        assert_eq!(v.count(Status::Learning), 10, "the cap holds");
    }

    #[test]
    fn the_summary_line_names_what_changed() {
        assert_eq!(Summary::default().line(), "");
        let s = Summary { new: vec!["a".into(), "b".into()], mastered: vec!["c".into()], due: 3, ..Summary::default() };
        assert_eq!(s.line(), "+2 new  1 mastered  3 due");
    }

    #[test]
    fn due_reviews_are_the_overdue_first() {
        let mut v = Vocab::default();
        v.words.insert("b1".into(), Word { status: Status::Review, due: 5, ..Word::default() });
        v.words.insert("a1".into(), Word { status: Status::Review, due: 9, ..Word::default() });
        v.words.insert("c1".into(), Word { status: Status::Review, due: 20, ..Word::default() });
        v.words.insert("d1".into(), learning("1", 1));
        let due: Vec<&str> = due_reviews(&v, 10).iter().map(|(w, _)| w.as_str()).collect();
        assert_eq!(due, ["b1", "a1"]);
    }

    // ---- stumbles ----

    #[test]
    fn a_stumbled_attempt_is_a_miss_that_keeps_its_count() {
        let mut v = Vocab::default();
        v.words.insert("cat".into(), learning("1111", 2));
        let s = apply(&mut v, &[hit("cat"), stumble("cat", 2), hit("dog")], &cfg(), 50);
        let cat = &v.words["cat"];
        assert_eq!((cat.attempts, cat.hits, cat.recent.as_str()), (6, 5, "111110"), "{cat:?}");
        assert_eq!((cat.stumbles, cat.stumbled_in, cat.stumbled_n, cat.stumble_days.clone()), (2, 1, 2, vec![[50, 2]]));
        assert_eq!(v.recent, "101");
        none(&s.mastered);
        // a second test: the in-test count starts over, the days add up, old days drop out
        apply(&mut v, &[stumble("cat", 1)], &cfg(), 52);
        apply(&mut v, &[stumble("cat", 1)], &cfg(), 60);
        let cat = &v.words["cat"];
        assert_eq!((cat.stumbles, cat.stumbled_in, cat.stumbled_n), (4, 3, 1));
        assert_eq!(cat.stumble_days, vec![[60, 1]], "a week of days is kept");
        // a word committed wrong counts as a stumble of one but not as a correction day
        apply(&mut v, &[miss("cat")], &cfg(), 61);
        let cat = &v.words["cat"];
        assert_eq!((cat.stumbles, cat.stumbled_n, cat.stumble_days.clone()), (5, 1, vec![[60, 1]]));
    }

    #[test]
    fn a_due_review_that_stumbles_is_a_miss() {
        let mut v = Vocab::default();
        v.words.insert("cat".into(), Word { status: Status::Review, level: 3, due: 60, ..Word::default() });
        let s = apply(&mut v, &[stumble("cat", 1), hit("cat")], &cfg(), 60);
        assert_eq!(s.demoted, ["cat"]);
        none(&s.promoted);
        assert_eq!((v.words["cat"].level, v.words["cat"].due), (2, 61));
    }

    #[test]
    fn words_that_stumbled_lately_are_drilled_first_inside_the_share_and_the_old_ones_are_not() {
        let mut v = Vocab { sessions: 10, ..Vocab::default() };
        let stumbled = |in_test, n| Word { stumbled_in: in_test, stumbled_n: n, stumbles: n, ..learning("1111111111", 3) };
        v.words.insert("fresh".into(), stumbled(10, 1)); // the last test
        v.words.insert("bad".into(), stumbled(9, 3)); // the one before, three times
        v.words.insert("old".into(), stumbled(8, 3)); // three tests ago: not recent
        for i in 0..5 {
            v.words.insert(format!("l{i}"), learning("1111111111", 3));
        }
        for i in 0..30 {
            v.words.insert(format!("z{i:02}"), Word { status: Status::Review, level: 2, due: 900, recent: "1111111111".into(), ..Word::default() });
        }
        let c = Config { new_per_session: 0, ..cfg() };
        let mut pool = names(5);
        let mut counts: HashMap<String, usize> = HashMap::new();
        for seed in 0..100 {
            let plan = plan(&v, &mut pool, &c, 1, 10, seed, &all, true);
            let n = |w: &str| plan.words.iter().filter(|x| *x == w).count();
            assert!(n("fresh") >= 1 && n("bad") >= 1, "seed {seed}: {:?}", plan.words);
            assert_eq!(plan.words.iter().filter(|w| !w.starts_with('z')).count(), 4, "the learning share is 40%: {:?}", plan.words);
            for w in &plan.words {
                *counts.entry(w.clone()).or_default() += 1;
            }
        }
        assert!(counts["old"] < counts["fresh"] && counts["l0"] < counts["fresh"], "{counts:?}");
        // with room for one, the worst comes first
        let plan = plan(&v, &mut pool, &c, 1, 4, 3, &all, true);
        assert_eq!(plan.words.iter().filter(|w| *w == "bad").count(), 1, "{:?}", plan.words);
        assert_eq!(plan.words.iter().filter(|w| *w == "fresh").count(), 0, "{:?}", plan.words);
    }

    #[test]
    fn a_stumble_weighs_more_the_more_there_were() {
        let base = learning("1111111111", 3);
        let one = Word { stumbled_n: 1, ..base.clone() };
        let four = Word { stumbled_n: 4, ..base.clone() };
        assert!(weight(&base, false) < weight(&one, true) && weight(&one, true) < weight(&four, true));
        assert!((weight(&four, false) - weight(&base, false)).abs() < 1e-9, "an old stumble weighs nothing");
        assert!(recently_stumbled(&Word { stumbled_in: 9, ..base.clone() }, 10, &cfg()));
        assert!(!recently_stumbled(&Word { stumbled_in: 8, ..base.clone() }, 10, &cfg()));
        assert!(!recently_stumbled(&base, 1, &cfg()), "never stumbled");
    }

    #[test]
    fn the_retuned_constants() {
        let c = cfg();
        assert_eq!((c.new_per_session, c.learning_cap, c.respace_gap, c.second_copies), (2, 10, 3, 3));
        assert!((c.accuracy_threshold - 0.70).abs() < 1e-9 && (c.mastery_accuracy - 0.90).abs() < 1e-9);
        assert!((c.review_share - 0.30).abs() < 1e-9 && (c.learning_share - 0.40).abs() < 1e-9);
        assert_eq!((c.stumble_sessions, c.stuck_attempts, c.stuck_days, c.stuck_per_test), (2, 30, 7, 2));
        assert!((c.speed_factor - 1.5).abs() < 1e-9 && (c.stuck_clean - 0.30).abs() < 1e-9);
    }

    // ---- composition of a whole test ----

    /// A vocabulary like the typist's: `n_learn` learning words, 20 reviews due, 60 not yet due.
    fn crowded_vocab(n_learn: usize, stuck: usize) -> Vocab {
        let mut v = Vocab { sessions: 5, ..Vocab::default() };
        for i in 0..n_learn {
            v.words.insert(format!("l{i:02}"), Word { stumbled_in: if i < 3 { 5 } else { 0 }, stumbled_n: u32::from(i < 3), ..learning("0100100010", 9) });
        }
        for i in 0..stuck {
            v.words.insert(format!("s{i:02}"), Word { stuck: true, last_seen: i as u32, ..learning("0000000000", 9) });
        }
        for i in 0..20 {
            v.words.insert(format!("d{i:02}"), Word { status: Status::Review, level: 2, due: 50 + i, recent: "1111111111".into(), ..Word::default() });
        }
        for i in 0..60 {
            v.words.insert(format!("z{i:02}"), Word { status: Status::Review, level: 2, due: 900, recent: if i < 40 { "1111111111" } else { "1111011111" }.into(), attempts: 10, ..Word::default() });
        }
        v
    }

    #[test]
    fn a_test_has_exactly_count_words_split_into_learning_due_and_an_easy_base() {
        let v = crowded_vocab(10, 4);
        let mut pool = names(30);
        let c = cfg();
        for seed in 0..50 {
            for count in [10, 25, 40] {
                let plan = plan(&v, &mut pool, &c, 100, count, seed, &all, true);
                assert_eq!(plan.words.len(), count, "seed {seed} count {count}");
                let of = |pre: char| plan.words.iter().filter(|w| w.starts_with(pre)).count();
                let share = |f: f64| (count as f64 * f).floor() as usize;
                assert!(of('l') + of('s') <= share(c.learning_share), "learning {} stuck {} of {count}: {:?}", of('l'), of('s'), plan.words);
                assert!(of('s') <= c.stuck_per_test, "stuck words are drilled lightly: {:?}", plan.words);
                assert_eq!(of('d'), share(c.review_share), "due reviews fill their share");
                assert_eq!(plan.due, of('d'));
                assert_eq!(of('z'), plan.words.len() - of('l') - of('s') - of('d'), "the rest is the easy base");
                assert!(of('z') >= count - share(c.learning_share) - share(c.review_share), "{count}: {:?}", plan.words);
                assert!(plan.fillers.iter().all(|f| f.starts_with('z')) && plan.fillers.len() == of('z'), "{:?}", plan.fillers);
                if count >= 25 {
                    for w in ["l00", "l01", "l02"] {
                        assert!(plan.words.contains(&w.to_owned()), "a word that stumbled lately is drilled: {w} in {:?}", plan.words);
                    }
                }
                let mut seen: HashMap<&str, usize> = HashMap::new();
                for w in &plan.words {
                    *seen.entry(w.as_str()).or_default() += 1;
                }
                assert!(seen.values().all(|n| *n <= 2), "{:?}", plan.words);
            }
        }
    }

    #[test]
    fn new_words_come_with_two_copies_inside_the_learning_share() {
        let mut v = crowded_vocab(6, 0);
        v.words.insert("z00".into(), word(Status::Review));
        let mut pool = names(30);
        let plan = plan(&v, &mut pool, &cfg(), 100, 25, 4, &all, true);
        assert_eq!(plan.new_words.len(), 2);
        assert_eq!(plan.words.len(), 25);
        for n in &plan.new_words {
            assert_eq!(plan.words.iter().filter(|w| *w == n).count(), 2, "{n}: {:?}", plan.words);
        }
        let learning_copies = plan.words.iter().filter(|w| w.starts_with('l') || plan.new_words.contains(w)).count();
        assert!(learning_copies <= 10, "{learning_copies}: {:?}", plan.words);
    }

    #[test]
    fn stuck_words_are_rotated_by_how_long_they_have_been_unseen() {
        let v = crowded_vocab(10, 5);
        let mut pool = names(5);
        let plan = plan(&v, &mut pool, &cfg(), 100, 25, 1, &all, true);
        let stuck: Vec<&String> = plan.words.iter().filter(|w| w.starts_with('s')).collect();
        assert_eq!(stuck.len(), 2, "{:?}", plan.words);
        assert!(stuck.contains(&&"s00".to_owned()) && stuck.contains(&&"s01".to_owned()), "the longest unseen: {stuck:?}");
    }

    // ---- the gate, the easy base, stuck words ----

    #[test]
    fn the_easy_base_counts_the_first_attempt_of_each_review_word_only() {
        let mut v = Vocab::default();
        v.words.insert("a1".into(), Word { status: Status::Review, level: 2, due: 90, ..Word::default() });
        v.words.insert("b1".into(), Word { status: Status::Review, level: 2, due: 90, ..Word::default() });
        v.words.insert("l1".into(), learning("1", 1));
        apply(&mut v, &[stumble("a1", 1), hit("a1"), hit("b1"), hit("l1"), miss("l1"), hit("new1"), hit("new1")], &cfg(), 50);
        assert_eq!(v.base_recent, "01", "a1's first attempt was not clean, b1's was; learning and new words are not the base");
        assert_eq!(v.base_tests, vec![[1, 2]]);
        assert_eq!(v.recent.len(), 7, "every attempt is still in the overall window");
        for i in 0..4 {
            apply(&mut v, &[hit("a1"), hit("b1")], &cfg(), 51 + i);
        }
        assert_eq!(v.base_tests, vec![[2, 2], [2, 2], [2, 2]], "the last three tests");
        assert!((recent_tests_accuracy(&v).unwrap() - 1.0).abs() < 1e-9);
        let mut w = Vocab::default();
        apply(&mut w, &[hit("only_new")], &cfg(), 5);
        assert!(w.base_recent.is_empty() && w.base_tests.is_empty(), "no review word, no base");
        assert!(learning_accuracy(&v).is_some());
    }

    #[test]
    fn a_word_is_stuck_after_enough_unclean_practice_and_leaves_the_cap() {
        let c = cfg();
        let mut v = Vocab::default();
        let base = Word { learn_n: 29, learn_hits: 3, days: 8, ..learning("0000100000", 8) };
        v.words.insert("hard".into(), base.clone());
        // the 30th attempt, unclean: 3 of 30 is 10%, over 7 days: stuck
        let s = apply(&mut v, &[stumble("hard", 1)], &c, 60);
        assert_eq!(s.stuck, ["hard"]);
        assert!(v.words["hard"].stuck && v.words["hard"].status == Status::Learning);
        assert_eq!((v.active_learning(), v.stuck_count()), (0, 1));
        assert!(s.line().contains("1 stuck"), "{}", s.line());
        // not enough attempts, days or a bad enough rate
        for w in [Word { learn_n: 28, ..base.clone() }, Word { days: 5, ..base.clone() }, Word { learn_hits: 9, ..base }] {
            let mut v = Vocab::default();
            v.words.insert("hard".into(), w);
            none(&apply(&mut v, &[stumble("hard", 1)], &c, 60).stuck);
        }
        // a stuck word that gets to 9 of 10 clean with a free slot comes back; without one it stays
        let mut v = Vocab::default();
        v.words.insert("hard".into(), Word { stuck: true, learn_n: 40, learn_hits: 8, days: 9, ..learning("0111111111", 9) });
        let s = apply(&mut v, &[hit("hard")], &c, 70);
        assert!(s.mastered == ["hard"] || s.unstuck == ["hard"], "{s:?}");
        let mut v = Vocab::default();
        for i in 0..10 {
            v.words.insert(format!("l{i}"), learning("1", 1));
        }
        // (slow for its level, so it is not mastered: mastery has no day count)
        for i in 0..12 {
            v.words.insert(format!("f{i}"), Word { status: Status::Review, level: 2, due: 999, attempts: 20, speed_ms: Some(1000.0), ..Word::default() });
        }
        v.words.insert("hard".into(), Word { stuck: true, learn_n: 40, learn_hits: 8, days: 2, last_seen: 70, speed_ms: Some(9000.0), attempts: 9, ..learning("1111111110", 2) });
        let s = apply(&mut v, &[hit("hard")], &c, 70);
        none(&s.mastered);
        none(&s.unstuck);
        assert!(v.words["hard"].stuck, "the pool is full: it waits");
        // entering learning (or lapsing back to it) starts the count over
        let mut v = Vocab::default();
        apply(&mut v, &[hit("fresh")], &c, 5);
        let w = &v.words["fresh"];
        assert_eq!((w.learn_n, w.learn_hits, w.practiced, w.stuck), (1, 1, 1, false));
    }

    #[test]
    fn mastery_clears_a_stuck_mark() {
        let mut v = Vocab::default();
        v.words.insert("hard".into(), Word { stuck: true, learn_n: 40, learn_hits: 8, days: 9, last_seen: 69, ..learning("1111111111", 9) });
        let s = apply(&mut v, &[hit("hard")], &cfg(), 70);
        assert_eq!(s.mastered, ["hard"]);
        let w = &v.words["hard"];
        assert_eq!((w.status, w.stuck), (Status::Review, false));
    }

    // ---- a simulated learner ----

    /// A learner that forgets, like the typist whose clean rate on a hard word is about 70%: a
    /// word is right the first time with `first`, and later with `ceil` decayed by the days since
    /// it was last typed clean, slower the more often it has been. Words in `easy` are clean 90% of
    /// the time; with `hard_p` every other word is clean with that chance only, learned or not.
    struct Learner {
        rng: Rng,
        stability: HashMap<String, f64>,
        last: HashMap<String, f64>,
        easy: HashSet<String>,
        hard_p: Option<f64>,
        first: f64,
        ceil: f64,
        clean: usize,
        attempts: usize,
    }

    impl Learner {
        fn new(seed: u64) -> Self {
            Self { rng: Rng::new(seed), stability: HashMap::new(), last: HashMap::new(), easy: HashSet::new(), hard_p: None, first: 0.40, ceil: 0.76, clean: 0, attempts: 0 }
        }

        fn unit(&mut self) -> f64 {
            (self.rng.next_u64() >> 11) as f64 / (1_u64 << 53) as f64
        }

        fn recall(&self, w: &str, at: f64, mood: f64) -> f64 {
            let p = if self.easy.contains(w) {
                0.90
            } else if let Some(p) = self.hard_p {
                p
            } else {
                self.last.get(w).map_or(self.first, |l| self.ceil * (-(at - l) / self.stability.get(w).copied().unwrap_or(3.0)).exp())
            };
            (p * mood).clamp(0.02, 0.99)
        }

        /// Whether the word is typed clean at time `at` (days).
        fn attempt(&mut self, w: &str, at: f64, mood: f64) -> bool {
            let p = self.recall(w, at, mood);
            let ok = self.unit() <= p;
            let s = self.stability.entry(w.to_owned()).or_insert(3.0);
            let same_day = self.last.get(w).is_some_and(|l| at - l < 1.0);
            *s = if ok { (*s * if same_day { 1.25 } else { 2.0 }).min(400.0) } else { (*s * 0.75).max(1.5) };
            self.last.insert(w.to_owned(), at);
            self.attempts += 1;
            self.clean += usize::from(ok);
            ok
        }
    }

    /// Types a test through the real typing session: a clean word is typed and committed; an
    /// unclean one is mostly typed wrong, corrected and typed right (a stumble), sometimes
    /// committed wrong. Returns the session.
    fn play(plan: &Plan, learner: &mut Learner, c: &Config, day: u32, mood: f64, requeue: bool) -> Session {
        let mut s = Session::new(plan.words.clone(), false);
        if requeue {
            s = s.with_requeue(c.respace_gap, plan.fillers.iter().cloned().collect());
        }
        let mut t = 0.0;
        let mut n: f64 = 0.0;
        while let Some(target) = s.target().map(str::to_owned) {
            t += learner.unit().mul_add(0.5, 1.0);
            let clean = learner.attempt(&target, n.mul_add(0.01, f64::from(day)), mood);
            n += 1.0;
            if clean {
                for ch in target.chars().chain(std::iter::once(' ')) {
                    s.apply(Input::Char(ch), t);
                }
            } else if learner.unit() < 0.75 {
                s.apply(Input::Char('#'), t);
                s.apply(Input::Char('#'), t);
                s.apply(Input::Backspace, t);
                s.apply(Input::Backspace, t);
                for ch in target.chars().chain(std::iter::once(' ')) {
                    s.apply(Input::Char(ch), t + 0.4);
                }
            } else {
                s.apply(Input::Char('#'), t);
                s.apply(Input::Commit, t);
            }
        }
        s
    }

    /// The typist's start, seeded as `lyk` seeds it from a tape: 120 words typed cleanly (review),
    /// 10 hard ones (learning).
    fn seeded(c: &Config, today: u32) -> (Vocab, HashSet<String>) {
        let mut tape: Vec<TapeWord> = (0..120)
            .map(|i| TapeWord { word: format!("e{i:03}"), attempts: 40, errors: if i % 2 == 0 { 4 } else { 10 }, days: 9, last_day: Some(i64::from(today)) })
            .collect();
        tape.extend((0..10).map(|i| TapeWord { word: format!("h{i:02}"), attempts: 30, errors: 20, days: 9, last_day: Some(i64::from(today)) }));
        let easy = tape.iter().filter(|t| t.word.starts_with('e')).map(|t| t.word.clone()).collect();
        (crate::vocab::seed(&tape, &[], c, today, &|_| true), easy)
    }

    struct Run {
        vocab: Vocab,
        /// (day, vocabulary size, frontier rank, active learning words, stuck words) at each day's end
        curve: Vec<(u32, usize, usize, usize, usize)>,
        plans: Vec<(u32, Plan)>,
        summaries: Vec<Summary>,
        /// Words typed, and how many of those attempts were clean
        typed: (usize, usize),
        start_size: usize,
    }

    /// `days` days of one 25-word test each. `bad` is a run of days `[from, to)` on which the typist
    /// is at 35% of their usual; `hard_p` makes every hard word that likely clean, always.
    fn simulate(days: u32, seed: u64, bad: Option<(u32, u32)>, retire: Option<(u32, &str)>, hard_p: Option<f64>) -> Run {
        simulate_with(days, seed, bad, retire, hard_p, false)
    }

    /// As `simulate`; `requeue` plays the tests with the in-test replacement of stumbled words on.
    fn simulate_with(days: u32, seed: u64, bad: Option<(u32, u32)>, retire: Option<(u32, &str)>, hard_p: Option<f64>, requeue: bool) -> Run {
        let c = cfg();
        let mut pool = names(5000);
        let (mut vocab, easy) = seeded(&c, 1000);
        let mut learner = Learner::new(seed);
        learner.easy = easy;
        learner.hard_p = hard_p;
        let start_size = vocab.size();
        let mut run = Run { vocab: Vocab::default(), curve: Vec::new(), plans: Vec::new(), summaries: Vec::new(), typed: (0, 0), start_size };
        // the due days are not a wall
        let mut load: HashMap<u32, usize> = HashMap::new();
        for w in vocab.words.values().filter(|w| w.status == Status::Review) {
            *load.entry(w.due).or_default() += 1;
        }
        assert!(load.values().all(|n| *n <= crate::vocab::SEED_MAX_PER_DAY), "at most {} due a day at the start: {load:?}", crate::vocab::SEED_MAX_PER_DAY);
        assert!(load.len() >= 8, "spread over days: {load:?}");
        for d in 0..days {
            let day = 1000 + d;
            if let Some((on, w)) = retire
                && d == on
            {
                vocab.words.insert(w.to_owned(), word(Status::Retired));
            }
            let mood = if bad.is_some_and(|(a, b)| (a..b).contains(&d)) { 0.35 } else { 1.0 };
            let plan = plan(&vocab, &mut pool, &c, day, 25, seed ^ u64::from(day), &all, true);
            assert_eq!(plan.words.len(), 25, "day {d}: a test has exactly the count: {:?}", plan.words);
            let learning_copies = plan.words.iter().filter(|w| vocab.words.get(*w).is_some_and(|x| x.status == Status::Learning) || plan.new_words.contains(w)).count();
            assert!(learning_copies <= 10, "day {d}: {learning_copies} learning copies of 25: {:?}", plan.words);
            assert!(plan.due <= 7, "day {d}: {} due", plan.due);
            let stuck_in = plan.words.iter().filter(|w| vocab.words.get(*w).is_some_and(|x| x.stuck)).count();
            assert!(stuck_in <= c.stuck_per_test, "day {d}: {stuck_in} stuck words in a test: {:?}", plan.words);
            if !plan.new_words.is_empty() {
                assert!(accuracy_ok(&vocab, &c), "day {d}: new words came with an easy base of {:?} over {} attempts", base_accuracy(&vocab), vocab.base_recent.len());
            }
            let session = play(&plan, &mut learner, &c, day, mood, requeue);
            assert_eq!(session.words.len(), 25, "day {d}: the test did not grow");
            assert_eq!(session.done.len(), 25, "day {d}: and all of it was typed");
            let attempts = attempts_of(&session);
            run.typed.0 += attempts.len();
            run.typed.1 += attempts.iter().filter(|a| a.hit).count();
            let summary = apply(&mut vocab, &attempts, &c, day);
            assert!(vocab.active_learning() <= c.learning_cap, "day {d}: {} learning", vocab.active_learning());
            assert!(summary.new.len() <= c.new_per_session, "day {d}: {} new", summary.new.len());
            let frontier = crate::vocab::find_frontier(&vocab, &mut pool).rank;
            run.curve.push((d + 1, vocab.size(), frontier, vocab.active_learning(), vocab.stuck_count()));
            run.plans.push((day, plan));
            run.summaries.push(summary);
        }
        run.vocab = vocab;
        run
    }

    #[test]
    fn ninety_simulated_days_obey_every_rule() {
        let run = simulate(90, 42, Some((40, 45)), Some((10, "w0003")), None);
        // the vocabulary grows steadily and never faster than K a day
        for &(d, size, ..) in &run.curve {
            assert!(size <= run.start_size + 2 * d as usize, "day {d}: vocabulary {size}");
        }
        let size = |d: u32| run.curve[d as usize - 1].1;
        assert!(size(90) >= run.start_size + 20, "growth is real: {} words after 90 days from {}", size(90), run.start_size);
        assert!(size(30) < size(60) && size(60) < size(90), "keeps growing: {} {} {}", size(30), size(60), size(90));
        // five bad days empty the easy base: the day after them introduces nothing
        let (_, after_bad) = &run.plans[45];
        assert!(after_bad.new_words.is_empty(), "no new words after five bad days: {:?}", after_bad.new_words);
        // retired words never appear after they are retired
        assert!(run.plans.iter().skip(10).all(|(_, p)| !p.words.contains(&"w0003".to_owned())), "w0003 is retired from day 10");
        assert_eq!(run.vocab.words["w0003"].status, Status::Retired);
        // words climb the levels
        let top = run.vocab.words.values().map(|w| w.level).max().unwrap_or(0);
        assert!(top >= 3, "a word reached level {top}");
        let clean = run.typed.1 as f64 / run.typed.0 as f64;
        assert!((0.65..=0.90).contains(&clean), "the simulated typist is clean {clean:.2} of the time");
        let wall = run.plans.iter().map(|(_, p)| p.due).max().unwrap_or(0);
        eprintln!("growth curve (day, vocabulary, frontier rank, learning, stuck): {:?}", run.curve.iter().filter(|c| c.0 % 10 == 0 || c.0 == 1).collect::<Vec<_>>());
        eprintln!(
            "after 90 days (from {} words): {} learning, {} stuck, {} review, levels {:?}, {} lapses, {} attempts {:.0}% clean, {} new words in total, most due reviews in a test {wall}",
            run.start_size,
            run.vocab.active_learning(),
            run.vocab.stuck_count(),
            run.vocab.count(Status::Review),
            (1..=6u8).map(|l| run.vocab.words.values().filter(|w| w.level == l && w.status == Status::Review).count()).collect::<Vec<_>>(),
            run.vocab.words.values().map(|w| w.lapses).sum::<u32>(),
            run.typed.0,
            clean * 100.0,
            run.summaries.iter().map(|s| s.new.len()).sum::<usize>()
        );
    }

    #[test]
    fn the_simulation_also_runs_with_the_requeue_on_and_both_keep_the_test_length() {
        let fixed = simulate_with(90, 42, Some((40, 45)), None, None, false);
        let requeued = simulate_with(90, 42, Some((40, 45)), None, None, true);
        // a fixed feed is exactly what was planned (`simulate` asserts the length and that every word was typed)
        for (_, p) in fixed.plans.iter().chain(&requeued.plans) {
            assert_eq!(p.words.len(), 25);
        }
        assert_eq!(requeued.plans.len(), 90);
        let at = |r: &Run, d: usize| r.curve[d - 1].1;
        eprintln!(
            "mastery without days, 90 days: fixed feed {} (day 30 {}, day 60 {}) words, requeued {} (day 30 {}, day 60 {}), {} and {} new words",
            fixed.vocab.size(), at(&fixed, 30), at(&fixed, 60), requeued.vocab.size(), at(&requeued, 30), at(&requeued, 60),
            fixed.summaries.iter().map(|s| s.new.len()).sum::<usize>(), requeued.summaries.iter().map(|s| s.new.len()).sum::<usize>()
        );
    }

    #[test]
    fn new_words_are_still_earned_while_every_learning_word_stumbles() {
        // the dead end: learning words clean 5% of the time, drilled every test, so the overall
        // clean share is tiny; the easy base is clean 90% so the gate is open, and the stuck words
        // free their slots
        let run = simulate(60, 3, None, None, Some(0.05));
        let new: usize = run.summaries.iter().map(|s| s.new.len()).sum();
        assert!(new >= 4, "{new} new words in 60 days with every learning word stumbling");
        let stuck_at = run.curve.iter().position(|c| c.4 > 0).expect("a word gets stuck");
        assert!(stuck_at < 35, "stuck by day {stuck_at}");
        assert!(run.curve.iter().all(|c| c.3 <= 10), "the cap holds: {:?}", run.curve);
        assert!(run.summaries.iter().map(|s| s.stuck.len()).sum::<usize>() >= 10, "the first ten learning words all get stuck");
        let overall = window_accuracy(&run.vocab).unwrap();
        assert!(overall < 0.6, "the overall clean share is poisoned by the drilled words: {overall:.2}");
        assert!(accuracy_ok(&run.vocab, &cfg()), "the easy base is clean: {:?}", base_accuracy(&run.vocab));
        assert!(base_accuracy(&run.vocab).unwrap() > 0.8);
        // the old gate (the overall share) would have been shut the whole time
        assert!(overall < cfg().accuracy_threshold);
        // a stuck word is still drilled, lightly
        let drilled = run.plans.iter().skip(stuck_at + 1).filter(|(_, p)| p.words.iter().any(|w| run.vocab.words.get(w).is_some_and(|x| x.stuck))).count();
        assert!(drilled > 20, "stuck words appear in {drilled} of the later tests");
    }

    #[test]
    fn a_mastered_word_stays_away_for_its_cooldown_and_then_returns() {
        let run = simulate(90, 7, None, None, None);
        // replay each plan: for every review word record the days it appeared, and check that after
        // it was passed at level L it did not come back before the interval unless it had no
        // competition for the slot (the filler of a short test)
        let mut seen: HashMap<String, Vec<u32>> = HashMap::new();
        for (day, p) in &run.plans {
            for w in &p.words {
                seen.entry(w.clone()).or_default().push(*day);
            }
        }
        let (mut checked, mut grew) = (0, 0);
        for (w, st) in &run.vocab.words {
            let Some(m) = st.mastered else { continue };
            if st.status != Status::Review || st.promotions < 3 {
                continue;
            }
            let days: Vec<u32> = seen[w].iter().copied().filter(|d| *d > m).collect();
            let mut distinct = days;
            distinct.dedup();
            let gaps: Vec<u32> = distinct.windows(2).map(|x| x[1] - x[0]).collect();
            assert!(gaps.iter().all(|g| *g >= 1), "{w}: {gaps:?}");
            assert!(distinct.first().is_none_or(|d| *d > m), "{w} came back the day it was mastered");
            // (a word that slipped comes back tomorrow, so only some words show the long gaps)
            grew += usize::from(gaps.iter().any(|g| *g >= 3));
            checked += 1;
        }
        assert!(checked > 0, "some word went through several reviews in 90 days");
        assert!(grew > 0, "some word returns on a growing schedule ({checked} checked)");
    }

    #[test]
    fn the_simulation_is_deterministic_for_a_seed() {
        let a = simulate(40, 5, Some((20, 22)), None, None);
        let b = simulate(40, 5, Some((20, 22)), None, None);
        assert_eq!(a.vocab, b.vocab);
        assert_eq!(a.plans, b.plans);
        let c = simulate(40, 6, Some((20, 22)), None, None);
        assert_ne!(a.plans, c.plans, "another seed, another feed");
    }

    #[test]
    fn a_learner_who_keeps_missing_gets_no_growth() {
        // a learner at 50% recall never has more than the learning cap of words from nothing
        let c = cfg();
        let mut pool = names(500);
        let mut vocab = Vocab::default();
        let mut learner = Learner::new(1);
        for d in 0..20u32 {
            let plan = plan(&vocab, &mut pool, &c, 1000 + d, 25, u64::from(d), &all, true);
            let attempts: Vec<Attempt> = plan.words.iter().map(|w| Attempt { word: w.clone(), hit: learner.rng.below(2) == 0, stumbles: 1, secs: Some(1.0) }).collect();
            apply(&mut vocab, &attempts, &c, 1000 + d);
        }
        assert!(vocab.size() <= c.learning_cap, "vocabulary {} for a learner at 50%", vocab.size());
    }
}

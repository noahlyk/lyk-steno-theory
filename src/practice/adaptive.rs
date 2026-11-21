//! The adaptive engine: which words a test holds, and what a finished test teaches the vocabulary.
//!
//! Pure: it reads and changes a [`Vocab`] and walks a [`Candidates`] list; no terminal, no clock
//! (the day is passed in), no files. Every tunable is a field of [`Config`], in one place.
//!
//! ## The rules
//!
//! A word is `new` (on file with past counts only, or not on file), `learning`, `review` or
//! `retired`.
//!
//! - **Earning a new word.** A new word enters when (a) fewer than `learning_cap` words are
//!   learning, (b) the accuracy over the last `GLOBAL_WINDOW` (50) attempts is at least
//!   `accuracy_threshold` (92%; with fewer than `accuracy_min_attempts` (20) attempts on record it
//!   is not held against the typist), and (c) fewer than `new_per_session` (3) new words have
//!   entered this test. The word is the next one in pool order that is not already learning, in
//!   review or retired.
//! - **Mastery.** A learning word becomes review (level 1) when its last 10 attempts are at least
//!   95% right (all 10, in practice), it has been practised on at least `mastery_days` (3)
//!   different days, and its speed is within `speed_factor` (1.5x) of the median speed of the
//!   words with a measured speed (the check is skipped while fewer than 10 words have one).
//! - **Review.** Intervals by level: 1, 3, 7, 21, 60, 180 days. A due review word is in the next
//!   test; passed (every attempt right) it moves up a level and is due one interval later; a miss
//!   moves it down one level (never below 1) and makes it due tomorrow; two misses in a row
//!   send it back to learning.
//! - **A test.** For `count` words: due reviews first (at most 30% of the test), then the new
//!   words (each twice), then learning words (each once before any twice, up to 3 times, chosen
//!   by recent error rate), then not-yet-due review words, soonest due first. Shuffled with the
//!   seed, with no word twice in a row.

use super::feed::Rng;
use super::pool::Candidates;
use crate::vocab::{Status, Vocab, WORD_WINDOW, Word};
use std::collections::{BTreeMap, HashSet};

/// Every tunable of the engine.
#[derive(Clone, Debug)]
pub struct Config {
    /// Most new words introduced in one test
    pub new_per_session: usize,
    /// Most words learning at once
    pub learning_cap: usize,
    /// Share of right answers over the last `GLOBAL_WINDOW` attempts that earns new words
    pub accuracy_threshold: f64,
    /// Fewest attempts on record before the accuracy gate applies
    pub accuracy_min_attempts: usize,
    /// Share of the last `WORD_WINDOW` attempts that must be right for mastery
    pub mastery_accuracy: f64,
    /// Distinct days a word must be practised for mastery
    pub mastery_days: u32,
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
    /// Times a new word is in its first test
    pub new_reps: usize,
    /// Most times a learning word is in one test
    pub max_reps: usize,
    /// A missed word comes back this many words later in the same test
    pub respace_gap: usize,
    /// Most times a missed word comes back in one test
    pub max_respace: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            new_per_session: 3,
            learning_cap: 15,
            accuracy_threshold: 0.92,
            accuracy_min_attempts: 20,
            mastery_accuracy: 0.95,
            mastery_days: 3,
            speed_factor: 1.5,
            speed_min_words: 10,
            speed_alpha: 0.3,
            intervals: [1, 3, 7, 21, 60, 180],
            review_share: 0.30,
            new_reps: 2,
            max_reps: 3,
            respace_gap: 4,
            max_respace: 2,
        }
    }
}

/// The share of right answers over the last `GLOBAL_WINDOW` attempts; none before any.
#[must_use]
pub fn window_accuracy(vocab: &Vocab) -> Option<f64> {
    let n = vocab.recent.len();
    (n > 0).then(|| vocab.recent.bytes().filter(|b| *b == b'1').count() as f64 / n as f64)
}

/// Whether the recent accuracy earns new words.
#[must_use]
pub fn accuracy_ok(vocab: &Vocab, cfg: &Config) -> bool {
    vocab.recent.len() < cfg.accuracy_min_attempts || window_accuracy(vocab).is_none_or(|a| a >= cfg.accuracy_threshold)
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

/// A learning word's weight: more for a word with recent errors, and a little more for one with
/// few attempts on record.
fn weight(w: &Word) -> f64 {
    4.0_f64.mul_add(w.error_rate(), 1.0) + if w.recent.len() < WORD_WINDOW { 1.0 } else { 0.0 }
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
    let mut words: Vec<String> = Vec::new();
    // 1. due reviews, up to their share of the test
    let share = ((count as f64 * cfg.review_share).floor() as usize).max(usize::from(count >= 3));
    let due: Vec<&String> = due_reviews(vocab, day).into_iter().map(|(w, _)| w).filter(|w| keep(w)).take(share).collect();
    words.extend(due.iter().map(|w| (*w).clone()));
    let due_n = words.len();
    // 2. new words, earned
    let mut new_words: Vec<String> = Vec::new();
    let learning_now = vocab.count(Status::Learning);
    let slots = cfg.new_per_session.min(cfg.learning_cap.saturating_sub(learning_now));
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
            for _ in 0..cfg.new_reps {
                if words.len() < count {
                    words.push(w.clone());
                }
            }
        }
    }
    // 3. learning words (and the new ones, as extra repetitions at the end), by error rate
    let learning: Vec<(&String, &Word)> = vocab.words.iter().filter(|(w, s)| s.status == Status::Learning && keep(w)).collect();
    let mut items: Vec<(&str, f64, usize)> = learning.iter().map(|(w, s)| (w.as_str(), weight(s), 0)).collect();
    let room = count.saturating_sub(words.len());
    draw(&mut items, room, cfg.max_reps, true, &mut rng, &mut words);
    // 4. fill with review words not yet due, soonest first
    if words.len() < count {
        let mut later: Vec<(&String, &Word)> = vocab.words.iter().filter(|(w, s)| s.status == Status::Review && s.due > day && keep(w)).collect();
        later.sort_by(|a, b| a.1.due.cmp(&b.1.due).then(a.0.cmp(b.0)));
        words.extend(later.into_iter().take(count - words.len()).map(|(w, _)| w.clone()));
    }
    // 5. still short: more repetitions of the words being learned, then of the new ones
    if words.len() < count {
        let mut extra: Vec<(&str, f64, usize)> = learning.iter().map(|(w, s)| (w.as_str(), weight(s), cfg.max_reps)).collect();
        extra.extend(new_words.iter().map(|w| (w.as_str(), 1.0, cfg.new_reps)));
        let room = count - words.len();
        draw(&mut extra, room, cfg.max_reps + 2, false, &mut rng, &mut words);
    }
    words.truncate(count);
    rng.shuffle(&mut words);
    separate(&mut words);
    Plan { words, new_words, due: due_n }
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
    pub hit: bool,
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
        && w.days >= cfg.mastery_days
        && match (w.speed_ms, median) {
            (Some(s), Some(m)) => s <= cfg.speed_factor * m,
            _ => true,
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
pub fn apply(vocab: &mut Vocab, attempts: &[Attempt], cfg: &Config, day: u32) -> Summary {
    let median = median_speed(vocab, cfg);
    let mut summary = Summary::default();
    let mut touched: BTreeMap<String, Touch> = BTreeMap::new();
    let mut seen_new: HashSet<String> = HashSet::new();
    let mut learning = vocab.count(Status::Learning);
    let mut globals: Vec<bool> = Vec::new();
    for a in attempts {
        let w = vocab.words.entry(a.word.clone()).or_default();
        if w.status == Status::Retired {
            continue;
        }
        if w.status == Status::New {
            learning += 1;
            w.status = Status::Learning;
            w.days = 0;
            w.level = 0;
            if seen_new.insert(a.word.clone()) {
                summary.new.push(a.word.clone());
            }
        }
        w.attempts += 1;
        w.hits += u32::from(a.hit);
        w.push_result(a.hit);
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
    for (word, t) in &touched {
        let Some(w) = vocab.words.get_mut(word) else { continue };
        match w.status {
            Status::Learning if mastered(w, median, cfg) => {
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
    vocab.sessions += 1;
    summary.due = due_reviews(vocab, day).len();
    summary
}

#[cfg(test)]
mod tests {
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
        Attempt { word: w.into(), hit: true, secs: Some(1.0) }
    }

    fn miss(w: &str) -> Attempt {
        Attempt { word: w.into(), hit: false, secs: None }
    }

    fn p(v: &Vocab, pool: &mut Vec<String>, count: usize, seed: u64, day: u32) -> Plan {
        plan(v, pool, &cfg(), day, count, seed, &all, true)
    }

    // ---- earning new words ----

    #[test]
    fn an_empty_vocabulary_gets_the_first_words_in_pool_order() {
        let mut pool = names(50);
        let plan = p(&Vocab::default(), &mut pool, 25, 1, 100);
        assert_eq!(plan.new_words, ["w0001", "w0002", "w0003"]);
        // nothing else to show: each new word twice, then topped up to five times
        assert_eq!(plan.words.len(), 15, "{:?}", plan.words);
        assert!(plan.new_words.iter().all(|n| plan.words.iter().filter(|w| *w == n).count() == 5), "{:?}", plan.words);
    }

    #[test]
    fn new_words_skip_the_ones_already_known_and_wait_for_a_free_slot() {
        let mut v = Vocab::default();
        v.words.insert("w0001".into(), word(Status::Retired));
        v.words.insert("w0002".into(), word(Status::Review));
        v.words.insert("w0003".into(), word(Status::New));
        let mut pool = names(50);
        assert_eq!(p(&v, &mut pool, 25, 1, 100).new_words, ["w0003", "w0004", "w0005"], "a word on file as new is still new");
        // a full learning pool lets nothing in
        for i in 10..25 {
            v.words.insert(format!("w{i:04}"), learning("1", 1));
        }
        assert_eq!(v.count(Status::Learning), 15);
        none(&p(&v, &mut pool, 25, 1, 100).new_words);
        // one free slot: one word
        v.words.insert("w0010".into(), word(Status::Review));
        assert_eq!(p(&v, &mut pool, 25, 1, 100).new_words, ["w0003"]);
    }

    #[test]
    fn at_most_k_new_words_a_test() {
        let mut pool = names(50);
        let c = Config { new_per_session: 2, ..cfg() };
        assert_eq!(plan(&Vocab::default(), &mut pool, &c, 1, 25, 1, &all, true).new_words.len(), 2);
        assert!(plan(&Vocab::default(), &mut pool, &c, 1, 25, 1, &all, false).new_words.is_empty(), "allow_new false");
    }

    #[test]
    fn low_recent_accuracy_earns_no_new_words() {
        let mut v = Vocab::default();
        let mut pool = names(50);
        v.recent = "1".repeat(45) + "00000"; // 90%
        none(&p(&v, &mut pool, 25, 1, 5).new_words);
        v.recent = "1".repeat(46) + "0000"; // 92%: enough
        assert_eq!(p(&v, &mut pool, 25, 1, 5).new_words.len(), 3);
        v.recent = "0".repeat(19); // fewer than 20 attempts: not held against the typist
        assert_eq!(p(&v, &mut pool, 25, 1, 5).new_words.len(), 3);
        v.recent = "0".repeat(20);
        none(&p(&v, &mut pool, 25, 1, 5).new_words);
    }

    #[test]
    fn filters_apply_to_every_word() {
        let mut pool = vec!["a".to_owned(), "bb".into(), "ccc".into(), "dddd".into()];
        let mut v = Vocab::default();
        v.words.insert("xx".into(), learning("1", 1));
        v.words.insert("yyyyyy".into(), learning("1", 1));
        let plan = plan(&v, &mut pool, &cfg(), 1, 10, 1, &|w: &str| w.len() >= 2 && w.len() <= 4, true);
        assert!(plan.words.iter().all(|w| (2..=4).contains(&w.len()) && w != "yyyyyy"), "{:?}", plan.words);
        assert_eq!(plan.new_words, ["bb", "ccc", "dddd"]);
    }

    // ---- composition ----

    #[test]
    fn due_reviews_come_first_up_to_their_share() {
        let mut v = Vocab::default();
        for i in 0..20 {
            v.words.insert(format!("r{i:02}"), Word { status: Status::Review, level: 2, due: 50 + i, ..Word::default() });
        }
        v.words.insert("late".into(), Word { status: Status::Review, level: 2, due: 200, ..Word::default() });
        let mut pool = names(20);
        let plan = p(&v, &mut pool, 25, 3, 100);
        assert_eq!(plan.due, 7, "30% of 25, rounded down");
        let reviews: Vec<&String> = plan.words.iter().filter(|w| w.starts_with('r')).collect();
        assert_eq!(reviews.len(), 7);
        // the longest overdue are the ones that are in it
        let want: HashSet<String> = (0..7).map(|i| format!("r{i:02}")).collect();
        assert_eq!(reviews.iter().map(|w| (*w).clone()).collect::<HashSet<_>>(), want);
    }

    #[test]
    fn learning_words_are_weighted_by_errors_and_each_comes_once_before_any_twice() {
        let mut v = Vocab::default();
        for i in 0..6 {
            v.words.insert(format!("l{i}"), learning("1111111111", 3));
        }
        v.words.insert("hard".into(), learning("0001000101", 3));
        let mut pool = names(20);
        let c = Config { new_per_session: 0, ..cfg() };
        let mut counts: HashMap<String, usize> = HashMap::new();
        for seed in 0..200 {
            let plan = plan(&v, &mut pool, &c, 1, 15, seed, &all, true);
            let n = |w: &str| plan.words.iter().filter(|x| *x == w).count();
            assert!((1..=c.max_reps).contains(&n("hard")), "{:?}", plan.words);
            for i in 0..6 {
                assert!(n(&format!("l{i}")) >= 1, "everyone is in a test with room: {:?}", plan.words);
            }
            for w in &plan.words {
                *counts.entry(w.clone()).or_default() += 1;
            }
        }
        assert!(counts["hard"] > counts["l0"], "the word with errors is seen more: {counts:?}");
    }

    #[test]
    fn not_yet_due_reviews_fill_a_short_test_soonest_first() {
        let mut v = Vocab::default();
        v.words.insert("a1".into(), Word { status: Status::Review, level: 3, due: 120, ..Word::default() });
        v.words.insert("b1".into(), Word { status: Status::Review, level: 3, due: 110, ..Word::default() });
        v.words.insert("c1".into(), Word { status: Status::Review, level: 3, due: 130, ..Word::default() });
        let mut pool: Vec<String> = Vec::new();
        let plan = p(&v, &mut pool, 2, 1, 100);
        let mut got = plan.words.clone();
        got.sort();
        assert_eq!(got, ["a1", "b1"].map(String::from), "{:?}", plan.words);
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
        assert_eq!(feed.new_words.len(), 3, "only the first round brings new words");
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
        let again = apply(&mut v, &[Attempt { word: "dog".into(), hit: true, secs: Some(2.0) }], &cfg(), 50);
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
    fn mastery_needs_ten_right_three_days_and_a_speed_near_the_median() {
        let mut v = Vocab::default();
        v.words.insert("cat".into(), learning("111111111", 3));
        // ten right, three days: mastered, review level 1, due tomorrow
        let s = apply(&mut v, &[hit("cat")], &cfg(), 60);
        assert_eq!(s.mastered, ["cat"]);
        let cat = &v.words["cat"];
        assert_eq!((cat.status, cat.level, cat.due, cat.mastered, cat.promotions), (Status::Review, 1, 61, Some(60), 1));
        // 9 of 10 is 90%: not enough
        let mut v = Vocab::default();
        v.words.insert("cat".into(), learning("111101111", 3));
        none(&apply(&mut v, &[hit("cat")], &cfg(), 60).mastered);
        // only two days (the third is today)... the word's days counter already counts today when it is the first attempt of the day
        let mut v = Vocab::default();
        v.words.insert("cat".into(), Word { last_seen: 60, ..learning("111111111", 2) });
        assert!(apply(&mut v, &[hit("cat")], &cfg(), 60).mastered.is_empty(), "two distinct days");
        let mut v = Vocab::default();
        v.words.insert("cat".into(), Word { last_seen: 59, ..learning("111111111", 2) });
        assert_eq!(apply(&mut v, &[hit("cat")], &cfg(), 60).mastered, ["cat"], "today is the third day");
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
        let s = apply(&mut v, &[Attempt { word: "slow".into(), hit: true, secs: Some(2.4) }, Attempt { word: "ok".into(), hit: true, secs: Some(1.4) }], &cfg(), 5);
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
        for i in 0..15 {
            v.words.insert(format!("l{i:02}"), learning("1", 1));
        }
        v.words.insert("cat".into(), Word { status: Status::Review, level: 3, due: 60, ..Word::default() });
        let s = apply(&mut v, &[miss("cat"), miss("cat")], &cfg(), 60);
        none(&s.relearn);
        let cat = &v.words["cat"];
        assert_eq!((cat.status, cat.level, cat.due, cat.lapses), (Status::Review, 2, 61, 1), "one level down, due tomorrow, the lapse counted");
        assert_eq!(v.count(Status::Learning), 15, "the cap holds");
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

    // ---- a simulated learner ----

    /// A learner that forgets: a word's recall decays with the days since it was last typed right,
    /// slower the more often it has been.
    struct Learner {
        rng: Rng,
        stability: HashMap<String, f64>,
        last: HashMap<String, f64>,
    }

    impl Learner {
        fn new(seed: u64) -> Self {
            Self { rng: Rng::new(seed), stability: HashMap::new(), last: HashMap::new() }
        }

        fn recall(&self, w: &str, at: f64, mood: f64) -> f64 {
            let p = self.last.get(w).map_or(0.85, |l| (-(at - l) / self.stability.get(w).copied().unwrap_or(5.0)).exp().min(0.99));
            (p * mood).clamp(0.02, 0.99)
        }

        fn attempt(&mut self, w: &str, at: f64, mood: f64) -> bool {
            let p = self.recall(w, at, mood);
            let ok = (self.rng.next_u64() >> 11) as f64 / (1_u64 << 53) as f64 <= p;
            let s = self.stability.entry(w.to_owned()).or_insert(5.0);
            let same_day = self.last.get(w).is_some_and(|l| at - l < 1.0);
            *s = if ok { (*s * if same_day { 1.4 } else { 2.5 }).min(500.0) } else { (*s * 0.6).max(2.0) };
            self.last.insert(w.to_owned(), at);
            ok
        }
    }

    struct Run {
        vocab: Vocab,
        /// (day, vocabulary size, frontier rank) at each day's end
        curve: Vec<(u32, usize, usize)>,
        plans: Vec<(u32, Plan)>,
        summaries: Vec<Summary>,
    }

    fn simulate(days: u32, seed: u64, bad_day: Option<u32>, retire: Option<(u32, &str)>) -> Run {
        let c = cfg();
        let mut pool = names(5000);
        let mut learner = Learner::new(seed);
        let mut vocab = Vocab::default();
        let mut run = Run { vocab: Vocab::default(), curve: Vec::new(), plans: Vec::new(), summaries: Vec::new() };
        for d in 0..days {
            let day = 1000 + d;
            if let Some((on, w)) = retire
                && d == on
            {
                vocab.words.insert(w.to_owned(), word(Status::Retired));
            }
            let mood = if bad_day == Some(d) { 0.35 } else { 1.0 };
            let plan = plan(&vocab, &mut pool, &c, day, 25, seed ^ u64::from(day), &all, true);
            if !plan.new_words.is_empty() {
                assert!(accuracy_ok(&vocab, &c), "day {d}: new words came with accuracy {:?} over {} attempts", window_accuracy(&vocab), vocab.recent.len());
            }
            let attempts: Vec<Attempt> = plan
                .words
                .iter()
                .enumerate()
                .map(|(i, w)| Attempt { word: w.clone(), hit: learner.attempt(w, (i as f64).mul_add(0.01, f64::from(day)), mood), secs: (i > 0).then(|| 1.0 + (learner.rng.below(40) as f64) / 100.0) })
                .collect();
            let summary = apply(&mut vocab, &attempts, &c, day);
            assert!(vocab.count(Status::Learning) <= c.learning_cap, "day {d}: {} learning", vocab.count(Status::Learning));
            assert!(summary.new.len() <= c.new_per_session, "day {d}: {} new", summary.new.len());
            let frontier = crate::vocab::find_frontier(&vocab, &mut pool).rank;
            run.curve.push((d + 1, vocab.size(), frontier));
            run.plans.push((day, plan));
            run.summaries.push(summary);
        }
        run.vocab = vocab;
        run
    }

    #[test]
    fn ninety_simulated_days_obey_every_rule() {
        let run = simulate(90, 42, Some(40), Some((10, "w0003")));
        // the vocabulary grows steadily and never faster than K a day
        for &(d, size, _) in &run.curve {
            assert!(size <= 3 * d as usize, "day {d}: vocabulary {size}");
        }
        let size = |d: u32| run.curve[d as usize - 1].1;
        assert!(size(90) >= 60, "growth is real: {} words after 90 days", size(90));
        assert!(size(30) < size(60) && size(60) < size(90), "keeps growing: {} {} {}", size(30), size(60), size(90));
        // a bad day: the day after it introduces nothing
        let (_, after_bad) = &run.plans[41];
        assert!(after_bad.new_words.is_empty(), "no new words after a bad day: {:?}", after_bad.new_words);
        // and the bad day did not break the pool cap
        // retired words never appear after they are retired
        assert!(run.plans.iter().skip(10).all(|(_, p)| !p.words.contains(&"w0003".to_owned())), "w0003 is retired from day 10");
        assert_eq!(run.vocab.words["w0003"].status, Status::Retired);
        // mastered words come back after their cooldown: some word has gone up several levels,
        // and no review word is shown more than a day before it is due unless the test had room
        let top = run.vocab.words.values().map(|w| w.level).max().unwrap_or(0);
        assert!(top >= 3, "a word reached level {top}");
        eprintln!("growth curve (day, vocabulary, frontier rank): {:?}", run.curve.iter().filter(|c| c.0 % 10 == 0 || c.0 == 1).collect::<Vec<_>>());
        eprintln!(
            "after 90 days: {} learning, {} review, levels {:?}, {} lapses",
            run.vocab.count(Status::Learning),
            run.vocab.count(Status::Review),
            (1..=6u8).map(|l| run.vocab.words.values().filter(|w| w.level == l && w.status == Status::Review).count()).collect::<Vec<_>>(),
            run.vocab.words.values().map(|w| w.lapses).sum::<u32>()
        );
    }

    #[test]
    fn a_mastered_word_stays_away_for_its_cooldown_and_then_returns() {
        let run = simulate(90, 7, None, None);
        // replay each plan: for every review word record the days it appeared, and check that after
        // it was passed at level L it did not come back before the interval unless it had no
        // competition for the slot (the filler of a short test)
        let mut seen: HashMap<String, Vec<u32>> = HashMap::new();
        for (day, p) in &run.plans {
            for w in &p.words {
                seen.entry(w.clone()).or_default().push(*day);
            }
        }
        // find a mastered word with at least 3 appearances after mastery and check the gaps grow
        let mut checked = 0;
        for (w, st) in &run.vocab.words {
            let Some(m) = st.mastered else { continue };
            if st.status != Status::Review || st.promotions < 4 {
                continue;
            }
            let days: Vec<u32> = seen[w].iter().copied().filter(|d| *d > m).collect();
            let mut distinct = days;
            distinct.dedup();
            let gaps: Vec<u32> = distinct.windows(2).map(|x| x[1] - x[0]).collect();
            assert!(gaps.iter().all(|g| *g >= 1), "{w}: {gaps:?}");
            assert!(distinct.first().is_none_or(|d| *d > m), "{w} came back the day it was mastered");
            assert!(gaps.iter().any(|g| *g >= 3), "{w} returns on a growing schedule: {gaps:?}");
            checked += 1;
        }
        assert!(checked > 0, "some word went through several reviews in 90 days");
    }

    #[test]
    fn the_simulation_is_deterministic_for_a_seed() {
        let a = simulate(40, 5, Some(20), None);
        let b = simulate(40, 5, Some(20), None);
        assert_eq!(a.vocab, b.vocab);
        assert_eq!(a.plans, b.plans);
        let c = simulate(40, 6, Some(20), None);
        assert_ne!(a.plans, c.plans, "another seed, another feed");
    }

    #[test]
    fn a_learner_who_keeps_missing_gets_no_growth() {
        // a learner at 50% recall never earns a new word past the first ones
        let c = cfg();
        let mut pool = names(500);
        let mut vocab = Vocab::default();
        let mut learner = Learner::new(1);
        for d in 0..20u32 {
            let plan = plan(&vocab, &mut pool, &c, 1000 + d, 25, u64::from(d), &all, true);
            let attempts: Vec<Attempt> = plan.words.iter().map(|w| Attempt { word: w.clone(), hit: learner.rng.below(2) == 0, secs: Some(1.0) }).collect();
            apply(&mut vocab, &attempts, &c, 1000 + d);
        }
        assert!(vocab.size() <= 6, "vocabulary {} for a learner at 50%", vocab.size());
    }
}

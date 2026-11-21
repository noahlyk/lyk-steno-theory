//! The typing engine: pure state, no terminal and no clock. Time is passed in as seconds.
//!
//! A word is committed by space or Enter. A wrong committed word is an error and the feed moves
//! on (monkeytype's "normal" behaviour); with `stop_on_error` the word is refused until it is
//! right. Backspace on an empty word reopens the previous committed word, so a steno `*` (which
//! arrives as backspaces over the word and its space) undoes whole words.

/// One thing the typist did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Input {
    Char(char),
    Commit,
    Backspace,
    DeleteWord,
}

/// A committed word.
#[derive(Clone, Debug, PartialEq)]
pub struct Done {
    pub target: String,
    pub typed: String,
    /// Seconds since the previous commit (since the first key for the first word)
    pub secs: f64,
}

impl Done {
    #[must_use]
    pub fn correct(&self) -> bool {
        self.target == self.typed
    }
}

/// A word that was missed and what was typed instead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Miss {
    pub word: String,
    pub typed: String,
}

/// Extra characters allowed past the end of a word.
const OVERFLOW: usize = 8;

pub struct Session {
    pub words: Vec<String>,
    pub done: Vec<Done>,
    pub typed: String,
    pub stop_on_error: bool,
    /// When the first key came
    pub started: Option<f64>,
    pub finished_at: Option<f64>,
    last_commit: f64,
    key_ok: usize,
    key_bad: usize,
    refused: Vec<Miss>,
    /// A missed word comes back this many words later, at most this many times: `(gap, times)`
    respace: Option<(usize, u32)>,
    /// How many times each missed word has been put back
    put_back: Vec<(String, u32)>,
}

impl Session {
    #[must_use]
    pub const fn new(words: Vec<String>, stop_on_error: bool) -> Self {
        Self {
            words,
            done: Vec::new(),
            typed: String::new(),
            stop_on_error,
            started: None,
            finished_at: None,
            last_commit: 0.0,
            key_ok: 0,
            key_bad: 0,
            refused: Vec::new(),
            respace: None,
            put_back: Vec::new(),
        }
    }

    /// The commits `stop_on_error` refused (wrong words that were not accepted).
    #[must_use]
    pub fn refusals(&self) -> &[Miss] {
        &self.refused
    }

    /// A missed word re-enters the feed `gap` words later, at most `times` times in all.
    #[must_use]
    pub const fn with_respacing(mut self, gap: usize, times: u32) -> Self {
        self.respace = Some((gap, times));
        self
    }

    /// Puts a missed word back `gap` words ahead (not next to itself), if it has been put back
    /// fewer than `times` times.
    fn put_back_missed(&mut self, target: &str) {
        let Some((gap, times)) = self.respace else { return };
        let used = self.put_back.iter().position(|(w, _)| w == target);
        if used.is_some_and(|i| self.put_back[i].1 >= times) {
            return;
        }
        match used {
            Some(i) => self.put_back[i].1 += 1,
            None => self.put_back.push((target.to_owned(), 1)),
        }
        let mut at = (self.done.len() + gap.max(1) - 1).min(self.words.len());
        while at < self.words.len() && (self.words[at] == target || at > 0 && self.words[at - 1] == target) {
            at += 1;
        }
        self.words.insert(at, target.to_owned());
    }

    /// The index of the word being typed.
    #[must_use]
    pub const fn current(&self) -> usize {
        self.done.len()
    }

    #[must_use]
    pub fn target(&self) -> Option<&str> {
        self.words.get(self.done.len()).map(String::as_str)
    }

    #[must_use]
    pub const fn finished(&self) -> bool {
        self.finished_at.is_some()
    }

    /// Stop the run at `now` (the time limit, or the typist quitting).
    pub const fn finish(&mut self, now: f64) {
        if self.finished_at.is_none() {
            self.finished_at = Some(now);
        }
    }

    pub fn apply(&mut self, input: Input, now: f64) {
        if self.finished() {
            return;
        }
        match input {
            Input::Char(' ') | Input::Commit => self.commit(now),
            Input::Char(c) => self.push(c, now),
            Input::Backspace => {
                if self.typed.pop().is_none() {
                    self.reopen();
                }
            }
            Input::DeleteWord => {
                if self.typed.is_empty() {
                    self.reopen();
                }
                self.typed.clear();
            }
        }
    }

    const fn start(&mut self, now: f64) {
        if self.started.is_none() {
            self.started = Some(now);
            self.last_commit = now;
        }
    }

    fn push(&mut self, c: char, now: f64) {
        let Some(target) = self.target() else { return };
        let at = self.typed.chars().count();
        if at >= target.chars().count() + OVERFLOW {
            return;
        }
        let hit = target.chars().nth(at) == Some(c);
        self.start(now);
        if hit {
            self.key_ok += 1;
        } else {
            self.key_bad += 1;
        }
        self.typed.push(c);
        // The last word is finished the moment it is typed right, with no space needed.
        if self.current() + 1 == self.words.len() && self.target() == Some(self.typed.as_str()) {
            self.commit(now);
        }
    }

    fn commit(&mut self, now: f64) {
        if self.typed.is_empty() {
            return;
        }
        let Some(target) = self.target().map(str::to_owned) else { return };
        let right = target == self.typed;
        if !right && self.stop_on_error {
            self.refused.push(Miss { word: target, typed: self.typed.clone() });
            return;
        }
        let secs = (now - self.last_commit).max(0.0);
        self.last_commit = now;
        self.done.push(Done { target: target.clone(), typed: std::mem::take(&mut self.typed), secs });
        if !right {
            self.put_back_missed(&target);
        }
        if self.done.len() >= self.words.len() {
            self.finish(now);
        }
    }

    /// Backspace over the space after the previous word: it is open again, as typed.
    fn reopen(&mut self) {
        if let Some(prev) = self.done.pop() {
            self.typed = prev.typed;
        }
    }

    /// The numbers for a finished (or abandoned) run, as of `now`.
    #[must_use]
    pub fn results(&self, now: f64) -> Results {
        let end = self.finished_at.unwrap_or(now);
        let secs = self.started.map_or(0.0, |s| (end - s).max(0.0));
        let correct_words = self.done.iter().filter(|d| d.correct()).count();
        let spaces = |n: usize| n.saturating_sub(1);
        let correct_chars: usize =
            self.done.iter().filter(|d| d.correct()).map(|d| d.typed.chars().count()).sum::<usize>() + spaces(correct_words);
        let raw_chars: usize = self.done.iter().map(|d| d.typed.chars().count()).sum::<usize>() + spaces(self.done.len());
        let per_minute = |chars: usize| if secs > 0.0 { chars as f64 / 5.0 / (secs / 60.0) } else { 0.0 };
        let keys = self.key_ok + self.key_bad;
        let mut misses: Vec<Miss> = self
            .done
            .iter()
            .filter(|d| !d.correct())
            .map(|d| Miss { word: d.target.clone(), typed: d.typed.clone() })
            .collect();
        misses.extend(self.refused.iter().cloned());
        let mut slow: Vec<(String, f64)> = self.done.iter().map(|d| (d.target.clone(), d.secs)).collect();
        slow.sort_by(|a, b| b.1.total_cmp(&a.1));
        slow.truncate(10);
        Results {
            secs,
            wpm: per_minute(correct_chars),
            raw: per_minute(raw_chars),
            accuracy: if keys == 0 { 100.0 } else { 100.0 * self.key_ok as f64 / keys as f64 },
            word_accuracy: if self.done.is_empty() { 100.0 } else { 100.0 * correct_words as f64 / self.done.len() as f64 },
            errors: misses.len(),
            words: self.done.len(),
            consistency: consistency(&self.done),
            slow,
            misses,
        }
    }
}

/// 100 minus the spread of the time per word as a share of its mean (0 to 100); none under
/// three words.
fn consistency(done: &[Done]) -> Option<f64> {
    if done.len() < 3 {
        return None;
    }
    // The first word's time starts at the first key, so it carries no recall time.
    let times: Vec<f64> = done.iter().skip(1).map(|d| d.secs).collect();
    let n = times.len() as f64;
    let mean = times.iter().sum::<f64>() / n;
    if mean <= 0.0 {
        return None;
    }
    let var = times.iter().map(|t| (t - mean).powi(2)).sum::<f64>() / n;
    Some((100.0 * (1.0 - var.sqrt() / mean)).clamp(0.0, 100.0))
}

#[derive(Clone, Debug)]
pub struct Results {
    pub secs: f64,
    pub wpm: f64,
    pub raw: f64,
    /// Share of key presses that matched the target at their position
    pub accuracy: f64,
    pub word_accuracy: f64,
    /// Wrong committed words (and, with `stop_on_error`, refused commits)
    pub errors: usize,
    pub words: usize,
    pub consistency: Option<f64>,
    /// The longest time per word, slowest first, at most ten
    pub slow: Vec<(String, f64)>,
    pub misses: Vec<Miss>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(words: &[&str], stop: bool) -> Session {
        Session::new(words.iter().map(|w| (*w).to_owned()).collect(), stop)
    }

    fn type_str(s: &mut Session, text: &str, t: f64) {
        for c in text.chars() {
            s.apply(Input::Char(c), t);
        }
    }

    #[test]
    fn a_missed_word_comes_back_a_few_words_later_at_most_twice() {
        let mut s = session(&["aa", "bb", "cc", "dd", "ee", "ff", "gg"], false).with_respacing(4, 2);
        type_str(&mut s, "xx ", 1.0); // aa missed: back three words after the next one
        assert_eq!(s.words, ["aa", "bb", "cc", "dd", "aa", "ee", "ff", "gg"].map(String::from), "{:?}", s.words);
        type_str(&mut s, "bb cc dd ", 2.0);
        type_str(&mut s, "xx ", 3.0); // aa missed again
        assert_eq!(s.words.iter().filter(|w| *w == "aa").count(), 3, "{:?}", s.words);
        let at = s.words.iter().rposition(|w| w == "aa").unwrap();
        assert!(at > s.current(), "ahead of the typist");
        // the third miss of aa is not put back
        let before = s.words.len();
        while s.target() != Some("aa") {
            let t = s.target().unwrap().to_owned();
            type_str(&mut s, &format!("{t} "), 4.0);
        }
        type_str(&mut s, "xx ", 5.0);
        assert_eq!(s.words.len(), before, "put back twice already");
        // the feed runs on to the end and the session finishes only then
        while !s.finished() {
            let t = s.target().unwrap().to_owned();
            type_str(&mut s, &format!("{t} "), 6.0);
        }
        assert_eq!(s.done.len(), s.words.len());
        assert_eq!(s.done.iter().filter(|d| !d.correct()).count(), 3);
    }

    #[test]
    fn a_missed_last_word_is_put_back_at_the_end_and_without_respacing_nothing_moves() {
        let mut s = session(&["aa", "bb"], false).with_respacing(4, 2);
        type_str(&mut s, "aa xx ", 1.0);
        assert!(!s.finished(), "the missed last word is not done yet");
        assert_eq!(s.words, ["aa", "bb", "bb"].map(String::from));
        type_str(&mut s, "bb", 2.0);
        assert!(s.finished());
        let mut plain = session(&["aa", "bb"], false);
        type_str(&mut plain, "xx ", 1.0);
        assert_eq!(plain.words, ["aa", "bb"].map(String::from));
    }

    #[test]
    fn a_correct_word_advances_on_space() {
        let mut s = session(&["the", "cat", "sat"], false);
        type_str(&mut s, "the ", 1.0);
        assert_eq!(s.current(), 1);
        assert_eq!(s.target(), Some("cat"));
        assert!(s.typed.is_empty(), "typed should be cleared, got {:?}", s.typed);
    }

    #[test]
    fn a_wrong_word_counts_and_the_feed_moves_on() {
        let mut s = session(&["the", "cat"], false);
        type_str(&mut s, "teh ", 1.0);
        assert_eq!(s.current(), 1);
        let r = s.results(2.0);
        assert_eq!(r.errors, 1);
        assert_eq!(r.misses, vec![Miss { word: "the".into(), typed: "teh".into() }]);
        assert!(r.word_accuracy.abs() < 1e-9, "word accuracy {}", r.word_accuracy);
    }

    #[test]
    fn stop_on_error_refuses_a_wrong_word() {
        let mut s = session(&["the", "cat"], true);
        type_str(&mut s, "teh ", 1.0);
        assert_eq!(s.current(), 0, "the wrong word must not advance");
        assert_eq!(s.typed, "teh");
        s.apply(Input::DeleteWord, 1.5);
        type_str(&mut s, "the ", 2.0);
        assert_eq!(s.current(), 1);
        assert_eq!(s.results(3.0).errors, 1);
    }

    #[test]
    fn backspace_and_word_delete() {
        let mut s = session(&["the", "cat"], false);
        type_str(&mut s, "thx", 1.0);
        s.apply(Input::Backspace, 1.1);
        assert_eq!(s.typed, "th");
        s.apply(Input::DeleteWord, 1.2);
        assert!(s.typed.is_empty(), "typed {:?}", s.typed);
    }

    #[test]
    fn backspace_over_the_space_reopens_the_word() {
        let mut s = session(&["the", "cat"], false);
        type_str(&mut s, "teh ", 1.0);
        assert_eq!(s.current(), 1);
        // a steno `*` undoing "teh ": the space, then h, e, t
        s.apply(Input::Backspace, 1.1);
        assert_eq!(s.current(), 0);
        assert_eq!(s.typed, "teh");
        for _ in 0..3 {
            s.apply(Input::Backspace, 1.2);
        }
        assert!(s.typed.is_empty(), "typed {:?}", s.typed);
        type_str(&mut s, "the ", 1.3);
        assert_eq!(s.results(2.0).errors, 0);
    }

    #[test]
    fn the_last_word_finishes_when_typed_right() {
        let mut s = session(&["a", "dog"], false);
        type_str(&mut s, "a dog", 1.0);
        assert!(s.finished());
        assert_eq!(s.results(9.0).words, 2);
    }

    #[test]
    fn the_last_word_wrong_finishes_on_commit() {
        let mut s = session(&["a", "dog"], false);
        type_str(&mut s, "a dig", 1.0);
        assert!(!s.finished());
        s.apply(Input::Commit, 1.5);
        assert!(s.finished());
        assert_eq!(s.results(9.0).errors, 1);
    }

    #[test]
    fn wpm_is_correct_characters_over_five_per_minute() {
        // ten words of five letters typed right: 50 letters and 9 spaces, in 54 s
        let words: Vec<String> = (0..10).map(|_| "hello".to_owned()).collect();
        let mut s = Session::new(words, false);
        for i in 0..10 {
            type_str(&mut s, "hello", f64::from(i) * 6.0);
            if i < 9 {
                s.apply(Input::Commit, f64::from(i) * 6.0);
            }
        }
        let r = s.results(60.0);
        assert!((r.secs - 54.0).abs() < 1e-9, "secs {}", r.secs);
        let want = 59.0 / 5.0 / (54.0 / 60.0);
        assert!((r.wpm - want).abs() < 1e-9, "wpm {} want {want}", r.wpm);
        assert!((r.raw - r.wpm).abs() < 1e-9, "raw {} wpm {}", r.raw, r.wpm);
    }

    #[test]
    fn raw_counts_wrong_words_and_wpm_does_not() {
        let mut s = session(&["cat", "dog", "end"], false);
        type_str(&mut s, "cat ", 0.0);
        type_str(&mut s, "dxg ", 30.0);
        let r = s.results(60.0);
        assert!(r.raw > r.wpm, "raw {} wpm {}", r.raw, r.wpm);
    }

    #[test]
    fn accuracy_counts_key_presses_even_after_a_correction() {
        let mut s = session(&["cat"], false);
        type_str(&mut s, "cx", 0.0);
        s.apply(Input::Backspace, 0.1);
        type_str(&mut s, "at", 0.2);
        let r = s.results(1.0);
        assert!((r.accuracy - 75.0).abs() < 1e-9, "accuracy {}", r.accuracy);
        assert_eq!(r.errors, 0);
    }

    #[test]
    fn extra_characters_are_capped_and_count_wrong() {
        let mut s = session(&["a", "b"], false);
        type_str(&mut s, &"x".repeat(40), 0.0);
        assert_eq!(s.typed.chars().count(), 1 + OVERFLOW);
    }

    #[test]
    fn slowest_words_are_ordered_and_capped() {
        let words: Vec<String> = (0..15).map(|i| format!("w{i}")).collect();
        let mut s = Session::new(words, false);
        let mut t = 0.0;
        for i in 0..14 {
            t += f64::from(i % 5 + 1);
            type_str(&mut s, &format!("w{i} "), t);
        }
        let r = s.results(100.0);
        assert!(r.slow.len() <= 10, "{:?}", r.slow);
        assert!(r.slow.windows(2).all(|w| w[0].1 >= w[1].1), "{:?}", r.slow);
    }

    #[test]
    fn even_timing_is_fully_consistent() {
        let words: Vec<String> = (0..6).map(|_| "x".to_owned()).collect();
        let mut s = Session::new(words, false);
        for i in 0..5 {
            type_str(&mut s, "x ", f64::from(i));
        }
        assert_eq!(s.results(5.0).consistency, Some(100.0));
        assert_eq!(session(&["a"], false).results(0.0).consistency, None);
    }

    #[test]
    fn nothing_typed_is_zero_not_a_division() {
        let r = session(&["a"], false).results(5.0);
        assert!(r.wpm.abs() < 1e-9 && r.secs.abs() < 1e-9, "wpm {} secs {}", r.wpm, r.secs);
    }
}

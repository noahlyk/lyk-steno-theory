//! The typing engine: pure state, no terminal and no clock. Time is passed in as seconds.
//!
//! A word is committed by space or Enter. A wrong committed word is an error and the feed moves
//! on (monkeytype's "normal" behaviour); with `stop_on_error` the word is refused until it is
//! right. Backspace on an empty word reopens the previous committed word, so a steno `*` (which
//! arrives as backspaces over the word and its space) undoes whole words.
//!
//! A **stumble** is a correction while a word was being typed: any run of backspaces or a word
//! delete that removed characters of the word, or reopened it after its space. A run of
//! backspaces (a steno `*` over a whole word) is one stumble. A word that committed right after a
//! stumble is not clean: the typist's corrections come before the commit, so the committed word
//! alone hides them.

use std::collections::HashSet;

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
    /// Corrections made while the word was open (runs of backspaces or word deletes)
    pub stumbles: u32,
    /// Where the copy put back after this word sits in `words`, and the filler word it replaced
    queued: Option<(usize, String)>,
}

impl Done {
    #[must_use]
    pub fn correct(&self) -> bool {
        self.target == self.typed
    }

    /// Committed right with no correction on the way.
    #[must_use]
    pub fn clean(&self) -> bool {
        self.correct() && self.stumbles == 0
    }
}

/// A word that was missed and what was typed instead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Miss {
    pub word: String,
    pub typed: String,
}

/// Most times one word is in a test, a re-queued copy included.
pub const MAX_COPIES: usize = 2;
/// Most times it is in a test once it has been unclean twice.
pub const MAX_COPIES_TWICE_UNCLEAN: usize = 3;

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
    /// Corrections so far in the open word
    stumbles: u32,
    /// The corrections of the words after a reopened one, kept until the typist gets back to them
    ahead: Vec<u32>,
    /// The last thing done was a removal (a run of backspaces counts once)
    removing: bool,
    /// The open word was refused (`stop_on_error`): that is already a miss, its deletes do not count
    refused_open: bool,
    /// A missed word comes back this many words later in place of a filler word: `(gap, fillers)`
    requeue: Option<(usize, HashSet<String>)>,
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
            stumbles: 0,
            ahead: Vec::new(),
            removing: false,
            refused_open: false,
            requeue: None,
        }
    }

    /// The commits `stop_on_error` refused (wrong words that were not accepted).
    #[must_use]
    pub fn refusals(&self) -> &[Miss] {
        &self.refused
    }

    /// A missed or stumbled word comes back `gap` words later *in place of* a later filler word
    /// (one of `fillers`), so the test keeps its length. A word is in the test at most twice
    /// (`MAX_COPIES`), three times once it has been unclean twice; with no filler word ahead
    /// nothing comes back.
    #[must_use]
    pub fn with_requeue(mut self, gap: usize, fillers: HashSet<String>) -> Self {
        self.requeue = Some((gap, fillers));
        self
    }

    /// Puts an unclean word back `gap` words ahead (not next to itself) over a filler word, when it
    /// is under its copy limit and a filler word is ahead. Returns where, and what it replaced.
    fn put_back_missed(&mut self, target: &str) -> Option<(usize, String)> {
        let (gap, fillers) = self.requeue.as_ref()?;
        let copies = self.words.iter().filter(|w| *w == target).count();
        // this attempt is not in `done` yet
        let unclean = self.done.iter().filter(|d| d.target == target && !d.clean()).count() + 1;
        if copies >= if unclean >= 2 { MAX_COPIES_TWICE_UNCLEAN } else { MAX_COPIES } {
            return None;
        }
        let from = self.done.len() + (*gap).max(1);
        let len = self.words.len();
        let at = (from..len).find(|&at| fillers.contains(&self.words[at]) && !(at > 0 && self.words[at - 1] == target) && !(at + 1 < len && self.words[at + 1] == target))?;
        let old = std::mem::replace(&mut self.words[at], target.to_owned());
        Some((at, old))
    }

    /// Takes back the copy put back after a word that is being reopened: the filler word it
    /// replaced returns.
    fn take_back(&mut self, target: &str, at: usize, old: String) {
        if self.words.get(at).is_some_and(|w| w == target) && at > self.done.len() {
            self.words[at] = old;
        }
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
            Input::Char(' ') | Input::Commit => {
                self.removing = false;
                self.commit(now);
            }
            Input::Char(c) => {
                self.removing = false;
                self.push(c, now);
            }
            Input::Backspace => {
                if self.typed.is_empty() {
                    self.reopen();
                } else {
                    self.typed.pop();
                    self.stumble();
                }
            }
            Input::DeleteWord => {
                if self.typed.is_empty() {
                    self.reopen();
                } else {
                    self.stumble();
                }
                self.typed.clear();
            }
        }
    }

    /// A removal from the open word: one stumble per run of removals.
    const fn stumble(&mut self) {
        if !self.removing && !self.refused_open {
            self.stumbles += 1;
        }
        self.removing = true;
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
            self.refused_open = true;
            return;
        }
        let secs = (now - self.last_commit).max(0.0);
        self.last_commit = now;
        let stumbles = std::mem::replace(&mut self.stumbles, self.ahead.pop().unwrap_or(0));
        self.refused_open = false;
        let mut done = Done { target: target.clone(), typed: std::mem::take(&mut self.typed), secs, stumbles, queued: None };
        // a word that was missed or needed a correction comes back
        if !right || stumbles > 0 {
            done.queued = self.put_back_missed(&target);
        }
        self.done.push(done);
        if self.done.len() >= self.words.len() {
            self.finish(now);
        }
    }

    /// Backspace over the space after the previous word: it is open again, as typed, and that
    /// is a correction of it. The copy put back after it is taken back.
    fn reopen(&mut self) {
        if let Some(prev) = self.done.pop() {
            if let Some((at, old)) = prev.queued {
                self.take_back(&prev.target, at, old);
            }
            self.typed = prev.typed;
            self.ahead.push(self.stumbles);
            self.stumbles = prev.stumbles;
            self.refused_open = false;
            self.removing = false;
            self.stumble();
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
        let stumbled: Vec<(String, u32)> = self.done.iter().filter(|d| d.stumbles > 0).map(|d| (d.target.clone(), d.stumbles)).collect();
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
            stumbled,
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
    /// The committed words that needed a correction, with how many, in order (right or wrong)
    pub stumbled: Vec<(String, u32)>,
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

    /// Types every remaining word right until `word` is the target.
    fn run_to(s: &mut Session, word: &str, t: f64) {
        while s.target() != Some(word) {
            let w = s.target().unwrap().to_owned();
            type_str(s, &format!("{w} "), t);
        }
    }

    fn fillers(words: &[&str]) -> HashSet<String> {
        words.iter().map(|w| (*w).to_owned()).collect()
    }

    #[test]
    fn a_missed_word_replaces_a_filler_three_words_later_and_the_test_keeps_its_length() {
        let mut s = session(&["aa", "bb", "cc", "dd", "ee", "ff", "gg"], false).with_requeue(3, fillers(&["dd", "ee", "ff", "gg"]));
        type_str(&mut s, "xx ", 1.0); // aa missed: back at the third word on, over a filler
        assert_eq!(s.words, ["aa", "bb", "cc", "aa", "ee", "ff", "gg"].map(String::from), "dd was the filler replaced: {:?}", s.words);
        type_str(&mut s, "bb cc ", 2.0);
        // the copy is typed wrong again: it was unclean twice, so a third copy may come back
        type_str(&mut s, "xx ", 3.0);
        assert_eq!(s.words.iter().filter(|w| *w == "aa").count(), 3, "{:?}", s.words);
        assert_eq!(s.words.len(), 7, "never longer");
        assert!(s.words.iter().rposition(|w| w == "aa").unwrap() > s.current(), "ahead of the typist");
        // a fourth is not put back
        run_to(&mut s, "aa", 4.0);
        let before = s.words.clone();
        type_str(&mut s, "xx ", 4.0);
        assert_eq!(s.words, before, "three copies is the most");
        while !s.finished() {
            let t = s.target().unwrap().to_owned();
            type_str(&mut s, &format!("{t} "), 6.0);
        }
        assert_eq!(s.done.len(), 7, "exactly the planned count");
    }

    #[test]
    fn a_word_is_at_most_twice_in_a_test_unless_it_was_unclean_twice() {
        // planned twice already: no copy for the first miss
        let mut s = session(&["aa", "bb", "aa", "cc", "dd", "ee"], false).with_requeue(3, fillers(&["cc", "dd", "ee"]));
        type_str(&mut s, "xx ", 1.0);
        assert_eq!(s.words, ["aa", "bb", "aa", "cc", "dd", "ee"].map(String::from), "{:?}", s.words);
        // the second miss makes it three
        type_str(&mut s, "bb xx ", 2.0);
        assert_eq!(s.words.iter().filter(|w| *w == "aa").count(), 3, "{:?}", s.words);
        assert_eq!(s.words.len(), 6);
    }

    #[test]
    fn nothing_comes_back_when_no_filler_is_ahead_or_when_the_word_would_touch_itself() {
        let mut s = session(&["aa", "bb", "cc"], false).with_requeue(3, fillers(&["zz"]));
        type_str(&mut s, "xx ", 1.0);
        assert_eq!(s.words, ["aa", "bb", "cc"].map(String::from), "no filler ahead");
        // the only filler is right after a copy of the word: not next to itself
        let mut s = session(&["aa", "bb", "cc", "dd", "ee"], false).with_requeue(1, fillers(&["bb", "cc"]));
        type_str(&mut s, "xx ", 1.0);
        assert_eq!(s.words, ["aa", "bb", "aa", "dd", "ee"].map(String::from), "bb touches the current aa, so cc is the one replaced: {:?}", s.words);
        // without requeueing nothing moves
        let mut plain = session(&["aa", "bb"], false);
        type_str(&mut plain, "xx ", 1.0);
        assert_eq!(plain.words, ["aa", "bb"].map(String::from));
    }

    // ---- stumbles ----

    fn stumbles(s: &Session) -> Vec<u32> {
        s.done.iter().map(|d| d.stumbles).collect()
    }

    #[test]
    fn a_clean_word_has_no_stumbles() {
        let mut s = session(&["the", "cat"], false);
        type_str(&mut s, "the cat", 1.0);
        assert_eq!(stumbles(&s), [0, 0]);
        assert!(s.done.iter().all(Done::clean));
        assert!(s.results(2.0).stumbled.is_empty(), "{:?}", s.results(2.0).stumbled);
    }

    #[test]
    fn typing_wrong_then_deleting_then_typing_right_is_one_stumble() {
        let mut s = session(&["the", "cat"], false);
        type_str(&mut s, "teh", 1.0);
        for _ in 0..3 {
            s.apply(Input::Backspace, 1.1); // a steno `*` over the word: one run
        }
        type_str(&mut s, "the ", 1.2);
        assert_eq!(stumbles(&s), [1]);
        let d = &s.done[0];
        assert!(d.correct() && !d.clean(), "committed right but not clean");
        assert_eq!(s.results(2.0).errors, 0, "the committed word is right");
        assert_eq!(s.results(2.0).stumbled, [("the".to_owned(), 1)]);
        // two separate corrections count twice, and a lone backspace of a right letter counts too
        type_str(&mut s, "cx", 2.0);
        s.apply(Input::Backspace, 2.1);
        type_str(&mut s, "a", 2.2);
        s.apply(Input::Backspace, 2.3);
        type_str(&mut s, "at", 2.4);
        assert_eq!(stumbles(&s), [1, 2]);
    }

    #[test]
    fn a_word_delete_is_one_stumble() {
        let mut s = session(&["the", "cat"], false);
        type_str(&mut s, "thx", 1.0);
        s.apply(Input::DeleteWord, 1.1);
        s.apply(Input::DeleteWord, 1.2); // nothing to delete and nothing before: not a second one
        type_str(&mut s, "the ", 1.3);
        assert_eq!(stumbles(&s), [1]);
    }

    #[test]
    fn backspacing_over_the_space_reopens_the_word_and_counts_against_it() {
        let mut s = session(&["the", "cat"], false);
        type_str(&mut s, "the ", 1.0);
        assert_eq!(stumbles(&s), [0]);
        s.apply(Input::Backspace, 1.1); // over the space: "the" is open again
        assert_eq!((s.current(), s.typed.as_str()), (0, "the"));
        s.apply(Input::Char(' '), 1.2); // and committed again, as it was
        assert_eq!(stumbles(&s), [1], "reopened once");
        // a steno `*` over "cat " then retyped: the run of backspaces is one stumble of cat
        type_str(&mut s, "cat", 2.0);
        assert!(s.finished());
        let mut s = session(&["the", "cat", "sat"], false);
        type_str(&mut s, "the cax ", 1.0);
        for _ in 0..4 {
            s.apply(Input::Backspace, 1.1); // the space, then x, a, c
        }
        assert_eq!((s.current(), s.typed.as_str()), (1, ""), "back to an empty cat");
        type_str(&mut s, "cat sat", 1.2);
        assert_eq!(stumbles(&s), [0, 1, 0], "{:?}", s.done);
        // a delete that goes back over two words counts against both
        let mut s = session(&["the", "cat", "sat"], false);
        type_str(&mut s, "the cat ", 1.0);
        for _ in 0..5 {
            s.apply(Input::Backspace, 1.1); // the space, cat, the space
        }
        assert_eq!((s.current(), s.typed.as_str()), (0, "the"), "{:?}", s.done);
        type_str(&mut s, " cat sat", 1.2);
        assert_eq!(stumbles(&s), [1, 1, 0]);
    }

    #[test]
    fn a_stumbled_or_wrong_word_is_put_back_and_taking_it_back_restores_the_filler() {
        let mut s = session(&["aa", "bb", "cc", "dd", "ee"], false).with_requeue(3, fillers(&["dd", "ee"]));
        type_str(&mut s, "ax", 1.0);
        s.apply(Input::Backspace, 1.1);
        type_str(&mut s, "a ", 1.2); // right in the end, after a stumble
        assert_eq!(s.words, ["aa", "bb", "cc", "aa", "ee"].map(String::from), "{:?}", s.words);
        assert!(!s.done[0].clean());
        // reopening it gives the filler back: it is typed again, but the test is as long as before
        s.apply(Input::Backspace, 1.3);
        assert_eq!(s.words, ["aa", "bb", "cc", "dd", "ee"].map(String::from), "{:?}", s.words);
        s.apply(Input::Char(' '), 1.4);
        assert_eq!(s.words.iter().filter(|w| *w == "aa").count(), 2, "put back again once it is committed unclean: {:?}", s.words);
        assert_eq!(s.words.len(), 5);
        // a clean word is not put back
        type_str(&mut s, "bb ", 2.0);
        assert_eq!(s.words.iter().filter(|w| *w == "bb").count(), 1);
    }

    #[test]
    fn a_refused_word_does_not_count_its_deletes_as_stumbles() {
        let mut s = session(&["cat"], true);
        type_str(&mut s, "cax ", 1.0);
        for _ in 0..3 {
            s.apply(Input::Backspace, 1.1);
        }
        type_str(&mut s, "cat", 1.2);
        assert_eq!(stumbles(&s), [0], "the refusal is the miss already");
        assert_eq!(s.refusals().len(), 1);
    }

    #[test]
    fn a_word_typed_wrong_and_committed_wrong_has_stumbles_only_if_it_was_corrected() {
        let mut s = session(&["aa", "bb"], false);
        type_str(&mut s, "xx ", 1.0);
        assert_eq!(stumbles(&s), [0]);
        assert!(!s.done[0].clean() && !s.done[0].correct());
        assert!(s.results(2.0).stumbled.is_empty(), "missed words are in `misses`, not `stumbled`");
    }

    #[test]
    fn a_missed_last_word_has_no_filler_after_it_and_so_stays_missed() {
        let mut s = session(&["aa", "bb"], false).with_requeue(4, fillers(&["bb"]));
        type_str(&mut s, "aa xx ", 1.0);
        assert!(s.finished(), "the test ends at its planned length");
        assert_eq!(s.words, ["aa", "bb"].map(String::from));
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

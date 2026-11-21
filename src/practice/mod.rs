//! `lyk practice`: a monkeytype-style word feed in the terminal, for a steno typist.
//!
//! The steno engine types finished words into the terminal as ordinary characters, so this reads
//! plain key presses; it never sees strokes. The screen shows the words and how the typing is
//! going, nothing else: no outlines, no keyboard, no hints. `--outlines` adds the lyk outline of
//! each missed or slow word to the results only.

// Counts and seconds become percentages and rates: the casts are exact at these sizes.
#![allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

mod engine;
mod feed;
mod history;
mod sources;
mod ui;

use crate::paths;
use anyhow::{Context, Result, bail};
use clap::{Args as ClapArgs, ValueEnum};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use engine::{Input, Session};
use ratatui::DefaultTerminal;
use std::collections::{HashMap, HashSet};
use std::io::IsTerminal;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, ValueEnum, PartialEq, Eq, Debug)]
pub enum Source {
    /// The commonest words (see --rank, --bands)
    Common,
    /// The weighted drill words of `lyk mistakes --drill`
    Mistakes,
    /// The words of a text file (--file)
    File,
    /// The phrasing grammar's phrases, typed as their text
    Phrases,
    /// Sentences from the books `lyk fetch` downloads
    Quotes,
}

impl Source {
    const fn name(self) -> &'static str {
        match self {
            Self::Common => "common",
            Self::Mistakes => "mistakes",
            Self::File => "file",
            Self::Phrases => "phrases",
            Self::Quotes => "quotes",
        }
    }
}

#[derive(ClapArgs, Debug)]
#[allow(clippy::struct_excessive_bools)]
pub struct Args {
    /// Where the words come from [default: common; file when --file is given]
    #[arg(long, value_enum)]
    source: Option<Source>,
    /// The text file for `--source file`
    #[arg(long, value_name = "PATH")]
    file: Option<PathBuf>,
    /// Rank bands of `common`, such as 1-200,201-1000 (1 is the commonest word)
    #[arg(long, value_name = "BANDS", conflicts_with = "rank")]
    bands: Option<String>,
    /// One rank band of `common`, such as 1-1000 (the default)
    #[arg(long, value_name = "MIN-MAX")]
    rank: Option<String>,
    /// Words in the run
    #[arg(long, default_value_t = 25, conflicts_with = "time")]
    count: usize,
    /// Run for this many seconds instead of a word count
    #[arg(long, value_name = "SECS")]
    time: Option<u64>,
    /// Seed for a repeatable feed
    #[arg(long)]
    seed: Option<u64>,
    /// Draw every band equally often, not every word equally
    #[arg(long)]
    weighted: bool,
    /// Use each word or phrase once
    #[arg(long)]
    no_repeat: bool,
    /// Drop any word with a capital
    #[arg(long)]
    lowercase_only: bool,
    /// Shortest word, in letters
    #[arg(long, default_value_t = 1)]
    min_len: usize,
    /// Longest word, in letters
    #[arg(long, default_value_t = 30)]
    max_len: usize,
    /// Allow capitalised words (names) where they are normally dropped
    #[arg(long)]
    names: bool,
    /// A wrong word is not accepted until it is right
    #[arg(long)]
    stop_on_error: bool,
    /// No live speed and accuracy line and no key hint
    #[arg(long)]
    quiet: bool,
    /// Show the lyk outline of each missed or slow word on the results (needs `lyk build`)
    #[arg(long)]
    outlines: bool,
    /// Do not record the run in the history
    #[arg(long)]
    no_save: bool,
    /// Print the recent runs and the trend, then exit
    #[arg(long)]
    history: bool,
    /// The built dictionary, for --outlines and `--source phrases`
    #[arg(long, default_value = paths::built_dir_text())]
    dict: PathBuf,
}

/// A timed run's feed is sized for this many words a second at most.
const TIMED_WORDS_PER_SECOND: u64 = 8;
const RECENT_RUNS: usize = 15;

pub fn run(args: &Args) -> Result<()> {
    if args.history {
        print!("{}", history::report(&history::read(&paths::practice_history()), RECENT_RUNS));
        return Ok(());
    }
    if !(std::io::stdin().is_terminal() && std::io::stdout().is_terminal()) {
        bail!("lyk practice needs a terminal: stdin and stdout must both be a TTY (run it in a terminal, not through a pipe or redirect); `lyk practice --history` works anywhere");
    }
    let source = args.source.unwrap_or_else(|| if args.file.is_some() { Source::File } else { Source::Common });
    let pool = make_pool(args, source)?;
    if feed::build(pool.clone(), &feed_opts(args, next_seed(args.seed, 0, random_seed()))).is_empty() {
        bail!("the filters leave no words to practise: loosen --min-len, --max-len, --lowercase-only or --rank");
    }
    let limit = args.time.map(|t| t as f64);
    let mut last: Option<Vec<String>> = None;
    let mut save_error: Option<String> = None;
    {
        let mut terminal = ratatui::try_init().context("cannot start the terminal screen")?;
        let _restore = Restore;
        let mut round = 0;
        loop {
            let words = feed::build(pool.clone(), &feed_opts(args, next_seed(args.seed, round, random_seed())));
            let mut session = Session::new(words, args.stop_on_error);
            let clock = Instant::now();
            let outcome = type_loop(&mut terminal, &mut session, &clock, args.quiet, limit)?;
            let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
            let Some(record) = conclude(&session, outcome, source.name(), now) else {
                if outcome == Outcome::Restart {
                    round += 1;
                    continue;
                }
                break;
            };
            if !args.no_save
                && let Err(e) = history::append(&paths::practice_history(), &record)
            {
                save_error = Some(format!("could not save the run to {}: {e:#}", paths::practice_history().display()));
            }
            let results = session.results(session.finished_at.unwrap_or(0.0));
            let outlines = if args.outlines { outlines_for(&results, &args.dict) } else { HashMap::new() };
            let lines = ui::result_lines(&results, source.name(), &outlines);
            if outcome == Outcome::Quit {
                // Esc or Ctrl-C quit at once, with no results screen
                last = Some(lines);
                break;
            }
            let after = show_results(&mut terminal, &lines)?;
            last = Some(lines);
            if after == After::Quit {
                break;
            }
            round += 1;
        }
    }
    match last {
        Some(lines) => {
            for line in &lines {
                println!("{line}");
            }
        }
        None => println!("no words typed"),
    }
    if let Some(e) = save_error {
        eprintln!("{e}");
    }
    Ok(())
}

/// How a test ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Outcome {
    /// The feed or the clock ran out
    Finished,
    /// Esc or Ctrl-C: the results so far
    Quit,
    /// Tab: abandoned for a fresh feed
    Restart,
}

/// The history record of a test, if it leaves one: an abandoned (restarted) test, or one with no
/// words typed, does not.
fn conclude(session: &Session, outcome: Outcome, source: &str, timestamp: u64) -> Option<history::Record> {
    if outcome == Outcome::Restart {
        return None;
    }
    let results = session.results(session.finished_at.unwrap_or(0.0));
    (results.words > 0).then(|| history::Record::from_results(&results, source, timestamp, outcome == Outcome::Finished))
}

/// The seed of test number `round` (0 is the first): `--seed` plus the round, so a seeded session
/// repeats exactly; without `--seed`, `random`.
const fn next_seed(seed: Option<u64>, round: u64, random: u64) -> u64 {
    match seed {
        Some(s) => s.wrapping_add(round),
        None => random,
    }
}

fn random_seed() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64)
}

/// Restores the terminal when dropped, however the run ends. (`ratatui::try_init` also installs a
/// panic hook that does the same.)
struct Restore;

impl Drop for Restore {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

fn feed_opts(args: &Args, seed: u64) -> feed::Opts {
    let size = args.time.map_or(args.count, |t| usize::try_from(t * TIMED_WORDS_PER_SECOND).unwrap_or(usize::MAX).max(200));
    feed::Opts {
        count: size,
        seed,
        no_repeat: args.no_repeat,
        lowercase_only: args.lowercase_only,
        min_len: args.min_len,
        max_len: args.max_len,
        weighted: args.weighted,
    }
}

/// The source's words, read once: the bands each test's feed is drawn from.
fn make_pool(args: &Args, source: Source) -> Result<Vec<Vec<String>>> {
    let bands = match source {
        Source::Common => {
            let spec = args.bands.as_deref().or(args.rank.as_deref()).unwrap_or("1-1000");
            let bands = feed::parse_bands(spec)?;
            let data = paths::data_dir();
            let skip = if args.names { std::collections::HashSet::new() } else { sources::names_in(&data) };
            let ranked = sources::common(&data.join("frequency/en_full.txt"), feed::reach(&bands), args.names, &skip)?;
            feed::band_units(&ranked, &bands)
        }
        Source::Mistakes => vec![sources::mistakes(&paths::mistakes_dir().join("drills.json"), args.names)?],
        Source::File => {
            let Some(path) = &args.file else { bail!("--source file needs --file PATH") };
            vec![sources::file(path, args.names)?]
        }
        Source::Phrases => vec![sources::phrases(&args.dict)?],
        Source::Quotes => vec![sources::quotes(&paths::data_dir().join("corpus"))?],
    };
    Ok(bands)
}

/// How often the screen is redrawn with no key pressed, so the clock and speed keep moving.
const REDRAW: Duration = Duration::from_millis(250);

/// Runs the screen until the feed or the clock ends, the typist quits or restarts.
fn type_loop(terminal: &mut DefaultTerminal, session: &mut Session, clock: &Instant, quiet: bool, limit: Option<f64>) -> Result<Outcome> {
    loop {
        let now = clock.elapsed().as_secs_f64();
        if let (Some(limit), Some(start)) = (limit, session.started)
            && now - start >= limit
        {
            session.finish(start + limit);
        }
        terminal.draw(|f| ui::draw(f, &ui::View { session, now, quiet, limit }))?;
        if session.finished() {
            return Ok(Outcome::Finished);
        }
        if !event::poll(REDRAW)? {
            continue;
        }
        match event::read()? {
            Event::Key(key) if key.kind != KeyEventKind::Release => match translate(key) {
                Key::Quit => {
                    session.finish(clock.elapsed().as_secs_f64());
                    return Ok(Outcome::Quit);
                }
                Key::Restart => return Ok(Outcome::Restart),
                Key::Input(input) => session.apply(input, clock.elapsed().as_secs_f64()),
                Key::Ignore => {}
            },
            _ => {}
        }
    }
}

enum Key {
    Quit,
    Restart,
    Input(Input),
    Ignore,
}

/// A key press during a test as the engine sees it. Ctrl-Backspace arrives as Ctrl-H (or
/// Backspace with a modifier) and Ctrl-W deletes a word; Ctrl-C and Esc quit, Tab restarts, and
/// `q` is an ordinary letter.
fn translate(key: KeyEvent) -> Key {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let word = key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
    match key.code {
        KeyCode::Esc => Key::Quit,
        KeyCode::Tab => Key::Restart,
        KeyCode::Char('c') if ctrl => Key::Quit,
        KeyCode::Char('w' | 'h') if ctrl => Key::Input(Input::DeleteWord),
        KeyCode::Backspace if word => Key::Input(Input::DeleteWord),
        KeyCode::Backspace => Key::Input(Input::Backspace),
        KeyCode::Enter => Key::Input(Input::Commit),
        KeyCode::Char(c) if !ctrl && !key.modifiers.contains(KeyModifiers::ALT) => Key::Input(Input::Char(c)),
        _ => Key::Ignore,
    }
}

/// What a key on the results screen asks for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum After {
    Quit,
    Restart,
}

/// Seconds after the results appear (and after each key swallowed) in which keys still arriving
/// from the run cannot dismiss the screen.
const RESULTS_QUIET: f64 = 0.7;

/// The results screen's key handling, with the time passed in. Ctrl-C and Esc quit at once (a
/// steno burst never sends them); inside the quiet window every other key is swallowed (and
/// extends it); after it, `q` quits and Tab and Enter restart.
struct Gate {
    quiet_until: f64,
}

impl Gate {
    const fn new(now: f64) -> Self {
        Self { quiet_until: now + RESULTS_QUIET }
    }

    fn key(&mut self, key: KeyEvent, now: f64) -> Option<After> {
        if key.code == KeyCode::Esc || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)) {
            return Some(After::Quit);
        }
        if now < self.quiet_until {
            self.quiet_until = now + RESULTS_QUIET;
            return None;
        }
        if key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) {
            return None;
        }
        match key.code {
            KeyCode::Char('q') => Some(After::Quit),
            KeyCode::Tab | KeyCode::Enter => Some(After::Restart),
            _ => None,
        }
    }
}

/// Shows the results until the typist quits or asks for another test.
fn show_results(terminal: &mut DefaultTerminal, lines: &[String]) -> Result<After> {
    terminal.draw(|f| ui::draw_results(f, lines))?;
    let clock = Instant::now();
    let mut gate = Gate::new(0.0);
    loop {
        if !event::poll(REDRAW)? {
            continue;
        }
        match event::read()? {
            Event::Key(k) if k.kind != KeyEventKind::Release => {
                if let Some(after) = gate.key(k, clock.elapsed().as_secs_f64()) {
                    return Ok(after);
                }
            }
            Event::Resize(..) => {
                terminal.draw(|f| ui::draw_results(f, lines))?;
            }
            _ => {}
        }
    }
}

/// The lyk outline of each missed or slow word, from the built dictionary: its fewest-stroke,
/// then shortest, entry. Empty when nothing is built.
fn outlines_for(results: &engine::Results, dir: &std::path::Path) -> HashMap<String, String> {
    let wanted: HashSet<&str> =
        results.slow.iter().map(|(w, _)| w.as_str()).chain(results.misses.iter().map(|m| m.word.as_str())).collect();
    let mut best: HashMap<String, String> = HashMap::new();
    for file in ["02-lyk-words.json", "02-lyk-short.json", "02-lyk-alternates.json", "02-lyk-extras.json"] {
        let Ok(text) = std::fs::read_to_string(dir.join(file)) else { continue };
        let Ok(map) = serde_json::from_str::<std::collections::BTreeMap<String, String>>(&text) else { continue };
        for (outline, word) in map {
            let word = word.to_lowercase();
            if !wanted.contains(word.as_str()) {
                continue;
            }
            let key = |o: &str| (o.matches('/').count(), o.len());
            if best.get(&word).is_none_or(|have| key(&outline) < key(have)) {
                best.insert(word, outline);
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn keys_map_to_inputs() {
        let none = KeyModifiers::NONE;
        assert!(matches!(translate(key(KeyCode::Char('a'), none)), Key::Input(Input::Char('a'))));
        assert!(matches!(translate(key(KeyCode::Char(' '), none)), Key::Input(Input::Char(' '))));
        assert!(matches!(translate(key(KeyCode::Enter, none)), Key::Input(Input::Commit)));
        assert!(matches!(translate(key(KeyCode::Backspace, none)), Key::Input(Input::Backspace)));
        assert!(matches!(translate(key(KeyCode::Backspace, KeyModifiers::CONTROL)), Key::Input(Input::DeleteWord)));
        assert!(matches!(translate(key(KeyCode::Char('w'), KeyModifiers::CONTROL)), Key::Input(Input::DeleteWord)));
        assert!(matches!(translate(key(KeyCode::Char('h'), KeyModifiers::CONTROL)), Key::Input(Input::DeleteWord)));
        assert!(matches!(translate(key(KeyCode::Char('c'), KeyModifiers::CONTROL)), Key::Quit));
        assert!(matches!(translate(key(KeyCode::Esc, none)), Key::Quit));
        assert!(matches!(translate(key(KeyCode::Tab, none)), Key::Restart));
        // `q` is an ordinary letter during a test
        assert!(matches!(translate(key(KeyCode::Char('q'), none)), Key::Input(Input::Char('q'))));
        assert!(matches!(translate(key(KeyCode::Char('x'), KeyModifiers::CONTROL)), Key::Ignore));
    }

    #[test]
    fn the_results_screen_keys() {
        let none = KeyModifiers::NONE;
        let ctrl = KeyModifiers::CONTROL;
        let after = |gate: &mut Gate, code, mods, now| gate.key(key(code, mods), now);
        let mut g = Gate::new(10.0);
        // inside the 0.7 s window: stray keys do nothing, and extend it
        assert_eq!(after(&mut g, KeyCode::Char('q'), none, 10.3), None);
        assert_eq!(after(&mut g, KeyCode::Tab, none, 10.8), None, "the window was extended by the key at 10.3 and 10.8");
        assert_eq!(after(&mut g, KeyCode::Enter, none, 10.9), None);
        // Esc and Ctrl-C work inside the window
        assert_eq!(after(&mut g, KeyCode::Esc, none, 10.95), Some(After::Quit));
        assert_eq!(after(&mut g, KeyCode::Char('c'), ctrl, 10.95), Some(After::Quit));
        // just after it, everything works at once
        assert_eq!(after(&mut g, KeyCode::Char('q'), none, 11.7), Some(After::Quit));
        let mut g = Gate::new(0.0);
        assert_eq!(after(&mut g, KeyCode::Esc, none, 0.1), Some(After::Quit));
        assert_eq!(after(&mut g, KeyCode::Tab, none, 0.7), Some(After::Restart));
        assert_eq!(after(&mut g, KeyCode::Enter, none, 5.0), Some(After::Restart));
        assert_eq!(after(&mut g, KeyCode::Char('c'), ctrl, 5.0), Some(After::Quit));
        assert_eq!(after(&mut g, KeyCode::Char('x'), none, 5.0), None);
        assert_eq!(after(&mut g, KeyCode::Char(' '), none, 5.0), None);
        assert_eq!(after(&mut g, KeyCode::Char('q'), ctrl, 5.0), None);
    }

    #[test]
    fn restart_advances_a_given_seed_and_randomises_otherwise() {
        assert_eq!(next_seed(Some(7), 0, 999), 7);
        assert_eq!(next_seed(Some(7), 1, 999), 8);
        assert_eq!(next_seed(Some(7), 2, 999), 9);
        assert_eq!(next_seed(Some(u64::MAX), 1, 999), 0, "wraps");
        assert_eq!(next_seed(None, 3, 999), 999);
        // different seeds give different feeds, the same seed the same feed
        let pool = vec![(0..500).map(|i| format!("w{i}")).collect::<Vec<_>>()];
        let opts = |seed| feed::Opts { count: 25, seed, no_repeat: false, lowercase_only: false, min_len: 1, max_len: 30, weighted: false };
        let a = feed::build(pool.clone(), &opts(7));
        assert_eq!(a, feed::build(pool.clone(), &opts(7)));
        assert_ne!(a, feed::build(pool, &opts(8)));
        assert_eq!(a.len(), 25);
    }

    #[test]
    fn a_restart_resets_the_session_and_leaves_no_history() {
        let words: Vec<String> = ["the", "cat", "sat"].map(String::from).to_vec();
        let mut s = Session::new(words.clone(), false);
        for c in "the ca".chars() {
            s.apply(Input::Char(c), 2.0);
        }
        assert!(s.started.is_some() && s.current() == 1 && !s.typed.is_empty());
        // abandoned by Tab: no record
        assert!(conclude(&s, Outcome::Restart, "common", 1).is_none(), "an abandoned test is not saved");
        // the next test is a fresh session: nothing carried over, the clock not started
        let fresh = Session::new(words, false);
        assert!(fresh.started.is_none() && fresh.current() == 0 && fresh.typed.is_empty() && !fresh.finished());
        assert_eq!(fresh.results(5.0).words, 0);
        // Esc with words typed does leave a record, marked incomplete; with none it does not
        s.finish(4.0);
        let rec = conclude(&s, Outcome::Quit, "common", 1).expect("words typed");
        assert!(!rec.complete && rec.count == 1);
        assert!(conclude(&fresh, Outcome::Quit, "common", 1).is_none(), "no words typed, no record");
        let mut done = Session::new(vec!["a".into()], false);
        done.apply(Input::Char('a'), 1.0);
        assert!(conclude(&done, Outcome::Finished, "common", 1).expect("finished").complete);
    }

    #[test]
    fn count_defaults_to_25() {
        use clap::Parser;
        #[derive(Parser)]
        struct Cli {
            #[command(flatten)]
            args: Args,
        }
        assert_eq!(Cli::parse_from(["x"]).args.count, 25);
    }

    #[test]
    fn a_pipe_is_refused_with_a_message() {
        // under `cargo test` stdin is not a terminal
        let args = Args {
            source: None,
            file: None,
            bands: None,
            rank: None,
            count: 5,
            time: None,
            seed: Some(1),
            weighted: false,
            no_repeat: false,
            lowercase_only: false,
            min_len: 1,
            max_len: 30,
            names: false,
            stop_on_error: false,
            quiet: false,
            outlines: false,
            no_save: true,
            history: false,
            dict: PathBuf::new(),
        };
        if !std::io::stdin().is_terminal() {
            let err = run(&args).unwrap_err().to_string();
            assert!(err.contains("needs a terminal"), "{err}");
        }
    }

    #[test]
    fn outlines_prefer_the_fewest_strokes_then_the_shortest() {
        let dir = std::env::temp_dir().join(format!("lyk-practice-outl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("02-lyk-words.json"), r#"{"TKOG/*": "dog", "TKOG": "dog", "KAT": "cat"}"#).unwrap();
        std::fs::write(dir.join("02-lyk-short.json"), r#"{"TKOGZ": "Dog"}"#).unwrap();
        let r = engine::Results {
            secs: 1.0,
            wpm: 0.0,
            raw: 0.0,
            accuracy: 100.0,
            word_accuracy: 100.0,
            errors: 0,
            words: 1,
            consistency: None,
            slow: vec![("dog".into(), 1.0)],
            misses: vec![],
        };
        let got = outlines_for(&r, &dir);
        assert_eq!(got.get("dog").map(String::as_str), Some("TKOG"));
        assert!(!got.contains_key("cat"), "only asked-for words: {got:?}");
        assert!(outlines_for(&r, &dir.join("missing")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

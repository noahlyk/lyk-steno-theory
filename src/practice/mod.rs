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
    #[arg(long, default_value_t = 50, conflicts_with = "time")]
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
    /// No live speed and accuracy line
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
    let words = make_feed(args, source)?;
    if words.is_empty() {
        bail!("the filters leave no words to practise: loosen --min-len, --max-len, --lowercase-only or --rank");
    }
    let limit = args.time.map(|t| t as f64);
    let mut session = Session::new(words, args.stop_on_error);
    let (results, lines, aborted) = {
        let mut terminal = ratatui::try_init().context("cannot start the terminal screen")?;
        let _restore = Restore;
        let clock = Instant::now();
        let aborted = type_loop(&mut terminal, &mut session, &clock, args.quiet, limit)?;
        let results = session.results(session.finished_at.unwrap_or(0.0));
        let outlines = if args.outlines && results.words > 0 { outlines_for(&results, &args.dict) } else { HashMap::new() };
        let lines = ui::result_lines(&results, source.name(), &outlines);
        if results.words > 0 {
            show_results(&mut terminal, &lines)?;
        }
        (results, lines, aborted)
    };
    if results.words == 0 {
        println!("no words typed");
        return Ok(());
    }
    for line in &lines {
        println!("{line}");
    }
    if !args.no_save {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let record = history::Record::from_results(&results, source.name(), now, !aborted);
        let path = paths::practice_history();
        if let Err(e) = history::append(&path, &record) {
            eprintln!("could not save the run to {}: {e:#}", path.display());
        }
    }
    Ok(())
}

/// Restores the terminal when dropped, however the run ends. (`ratatui::try_init` also installs a
/// panic hook that does the same.)
struct Restore;

impl Drop for Restore {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

fn make_feed(args: &Args, source: Source) -> Result<Vec<String>> {
    let size = args.time.map_or(args.count, |t| usize::try_from(t * TIMED_WORDS_PER_SECOND).unwrap_or(usize::MAX).max(200));
    let opts = feed::Opts {
        count: size,
        seed: args.seed.unwrap_or_else(|| SystemTime::now().duration_since(UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64)),
        no_repeat: args.no_repeat,
        lowercase_only: args.lowercase_only,
        min_len: args.min_len,
        max_len: args.max_len,
        weighted: args.weighted,
    };
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
    // A feed that runs short (--no-repeat on a small pool) is as long as it can be.
    Ok(feed::build(bands, &opts))
}

/// Runs the screen until the feed or the clock ends or the typist quits. True if they quit.
fn type_loop(terminal: &mut DefaultTerminal, session: &mut Session, clock: &Instant, quiet: bool, limit: Option<f64>) -> Result<bool> {
    loop {
        let now = clock.elapsed().as_secs_f64();
        if let (Some(limit), Some(start)) = (limit, session.started)
            && now - start >= limit
        {
            session.finish(start + limit);
        }
        terminal.draw(|f| ui::draw(f, &ui::View { session, now, quiet, limit }))?;
        if session.finished() {
            return Ok(false);
        }
        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        match event::read()? {
            Event::Key(key) if key.kind != KeyEventKind::Release => match translate(key) {
                Key::Quit => {
                    session.finish(clock.elapsed().as_secs_f64());
                    return Ok(true);
                }
                Key::Input(input) => session.apply(input, clock.elapsed().as_secs_f64()),
                Key::Ignore => {}
            },
            _ => {}
        }
    }
}

enum Key {
    Quit,
    Input(Input),
    Ignore,
}

/// A key press as the engine sees it. Ctrl-Backspace arrives as Ctrl-H (or Backspace with a
/// modifier) and Ctrl-W deletes a word; Ctrl-C and Esc quit.
fn translate(key: KeyEvent) -> Key {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let word = key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
    match key.code {
        KeyCode::Esc => Key::Quit,
        KeyCode::Char('c') if ctrl => Key::Quit,
        KeyCode::Char('w' | 'h') if ctrl => Key::Input(Input::DeleteWord),
        KeyCode::Backspace if word => Key::Input(Input::DeleteWord),
        KeyCode::Backspace => Key::Input(Input::Backspace),
        KeyCode::Enter => Key::Input(Input::Commit),
        KeyCode::Char(c) if !ctrl && !key.modifiers.contains(KeyModifiers::ALT) => Key::Input(Input::Char(c)),
        _ => Key::Ignore,
    }
}

/// Shows the results until a key. Keys still arriving from the run (strokes already on their
/// way) are let go first, so the screen is not dismissed by them.
fn show_results(terminal: &mut DefaultTerminal, lines: &[String]) -> Result<()> {
    terminal.draw(|f| ui::draw_results(f, lines))?;
    while event::poll(Duration::from_millis(700))? {
        event::read()?;
    }
    loop {
        match event::read()? {
            Event::Key(k) if k.kind != KeyEventKind::Release => return Ok(()),
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
        assert!(matches!(translate(key(KeyCode::Tab, none)), Key::Ignore));
        assert!(matches!(translate(key(KeyCode::Char('x'), KeyModifiers::CONTROL)), Key::Ignore));
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

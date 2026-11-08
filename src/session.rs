//! The interactive session a query command opens when it is given no argument (`lookup`,
//! `resolve`, `explain`, `ergonomics`, `paths`, `family`), after keymux's `steno keys`: a prompt,
//! and under it the answer for what is typed so far, redrawn on every key. Outlines are coloured
//! by finger where the terminal has colour. The answers are [`crate::lookup_view`]'s, all three
//! theories; the commands that explain more than a lookup (`explain`, `ergonomics`, `paths`,
//! `family`) run their full report on Enter.
//!
//! Keys: type to search; Backspace; Ctrl-W deletes a word, Ctrl-U the line; Enter keeps the
//! answer in the scrollback and opens a fresh prompt (or runs the command's report, for those
//! four); Esc clears the line, or quits when it is already empty; Ctrl-D, Ctrl-C and Enter on
//! an empty line quit. Piped in or out, there is no redrawing: each line read prints its answer.

use crate::lookup_view::{Lookup, Style, render_all, terminal_colour};
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::{cursor, queue, terminal};
use std::io::{BufRead, IsTerminal, Write};

/// What the session does with a finished line.
pub enum Mode<'a> {
    /// `lookup`: the live view is the answer; Enter keeps it
    Lookup,
    /// `resolve`: the same, with the query always read as a stroke outline
    Resolve,
    /// `explain`, `ergonomics`, `paths`, `family`: Enter runs the command's own report for the
    /// line; `name` is what the prompt's hint calls it
    Report { name: &'static str, run: &'a mut dyn FnMut(&str) -> Result<()> },
}

/// A key, with the terminal's details taken out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Backspace,
    DeleteWord,
    ClearLine,
    Enter,
    Escape,
    Quit,
}

/// What a key did to the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Redraw the line and its answer
    Redraw,
    /// Keep this line's answer and start a new prompt
    Submit(String),
    Quit,
}

/// The line being typed.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct State {
    pub query: String,
}

impl State {
    /// Apply one key. Pure: the terminal loop and the tests both go through here.
    pub fn press(&mut self, key: Key) -> Outcome {
        match key {
            Key::Char(c) => {
                self.query.push(c);
                Outcome::Redraw
            }
            Key::Backspace => {
                self.query.pop();
                Outcome::Redraw
            }
            Key::DeleteWord => {
                let kept = self.query.trim_end().trim_end_matches(|c: char| !c.is_whitespace()).len();
                self.query.truncate(kept);
                Outcome::Redraw
            }
            Key::Escape if self.query.is_empty() => Outcome::Quit,
            Key::ClearLine | Key::Escape => {
                self.query.clear();
                Outcome::Redraw
            }
            Key::Enter if self.query.trim().is_empty() => Outcome::Quit,
            Key::Enter => {
                let line = std::mem::take(&mut self.query);
                Outcome::Submit(line.trim().to_owned())
            }
            Key::Quit => Outcome::Quit,
        }
    }
}

/// The terminal event as a [`Key`], `None` for one the session ignores.
#[must_use]
pub fn key_of(event: KeyEvent) -> Option<Key> {
    if event.kind == KeyEventKind::Release {
        return None;
    }
    let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);
    match event.code {
        KeyCode::Char('c' | 'd') if ctrl => Some(Key::Quit),
        KeyCode::Char('w') if ctrl => Some(Key::DeleteWord),
        KeyCode::Char('u') if ctrl => Some(Key::ClearLine),
        KeyCode::Char(c) if !ctrl => Some(Key::Char(c)),
        KeyCode::Backspace => Some(Key::Backspace),
        KeyCode::Enter => Some(Key::Enter),
        KeyCode::Esc => Some(Key::Escape),
        _ => None,
    }
}

const PROMPT: &str = "› ";

/// The hint under the first prompt.
fn hint(mode: &Mode<'_>) -> String {
    let enter = match mode {
        Mode::Report { name, .. } => format!("Enter runs {name}"),
        _ => "Enter keeps the answer".to_owned(),
    };
    format!("type to search, all three theories live · {enter} · Esc clears · empty Enter or Ctrl-D quits")
}

/// Run the session on the terminal, or line by line when stdin or stdout is not one.
pub fn run(lookup: &Lookup<'_>, mut mode: Mode<'_>) -> Result<()> {
    let force_stroke = matches!(mode, Mode::Resolve);
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return run_lines(lookup, &mut mode, force_stroke);
    }
    let colour = terminal_colour();
    let mut out = std::io::stdout();
    let dim = |text: &str| if colour { format!("\x1b[2m{text}\x1b[0m") } else { text.to_owned() };
    writeln!(out, "{}", dim(&hint(&mode)))?;
    terminal::enable_raw_mode()?;
    let result = (|| -> Result<()> {
        let mut state = State::default();
        let mut drawn = 0;
        draw(&mut out, lookup, &state, force_stroke, colour, &mut drawn)?;
        loop {
            let event = event::read()?;
            let Event::Key(key) = event else {
                if matches!(event, Event::Resize(..)) {
                    draw(&mut out, lookup, &state, force_stroke, colour, &mut drawn)?;
                }
                continue;
            };
            let Some(key) = key_of(key) else { continue };
            match state.press(key) {
                Outcome::Redraw => draw(&mut out, lookup, &state, force_stroke, colour, &mut drawn)?,
                Outcome::Quit => {
                    finish(&mut out, drawn, true)?;
                    return Ok(());
                }
                Outcome::Submit(line) => {
                    finish(&mut out, drawn, false)?;
                    drawn = 0;
                    if let Mode::Report { run, .. } = &mut mode {
                        terminal::disable_raw_mode()?;
                        let ran = run(&line);
                        terminal::enable_raw_mode()?;
                        if let Err(error) = ran {
                            write!(out, "{error:#}\r\n")?;
                        }
                        write!(out, "\r\n")?;
                    }
                    draw(&mut out, lookup, &state, force_stroke, colour, &mut drawn)?;
                }
            }
        }
    })();
    terminal::disable_raw_mode()?;
    result
}

/// Redraw the prompt and the answer under it, leaving the cursor on the prompt.
fn draw(
    out: &mut impl Write,
    lookup: &Lookup<'_>,
    state: &State,
    force_stroke: bool,
    colour: bool,
    drawn: &mut usize,
) -> Result<()> {
    let (columns, rows) = terminal::size().unwrap_or((80, 24));
    let style = Style { colour, columns: Some(usize::from(columns)) };
    let mut lines = render_all(&lookup.live_view(&state.query, force_stroke), style);
    // Never taller than the screen, or the prompt scrolls away
    let room = usize::from(rows).saturating_sub(3).max(1);
    if lines.len() > room {
        let more = lines.len() - room + 1;
        lines.truncate(room - 1);
        lines.push(format!("… {more} more lines"));
    }
    queue!(out, cursor::MoveToColumn(0), terminal::Clear(terminal::ClearType::FromCursorDown))?;
    let prompt = if colour { format!("\x1b[35;1m{PROMPT}\x1b[0m") } else { PROMPT.to_owned() };
    write!(out, "{prompt}{}", state.query)?;
    for line in &lines {
        write!(out, "\r\n{line}")?;
    }
    if !lines.is_empty() {
        queue!(out, cursor::MoveUp(u16::try_from(lines.len()).unwrap_or(u16::MAX)))?;
    }
    let at = PROMPT.chars().count() + state.query.chars().count();
    queue!(out, cursor::MoveToColumn(u16::try_from(at).unwrap_or(u16::MAX)))?;
    *drawn = lines.len();
    out.flush()?;
    Ok(())
}

/// Leave the answer on screen and move the cursor below it (or, on quit, onto a clean line).
fn finish(out: &mut impl Write, drawn: usize, quit: bool) -> Result<()> {
    if quit {
        queue!(out, cursor::MoveToColumn(0), terminal::Clear(terminal::ClearType::FromCursorDown))?;
        out.flush()?;
        return Ok(());
    }
    for _ in 0..=drawn {
        write!(out, "\r\n")?;
    }
    out.flush()?;
    Ok(())
}

/// Piped session: each line read prints its answer (a line that is empty ends it).
fn run_lines(lookup: &Lookup<'_>, mode: &mut Mode<'_>, force_stroke: bool) -> Result<()> {
    let style = Style { colour: false, columns: None };
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        let query = line.trim();
        if query.is_empty() {
            break;
        }
        if let Mode::Report { run, .. } = mode {
            run(query)?;
        } else {
            for text in render_all(&lookup.view(query, force_stroke), style) {
                println!("{text}");
            }
        }
        println!();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(keys: &[Key]) -> (State, Vec<Outcome>) {
        let mut state = State::default();
        let outcomes = keys.iter().map(|k| state.press(*k)).collect();
        (state, outcomes)
    }

    fn chars(text: &str) -> Vec<Key> {
        text.chars().map(Key::Char).collect()
    }

    #[test]
    fn typing_builds_the_line_and_backspace_takes_a_character() {
        let mut keys = chars("cat");
        keys.push(Key::Backspace);
        let (state, outcomes) = typed(&keys);
        assert_eq!(state.query, "ca");
        assert!(outcomes.iter().all(|o| *o == Outcome::Redraw));
    }

    #[test]
    fn ctrl_w_deletes_the_last_word_and_ctrl_u_the_line() {
        let mut keys = chars("tell me how");
        keys.push(Key::DeleteWord);
        assert_eq!(typed(&keys).0.query, "tell me ");
        let mut keys = chars("tell me how");
        keys.push(Key::ClearLine);
        assert_eq!(typed(&keys).0.query, "");
    }

    #[test]
    fn enter_submits_a_line_and_starts_a_fresh_one() {
        let mut keys = chars("cat");
        keys.push(Key::Enter);
        let (state, outcomes) = typed(&keys);
        assert_eq!(outcomes.last(), Some(&Outcome::Submit("cat".to_owned())));
        assert_eq!(state.query, "");
    }

    #[test]
    fn enter_on_an_empty_line_quits() {
        assert_eq!(typed(&[Key::Enter]).1, [Outcome::Quit]);
        assert_eq!(typed(&chars("  ")).0.query, "  ");
        let mut keys = chars("  ");
        keys.push(Key::Enter);
        assert_eq!(typed(&keys).1.last(), Some(&Outcome::Quit));
    }

    #[test]
    fn escape_clears_a_line_and_quits_from_an_empty_one() {
        let mut keys = chars("cat");
        keys.push(Key::Escape);
        let (state, outcomes) = typed(&keys);
        assert_eq!(state.query, "");
        assert_eq!(outcomes.last(), Some(&Outcome::Redraw));
        keys.push(Key::Escape);
        assert_eq!(typed(&keys).1.last(), Some(&Outcome::Quit));
    }

    #[test]
    fn ctrl_c_and_ctrl_d_quit_even_with_text_typed() {
        let mut keys = chars("cat");
        keys.push(Key::Quit);
        assert_eq!(typed(&keys).1.last(), Some(&Outcome::Quit));
    }

    #[test]
    fn terminal_events_become_keys() {
        let press = |code, modifiers| key_of(KeyEvent::new(code, modifiers));
        assert_eq!(press(KeyCode::Char('a'), KeyModifiers::NONE), Some(Key::Char('a')));
        assert_eq!(press(KeyCode::Char('A'), KeyModifiers::SHIFT), Some(Key::Char('A')));
        assert_eq!(press(KeyCode::Char('d'), KeyModifiers::CONTROL), Some(Key::Quit));
        assert_eq!(press(KeyCode::Char('c'), KeyModifiers::CONTROL), Some(Key::Quit));
        assert_eq!(press(KeyCode::Char('w'), KeyModifiers::CONTROL), Some(Key::DeleteWord));
        assert_eq!(press(KeyCode::Char('x'), KeyModifiers::CONTROL), None);
        assert_eq!(press(KeyCode::Esc, KeyModifiers::NONE), Some(Key::Escape));
        assert_eq!(press(KeyCode::Up, KeyModifiers::NONE), None);
    }
}

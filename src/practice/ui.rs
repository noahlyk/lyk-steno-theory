//! What is drawn: the word feed (and a quiet stats line), and the results screen. Nothing else:
//! no outlines, no keyboard, no hints.

use super::engine::Session;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

const MAX_WIDTH: u16 = 80;
const LINES: usize = 3;
const DIM: Color = Color::DarkGray;
/// The live line: dim next to the words, but easy to read.
const LIVE: Color = Color::Gray;

/// What the feed screen needs besides the session.
pub struct View<'a> {
    pub session: &'a Session,
    pub now: f64,
    pub quiet: bool,
    /// The time limit in seconds, in a timed run
    pub limit: Option<f64>,
}

/// Which words go on which line when the lines are `width` wide (indices into `words`).
#[must_use]
pub fn wrap(words: &[String], typed_len: impl Fn(usize) -> usize, width: usize) -> Vec<Vec<usize>> {
    let mut lines: Vec<Vec<usize>> = vec![Vec::new()];
    let mut used = 0;
    for (i, w) in words.iter().enumerate() {
        // A word is as wide as the longer of itself and what was typed in it, plus its space
        let need = w.chars().count().max(typed_len(i)) + 1;
        if used + need > width && used > 0 {
            lines.push(Vec::new());
            used = 0;
        }
        lines.last_mut().expect("lines starts non-empty").push(i);
        used += need;
    }
    lines
}

fn char_style(state: Option<bool>, cursor: bool) -> Style {
    let base = match state {
        Some(true) => Style::default().fg(Color::Green),
        Some(false) => Style::default().fg(Color::Red),
        None => Style::default().fg(DIM),
    };
    if cursor { base.add_modifier(Modifier::UNDERLINED | Modifier::BOLD).fg(Color::White) } else { base }
}

/// One word as spans: typed characters green or red against the target, the rest dim, extras red.
fn word_spans(target: &str, typed: Option<&str>, current: bool) -> Vec<Span<'static>> {
    let target: Vec<char> = target.chars().collect();
    let typed: Vec<char> = typed.map_or_else(Vec::new, |t| t.chars().collect());
    let mut spans = Vec::new();
    for (i, &c) in target.iter().enumerate() {
        let state = typed.get(i).map(|&t| t == c);
        let style = match state {
            Some(_) => char_style(state, false),
            None => char_style(None, current && i == typed.len()),
        };
        spans.push(Span::styled(c.to_string(), style));
    }
    for &extra in typed.iter().skip(target.len()) {
        spans.push(Span::styled(extra.to_string(), Style::default().fg(Color::Red).add_modifier(Modifier::CROSSED_OUT)));
    }
    if current && typed.len() >= target.len() {
        spans.push(Span::styled(" ", Style::default().add_modifier(Modifier::UNDERLINED)));
        return spans;
    }
    spans.push(Span::raw(" "));
    spans
}

/// The live line above the feed: `wpm 84   acc 97%   12/25   0:09` (words done out of the count
/// and the time so far), or in a timed run the words done and the time left. The clock starts at
/// the first key, so until then it is `wpm 0` and the full time.
#[must_use]
pub fn stats_line(session: &Session, now: f64, limit: Option<f64>) -> String {
    let r = session.results(now);
    let clock = |s: f64| format!("{}:{:02}", (s / 60.0) as u64, (s % 60.0) as u64);
    let progress = limit.map_or_else(
        || format!("{}/{}   {}", session.current(), session.words.len(), clock(r.secs)),
        |l| format!("{} words   {}", session.current(), clock((l - r.secs).max(0.0).ceil())),
    );
    format!("wpm {:.0}   acc {:.0}%   {progress}", r.wpm, r.accuracy)
}

/// The key hint under the feed.
pub const HINT: &str = "tab restart  esc quit";
/// The key hint at the foot of the results screen.
pub const RESULTS_HINT: &str = "tab/enter again  q/esc quit";

pub fn draw(frame: &mut Frame, view: &View) {
    let area = frame.area();
    let width = area.width.saturating_sub(4).clamp(10, MAX_WIDTH);
    let s = view.session;
    let typed_len = |i: usize| if i == s.current() { s.typed.chars().count() } else { s.done.get(i).map_or(0, |d| d.typed.chars().count()) };
    let lines = wrap(&s.words, typed_len, usize::from(width));
    let cur_line = lines.iter().position(|l| l.contains(&s.current().min(s.words.len().saturating_sub(1)))).unwrap_or(0);
    let first = cur_line.saturating_sub(1);
    let rows: Vec<Line> = lines
        .iter()
        .skip(first)
        .take(LINES)
        .map(|idx| {
            let mut spans = Vec::new();
            for &i in idx {
                let cur = i == s.current();
                let typed = if cur { Some(s.typed.as_str()) } else { s.done.get(i).map(|d| d.typed.as_str()) };
                spans.extend(word_spans(&s.words[i], typed, cur));
            }
            Line::from(spans)
        })
        .collect();
    let height = LINES as u16 + if view.quiet { 0 } else { 4 };
    let top = area.y + area.height.saturating_sub(height) / 2;
    let x = area.x + area.width.saturating_sub(width) / 2;
    let block = Rect { x, y: top, width: width.min(area.width), height: height.min(area.height) };
    let extra = if view.quiet { 0 } else { 2 };
    let parts = Layout::vertical([Constraint::Length(extra), Constraint::Length(LINES as u16), Constraint::Length(extra)]).split(block);
    if !view.quiet {
        frame.render_widget(Paragraph::new(stats_line(s, view.now, view.limit)).style(Style::default().fg(LIVE)).alignment(Alignment::Left), parts[0]);
        frame.render_widget(Paragraph::new(format!("\n{HINT}")).style(Style::default().fg(DIM)), parts[2]);
    }
    frame.render_widget(Paragraph::new(rows), parts[1]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::practice::engine::Input;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn session(words: &[&str]) -> Session {
        Session::new(words.iter().map(|w| (*w).to_owned()).collect(), false)
    }

    fn screen(term: &Terminal<TestBackend>) -> Vec<String> {
        let buf = term.backend().buffer();
        (0..buf.area.height)
            .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_owned()).collect::<String>().trim_end().to_owned())
            .collect()
    }

    #[test]
    fn the_feed_shows_words_colours_and_a_quiet_stats_line() {
        let mut s = session(&["the", "cat", "sat", "on", "mat"]);
        for c in "the cxt".chars() {
            s.apply(Input::Char(c), 1.0);
        }
        let mut term = Terminal::new(TestBackend::new(40, 9)).unwrap();
        term.draw(|f| draw(f, &View { session: &s, now: 3.0, quiet: false, limit: None })).unwrap();
        let rows = screen(&term);
        let text = rows.join("\n");
        assert!(text.contains("the cxt sat on mat") || text.contains("the cat sat on mat"), "{text}");
        assert!(text.contains("wpm") && text.contains("1/5"), "{text}");
        // colours: the committed `the` green, the mistyped `a` of `cat` red, the untyped `sat` dim
        let find = |ch: &str, nth: usize| {
            let buf = term.backend().buffer();
            (0..buf.area.height)
                .flat_map(|y| (0..buf.area.width).map(move |x| (x, y)))
                .filter(|&(x, y)| buf[(x, y)].symbol() == ch)
                .nth(nth)
                .map(|(x, y)| buf[(x, y)].fg)
                .unwrap()
        };
        assert_eq!(find("t", 0), Color::Green);
        assert_eq!(find("a", 1), Color::Red, "the first a is in the live line");
        assert_eq!(find("s", 0), DIM);
        // nothing else on screen: no outline, no key diagram
        assert!(!text.contains("KAT") && !text.contains("hint"), "{text}");
    }

    #[test]
    fn quiet_hides_the_stats_line() {
        let s = session(&["the", "cat"]);
        let mut term = Terminal::new(TestBackend::new(40, 9)).unwrap();
        term.draw(|f| draw(f, &View { session: &s, now: 3.0, quiet: true, limit: None })).unwrap();
        let text = screen(&term).join("\n");
        assert!(!text.contains("wpm"), "{text}");
        assert!(text.contains("the cat"), "{text}");
    }

    #[test]
    fn a_long_feed_wraps_and_scrolls_with_the_current_word() {
        let words: Vec<String> = (0..60).map(|i| format!("word{i}")).collect();
        let lines = wrap(&words, |_| 0, 30);
        assert!(lines.len() > 3, "{} lines", lines.len());
        assert!(lines.iter().all(|l| l.len() <= 5), "{lines:?}");
        let mut s = Session::new(words, false);
        for i in 0..30 {
            for c in format!("word{i} ").chars() {
                s.apply(Input::Char(c), 1.0);
            }
        }
        let mut term = Terminal::new(TestBackend::new(34, 9)).unwrap();
        term.draw(|f| draw(f, &View { session: &s, now: 5.0, quiet: true, limit: None })).unwrap();
        let text = screen(&term).join("\n");
        assert!(text.contains("word30"), "the current word must be on screen: {text}");
        assert!(!text.contains("word0 "), "{text}");
    }

    #[test]
    fn timed_runs_show_the_time_left() {
        let mut s = session(&["a", "b"]);
        assert_eq!(stats_line(&s, 5.0, Some(60.0)), "wpm 0   acc 100%   0 words   1:00");
        s.apply(Input::Char('a'), 10.0);
        let line = stats_line(&s, 22.0, Some(60.0));
        assert!(line.ends_with("0 words   0:48"), "{line}");
    }

    #[test]
    fn the_clock_starts_at_the_first_key_and_the_live_line_decays() {
        let mut s = session(&["hello", "world", "again", "words", "more"]);
        // 7 s pass with no key: the clock has not started
        assert_eq!(stats_line(&s, 7.0, None), "wpm 0   acc 100%   0/5   0:00");
        assert!(s.started.is_none());
        // the first key at t=13 starts the clock
        for c in "hello ".chars() {
            s.apply(Input::Char(c), 13.0);
        }
        assert_eq!(s.started, Some(13.0));
        // 5 correct characters over 6 s: 5 / 5 / (6 / 60) = 10 wpm
        assert_eq!(stats_line(&s, 19.0, None), "wpm 10   acc 100%   1/5   0:06");
        // no key for 6 more seconds: the clock runs on and the speed falls
        assert_eq!(stats_line(&s, 25.0, None), "wpm 5   acc 100%   1/5   0:12");
    }

    #[test]
    fn the_hint_and_the_live_line_show_and_quiet_hides_both() {
        let s = session(&["the", "cat"]);
        let mut term = Terminal::new(TestBackend::new(60, 12)).unwrap();
        term.draw(|f| draw(f, &View { session: &s, now: 0.0, quiet: false, limit: None })).unwrap();
        let rows = screen(&term);
        let live = rows.iter().position(|r| r.contains("wpm 0   acc 100%   0/2   0:00")).expect("live line");
        let feed = rows.iter().position(|r| r.contains("the cat")).expect("feed");
        let hint = rows.iter().position(|r| r.contains(HINT)).expect("hint");
        assert!(live < feed && feed < hint, "{rows:?}");
        assert_eq!(HINT, "tab restart  esc quit");
        let buf = term.backend().buffer();
        let fg_at = |row: usize| (0..buf.area.width).map(|x| &buf[(x, row as u16)]).find(|c| c.symbol() != " ").map(|c| c.fg).unwrap();
        assert_eq!(fg_at(hint), DIM);
        assert_eq!(fg_at(live), LIVE);
        term.draw(|f| draw(f, &View { session: &s, now: 0.0, quiet: true, limit: None })).unwrap();
        let text = screen(&term).join("\n");
        assert!(!text.contains("wpm") && !text.contains("restart"), "{text}");
        assert!(text.contains("the cat"), "{text}");
    }
}

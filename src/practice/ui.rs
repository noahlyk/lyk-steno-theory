//! What is drawn: the word feed (and a quiet stats line), and the results screen. Nothing else:
//! no outlines, no keyboard, no hints.

use super::engine::{Results, Session};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use std::collections::HashMap;

const MAX_WIDTH: u16 = 80;
const LINES: usize = 3;
const DIM: Color = Color::DarkGray;

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

/// The live line: `47 wpm  98%  0:12 / 1:00`, or the words done out of the count.
#[must_use]
pub fn stats_line(session: &Session, now: f64, limit: Option<f64>) -> String {
    let r = session.results(now);
    let elapsed = r.secs;
    let clock = |s: f64| format!("{}:{:02}", (s / 60.0) as u64, (s % 60.0) as u64);
    let progress = limit.map_or_else(
        || format!("{}/{}", session.current(), session.words.len()),
        |l| format!("{} / {}", clock(elapsed), clock(l)),
    );
    format!("{:.0} wpm  {:.0}%  {progress}", r.wpm, r.accuracy)
}

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
    let height = LINES as u16 + if view.quiet { 0 } else { 2 };
    let top = area.y + area.height.saturating_sub(height) / 2;
    let x = area.x + area.width.saturating_sub(width) / 2;
    let block = Rect { x, y: top, width: width.min(area.width), height: height.min(area.height) };
    let parts = Layout::vertical([Constraint::Length(if view.quiet { 0 } else { 2 }), Constraint::Min(1)]).split(block);
    if !view.quiet {
        frame.render_widget(Paragraph::new(stats_line(s, view.now, view.limit)).style(Style::default().fg(DIM)).alignment(Alignment::Left), parts[0]);
    }
    frame.render_widget(Paragraph::new(rows), parts[1]);
}

/// The results as lines of text (the screen shows them, and they are printed after it closes).
/// `outlines` has a word's outline when `--outlines` found one.
#[must_use]
pub fn result_lines(r: &Results, source: &str, outlines: &HashMap<String, String>) -> Vec<String> {
    let outline = |w: &str| outlines.get(w).map_or_else(String::new, |o| format!("  {o}"));
    let mut out = vec![
        format!("{:.0} wpm   raw {:.0}   accuracy {:.1}%   words {:.1}%", r.wpm, r.raw, r.accuracy, r.word_accuracy),
        format!("errors {}   time {:.1}s   words {}   source {source}", r.errors, r.secs, r.words),
    ];
    out.push(r.consistency.map_or_else(|| "consistency  n/a (under three words)".to_owned(), |c| format!("consistency  {c:.0}%")));
    if !r.slow.is_empty() {
        out.push(String::new());
        out.push("slowest (seconds since the word before)".to_owned());
        for (w, t) in &r.slow {
            out.push(format!("  {w:<16}{t:>6.2}{}", outline(w)));
        }
    }
    if !r.misses.is_empty() {
        out.push(String::new());
        out.push("missed".to_owned());
        for m in &r.misses {
            out.push(format!("  {:<16}typed {}{}", m.word, if m.typed.is_empty() { "(nothing)" } else { &m.typed }, outline(&m.word)));
        }
    }
    out
}

pub fn draw_results(frame: &mut Frame, lines: &[String]) {
    let area = frame.area();
    let width = area.width.saturating_sub(4).clamp(10, MAX_WIDTH);
    let rows: Vec<Line> = lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let style = match i {
                0 => Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                1 | 2 => Style::default().fg(Color::Gray),
                _ if !l.starts_with(' ') && !l.is_empty() => Style::default().fg(Color::Yellow),
                _ => Style::default(),
            };
            Line::styled(l.clone(), style)
        })
        .collect();
    let x = area.x + area.width.saturating_sub(width) / 2;
    frame.render_widget(
        Paragraph::new(rows),
        Rect { x, y: area.y + 1, width: width.min(area.width), height: area.height.saturating_sub(2) },
    );
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
        assert_eq!(find("a", 0), Color::Red);
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
    fn timed_runs_show_a_clock() {
        let mut s = session(&["a", "b"]);
        s.apply(Input::Char('a'), 0.0);
        assert!(stats_line(&s, 12.0, Some(60.0)).ends_with("0:12 / 1:00"), "{}", stats_line(&s, 12.0, Some(60.0)));
    }

    #[test]
    fn the_results_name_no_outlines_unless_given() {
        let mut s = session(&["the", "cat", "sat"]);
        for (t, w) in [(1.0, "teh "), (3.0, "cat "), (4.0, "sat")] {
            for c in w.chars() {
                s.apply(Input::Char(c), t);
            }
        }
        let r = s.results(10.0);
        let none = result_lines(&r, "common", &HashMap::new()).join("\n");
        assert!(none.contains("missed") && none.contains("typed teh") && none.contains("slowest"), "{none}");
        assert!(!none.contains("-T"), "{none}");
        let with = result_lines(&r, "common", &HashMap::from([("the".to_owned(), "-T".to_owned())])).join("\n");
        assert!(with.contains("the") && with.contains("-T"), "{with}");
        let mut term = Terminal::new(TestBackend::new(60, 20)).unwrap();
        term.draw(|f| draw_results(f, &result_lines(&r, "common", &HashMap::new()))).unwrap();
        assert!(screen(&term).join("\n").contains("accuracy"), "results screen shows accuracy");
    }
}

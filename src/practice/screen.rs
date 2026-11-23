//! The results screen, after monkeytype's: big `wpm` and `acc` numbers, a chart of speed over the
//! test, a row of small stats, then panels for what the test changed (stumbled, mastered, new,
//! stuck, the learning set) and the level bars. One screen that fits 80x24: a larger terminal gets
//! a bigger chart and, from 100 columns, panels side by side; a smaller one loses the chart first,
//! then the lowest panels, and keeps `wpm`, `acc`, mastered, stumbled and the footer.

use super::report::{self, Heat, Screen};
use super::ui::RESULTS_HINT;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Chart, Dataset, GraphType, Paragraph};

/// Dim text: labels, axes, the footer.
const DIM: Color = Color::DarkGray;
/// The big numbers.
const BIG: Color = Color::White;
pub const RED: Color = Color::Red;
pub const GREEN: Color = Color::Green;
pub const YELLOW: Color = Color::Yellow;
/// New words.
pub const CYAN: Color = Color::Cyan;
/// Stuck words (xterm 208).
pub const ORANGE: Color = Color::Indexed(208);
/// Columns the labels of the panels take (`stumbled  `).
const LABEL: usize = 10;
/// Width of the big numbers' column.
const STATS_W: u16 = 16;
/// Narrowest terminal that gets the chart, and the least height.
const CHART_MIN: (u16, u16) = (76, 24);
/// Narrowest terminal with panels in two columns.
const TWO_COLUMNS: u16 = 100;

/// The 3x5 font: digits and `%`.
const fn glyph(c: char) -> [&'static str; 5] {
    match c {
        '0' => ["███", "█ █", "█ █", "█ █", "███"],
        '1' => [" █ ", "██ ", " █ ", " █ ", "███"],
        '2' => ["███", "  █", "███", "█  ", "███"],
        '3' => ["███", "  █", "███", "  █", "███"],
        '4' => ["█ █", "█ █", "███", "  █", "  █"],
        '5' => ["███", "█  ", "███", "  █", "███"],
        '6' => ["███", "█  ", "███", "█ █", "███"],
        '7' => ["███", "  █", "  █", "  █", "  █"],
        '8' => ["███", "█ █", "███", "█ █", "███"],
        '9' => ["███", "█ █", "███", "  █", "███"],
        '%' => ["█ █", "  █", " █ ", "█  ", "█ █"],
        _ => ["   "; 5],
    }
}

/// `text` in the big font: five rows, a blank column between glyphs.
#[must_use]
pub fn big(text: &str) -> [String; 5] {
    let mut rows: [String; 5] = Default::default();
    for (i, c) in text.chars().enumerate() {
        for (row, part) in rows.iter_mut().zip(glyph(c)) {
            if i > 0 {
                row.push(' ');
            }
            row.push_str(part);
        }
    }
    rows
}

/// A label over big numbers: six lines.
fn stat_block(label: &str, value: &str) -> Vec<Line<'static>> {
    let mut lines = vec![Line::styled(label.to_owned(), Style::default().fg(DIM))];
    lines.extend(big(value).into_iter().map(|r| Line::styled(r, Style::default().fg(BIG).add_modifier(Modifier::BOLD))));
    lines
}

/// The speed and accuracy as the big numbers show them.
#[must_use]
pub fn big_values(s: &Screen) -> (String, String) {
    (format!("{:.0}", s.wpm), format!("{:.0}%", s.accuracy))
}

/// A panel's lines: a label, then the items in order, wrapped at `width` with the label's width
/// kept as an indent.
fn panel(label: &str, color: Color, items: Vec<(String, Style)>, width: usize) -> Vec<Line<'static>> {
    let label_style = Style::default().fg(color).add_modifier(Modifier::BOLD);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut spans = vec![Span::styled(format!("{label:<LABEL$}"), label_style)];
    let mut used = LABEL;
    for (text, style) in items {
        let need = text.chars().count() + if used > LABEL { 2 } else { 0 };
        if used + need > width && used > LABEL {
            lines.push(Line::from(std::mem::take(&mut spans)));
            spans = vec![Span::raw(" ".repeat(LABEL))];
            used = LABEL;
        }
        if used > LABEL {
            spans.push(Span::raw("  "));
            used += 2;
        }
        used += text.chars().count();
        spans.push(Span::styled(text, style));
    }
    lines.push(Line::from(spans));
    lines
}

fn word_style(color: Color) -> Style {
    Style::default().fg(color)
}

/// The panels of a screen, each as its lines at `width`: stumbled, mastered, new, stuck, wordset.
struct Panels {
    stumbled: Vec<Line<'static>>,
    mastered: Vec<Line<'static>>,
    new: Vec<Line<'static>>,
    stuck: Vec<Line<'static>>,
    chips: Vec<Line<'static>>,
}

fn panels(s: &Screen, width: usize) -> Panels {
    let e = &s.extra;
    let words = |ws: &[String], color: Color| ws.iter().map(|w| (w.clone(), word_style(color))).collect::<Vec<_>>();
    let stumbled = if e.stumbled.is_empty() {
        Vec::new()
    } else {
        panel("stumbled", RED, e.stumbled.iter().map(|(w, n)| (format!("{}{}", report::counted(w, *n), s.outline(w)), word_style(RED))).collect(), width)
    };
    let done: Vec<String> = e.mastered.iter().chain(&e.retired).cloned().collect();
    let mastered = if done.is_empty() { Vec::new() } else { panel("mastered", GREEN, words(&done, GREEN), width) };
    let new = if e.new.is_empty() { Vec::new() } else { panel("new", CYAN, words(&e.new, CYAN), width) };
    let stuck = if e.stuck.is_empty() { Vec::new() } else { panel("stuck", ORANGE, words(&e.stuck, ORANGE), width) };
    let chips = if e.chips.is_empty() {
        Vec::new()
    } else {
        let color = |h: Heat| match h {
            Heat::Red => RED,
            Heat::Yellow => YELLOW,
            Heat::Green => GREEN,
        };
        panel("wordset", YELLOW, e.chips.iter().map(|c| (format!("{} {}/{}", c.word, c.clean, crate::vocab::WORD_WINDOW), word_style(color(c.heat)))).collect(), width)
    };
    Panels { stumbled, mastered, new, stuck, chips }
}

/// Keeps the first `rows` lines, with an ellipsis ending the last when some were cut.
fn clip(mut lines: Vec<Line<'static>>, rows: usize) -> Vec<Line<'static>> {
    if lines.len() > rows {
        lines.truncate(rows);
        if let Some(last) = lines.last_mut() {
            last.spans.push(Span::styled(" …", Style::default().fg(DIM)));
        }
    }
    lines
}

/// The small stats row, as `(label, value)` columns that fit `width`: `test` goes first when the
/// row is too wide, then `consistency`, then `raw`.
fn small_stats(s: &Screen, width: usize) -> Vec<(String, String)> {
    let mut cols = vec![
        ("test", s.test.clone()),
        ("raw", format!("{:.0}", s.raw)),
        ("characters", format!("{}/{}/{}/{}", s.chars.correct, s.chars.incorrect, s.chars.extra, s.chars.missed)),
        ("consistency", s.consistency.map_or_else(|| "n/a".to_owned(), |c| format!("{c:.0}%"))),
        ("time", report::clock(s.secs)),
    ];
    let wide = |c: &[(&str, String)]| c.iter().map(|(l, v)| l.chars().count().max(v.chars().count())).sum::<usize>() + 3 * c.len().saturating_sub(1);
    for drop in ["test", "consistency", "raw"] {
        if wide(&cols) <= width {
            break;
        }
        cols.retain(|(l, _)| *l != drop);
    }
    cols.into_iter().map(|(l, v)| (l.to_owned(), v)).collect()
}

fn small_stats_lines(cols: &[(String, String)]) -> Vec<Line<'static>> {
    let (mut labels, mut values) = (Vec::new(), Vec::new());
    for (i, (l, v)) in cols.iter().enumerate() {
        let w = l.chars().count().max(v.chars().count());
        let gap = if i + 1 < cols.len() { "   " } else { "" };
        labels.push(Span::styled(format!("{l:<w$}{gap}"), Style::default().fg(DIM)));
        values.push(Span::styled(format!("{v:<w$}{gap}"), Style::default().fg(Color::Gray)));
    }
    vec![Line::from(labels), Line::from(values)]
}

fn progress_lines(s: &Screen, width: usize) -> Vec<Line<'static>> {
    let Some(p) = &s.progress else { return Vec::new() };
    let mut lines = vec![Line::styled(report::progress_line_fit(p, width), Style::default().fg(Color::Gray))];
    if let Some(g) = &p.goal {
        lines.push(Line::styled(report::goal_line_fit(g, width), Style::default().fg(DIM)));
    }
    lines
}

/// The chart: speed and raw speed as lines, errors as red points, quiet axes.
fn draw_chart(frame: &mut Frame, area: Rect, s: &Screen) {
    let wpm = &s.series.wpm;
    if wpm.len() < 2 || area.width < 12 || area.height < 4 {
        return;
    }
    let top = s.series.wpm.iter().chain(&s.series.raw).map(|p| p.1).fold(10.0_f64, f64::max);
    let top = (top / 10.0).ceil() * 10.0;
    let last = wpm.len() as f64;
    let label = |t: String| Span::styled(t, Style::default().fg(DIM));
    let sets = vec![
        Dataset::default().marker(Marker::Braille).graph_type(GraphType::Line).style(Style::default().fg(Color::DarkGray)).data(&s.series.raw),
        Dataset::default().marker(Marker::Braille).graph_type(GraphType::Line).style(Style::default().fg(YELLOW)).data(&s.series.wpm),
        Dataset::default().marker(Marker::Dot).graph_type(GraphType::Scatter).style(Style::default().fg(RED)).data(&s.series.errors),
    ];
    let x = Axis::default().style(Style::default().fg(DIM)).bounds([1.0, last]).labels(vec![label("1s".into()), label(format!("{last:.0}s"))]);
    let y = Axis::default().style(Style::default().fg(DIM)).bounds([0.0, top]).labels(vec![label("0".into()), label(format!("{top:.0}"))]);
    frame.render_widget(Chart::new(sets).x_axis(x).y_axis(y), area);
}

/// A rectangle inside `area`, never outside it.
fn within(area: Rect, x: u16, y: u16, width: u16, height: u16) -> Rect {
    Rect { x, y, width, height }.intersection(area)
}

/// One band under the numbers: one column of lines, or two side by side.
struct Slot {
    cols: Vec<Vec<Line<'static>>>,
    /// Its columns share the row: the two-column layout
    paired: bool,
}

impl Slot {
    fn one(lines: Vec<Line<'static>>) -> Self {
        Self { cols: vec![lines], paired: false }
    }

    fn rows(&self) -> usize {
        self.cols.iter().map(Vec::len).max().unwrap_or(0)
    }
}

pub fn draw(frame: &mut Frame, scr: &Screen) {
    let area = frame.area();
    if area.width == 0 || area.height == 0 {
        return;
    }
    let margin = if area.width >= 84 { 2 } else { u16::from(area.width >= 40) };
    let inner = within(area, area.x + margin, area.y, area.width.saturating_sub(2 * margin), area.height);
    let width = usize::from(inner.width);
    // the footer, bottom row
    let body_h = if area.height >= 3 { area.height - 1 } else { area.height };
    if area.height >= 3 {
        frame.render_widget(Paragraph::new(RESULTS_HINT).style(Style::default().fg(DIM)), within(area, inner.x, area.y + area.height - 1, inner.width, 1));
    }
    let chart_mode = area.width >= CHART_MIN.0 && area.height >= CHART_MIN.1;
    let big_mode = inner.width >= 32 && body_h >= 8;
    let pad = u16::from(body_h >= 26);
    let top_h: u16 = if chart_mode {
        12 + ((area.height - CHART_MIN.1) / 2).min(6)
    } else if big_mode {
        6
    } else {
        1
    };
    let top_y = area.y + pad;
    let (wpm, acc) = big_values(scr);
    if chart_mode {
        frame.render_widget(Paragraph::new([stat_block("wpm", &wpm), stat_block("acc", &acc)].concat()), within(area, inner.x, top_y, STATS_W, 12));
        draw_chart(frame, within(area, inner.x + STATS_W + 2, top_y, inner.width.saturating_sub(STATS_W + 2), top_h), scr);
    } else if big_mode {
        frame.render_widget(Paragraph::new(stat_block("wpm", &wpm)), within(area, inner.x, top_y, STATS_W, 6));
        frame.render_widget(Paragraph::new(stat_block("acc", &acc)), within(area, inner.x + STATS_W + 2, top_y, STATS_W, 6));
    } else {
        let line = format!("wpm {wpm}   acc {acc}");
        frame.render_widget(Paragraph::new(line).style(Style::default().fg(BIG).add_modifier(Modifier::BOLD)), within(area, inner.x, top_y, inner.width, 1));
    }
    let below = usize::from(body_h.saturating_sub(pad + top_h));
    if below == 0 {
        return;
    }
    // What goes under the numbers: the slots in display order, the order they are given rows in
    // (the first is kept longest), and the most rows each may take.
    let two = inner.width >= TWO_COLUMNS;
    let col_w = if two { width.saturating_sub(2) / 2 } else { width };
    let pans = panels(scr, col_w);
    let full = panels(scr, width);
    let stats = Slot::one(small_stats_lines(&small_stats(scr, width)));
    let progress = Slot::one(progress_lines(scr, width));
    let extra_rows = usize::from(area.height.saturating_sub(24) / 3);
    let (mut slots, order, caps): (Vec<Slot>, Vec<usize>, Vec<usize>) = if two {
        (
            vec![stats, Slot { cols: vec![pans.stumbled, pans.mastered], paired: true }, Slot { cols: vec![pans.new, pans.stuck], paired: true }, Slot::one(full.chips), progress],
            vec![1, 0, 4, 2, 3],
            vec![2, 4, 3, 4 + extra_rows, 2],
        )
    } else {
        (
            vec![stats, Slot::one(pans.stumbled), Slot::one(pans.mastered), Slot::one(pans.new), Slot::one(pans.stuck), Slot::one(full.chips), progress],
            vec![2, 1, 0, 6, 3, 4, 5],
            vec![2, 3, 2, 2, 1, 3 + extra_rows, 2],
        )
    };
    let mut left = below;
    // one blank row under the numbers when there is room for it after everything else
    let wanted: usize = slots.iter().zip(&caps).map(|(sl, c)| sl.rows().min(*c)).sum();
    let gap = usize::from(left > wanted);
    left -= gap;
    let mut rows = vec![0; slots.len()];
    for &i in &order {
        rows[i] = slots[i].rows().min(caps[i]).min(left);
        left -= rows[i];
    }
    let mut row_y = top_y + top_h + u16::try_from(gap).unwrap_or(0);
    for (slot, n) in slots.iter_mut().zip(rows) {
        if n == 0 {
            continue;
        }
        let height = u16::try_from(n).unwrap_or(0);
        let paired = slot.paired;
        for (col, lines) in std::mem::take(&mut slot.cols).into_iter().enumerate() {
            let (left, span) = if paired { (inner.x + u16::try_from(col * (col_w + 2)).unwrap_or(0), u16::try_from(col_w).unwrap_or(0)) } else { (inner.x, inner.width) };
            frame.render_widget(Paragraph::new(clip(lines, n)), within(area, left, row_y, span, height));
        }
        row_y += height;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::practice::engine::{Chars, Series};
    use crate::practice::report::{Chip, Extra};
    use crate::practice::tiers::Progress;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::collections::HashMap;

    fn series(secs: usize) -> Series {
        let wpm: Vec<(f64, f64)> = (1..=secs).map(|i| (i as f64, ((i % 7) as f64).mul_add(5.0, 40.0))).collect();
        let raw: Vec<(f64, f64)> = (1..=secs).map(|i| (i as f64, ((i % 5) as f64).mul_add(9.0, 45.0))).collect();
        let errors = vec![(3.0, 60.0), (9.0, 70.0)];
        Series { wpm, raw, errors }
    }

    fn chips() -> Vec<Chip> {
        let heat = |n| report::heat(n, 0.9);
        [("find", 9), ("same", 6), ("such", 2), ("thing", 5), ("year", 0)].iter().map(|(w, n)| Chip { word: (*w).to_owned(), clean: *n, heat: heat(*n) }).collect()
    }

    fn screen(goal: bool) -> Screen {
        let extra = Extra {
            stumbled: vec![("find".into(), 3), ("could".into(), 1), ("every".into(), 2), ("thing".into(), 1)],
            mastered: vec!["about".into(), "other".into()],
            retired: vec![],
            new: vec!["people".into(), "years".into()],
            stuck: vec!["through".into()],
            chips: chips(),
        };
        let progress = Progress {
            level: 1,
            label: "monkeytype 200".into(),
            known: 104,
            size: 200,
            all_complete: false,
            next: Some("monkeytype 1k".into()),
            goal: goal.then(|| ("monkeytype 1k".to_owned(), 130, 1008)),
        };
        Screen {
            wpm: 84.0,
            raw: 91.0,
            accuracy: 97.0,
            word_accuracy: 92.0,
            consistency: Some(78.0),
            secs: 31.4,
            test: "25 words  monkeytype 200".into(),
            chars: Chars { correct: 118, incorrect: 3, extra: 0, missed: 0 },
            series: series(31),
            extra,
            progress: Some(progress),
            outlines: HashMap::new(),
            vocab_line: String::new(),
        }
    }

    fn render(scr: &Screen, w: u16, h: u16) -> Terminal<TestBackend> {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw(f, scr)).unwrap();
        term
    }

    fn rows(term: &Terminal<TestBackend>) -> Vec<String> {
        let buf = term.backend().buffer();
        (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_owned()).collect::<String>().trim_end().to_owned()).collect()
    }

    /// The colour of the first cell that starts `text` on the screen.
    fn colour_of(term: &Terminal<TestBackend>, text: &str) -> Option<Color> {
        let buf = term.backend().buffer();
        for y in 0..buf.area.height {
            let joined: String = (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect();
            if let Some(at) = joined.find(text) {
                let col = joined[..at].chars().count();
                return Some(buf[(u16::try_from(col).unwrap(), y)].fg);
            }
        }
        None
    }

    #[test]
    fn the_big_font_draws_digits_and_percent() {
        assert_eq!(big("84")[0], "███ █ █");
        assert_eq!(big("0")[2], "█ █");
        for d in "0123456789%".chars() {
            assert!(glyph(d).iter().all(|r| r.chars().count() == 3), "{d}");
        }
        assert!(big("100").iter().all(|r| r.chars().count() == 11));
        assert_eq!(big("").len(), 5);
    }

    #[test]
    fn at_80x24_one_screen_has_the_numbers_the_chart_and_every_panel_that_fits() {
        let term = render(&screen(true), 80, 24);
        let text = rows(&term);
        let all = text.join("\n");
        eprintln!("{all}");
        assert_eq!(text.len(), 24);
        assert!(all.contains("wpm") && all.contains("acc"), "{all}");
        assert!(all.contains("█ █") && all.contains("███"), "big digits: {all}");
        assert!(all.contains("characters") && all.contains("118/3/0/0") && all.contains("consistency") && all.contains("78%"), "{all}");
        assert!(all.contains("25 words  monkeytype 200"), "{all}");
        assert!(all.contains("stumbled") && all.contains("find×3") && all.contains("every×2") && all.contains("could"), "{all}");
        assert!(all.contains("mastered") && all.contains("about"), "{all}");
        assert!(all.contains("lv1  monkeytype 200  [█████░░░░░] 52%  104/200 known   next: monkeytype 1k"), "{all}");
        assert!(text.last().is_some_and(|r| r.trim() == RESULTS_HINT), "the footer is the last row: {all}");
        assert!(all.chars().any(|c| ('\u{2800}'..='\u{28ff}').contains(&c)), "the chart is drawn in braille: {all}");
    }

    #[test]
    fn panel_colours_are_red_green_yellow_cyan_and_orange() {
        let term = render(&screen(true), 100, 30);
        let all = rows(&term).join("\n");
        eprintln!("{all}");
        assert_eq!(colour_of(&term, "find×3"), Some(RED));
        assert_eq!(colour_of(&term, "about"), Some(GREEN));
        assert_eq!(colour_of(&term, "people"), Some(CYAN));
        assert_eq!(colour_of(&term, "through"), Some(ORANGE));
        // the learning set: green at 9 of 10, yellow between, red under 30%
        assert_eq!(colour_of(&term, "find 9/10"), Some(GREEN));
        assert_eq!(colour_of(&term, "same 6/10"), Some(YELLOW));
        assert_eq!(colour_of(&term, "such 2/10"), Some(RED));
        assert_eq!(colour_of(&term, "year 0/10"), Some(RED));
        assert!(all.contains("goal  monkeytype 1k"), "{all}");
        assert!(all.contains("wordset"), "{all}");
    }

    #[test]
    fn a_larger_terminal_gets_a_bigger_chart_and_panels_side_by_side() {
        let term = render(&screen(true), 120, 40);
        let text = rows(&term);
        let all = text.join("\n");
        eprintln!("{all}");
        assert!(text.iter().any(|r| r.contains("stumbled") && r.contains("mastered")), "stumbled and mastered share a row: {all}");
        assert!(text.iter().any(|r| r.contains("new") && r.contains("stuck")), "{all}");
        // the chart is taller than at 80x24: its axis label sits lower than row 12
        let axis = text.iter().position(|r| r.contains("31s")).expect("x axis");
        assert!(axis > 12, "axis at row {axis}");
        assert!(text.last().is_some_and(|r| r.trim() == RESULTS_HINT));
    }

    #[test]
    fn a_small_terminal_drops_the_chart_then_the_wordset_and_keeps_the_essentials() {
        let term = render(&screen(true), 60, 20);
        let text = rows(&term);
        let all = text.join("\n");
        eprintln!("{all}");
        assert_eq!(text.len(), 20);
        assert!(!all.chars().any(|c| ('\u{2800}'..='\u{28ff}').contains(&c)), "no chart: {all}");
        assert!(all.contains("wpm") && all.contains("acc"), "{all}");
        assert!(all.contains("stumbled") && all.contains("mastered"), "{all}");
        assert!(text.last().is_some_and(|r| r.trim() == RESULTS_HINT), "{all}");
        assert!(text.iter().all(|r| r.chars().count() <= 60), "{all}");
        assert!(all.contains("lv1  monkeytype 200") && !all.contains("known   next: m\n"), "the progress line is shortened, not cut: {all}");
    }

    #[test]
    fn nothing_draws_outside_the_frame_at_any_size() {
        for goal in [false, true] {
            let scr = screen(goal);
            for w in [1, 3, 10, 20, 31, 40, 59, 60, 75, 76, 80, 99, 100, 120, 200] {
                for h in [1, 2, 3, 5, 8, 12, 14, 19, 23, 24, 25, 26, 30, 40, 60] {
                    let term = render(&scr, w, h);
                    assert_eq!((term.backend().buffer().area.width, term.backend().buffer().area.height), (w, h));
                }
            }
        }
        // an empty test: nothing typed, no series, no panels
        let mut empty = screen(false);
        empty.series = Series::default();
        empty.extra = Extra::default();
        empty.progress = None;
        let all = rows(&render(&empty, 80, 24)).join("\n");
        assert!(all.contains("wpm") && !all.contains("stumbled"), "{all}");
    }

    #[test]
    fn the_footer_is_the_same_text_everywhere_there_is_room_for_it() {
        assert_eq!(RESULTS_HINT, "tab/enter again  q/esc quit");
        for (w, h) in [(80, 24), (60, 20), (120, 40), (40, 8)] {
            let text = rows(&render(&screen(false), w, h));
            assert!(text.last().is_some_and(|r| r.trim() == RESULTS_HINT), "{w}x{h}: {text:?}");
        }
    }

    #[test]
    fn panels_wrap_and_long_ones_end_in_an_ellipsis_when_cut() {
        let mut scr = screen(false);
        scr.extra.stumbled = (0..60).map(|i| (format!("word{i}"), 1)).collect();
        let text = rows(&render(&scr, 80, 24));
        let all = text.join("\n");
        assert!(all.contains("stumbled  word0") && all.contains("…"), "{all}");
        // continuation lines are indented under the items
        let cont = text.iter().skip_while(|r| !r.contains("stumbled")).nth(1).expect("a second line");
        assert!(cont.trim_start().starts_with("word"), "{all}");
    }
}

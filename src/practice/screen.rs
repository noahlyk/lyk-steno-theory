//! The results screen, after monkeytype's: big `wpm` and `acc` numbers, a chart of speed over the
//! test, a row of small stats, then panels for what the test changed (stumbled, mastered, new,
//! stuck, the learning set) and the level bars. One screen that fits 80x24: a larger terminal gets
//! a bigger chart; the block is at most 104 columns wide and centred, one panel to a row; a smaller
//! terminal loses the chart first,
//! then the lowest panels, and keeps `wpm`, `acc`, mastered, stumbled and the footer.

use super::report::{self, Heat, Screen};
use super::tiers;
use super::ui::RESULTS_HINT;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Chart, Dataset, GraphType, Paragraph};

/// Dim text: labels, axes, the footer.
const DIM: Color = Color::Rgb(98, 104, 122);
/// Body text: soft white, never pure.
const TEXT: Color = Color::Rgb(205, 208, 218);
/// The big numbers.
const BIG: Color = TEXT;
pub const RED: Color = Color::Rgb(218, 125, 138);
pub const GREEN: Color = Color::Rgb(140, 190, 158);
pub const YELLOW: Color = Color::Rgb(224, 190, 118);
/// New words.
pub const CYAN: Color = Color::Rgb(125, 180, 202);
/// Stuck words (xterm 208).
pub const ORANGE: Color = Color::Rgb(214, 152, 100);
/// Columns the labels of the panels take (`stumbled  `).
const LABEL: usize = 10;
/// Width of the big numbers' column.
const STATS_W: u16 = 16;

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
fn stat_block(label: &str, value: &str, color: Color) -> Vec<Line<'static>> {
    let mut lines = vec![Line::styled(label.to_owned(), Style::default().fg(DIM))];
    lines.extend(big(value).into_iter().map(|r| Line::styled(r, Style::default().fg(color).add_modifier(Modifier::BOLD))));
    lines
}

/// The speed and accuracy as the big numbers show them.
#[must_use]
pub fn big_values(s: &Screen) -> (String, String) {
    (format!("{:.0}", s.wpm), format!("{:.0}%", s.accuracy))
}

/// The grid cell for these items: the widest plus three columns of gap.
fn cell_of<'a>(texts: impl Iterator<Item = &'a String>) -> usize {
    texts.map(|t| t.chars().count()).max().unwrap_or(0) + 3
}

/// A panel's lines: the label, then the items in an aligned grid (every cell as wide as the widest
/// item), the label's width kept as an indent on the later rows.
fn panel(label: &str, color: Color, items: &[(String, Style)], width: usize, cell: usize) -> Vec<Line<'static>> {
    let label_style = Style::default().fg(color);
    let per = (width.saturating_sub(LABEL) / cell).max(1);
    items
        .chunks(per)
        .enumerate()
        .map(|(row, chunk)| {
            let head = if row == 0 { Span::styled(format!("{label:<LABEL$}"), label_style) } else { Span::raw(" ".repeat(LABEL)) };
            let mut spans = vec![head];
            spans.extend(chunk.iter().map(|(t, st)| Span::styled(format!("{t:<cell$}"), *st)));
            Line::from(spans)
        })
        .collect()
}

fn word_style(color: Color) -> Style {
    Style::default().fg(color)
}

/// The word panels of a screen, each as its lines at `width`: stumbled, mastered, new, stuck.
struct Panels {
    stumbled: Vec<Line<'static>>,
    mastered: Vec<Line<'static>>,
    new: Vec<Line<'static>>,
    stuck: Vec<Line<'static>>,
}

fn panels(s: &Screen, width: usize) -> Panels {
    let e = &s.extra;
    let stumbled: Vec<(String, Style)> = e.stumbled.iter().map(|(w, n)| (format!("{}{}", report::counted(w, *n), s.outline(w)), word_style(RED))).collect();
    let done: Vec<(String, Style)> = e.mastered.iter().chain(&e.retired).map(|w| (w.clone(), word_style(GREEN))).collect();
    let words = |ws: &[String], color: Color| ws.iter().map(|w| (w.clone(), word_style(color))).collect::<Vec<_>>();
    let new = words(&e.new, CYAN);
    let stuck = words(&e.stuck, ORANGE);
    // one column width for all four, so their words line up
    let cell = cell_of([&stumbled, &done, &new, &stuck].into_iter().flatten().map(|(t, _)| t));
    let make = |label: &str, color: Color, items: Vec<(String, Style)>| if items.is_empty() { Vec::new() } else { panel(label, color, &items, width, cell) };
    Panels { stumbled: make("stumbled", RED, stumbled), mastered: make("mastered", GREEN, done), new: make("new", CYAN, new), stuck: make("stuck", ORANGE, stuck) }
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
        values.push(Span::styled(format!("{v:<w$}{gap}"), Style::default().fg(TEXT)));
    }
    vec![Line::from(labels), Line::from(values)]
}

fn progress_lines(s: &Screen, width: usize) -> Vec<Line<'static>> {
    let Some(p) = &s.progress else { return Vec::new() };
    let mut lines = vec![Line::styled(report::progress_line_fit(p, width), Style::default().fg(TEXT))];
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

/// The accuracy's colour: green from 97%, yellow from 90%, red under.
fn acc_colour(acc: f64) -> Color {
    if acc >= 97.0 {
        GREEN
    } else if acc >= 90.0 {
        YELLOW
    } else {
        RED
    }
}

/// The learning set as a grid: aligned cells, the label on the first row, colour by progress.
fn chip_lines(s: &Screen, width: usize) -> Vec<Line<'static>> {
    let chips = &s.extra.chips;
    if chips.is_empty() {
        return Vec::new();
    }
    let color = |h: Heat| match h {
        Heat::Red => RED,
        Heat::Yellow => YELLOW,
        Heat::Green => GREEN,
    };
    let items: Vec<(String, Style)> = chips.iter().map(|c| (format!("{} {}/{}", c.word, c.clean, crate::vocab::WORD_WINDOW), word_style(color(c.heat)))).collect();
    let cell = cell_of(items.iter().map(|(t, _)| t));
    panel("wordset", YELLOW, &items, width, cell)
}

/// One bar: `filled` green-ish blocks of `width`, the rest dim.
fn bar(percent: usize, width: usize, color: Color) -> Vec<Span<'static>> {
    let filled = (percent.min(100) * width + 50) / 100;
    vec![Span::styled("━".repeat(filled), Style::default().fg(color)), Span::styled("─".repeat(width - filled), Style::default().fg(DIM))]
}

/// The level and goal as aligned bars: name, bar, percent and counts (and the next level when
/// there is room). None when the width is too small for a bar; the caller then uses the text lines.
fn bar_lines(s: &Screen, width: usize) -> Option<Vec<Line<'static>>> {
    let p = s.progress.as_ref()?;
    let level = format!("lv{}  {}", p.level, p.label);
    let goal = p.goal.as_ref().map(|(label, known, size)| (format!("goal  {label}"), *known, *size));
    let name_w = level.chars().count().max(goal.as_ref().map_or(0, |g| g.0.chars().count())) + 2;
    let tail = |known: usize, size: usize| format!("{:>3}%  {known}/{size}", tiers::percent(known, size));
    let tail_w = tail(p.known, p.size).chars().count().max(goal.as_ref().map_or(0, |g| tail(g.1, g.2).chars().count()));
    let next = (!p.all_complete).then(|| p.next.as_ref().map(|n| format!("next  {n}"))).flatten();
    let room = width.checked_sub(name_w + tail_w + 2)?;
    let (bar_w, next) = match next {
        Some(n) if room >= 12 + n.chars().count() + 3 => (24.min(room - n.chars().count() - 3), Some(n)),
        _ => (24.min(room), None),
    };
    if bar_w < 6 {
        return None;
    }
    let dim = Style::default().fg(DIM);
    let row = |name: String, known: usize, size: usize, color: Color, next: Option<&String>| {
        let mut spans = vec![Span::styled(format!("{name:<name_w$}"), Style::default().fg(TEXT))];
        spans.extend(bar(tiers::percent(known, size), bar_w, color));
        spans.push(Span::styled(format!("  {:<tail_w$}", tail(known, size)), Style::default().fg(TEXT)));
        if let Some(n) = next {
            spans.push(Span::styled(format!("   {n}"), dim));
        }
        Line::from(spans)
    };
    let mut lines = vec![row(level, p.known, p.size, GREEN, next.as_ref())];
    if let Some((name, known, size)) = goal {
        lines.push(row(name, known, size, CYAN, None));
    }
    Some(lines)
}

/// The widest block of the screen: a wider terminal centres it rather than stretching it.
const MAX_W: u16 = 104;
/// The least height for the tall layout: the numbers stacked on the left, the chart beside them.
const TALL: u16 = 36;

pub fn draw(frame: &mut Frame, scr: &Screen) {
    let area = frame.area();
    if area.width == 0 || area.height == 0 {
        return;
    }
    let margin = if area.width >= 84 { 2 } else { u16::from(area.width >= 40) };
    let avail = area.width.saturating_sub(2 * margin);
    let inner_w = avail.min(MAX_W);
    let inner = within(area, area.x + margin + (avail - inner_w) / 2, area.y, inner_w, area.height);
    let width = usize::from(inner.width);
    // Under the numbers, in groups: the small stats; what the test changed; the learning set; the
    // bars. Rows go to the groups in the order of `order` (the first is kept longest), then any
    // left over become blank rows between the groups, top down.
    let extra = usize::from(area.height.saturating_sub(24) / 4);
    let cap = |lines: Vec<Line<'static>>, n: usize| clip(lines, n);
    let pans = panels(scr, width);
    let words: Vec<Line<'static>> = [cap(pans.stumbled, 3 + extra), cap(pans.mastered, 2), cap(pans.new, 2), cap(pans.stuck, 2)].concat();
    let bars = progress_lines_aligned(scr, width);
    let mut groups: Vec<Vec<Line<'static>>> = vec![small_stats_lines(&small_stats(scr, width)), words, chip_lines(scr, width), bars];
    // The block is as wide as its widest row (at least 72 columns for the chart), centred: the
    // margins left and right are equal.
    let wide = u16::try_from(groups.iter().flatten().map(Line::width).max().unwrap_or(0)).unwrap_or(u16::MAX);
    let block_w = wide.max(72.min(inner.width)).min(inner.width);
    let inner = Rect { x: inner.x + (inner.width - block_w) / 2, width: block_w, ..inner };
    // the footer, bottom row
    let body_h = if area.height >= 3 { area.height - 1 } else { area.height };
    if area.height >= 3 {
        frame.render_widget(Paragraph::new(RESULTS_HINT).style(Style::default().fg(DIM)), within(area, inner.x, area.y + area.height - 1, inner.width, 1));
    }
    let tall = inner.width >= 60 && area.height >= TALL;
    let chart_beside = !tall && inner.width >= 60 && area.height >= 20;
    let big_mode = inner.width >= 32 && body_h >= 8;
    let pad = u16::from(body_h >= 26);
    let top_h: u16 = if tall {
        12 + ((area.height - TALL) / 3).min(4)
    } else if chart_beside || big_mode {
        if chart_beside { 7 + (area.height.saturating_sub(24) / 2).min(5) } else { 6 }
    } else {
        1
    };
    // Rows under the numbers: to the groups first, then one separator row before each group while
    // rows are left; what is still left over is split above and below, so the block is centred.
    let below = usize::from(body_h.saturating_sub(pad + top_h));
    let order = [1, 3, 0, 2];
    let caps = [2, usize::MAX, 2 + extra, 2];
    let mut left = below;
    let mut rows = [0usize; 4];
    for &i in &order {
        rows[i] = groups[i].len().min(caps[i]).min(left);
        left -= rows[i];
    }
    let mut gaps = [false; 4];
    for (gap, n) in gaps.iter_mut().zip(rows) {
        if n > 0 && left > 0 {
            *gap = true;
            left -= 1;
        }
    }
    let top_y = area.y + pad + u16::try_from(left / 2).unwrap_or(0);
    let (wpm, acc) = big_values(scr);
    let wpm_block = stat_block("wpm", &wpm, TEXT);
    let acc_block = stat_block("acc", &acc, acc_colour(scr.accuracy));
    if tall {
        frame.render_widget(Paragraph::new([wpm_block, acc_block].concat()), within(area, inner.x, top_y, STATS_W, 12));
        draw_chart(frame, within(area, inner.x + STATS_W + 2, top_y, inner.width.saturating_sub(STATS_W + 2), top_h), scr);
    } else if big_mode {
        frame.render_widget(Paragraph::new(wpm_block), within(area, inner.x, top_y, STATS_W, 6));
        frame.render_widget(Paragraph::new(acc_block), within(area, inner.x + STATS_W, top_y, STATS_W, 6));
        if chart_beside {
            let x = inner.x + 2 * STATS_W + 2;
            draw_chart(frame, within(area, x, top_y, (inner.x + inner.width).saturating_sub(x), top_h), scr);
        }
    } else {
        let line = format!("wpm {wpm}   acc {acc}");
        frame.render_widget(Paragraph::new(line).style(Style::default().fg(BIG).add_modifier(Modifier::BOLD)), within(area, inner.x, top_y, inner.width, 1));
    }
    let mut row_y = top_y + top_h;
    let titles = [None, Some("this test"), Some("learning"), Some("level")];
    for (g, (lines, n)) in groups.iter_mut().zip(rows).enumerate() {
        if n == 0 {
            continue;
        }
        if gaps[g] {
            if let Some(title) = titles[g] {
                let rule = format!("── {title} ");
                let fill = usize::from(inner.width).saturating_sub(rule.chars().count());
                frame.render_widget(Paragraph::new(format!("{rule}{}", "─".repeat(fill))).style(Style::default().fg(DIM)), within(area, inner.x, row_y, inner.width, 1));
            }
            row_y += 1;
        }
        let height = u16::try_from(n).unwrap_or(0);
        frame.render_widget(Paragraph::new(clip(std::mem::take(lines), n)), within(area, inner.x, row_y, inner.width, height));
        row_y += height;
    }
}

/// The bars when there is room for them, the shortened text lines otherwise.
fn progress_lines_aligned(s: &Screen, width: usize) -> Vec<Line<'static>> {
    bar_lines(s, width).unwrap_or_else(|| progress_lines(s, width))
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
        assert!(all.contains("lv1  monkeytype 200") && all.contains("52%  104/200") && all.contains("next  monkeytype 1k"), "{all}");
        assert!(all.contains('━') && all.contains('─'), "the bar: {all}");
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
    fn a_larger_terminal_gets_the_tall_layout_one_panel_to_a_row_and_a_centred_block() {
        let term = render(&screen(true), 120, 40);
        let text = rows(&term);
        let all = text.join("\n");
        eprintln!("{all}");
        let has = |w: &str| text.iter().filter(|r| r.trim_start().starts_with(w)).count();
        assert!(has("stumbled") == 1 && has("mastered") == 1 && has("new") == 1 && has("stuck") == 1, "one row each, never side by side: {all}");
        assert!(!text.iter().any(|r| r.contains("stumbled") && r.contains("mastered")), "{all}");
        // wider than the cap, the block is centred, not stretched
        let wide = rows(&render(&screen(true), 200, 40));
        let left = wide.iter().find(|r| r.contains("stumbled")).map_or(0, |r| r.len() - r.trim_start().len());
        assert!(left >= 40 && wide.iter().all(|r| r.chars().count() <= 200), "centred: left {left}");
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

    #[test]
    fn spare_rows_are_split_above_and_below_so_the_block_is_vertically_centred() {
        for (w, h) in [(120, 50), (100, 44), (140, 60)] {
            let text = rows(&render(&screen(true), w, h));
            let used: Vec<usize> = text.iter().enumerate().filter(|(i, r)| !r.is_empty() && *i + 1 < text.len()).map(|(i, _)| i).collect();
            let (first, last) = (used[0], used[used.len() - 1]);
            let above = first;
            let below = text.len() - 2 - last; // the footer is the last row
            assert!(above.abs_diff(below) <= 1, "{w}x{h}: {above} rows above, {below} below");
        }
    }
}

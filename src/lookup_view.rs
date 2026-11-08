//! What `lookup` (and `resolve`) shows for a query, as data: a list of [`Line`]s that
//! [`render`] turns into text, plain for a pipe or a file, coloured for a terminal. The one-shot
//! command and the interactive session ([`crate::session`]) share this, so a query typed live
//! and the same query given as an argument print the same lines. Mirrors keymux's
//! `steno lookup` / `steno keys`, except that every row has all three theories (lyk, Plover,
//! Lapwing) where keymux resolves the one engine setup.

use crate::analysis::Setup;
use crate::generate::{self, Entry};
use crate::reverse::{self, Query, WordLookup};
use crate::stroke::{self, Stroke, render_outline};
use std::collections::HashMap;

/// How many other words an interactive query lists under the exact answer.
pub const MATCHES: usize = 8;

/// One line of a lookup view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// A word of the query, heading its three theory rows
    Word(String),
    /// `  lyk       KAT`: a theory's label (padded to `width`) and its outline, `None` for no entry
    Entry { label: String, width: usize, outline: Option<String> },
    /// `KAT  ->  cat  (in the dictionary)`
    Resolve { outline: String, result: String },
    /// A plain remark
    Note(String),
    /// A dim heading over the matches table
    Heading(String),
    /// A row of the matches table: word, lyk, plover, lapwing (the header row when `header`)
    Cells { cells: [String; 4], widths: [usize; 4], header: bool },
}

/// The built dictionary, indexed for lookups. Built once per command; cheap enough to ask per
/// keystroke.
pub struct Lookup<'a> {
    setup: &'a Setup,
    entries: &'a [Entry],
    dictionary: HashMap<&'a str, &'a [Stroke]>,
    by_outline: HashMap<String, &'a str>,
    stems: HashMap<Stroke, String>,
    /// Every word with an outline, most common first
    ranked: Vec<&'a str>,
}

impl<'a> Lookup<'a> {
    pub fn new(setup: &'a Setup, entries: &'a [Entry]) -> Self {
        let dictionary = generate::cheapest(entries);
        let mut ranked: Vec<&str> = dictionary.keys().copied().collect();
        ranked.sort_by_key(|w| (setup.rank.get(*w).copied().unwrap_or(usize::MAX), *w));
        Self {
            setup,
            entries,
            dictionary,
            by_outline: generate::index_of(entries),
            stems: generate::single_stroke_stems_of(entries),
            ranked,
        }
    }

    pub const fn dictionary(&self) -> &HashMap<&'a str, &'a [Stroke]> {
        &self.dictionary
    }

    /// What a stroke outline resolves to: a real entry, the rules' best reading of a miss, or a
    /// true miss. Shared by `resolve` and the stroke half of `lookup`.
    #[must_use]
    pub fn resolve(&self, strokes: &[Stroke]) -> Line {
        let outline = render_outline(strokes);
        let result = self.by_outline.get(&outline).map_or_else(
            || {
                self.setup.rules.resolve(strokes, Some(&self.stems)).map_or_else(
                    || "no reading the rules allow: a true miss".to_owned(),
                    |reading| format!("{reading}  (not in the dictionary; the rules' best reading)"),
                )
            },
            |word| format!("{word}  (in the dictionary)"),
        );
        Line::Resolve { outline, result }
    }

    /// The three theory rows of one word: see [`reverse::reverse_word`].
    #[must_use]
    pub fn word_lines(found: &WordLookup) -> Vec<Line> {
        let lyk_label = match (&found.composed_of, found.lyk_deployed) {
            (_, true) => "lyk".to_owned(),
            (Some(pieces), false) => format!("lyk (composed: {})", pieces.join(" + ")),
            (None, false) => "lyk (not deployed - rules' own reading)".to_owned(),
        };
        let label = |name: &str, pieces: &Option<Vec<String>>| {
            pieces.as_ref().map_or_else(|| name.to_owned(), |p| format!("{name} (composed: {})", p.join(" + ")))
        };
        let plover_label = label("plover", &found.plover_composed_of);
        let lapwing_label = label("lapwing", &found.lapwing_composed_of);
        let width = lyk_label.len().max(plover_label.len()).max(lapwing_label.len());
        let entry = |label: String, outline: &Option<String>| Line::Entry { label, width, outline: outline.clone() };
        vec![
            Line::Word(found.word.clone()),
            entry(lyk_label, &found.lyk),
            entry(plover_label, &found.plover),
            entry(lapwing_label, &found.lapwing),
        ]
    }

    /// The view of a query exactly as the one-shot `lookup` prints it. `force_stroke` is
    /// `resolve`: the query must be an outline, and is not one if it does not parse.
    #[must_use]
    pub fn view(&self, query: &str, force_stroke: bool) -> Vec<Line> {
        if force_stroke {
            return stroke::parse_outline(query.trim()).map_or_else(
                || vec![Line::Note(format!("{:?} is not a stroke outline", query.trim()))],
                |strokes| vec![self.resolve(&strokes)],
            );
        }
        match reverse::classify(query) {
            Query::Stroke(strokes) => vec![self.resolve(&strokes)],
            Query::Text(phrase) if phrase.is_empty() => vec![Line::Note("Nothing to look up".to_owned())],
            Query::Text(phrase) => reverse::reverse_phrase(self.setup, &self.dictionary, &phrase)
                .iter()
                .flat_map(Self::word_lines)
                .collect(),
        }
    }

    /// The live view: what [`Self::view`] gives, plus the words that start like the last word
    /// typed (or whose lyk outline starts like a typed outline), three theories in columns.
    /// A last word that is only the start of other words is not looked up as if it were whole.
    #[must_use]
    pub fn live_view(&self, query: &str, force_stroke: bool) -> Vec<Line> {
        let stroke_query = force_stroke || matches!(reverse::classify(query), Query::Stroke(_));
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }
        if stroke_query {
            let mut lines = self.view(query, force_stroke);
            let rows = self.outline_matches(trimmed);
            lines.extend(self.table(&format!("outlines starting {trimmed}"), &rows));
            return lines;
        }
        let last = trimmed.rsplit(' ').next().unwrap_or(trimmed).to_lowercase();
        let rows: Vec<&str> = self.word_matches(&last);
        let exact = self.dictionary.contains_key(last.as_str());
        let mut lines = if exact || rows.is_empty() {
            self.view(query, false)
        } else {
            // Only a prefix of something: show the earlier words whole, the last as its matches
            let head = trimmed[..trimmed.len() - last.len()].trim();
            if head.is_empty() { Vec::new() } else { self.view(head, false) }
        };
        let others: Vec<(&str, String)> = rows
            .into_iter()
            .filter(|w| *w != last)
            .take(MATCHES)
            .map(|w| (w, self.dictionary.get(w).map_or_else(|| "?".to_owned(), |o| render_outline(o))))
            .collect();
        lines.extend(self.table(&format!("words starting {last:?}"), &others));
        lines
    }

    /// The commonest words that start with `prefix`.
    fn word_matches(&self, prefix: &str) -> Vec<&'a str> {
        if prefix.is_empty() {
            return Vec::new();
        }
        self.ranked.iter().copied().filter(|w| w.starts_with(prefix)).take(MATCHES + 1).collect()
    }

    /// The commonest words with an outline that starts with `prefix` (other than exactly it),
    /// each with that outline.
    fn outline_matches(&self, prefix: &str) -> Vec<(&'a str, String)> {
        let mut found: Vec<(usize, &str, String)> = self
            .entries
            .iter()
            .filter_map(|e| {
                let text = render_outline(&e.outline);
                (text != prefix && text.starts_with(prefix))
                    .then(|| (self.setup.rank.get(&e.word).copied().unwrap_or(usize::MAX), e.word.as_str(), text))
            })
            .collect();
        found.sort_unstable();
        found.dedup();
        found.into_iter().map(|(_, w, o)| (w, o)).take(MATCHES).collect()
    }

    /// A word / lyk / plover / lapwing table, with its heading; empty for no rows.
    fn table(&self, heading: &str, words: &[(&str, String)]) -> Vec<Line> {
        if words.is_empty() {
            return Vec::new();
        }
        let mut rows: Vec<[String; 4]> = vec![["word", "lyk", "plover", "lapwing"].map(str::to_owned)];
        for (word, lyk) in words {
            let (plover, lapwing) = self.setup.gold.shortest(word);
            rows.push([
                (*word).to_owned(),
                lyk.clone(),
                plover.unwrap_or("?").to_owned(),
                lapwing.unwrap_or("?").to_owned(),
            ]);
        }
        let widths: [usize; 4] = std::array::from_fn(|c| rows.iter().map(|r| r[c].chars().count()).max().unwrap_or(0));
        let mut lines = vec![Line::Heading(heading.to_owned())];
        lines.extend(rows.into_iter().enumerate().map(|(i, cells)| Line::Cells { cells, widths, header: i == 0 }));
        lines
    }
}

/// How a view is drawn.
#[derive(Debug, Clone, Copy, Default)]
pub struct Style {
    /// Colour the outlines by finger and dim the labels
    pub colour: bool,
    /// Cut a line to this many columns
    pub columns: Option<usize>,
}

/// Whether stdout can show colour: a terminal, `NO_COLOR` unset, `TERM` not `dumb`.
#[must_use]
pub fn terminal_colour() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal()
        && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
        && std::env::var("TERM").map_or(true, |t| t != "dumb")
}

const RESET: &str = "\x1b[0m";
const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";

/// The SGR colour of a key's finger, by bit: pinky red, ring yellow, middle green, index cyan,
/// thumbs magenta. The same hands mirror, as on the keyboard.
const fn finger_colour(bit: u32) -> &'static str {
    match bit {
        1 | 19..=22 => "31",
        2 | 3 | 17 | 18 => "33",
        4 | 5 | 15 | 16 => "32",
        6 | 7 | 10 | 13 | 14 => "36",
        _ => "35",
    }
}

/// The finger pairs: two keys one finger presses together (`TK`, `PW`, `HR`, `-FR`, `-PB`, `-LG`,
/// `-TS`, `-DZ`) and the thumbs' `AO`, `EU`. Both keys of a pair held at once are underlined.
const PAIRS: [(u32, u32); 10] = [(2, 3), (4, 5), (6, 7), (8, 9), (11, 12), (13, 14), (15, 16), (17, 18), (19, 20), (21, 22)];

/// `outline` with each key in its finger's colour and finger pairs underlined: plain text when
/// it does not parse as an outline.
#[must_use]
pub fn paint_outline(outline: &str) -> String {
    const KEYS: &[u8; 23] = b"#STKPWHRAO*EUFRPBLGTSDZ";
    let Some(strokes) = stroke::parse_outline(outline) else {
        return outline.to_owned();
    };
    let mut out = String::new();
    for (i, stroke) in strokes.iter().enumerate() {
        if i > 0 {
            out.push('/');
        }
        let bits = stroke.0;
        if bits & Stroke::JOIN != 0 {
            let _ = std::fmt::Write::write_fmt(&mut out, format_args!("{DIM}+{RESET}"));
        } else if bits & Stroke::SPELL != 0 {
            let _ = std::fmt::Write::write_fmt(&mut out, format_args!("{DIM}~{RESET}"));
        }
        // As `Stroke`'s Display: a stroke with no vowel or star writes a hyphen before its right keys
        let mut hyphen = bits & 0x1F00 == 0;
        for (index, &key) in (0u32..).zip(KEYS.iter()) {
            if bits & (1 << index) == 0 {
                continue;
            }
            if hyphen && index >= 13 {
                out.push('-');
                hyphen = false;
            }
            let paired = PAIRS.iter().any(|&(a, b)| (index == a || index == b) && bits & (1 << a) != 0 && bits & (1 << b) != 0);
            let underline = if paired { "\x1b[4m" } else { "" };
            let _ = std::fmt::Write::write_fmt(
                &mut out,
                format_args!("\x1b[{}m{underline}{}{RESET}", finger_colour(index), key as char),
            );
        }
    }
    out
}

fn clip(text: &str, room: usize) -> String {
    if text.chars().count() <= room {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(room.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

/// One line as text. Plain with `colour` off: exactly what `lookup` has always printed.
#[must_use]
pub fn render(line: &Line, style: Style) -> String {
    let room = style.columns.map_or(usize::MAX, |c| c.saturating_sub(1));
    let dim = |text: &str| if style.colour { format!("{DIM}{text}{RESET}") } else { text.to_owned() };
    let outline = |text: &str, room: usize| {
        let text = clip(text, room);
        if style.colour { paint_outline(&text) } else { text }
    };
    match line {
        Line::Word(word) => {
            let word = clip(word, room);
            if style.colour { format!("{BOLD}{word}{RESET}") } else { word }
        }
        Line::Entry { label, width, outline: found } => {
            let pad = " ".repeat(width.saturating_sub(label.chars().count()));
            let used = 2 + width + 1;
            let shown = found.as_deref().map_or_else(
                || "?".to_owned(),
                |o| outline(o, room.saturating_sub(used)),
            );
            format!("  {}{pad} {shown}", dim(label))
        }
        Line::Resolve { outline: o, result } => {
            let head = outline(o, room / 2);
            format!("{head}  ->  {}", clip(result, room.saturating_sub(o.chars().count() + 6)))
        }
        Line::Note(text) => clip(text, room),
        Line::Heading(text) => dim(&clip(text, room)),
        Line::Cells { cells, widths, header } => {
            let mut text = String::from("  ");
            for (c, cell) in cells.iter().enumerate() {
                let pad = " ".repeat(widths[c].saturating_sub(cell.chars().count()) + 2);
                let shown = if *header {
                    dim(cell)
                } else if c == 0 {
                    cell.clone()
                } else if style.colour && cell != "?" {
                    paint_outline(cell)
                } else {
                    cell.clone()
                };
                text.push_str(&shown);
                if c < 3 {
                    text.push_str(&pad);
                }
            }
            text
        }
    }
}

/// A whole view as lines of text, one string per line.
#[must_use]
pub fn render_all(lines: &[Line], style: Style) -> Vec<String> {
    lines.iter().map(|l| render(l, style)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stroke::parse_outline;

    fn entry(label: &str, width: usize, outline: Option<&str>) -> Line {
        Line::Entry { label: label.to_owned(), width, outline: outline.map(str::to_owned) }
    }

    #[test]
    fn plain_rendering_is_the_layout_lookup_always_printed() {
        let found = WordLookup {
            word: "everybody's".into(),
            lyk: Some("EFB/+-PBTS".into()),
            lyk_deployed: false,
            composed_of: Some(vec!["everybody".into(), "'s".into()]),
            plover: None,
            plover_composed_of: None,
            lapwing: Some("EFRB/AES".into()),
            lapwing_composed_of: Some(vec!["everybody".into(), "'s".into()]),
        };
        let text: Vec<String> = render_all(&Lookup::word_lines(&found), Style::default());
        assert_eq!(
            text,
            [
                "everybody's",
                "  lyk (composed: everybody + 's)     EFB/+-PBTS",
                "  plover                             ?",
                "  lapwing (composed: everybody + 's) EFRB/AES",
            ]
        );
    }

    #[test]
    fn a_resolve_line_reads_outline_arrow_result() {
        let line = Line::Resolve { outline: "KAT".into(), result: "cat  (in the dictionary)".into() };
        assert_eq!(render(&line, Style::default()), "KAT  ->  cat  (in the dictionary)");
    }

    #[test]
    fn colour_adds_escapes_and_plain_has_none() {
        let line = entry("lyk", 3, Some("TKOG"));
        assert!(!render(&line, Style::default()).contains('\x1b'));
        assert!(render(&line, Style { colour: true, columns: None }).contains("\x1b["));
    }

    #[test]
    fn painted_outline_has_the_same_letters_as_the_plain_one() {
        for outline in ["KAT", "TKOG", "-T", "TPAEU/+HR", "~A/+PW", "STKPWHRAO*EUFRPBLGTSDZ", "EFB/+-PBTS", "AOEUS"] {
            let strip = |s: &str| {
                let mut out = String::new();
                let mut chars = s.chars();
                while let Some(c) = chars.next() {
                    if c == '\x1b' {
                        for d in chars.by_ref() {
                            if d == 'm' {
                                break;
                            }
                        }
                    } else {
                        out.push(c);
                    }
                }
                out
            };
            assert_eq!(strip(&paint_outline(outline)), render_outline(&parse_outline(outline).unwrap()), "{outline}");
        }
    }

    #[test]
    fn a_finger_pair_is_underlined_only_when_both_keys_are_held() {
        let both = paint_outline("TKOG");
        assert!(both.contains("\x1b[33m\x1b[4mT"), "T and K are the ring finger's pair: {both:?}");
        let one = paint_outline("TOG");
        assert!(!one.contains("\x1b[4m"), "{one:?}");
    }

    #[test]
    fn fingers_have_their_own_colour() {
        assert_eq!(finger_colour(1), "31", "left pinky");
        assert_eq!(finger_colour(21), "31", "right pinky D");
        assert_eq!(finger_colour(6), "36", "left index H");
        assert_eq!(finger_colour(13), "36", "right index F");
        assert_ne!(finger_colour(2), finger_colour(4));
    }

    #[test]
    fn a_narrow_terminal_cuts_a_long_outline_with_an_ellipsis() {
        let line = entry("lyk", 3, Some("STKPWHRAO*EUFRPBLGTSDZ/STKPWHRAO*EUFRPBLGTSDZ"));
        let text = render(&line, Style { colour: false, columns: Some(20) });
        assert!(text.chars().count() <= 19, "{text}");
        assert!(text.ends_with('…'));
    }

    #[test]
    fn the_matches_table_aligns_its_columns() {
        let line = |cells: [&str; 4]| Line::Cells { cells: cells.map(str::to_owned), widths: [7, 6, 6, 7], header: false };
        let a = render(&line(["cat", "KAT", "KAT", "KAT"]), Style::default());
        let b = render(&line(["catalog", "KAT/-L", "?", "?"]), Style::default());
        assert_eq!(a.find("KAT"), Some(11));
        assert_eq!(b.find("KAT"), Some(11));
    }

    fn text(lines: &[Line]) -> Vec<String> {
        render_all(lines, Style::default())
    }

    #[test]
    fn a_word_query_shows_all_three_theories_and_a_stroke_query_resolves() {
        let (setup, generated) = (crate::dictionary_fixture::setup(), crate::dictionary_fixture::generated());
        let lookup = Lookup::new(setup, &generated.entries);
        let cat = text(&lookup.view("cat", false));
        assert_eq!(cat.len(), 4);
        assert_eq!(cat[0], "cat");
        assert!(cat[1].starts_with("  lyk ") && cat[2].starts_with("  plover ") && cat[3].starts_with("  lapwing "));
        let resolved = text(&lookup.view("KAT", false));
        assert_eq!(resolved, ["KAT  ->  cat  (in the dictionary)"]);
        assert_eq!(text(&lookup.view("   ", false)), ["Nothing to look up"]);
    }

    #[test]
    fn resolve_reads_the_query_as_an_outline_or_says_it_is_not_one() {
        let (setup, generated) = (crate::dictionary_fixture::setup(), crate::dictionary_fixture::generated());
        let lookup = Lookup::new(setup, &generated.entries);
        assert_eq!(text(&lookup.view("KAT", true)), ["KAT  ->  cat  (in the dictionary)"]);
        assert_eq!(text(&lookup.view("cat!", true)), ["\"cat!\" is not a stroke outline"]);
    }

    #[test]
    fn the_live_view_adds_the_commonest_words_that_start_like_the_last_word() {
        let (setup, generated) = (crate::dictionary_fixture::setup(), crate::dictionary_fixture::generated());
        let lookup = Lookup::new(setup, &generated.entries);
        let live = text(&lookup.live_view("cat", false));
        assert_eq!(live[..4], text(&lookup.view("cat", false))[..], "the exact answer comes first");
        assert!(live.iter().any(|l| l.contains("words starting \"cat\"")), "{live:?}");
        let header = live.iter().find(|l| l.contains("lapwing") && l.contains("plover") && l.contains("word")).unwrap();
        assert!(header.contains("lyk"));
        let rows = live.len() - 4 - 2;
        assert!((1..=MATCHES).contains(&rows), "{live:?}");
        assert!(live.iter().skip(4).all(|l| !l.starts_with("cat ")), "the word itself is not repeated");
    }

    #[test]
    fn a_last_word_that_is_only_a_start_is_listed_by_its_matches_not_looked_up_whole() {
        let (setup, generated) = (crate::dictionary_fixture::setup(), crate::dictionary_fixture::generated());
        let lookup = Lookup::new(setup, &generated.entries);
        let live = text(&lookup.live_view("tell me catal", false));
        assert_eq!(live[0], "tell");
        assert_eq!(live[4], "me");
        assert!(live.iter().all(|l| l.trim() != "catal"), "{live:?}");
        assert!(live.iter().any(|l| l.contains("words starting \"catal\"")), "{live:?}");
    }

    #[test]
    fn a_stroke_query_lists_words_whose_outline_starts_with_it() {
        let (setup, generated) = (crate::dictionary_fixture::setup(), crate::dictionary_fixture::generated());
        let lookup = Lookup::new(setup, &generated.entries);
        let live = text(&lookup.live_view("KA", false));
        assert!(live[0].starts_with("KA  ->"), "{live:?}");
        assert!(live[1].contains("outlines starting KA"));
        let rows: Vec<&String> = live.iter().skip(3).collect();
        assert!(!rows.is_empty() && rows.len() <= MATCHES);
        assert!(rows.iter().all(|r| r.split_whitespace().nth(1).is_some_and(|o| o.starts_with("KA") && o != "KA")), "{rows:?}");
    }

    #[test]
    fn an_empty_live_query_shows_nothing() {
        let (setup, generated) = (crate::dictionary_fixture::setup(), crate::dictionary_fixture::generated());
        let lookup = Lookup::new(setup, &generated.entries);
        assert!(lookup.live_view("  ", false).is_empty());
    }
}

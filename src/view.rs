//! An [`Answer`] as lines of text: the one view `lookup`, `keys`, `resolve` and `stroke` print,
//! and the interactive session redraws on every key. Keymux's `steno/view.rs`, with an outline
//! column per theory.
//!
//! The columns follow `quote` (`word`, then `lyk`, `plover` and `lapwing`, each at least 12
//! wide), one outline per theory per row. Each outline is set in colour where the terminal has
//! it, a colour per part of the keyboard as on keymux's overlay tape, and keys one finger plays
//! together underlined as one. Colour never changes the text: the plain notation is identical.

use crate::answer::{Answer, Cell, How, Mark, Row, THEORIES};
use crate::notation::Piece;
use crate::stroke::Stroke;

/// The narrowest an outline column is set, as in `quote`.
const OUTLINE_WIDTH: usize = 12;
/// The narrowest the word column is set, as in `quote`.
const WORD_WIDTH: usize = 4;

const RESET: &str = "\x1b[0m";

/// `text` with the SGR codes `codes` on when `color`, else as it is.
fn paint(text: &str, codes: &str, color: bool) -> String {
    if color { format!("\x1b[{codes}m{text}{RESET}") } else { text.to_owned() }
}

/// Whether stdout can show colour: a terminal, `NO_COLOR` unset, `TERM` not `dumb`.
#[must_use]
pub fn terminal_colour() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal()
        && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
        && std::env::var("TERM").map_or(true, |t| t != "dumb")
}

/// How many columns `text` takes on screen, escape sequences not counted.
#[must_use]
pub fn visible_width(text: &str) -> usize {
    let mut width = 0;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            for d in chars.by_ref() {
                if d == 'm' {
                    break;
                }
            }
        } else {
            width += 1;
        }
    }
    width
}

/// `text` cut to `max` columns with a `…`, keeping its escape sequences whole.
#[must_use]
pub fn truncate(text: &str, max: usize) -> String {
    if visible_width(text) <= max {
        return text.to_owned();
    }
    let mut out = String::new();
    let (mut width, mut styled) = (0, false);
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            styled = true;
            out.push(c);
            for d in chars.by_ref() {
                out.push(d);
                if d == 'm' {
                    break;
                }
            }
        } else if width + 1 < max {
            out.push(c);
            width += 1;
        } else {
            break;
        }
    }
    out.push('…');
    if styled {
        out.push_str(RESET);
    }
    out
}

/// The qwerty key that presses each stroke key, by bit: keymux's default layout, with lyk's
/// config (`"*": Key(KC_Y)` so the star is the right index only, `"+": Key(KC_T)` and the alias
/// `"+|G": Key(KC_G)` so the join key is the left index's inner column; `Q` also presses `S-`
/// beside `A`). Where a key has two, the first is shown.
const KEY_FOR_BIT: [char; 23] = [
    '1', 'A', 'W', 'S', 'E', 'D', 'R', 'F', 'C', 'V', 'Y', 'N', 'M', 'U', 'J', 'I', 'K', 'O', 'L', 'P', ';', '[', '\'',
];
const JOIN_KEY: char = 'T';

/// The finger group of a key by bit (keys one finger plays together share one), or `None`:
/// keymux's `Layout::plover` groups.
const GROUP_FOR_BIT: [Option<u8>; 23] = [
    None,
    Some(10),
    Some(0),
    Some(0),
    Some(1),
    Some(1),
    Some(2),
    Some(2),
    Some(3),
    Some(3),
    None,
    Some(4),
    Some(4),
    Some(5),
    Some(5),
    Some(6),
    Some(6),
    Some(7),
    Some(7),
    Some(8),
    Some(8),
    Some(9),
    Some(9),
];

/// The finger groups a piece's keys belong to, one bit each.
fn finger_groups(keys: u32) -> u16 {
    (0..23usize).filter(|bit| keys & (1 << bit) != 0).filter_map(|bit| GROUP_FOR_BIT[bit]).fold(0, |a, g| a | (1 << g))
}

/// A piece's SGR colour by the part of the keyboard its keys are on: green left hand, yellow
/// vowels, red right hand, cyan for the star, number and extra keys; dim for the `-` that only
/// parts the hands.
const fn piece_colour(keys: u32) -> &'static str {
    if keys == 0 {
        "2"
    } else if keys & (Stroke::JOIN | Stroke::SPELL | Stroke::STAR | 1) != 0 {
        "36"
    } else if keys & Stroke::LEFT != 0 {
        "32"
    } else if keys & Stroke::VOWELS != 0 {
        "33"
    } else {
        "31"
    }
}

/// One stroke in raw notation as pieces, a key each: how Plover's and Lapwing's columns are
/// written.
fn raw_pieces(stroke: Stroke) -> Vec<Piece> {
    let text = stroke.to_string();
    let mut keys = (0..23u32).filter(|bit| stroke.0 & (1 << bit) != 0).map(|bit| 1u32 << bit);
    text.chars()
        .map(|c| {
            let keys = match c {
                '+' => Stroke::JOIN,
                '~' => Stroke::SPELL,
                '-' => 0,
                _ => keys.next().unwrap_or(0),
            };
            Piece { text: c.to_string(), keys }
        })
        .collect()
}

/// One stroke's pieces, each coloured by its part of the keyboard, and pieces one finger plays
/// together (a finger group shared with the piece beside it) underlined.
fn pieces_text(pieces: &[Piece]) -> String {
    let mut out = String::new();
    for (i, piece) in pieces.iter().enumerate() {
        let groups = finger_groups(piece.keys);
        let beside = |j: Option<usize>| j.and_then(|j| pieces.get(j)).is_some_and(|p| finger_groups(p.keys) & groups != 0);
        let mut codes = piece_colour(piece.keys).to_owned();
        if groups != 0 && (beside(i.checked_sub(1)) || beside(Some(i + 1))) {
            codes.push_str(";4");
        }
        out.push_str(&paint(&piece.text, &codes, true));
    }
    out
}

/// One stroke in raw notation, coloured (see [`pieces_text`]).
#[cfg(test)]
fn stroke_text(stroke: Stroke) -> String {
    pieces_text(&raw_pieces(stroke))
}

/// The physical keys that press each stroke, strokes split by `/`; keys within a stroke run
/// together, the join key first and the number key next, then the rest in order.
#[must_use]
pub fn keys_text(strokes: &[Stroke]) -> String {
    strokes
        .iter()
        .map(|stroke| {
            let mut keys = String::new();
            if stroke.0 & Stroke::JOIN != 0 {
                keys.push(JOIN_KEY);
            }
            keys.extend((0..23u32).filter(|bit| stroke.0 & (1 << bit) != 0).map(|bit| KEY_FOR_BIT[bit as usize]));
            keys
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Everything a view needs besides the answer.
#[derive(Debug, Clone, Copy)]
pub struct View {
    /// The physical keys that press lyk's outline of each row
    pub show_keys: bool,
    /// Whether to colour and underline
    pub color: bool,
}

impl View {
    /// Every line to print for `answer`: the header and rows, a note under each word that was
    /// composed or spelled out, and what the best outlines add up to.
    #[must_use]
    pub fn lines(self, answer: &Answer) -> Vec<String> {
        if let Some(problem) = &answer.problem {
            return vec![paint(problem, "31", self.color)];
        }
        if answer.rows.is_empty() {
            return Vec::new();
        }
        // A stroke is lyk's alone: only its column
        let forward = answer.rows.iter().any(|r| matches!(r.mark, Mark::Forward | Mark::Nearby));
        let columns = if forward { 1 } else { THEORIES.len() };

        let cells: Vec<Vec<(String, usize)>> =
            answer.rows.iter().map(|row| (0..columns).map(|t| self.outline_cell(row, t)).collect()).collect();
        let words: Vec<String> = answer.rows.iter().map(Self::word_text).collect();
        let word_width = words.iter().map(|w| w.chars().count()).max().unwrap_or(0).max(WORD_WIDTH);
        let widths: Vec<usize> = (0..columns)
            .map(|t| cells.iter().map(|row| row[t].1).max().unwrap_or(0).max(OUTLINE_WIDTH))
            .collect();

        let bold = |text: &str| paint(text, "1", self.color);
        let mut header = bold(&format!("{:<word_width$}", "word"));
        for (name, width) in THEORIES.iter().zip(&widths) {
            header.push_str("  ");
            header.push_str(&bold(&format!("{name:<width$}")));
        }
        if self.show_keys {
            header.push_str("  ");
            header.push_str(&bold("keys"));
        }
        let mut lines = vec![header.trim_end().to_owned()];

        for ((row, word), outline) in answer.rows.iter().zip(&words).zip(&cells) {
            let padded = format!("{word}{}", " ".repeat(word_width.saturating_sub(word.chars().count())));
            let word = match row.mark {
                Mark::Missing | Mark::Completion | Mark::Nearby => paint(&padded, "2", self.color),
                _ => paint(&padded, "1", self.color),
            };
            let mut line = word;
            for (t, (text, width)) in outline.iter().enumerate() {
                line.push_str("  ");
                line.push_str(text);
                line.push_str(&" ".repeat(widths[t] - width));
            }
            if self.show_keys
                && let Some(cell) = &row.cells[0]
            {
                line.push_str("  ");
                line.push_str(&paint(&keys_text(&cell.strokes), "32", self.color));
            }
            lines.push(line.trim_end().to_owned());
        }

        lines.extend(answer.rows.iter().filter_map(Self::note));
        if answer.rows.iter().any(|r| r.cells[0].as_ref().is_some_and(|c| c.how == How::Solver)) {
            lines.push(paint(
                "  dim outlines are not entries: keymux reads them with the solver, and the lookup checked it reads this word",
                "2",
                self.color,
            ));
        }
        lines.extend(answer.rows.iter().flat_map(|row| row.detail.iter().cloned()));
        let totals = answer.totals();
        if totals.strokes > 0 {
            lines.push(String::new());
            lines.push(paint(
                &format!(
                    "keys (best outline of each word): {} in {} stroke{}",
                    totals.keys,
                    totals.strokes,
                    if totals.strokes == 1 { "" } else { "s" }
                ),
                "2",
                self.color,
            ));
            lines.push(paint(
                &format!(
                    "keys (shared vocabulary only, fair comparison): lyk-vs-plover {} plover {}   lyk-vs-lapwing {} lapwing {}",
                    totals.vs_plover.0, totals.vs_plover.1, totals.vs_lapwing.0, totals.vs_lapwing.1
                ),
                "2",
                self.color,
            ));
        }
        lines
    }

    /// Whether lyk's outline for this word is not an entry of its own that a stroke can be
    /// typed for: a word shown in quotes.
    fn quoted(row: &Row) -> bool {
        row.mark == Mark::Entry
            && matches!(row.cells[0].as_ref().map(|c| &c.how), Some(How::Reading | How::Spelled))
    }

    fn word_text(row: &Row) -> String {
        if Self::quoted(row) { format!("\"{}\"", row.word) } else { row.word.clone() }
    }

    /// The note under the table about a row: a word composed from entries (in each theory that
    /// had to), or one with no entry of its own that is shown spelled out.
    fn note(row: &Row) -> Option<String> {
        if row.mark == Mark::Forward {
            let reading = matches!(row.cells[0].as_ref().map(|c| &c.how), Some(How::Reading));
            return reading.then(|| {
                let outline = row.cells[0].as_ref().map_or_default(|c| crate::notation::outline(&c.strokes));
                format!("  {outline} is not an entry - shown as the rules' own reading of it (in quotes)")
            });
        }
        if row.mark != Mark::Entry || row.word.is_empty() {
            return None;
        }
        let composed: Vec<String> = row
            .cells
            .iter()
            .zip(THEORIES)
            .filter_map(|(cell, name)| match cell.as_ref().map(|c| &c.how) {
                Some(How::Composed(pieces)) => Some(format!("{name}: {}", pieces.join(" + "))),
                _ => None,
            })
            .collect();
        if !composed.is_empty() {
            return Some(format!("  {} was not a whole entry - composed ({})", row.word, composed.join("; ")));
        }
        match row.cells[0].as_ref().map(|c| &c.how) {
            Some(How::Spelled) => {
                Some(format!("  {} has no dictionary entry - shown fingerspelled instead (in quotes)", row.word))
            }
            Some(How::Reading) => Some(format!(
                "  {} has no dictionary entry - shown as the rules' own reading instead (in quotes)",
                row.word
            )),
            _ => None,
        }
    }

    /// The outline column of a row for theory `t` and its width on screen: lyk's in the short
    /// notation ([`crate::notation`]), Plover's and Lapwing's raw (their own theories' keys).
    fn outline_cell(self, row: &Row, t: usize) -> (String, usize) {
        let missing = |text: &str, codes: &str| (paint(text, codes, self.color), text.len());
        match &row.cells[t] {
            None if row.mark == Mark::Missing && t == 0 => missing("no stroke", "31;2"),
            None => missing("-", "2"),
            Some(Cell { strokes, how }) => {
                let pieces: Vec<Vec<Piece>> = if t == 0 {
                    crate::notation::shown().outline_pieces(strokes)
                } else {
                    strokes.iter().map(|&s| raw_pieces(s)).collect()
                };
                let plain = pieces.iter().map(|s| s.iter().map(|p| p.text.as_str()).collect::<String>()).collect::<Vec<_>>().join("/");
                let width = plain.chars().count();
                if !self.color {
                    return (plain, width);
                }
                // Not an entry: keymux reads it with the solver, shown dim
                if *how == How::Solver {
                    return (paint(&plain, "2", true), width);
                }
                let slash = paint("/", "2", true);
                (pieces.iter().map(|s| pieces_text(s)).collect::<Vec<_>>().join(&slash), width)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::answer::{Ask, Lookup};

    fn lookup() -> Lookup<'static> {
        Lookup::new(crate::dictionary_fixture::setup(), &crate::dictionary_fixture::generated().entries, None)
    }

    const ALL: Ask = Ask { fast: false, stroke_mode: false, extra: 8 };

    fn strip(text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars();
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
    }

    fn lines(query: &str, ask: Ask, show_keys: bool, color: bool) -> Vec<String> {
        View { show_keys, color }.lines(&lookup().answer(query, ask))
    }

    #[test]
    fn the_header_is_quotes_with_lowercase_names() {
        let lines = lines("cat", ALL, false, false);
        let header: Vec<&str> = lines[0].split_whitespace().collect();
        assert_eq!(header, ["word", "lyk", "plover", "lapwing"]);
        assert!(lines[1].starts_with("cat "));
    }

    #[test]
    fn the_keys_column_is_the_physical_keys_of_lyks_outline() {
        let lines = lines("cat", ALL, true, false);
        assert!(lines[0].ends_with("keys"));
        // K is S, A is C, the right-hand T is P: `KAT` on qwerty is S C P
        assert!(lines[1].ends_with("SCP"), "{}", lines[1]);
        assert_eq!(keys_text(&crate::stroke::parse_outline("KAT/+-D").unwrap()), "SCP/T[");
        // the join key is T (the left index's inner column) and the star Y (the right index)
        assert_eq!(keys_text(&crate::stroke::parse_outline("PHA*/+*").unwrap()), "ERCY/TY");
    }

    #[test]
    fn colour_only_adds_escape_sequences() {
        for query in ["cat", "tell me how", "KAT", "cat!x"] {
            let plain = lines(query, ALL, true, false);
            let coloured = lines(query, ALL, true, true);
            assert!(plain.iter().all(|l| !l.contains('\x1b')));
            assert!(coloured.iter().any(|l| l.contains("\x1b[")), "{query}");
            let stripped: Vec<String> = coloured.iter().map(|l| strip(l)).collect();
            assert_eq!(stripped, plain, "{query}");
        }
    }

    #[test]
    fn a_finger_group_is_underlined_only_when_both_keys_are_held() {
        let both = stroke_text(Stroke::parse("TKOG").unwrap());
        assert!(both.contains("\x1b[32;4mT") && both.contains("\x1b[32;4mK"), "{both:?}");
        let one = stroke_text(Stroke::parse("TOG").unwrap());
        assert!(!one.contains(";4"), "{one:?}");
    }

    #[test]
    fn parts_are_coloured_by_part_of_the_keyboard() {
        let text = stroke_text(Stroke::parse("KA*FT").unwrap());
        assert!(text.contains("\x1b[32m") || text.contains("\x1b[32;4m"), "left hand is green");
        assert!(text.contains("\x1b[33mA"), "vowels are yellow");
        assert!(text.contains("\x1b[36m*"), "the star is cyan");
        assert!(text.contains("\x1b[31m"), "right hand is red");
        let join = stroke_text(Stroke::parse("+-D").unwrap());
        assert!(join.starts_with("\x1b[36m+"), "{join:?}");
        assert!(join.contains("\x1b[2m-"), "the hyphen is dim");
    }

    #[test]
    fn plain_text_spells_lyks_outlines_short_and_the_others_raw() {
        let lines = lines("KAT", ALL, false, false);
        assert!(lines[0].starts_with("word") && lines[0].contains("lyk") && !lines[0].contains("plover"));
        assert!(lines[1].starts_with("\"cat\"") && lines[1].contains("KAT"), "{lines:?}");
        // lyk's column short, Plover's and Lapwing's raw; a short or a raw query alike
        for query in ["enemy", "NAEM", "TPHAEPL"] {
            let first = |q: &str| self::lines(q, ALL, false, false)[1].clone();
            let line = first(query);
            assert!(line.contains("NAEM"), "{query}: {line}");
        }
        let enemy = self::lines("enemy", ALL, false, false);
        assert!(enemy[1].split_whitespace().skip(2).all(|o| o == "TPHAEPL"), "{enemy:?}");
        let flower = self::lines("flower", ALL, false, false);
        assert!(flower[1].split_whitespace().nth(1) == Some("FLOUR"), "{flower:?}");
        assert!(self::lines("FLOUR", ALL, false, false)[1].contains("\"flower\""));
    }

    #[test]
    fn an_alias_is_coloured_as_one_piece_of_its_hand() {
        let cell = Cell { strokes: crate::stroke::parse_outline("TPHAEPL/+-PB").unwrap(), how: How::Entry };
        let row = Row { word: "x".into(), cells: [Some(cell), None, None], mark: Mark::Entry, detail: Vec::new() };
        let (text, width) = View { show_keys: false, color: true }.outline_cell(&row, 0);
        assert_eq!(strip(&text), "NAEM/+-N");
        assert_eq!(width, 8);
        assert!(text.contains("\x1b[32mN\x1b[0m"), "N (TPH) green, one piece: {text:?}");
        assert!(text.contains("\x1b[31mM\x1b[0m"), "M (-PL) red: {text:?}");
        assert!(text.contains("\x1b[36m+"), "the join key cyan: {text:?}");
        // A number-key stroke and an outline written its own way
        let cell = |raw: &str| Row { word: "x".into(), cells: [Some(Cell { strokes: crate::stroke::parse_outline(raw).unwrap(), how: How::Entry }), None, None], mark: Mark::Entry, detail: Vec::new() };
        let plain = View { show_keys: false, color: false };
        assert_eq!(plain.outline_cell(&cell("#TPH-PB"), 0).0, "#N-N");
        assert_eq!(plain.outline_cell(&cell("ORB"), 0).0, "ORB");
        assert_eq!(strip(&View { show_keys: false, color: true }.outline_cell(&cell("ORB"), 0).0), "ORB");
    }

    #[test]
    fn a_missing_word_says_no_stroke_and_a_bad_stroke_says_why() {
        let missing = View { show_keys: false, color: false }.lines(&Answer {
            rows: vec![Row { word: "zzz".into(), cells: [None, None, None], mark: Mark::Missing, detail: Vec::new() }],
            problem: None,
        });
        assert!(missing[1].starts_with("zzz") && missing[1].contains("no stroke"), "{missing:?}");
        let bad = lines("cat!", Ask { stroke_mode: true, ..ALL }, false, false);
        assert_eq!(bad, ["\"cat!\" is not stroke notation"]);
    }

    #[test]
    fn the_totals_close_the_table_for_a_word_but_not_a_stroke() {
        let word = lines("tell me", ALL, false, false);
        let n = word.len();
        assert!(word[n - 2].starts_with("keys (best outline of each word): "), "{word:?}");
        assert!(word[n - 1].starts_with("keys (shared vocabulary only, fair comparison): lyk-vs-plover "), "{word:?}");
        assert!(lines("KAT", ALL, false, false).iter().all(|l| !l.starts_with("keys (")));
    }

    #[test]
    fn a_composed_word_has_a_note_naming_the_theory() {
        let composed = Row {
            word: "everybody's".into(),
            cells: [
                Some(Cell {
                    strokes: crate::stroke::parse_outline("EFB/+-PBTS").unwrap(),
                    how: How::Composed(vec!["everybody".into(), "'s".into()]),
                }),
                None,
                Some(Cell {
                    strokes: crate::stroke::parse_outline("EFRB/AES").unwrap(),
                    how: How::Composed(vec!["everybody".into(), "'s".into()]),
                }),
            ],
            mark: Mark::Entry,
            detail: Vec::new(),
        };
        let lines = View { show_keys: false, color: false }
            .lines(&Answer { rows: vec![composed], problem: None });
        assert!(
            lines.contains(
                &"  everybody's was not a whole entry - composed (lyk: everybody + 's; lapwing: everybody + 's)"
                    .to_owned()
            ),
            "{lines:?}"
        );
        assert!(lines[1].contains("EFB/+-NTS") && lines[1].contains(" - "), "{lines:?}");
    }

    #[test]
    fn a_solver_read_outline_is_dim_and_noted() {
        let cell = |outline: &str, how| Some(Cell { strokes: crate::stroke::parse_outline(outline).unwrap(), how });
        let rows = vec![
            Row { word: "well".into(), cells: [cell("WEL", How::Entry), None, None], mark: Mark::Entry, detail: Vec::new() },
            Row { word: String::new(), cells: [cell("WE/+-L", How::Solver), None, None], mark: Mark::Entry, detail: Vec::new() },
        ];
        let answer = Answer { rows, problem: None };
        let coloured = View { show_keys: false, color: true }.lines(&answer);
        assert!(coloured[2].contains("\x1b[2mWE/+-L"), "{:?}", coloured[2]);
        assert!(!coloured[1].contains("\x1b[2mWEL"), "{:?}", coloured[1]);
        let plain = View { show_keys: false, color: false }.lines(&answer);
        assert!(plain.iter().any(|l| l.contains("dim outlines are not entries")), "{plain:?}");
        // no solver-read outline, no note
        let entries = Answer { rows: vec![answer.rows[0].clone()], problem: None };
        assert!(View { show_keys: false, color: false }.lines(&entries).iter().all(|l| !l.contains("dim outlines")));
    }

    #[test]
    fn truncating_keeps_escapes_whole_and_ends_with_an_ellipsis() {
        let line = paint("abcdefghij", "31", true);
        let cut = truncate(&line, 5);
        assert_eq!(strip(&cut), "abcd…");
        assert!(visible_width(&cut) <= 5);
        assert_eq!(truncate("short", 20), "short");
    }
}

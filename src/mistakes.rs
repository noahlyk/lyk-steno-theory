//! `mistakes`: a retroactive report on the keymux steno tape, and a personal drill set from it.
//!
//! It runs once when asked: it reads the tape file (read-only), prints or writes its output and
//! exits. Nothing watches the tape, nothing is cached and nothing is logged while typing.
//!
//! An *event* is a stroke that was undone (or a `?` miss) and the stroke struck right after it to
//! say the same thing: the *intended* one. The tape is replayed with an undo stack (every `*`
//! pops the last live stroke). A pair counts as a mistake only when the two outlines are a
//! near-miss: one key added or missing, a neighbouring key swapped, the other hand's twin key,
//! two keys off, strokes in the wrong order, or a stroke added or dropped. Any other pair (a
//! different word altogether, the same stroke again) is not a mistake and is only counted.
//!
//! Privacy: the tape holds the user's real typing. Lines the keyboard typed (`typed`) are never
//! read past their timestamp, and a word reaches any output only if it is an ordinary word of the
//! dictionary. Everything written goes to a gitignored folder (`out/mistakes/`).

#![allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::too_many_lines)]

use crate::stroke::Stroke;
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

/// A retry later than this many seconds after the last stroke is no retry.
const GAP: i64 = 15;
/// A day with fewer strokes than this is left out of the trend table.
const MIN_DAY_STROKES: usize = 300;

pub struct Options<'a> {
    pub tape: &'a Path,
    pub dict: &'a Path,
    pub last: Option<usize>,
    pub since: Option<&'a str>,
    pub min_errors: usize,
    pub min_attempts: usize,
    pub top: usize,
    pub json: bool,
    pub drill: bool,
    pub out: Option<&'a Path>,
    /// How many words the Monkeytype export holds
    pub words: usize,
}

// ---------------------------------------------------------------------------------------------
// The tape

#[derive(Clone, Debug, PartialEq, Eq)]
enum Kind {
    /// A stroke that typed this word (trimmed, lower case); `chars` is the length the tape logged
    Text { word: String, chars: usize },
    Miss,
    /// `*`, with the number of characters it deleted when the tape says
    Undo(Option<usize>),
    /// A command, a key code or a stroke with no output
    Other,
    /// A line the keyboard typed: a barrier, never read
    Typed,
}

#[derive(Clone, Debug)]
struct Ev {
    day: u32,
    sec: u32,
    stroke: String,
    kind: Kind,
}

fn parse_line(line: &str) -> Option<(u32, String, Kind)> {
    let b = line.as_bytes();
    if b.len() < 10 || b[2] != b':' || b[5] != b':' || !line.is_char_boundary(8) {
        return None;
    }
    let n = |r: std::ops::Range<usize>| line.get(r)?.parse::<u32>().ok();
    let sec = n(0..2)? * 3600 + n(3..5)? * 60 + n(6..8)?;
    let rest = line[8..].trim_start();
    let (stroke, result) = rest.split_once(char::is_whitespace).map_or((rest, ""), |(a, b)| (a, b.trim()));
    if stroke == "typed" {
        return Some((sec, String::new(), Kind::Typed));
    }
    let kind = if stroke == "*" {
        let n = result.strip_prefix('-').and_then(|r| r.split_whitespace().next()).and_then(|d| d.parse().ok());
        Kind::Undo(n)
    } else if result == "?" || result.starts_with("? ") {
        Kind::Miss
    } else if let Some(quoted) = result.strip_prefix('"') {
        let main = quoted.split(" plugin:").next().unwrap_or(quoted).trim_end();
        match main.strip_suffix('"') {
            Some(inner) if !inner.contains('\\') => Kind::Text { word: inner.trim().to_lowercase(), chars: inner.chars().count() },
            _ => Kind::Other,
        }
    } else {
        Kind::Other
    };
    Some((sec, stroke.to_owned(), kind))
}

/// The tape's events, each with a day number: the time of day going backwards starts a new day.
fn parse_tape(text: &str) -> Vec<Ev> {
    let (mut day, mut prev) = (0u32, 0u32);
    let mut out = Vec::new();
    for line in text.lines() {
        let Some((sec, stroke, kind)) = parse_line(line) else { continue };
        if sec < prev {
            day += 1;
        }
        prev = sec;
        out.push(Ev { day, sec, stroke, kind });
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Dates: the tape has only the time of day, so days are counted and the last one is anchored

const fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

const fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }, m, d)
}

fn date_string(day_number: i64) -> String {
    let (y, m, d) = civil_from_days(day_number);
    format!("{y:04}-{m:02}-{d:02}")
}

fn parse_date(text: &str) -> Option<i64> {
    let mut p = text.split('-');
    let (y, m, d) = (p.next()?.parse().ok()?, p.next()?.parse().ok()?, p.next()?.parse().ok()?);
    ((1..=12).contains(&m) && (1..=31).contains(&d) && p.next().is_none()).then(|| days_from_civil(y, m, d))
}

/// The local calendar day of the tape's last line: the file was last written when that line was,
/// so the difference between the file time (UTC) and the line's time of day is the UTC offset.
fn anchor_day(tape: &Path, last_sec: u32) -> Option<i64> {
    let secs = i64::try_from(std::fs::metadata(tape).ok()?.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs()).ok()?;
    let mut offset = (secs.rem_euclid(86_400) - i64::from(last_sec)).rem_euclid(86_400);
    if offset > 43_200 {
        offset -= 86_400;
    }
    Some((secs - offset).div_euclid(86_400))
}

/// Keep only the days asked for. `anchor` is the date of the last day.
fn window(events: Vec<Ev>, last: Option<usize>, since: Option<i64>, anchor: Option<i64>) -> (Vec<Ev>, Vec<(u32, Option<i64>)>) {
    let Some(final_day) = events.last().map(|e| e.day) else { return (events, Vec::new()) };
    let date_of = |day: u32| anchor.map(|a| a - i64::from(final_day - day));
    let active: Vec<u32> = {
        let mut seen: Vec<u32> = events.iter().map(|e| e.day).collect();
        seen.dedup();
        seen
    };
    let from_last = last.map_or(0, |n| active.len().saturating_sub(n));
    let kept: Vec<u32> = active
        .iter()
        .enumerate()
        .filter(|(i, d)| *i >= from_last && since.is_none_or(|s| date_of(**d).is_some_and(|x| x >= s)))
        .map(|(_, d)| *d)
        .collect();
    let days = kept.iter().map(|d| (*d, date_of(*d))).collect();
    (events.into_iter().filter(|e| kept.contains(&e.day)).collect(), days)
}

// ---------------------------------------------------------------------------------------------
// Keys

const NAMES: [&str; 24] = [
    "#", "S", "T", "K", "P", "W", "H", "R", "A", "O", "*", "E", "U", "-F", "-R", "-P", "-B", "-L", "-G", "-T", "-S", "-D", "-Z", "+",
];

fn bits(mask: u32) -> impl Iterator<Item = u32> {
    (0..24).filter(move |b| mask >> b & 1 == 1)
}

const fn finger(bit: u32) -> &'static str {
    match bit {
        1 => "left pinky",
        2 | 3 => "left ring",
        4 | 5 => "left middle",
        6 | 7 => "left index",
        8 | 9 => "left thumb",
        10 => "star",
        11 | 12 => "right thumb",
        13..=16 => "right index",
        17 | 18 => "right middle",
        19 | 20 => "right ring",
        21 | 22 => "right pinky",
        _ => "other",
    }
}

const fn hand(bit: u32) -> &'static str {
    match bit {
        1..=9 => "left",
        11..=22 => "right",
        10 => "star",
        _ => "other",
    }
}

const fn row(bit: u32) -> &'static str {
    match bit {
        2 | 4 | 6 | 13 | 15 | 17 | 19 | 21 => "top",
        3 | 5 | 7 | 14 | 16 | 18 | 20 | 22 => "bottom",
        1 | 10 => "tall (S, star)",
        8 | 9 | 11 | 12 => "vowels",
        _ => "other",
    }
}

/// Where a key sits: a column, and the rows it covers (the tall S and star cover two).
const fn grid(bit: u32) -> (i32, i32, i32) {
    match bit {
        1 => (0, 0, 1),
        2 => (1, 0, 0),
        3 => (1, 1, 1),
        4 => (2, 0, 0),
        5 => (2, 1, 1),
        6 => (3, 0, 0),
        7 => (3, 1, 1),
        8 => (3, 2, 2),
        9 => (4, 2, 2),
        10 => (4, 0, 1),
        11 => (5, 2, 2),
        12 => (6, 2, 2),
        13 => (5, 0, 0),
        14 => (5, 1, 1),
        15 => (6, 0, 0),
        16 => (6, 1, 1),
        17 => (7, 0, 0),
        18 => (7, 1, 1),
        19 => (8, 0, 0),
        20 => (8, 1, 1),
        21 => (9, 0, 0),
        22 => (9, 1, 1),
        _ => (20, 20, 20),
    }
}

fn neighbours(a: u32, b: u32) -> bool {
    let ((xa, a0, a1), (xb, b0, b1)) = (grid(a), grid(b));
    let dy = if a1 < b0 { b0 - a1 } else { (a0 - b1).max(0) };
    (xa - xb).abs() <= 1 && dy <= 1
}

fn twins(a: u32, b: u32) -> bool {
    matches!((a.min(b), a.max(b)), (1, 20) | (2, 19) | (4, 15) | (7, 14))
}

// ---------------------------------------------------------------------------------------------
// Near-misses

/// How a struck stroke differs from the intended one.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Slip {
    class: &'static str,
    /// Keys struck that were not wanted
    added: Vec<u32>,
    /// Keys wanted that were not struck
    missed: Vec<u32>,
}

fn slip(struck: &str, intended: &str) -> Option<Slip> {
    let (a, b) = (Stroke::parse(struck)?.0, Stroke::parse(intended)?.0);
    let (added, missed): (Vec<u32>, Vec<u32>) = (bits(a & !b).collect(), bits(b & !a).collect());
    let class = match (added.len(), missed.len()) {
        (1, 0) => "one key added",
        (0, 1) => "one key missing",
        (1, 1) if twins(added[0], missed[0]) => "the other hand's twin key",
        (1, 1) if neighbours(added[0], missed[0]) => "a neighbour key instead",
        (1, 1) => "a distant key instead",
        (2, 0) => "two keys added",
        (0, 2) => "two keys missing",
        _ => return None,
    };
    Some(Slip { class, added, missed })
}

// ---------------------------------------------------------------------------------------------
// Replaying the tape

/// A mistake: `struck` was undone or missed, `intended` was struck for the same thing.
#[derive(Clone, Debug)]
struct Fix {
    day: u32,
    struck: String,
    /// What the struck stroke typed, when it typed a word
    struck_word: Option<String>,
    intended: String,
    word: String,
    slip: Slip,
}

#[derive(Default, Debug)]
struct Replay {
    fixes: Vec<Fix>,
    /// Strokes that stayed typed (never undone), by event index
    kept: Vec<usize>,
    /// Chains where two strokes came back in the other order
    reordered: usize,
    /// Undone or missed, and the retry was the same stroke again
    restruck: usize,
    /// Undone or missed, and the retry was another way to type the same text
    same_text: usize,
    /// Undone or missed with a retry that is not a near-miss
    unrelated: usize,
    /// Undone or missed with no retry within the gap
    no_retry: usize,
    undos: usize,
    undos_with_empty_stack: usize,
    /// Undos whose character count equals the popped stroke's text: how well the replay tracks
    /// the engine's own undo stack
    undo_checked: usize,
    undo_matched: usize,
}

/// Per word: mistakes, the slips made, the intended strokes seen
type WordTally<'a> = (usize, HashMap<String, usize>, HashMap<&'a str, usize>);

struct Pending {
    run: Vec<usize>,
    after: Vec<usize>,
}

fn replay(events: &[Ev]) -> Replay {
    let mut r = Replay::default();
    let mut stack: Vec<usize> = Vec::new();
    let mut popped = vec![false; events.len()];
    let mut p = Pending { run: Vec::new(), after: Vec::new() };
    let mut last_t = i64::MIN;
    for (i, e) in events.iter().enumerate() {
        let t = i64::from(e.day) * 86_400 + i64::from(e.sec);
        if t.saturating_sub(last_t) > GAP {
            flush(events, &mut p, &mut r);
        }
        last_t = t;
        match &e.kind {
            Kind::Typed => {
                flush(events, &mut p, &mut r);
                stack.clear();
            }
            Kind::Undo(chars) => {
                r.undos += 1;
                let Some(top) = stack.pop() else {
                    r.undos_with_empty_stack += 1;
                    continue;
                };
                popped[top] = true;
                if let (Kind::Text { chars: c, .. }, Some(n)) = (&events[top].kind, chars) {
                    r.undo_checked += 1;
                    r.undo_matched += usize::from(c == n);
                }
                if !matches!(events[top].kind, Kind::Text { .. } | Kind::Miss) {
                    continue;
                }
                if p.after.last() == Some(&top) {
                    p.after.pop();
                    p.run.push(top);
                } else {
                    if !p.after.is_empty() {
                        flush(events, &mut p, &mut r);
                    }
                    if !p.run.contains(&top) {
                        p.run.push(top);
                    }
                }
            }
            Kind::Miss => {
                if !p.after.is_empty() {
                    flush(events, &mut p, &mut r);
                }
                stack.push(i);
                p.run.push(i);
            }
            Kind::Text { .. } => {
                stack.push(i);
                if !p.run.is_empty() {
                    p.after.push(i);
                    if p.after.len() > p.run.len() {
                        flush(events, &mut p, &mut r);
                    }
                }
            }
            Kind::Other => {
                stack.push(i);
                flush(events, &mut p, &mut r);
            }
        }
    }
    flush(events, &mut p, &mut r);
    r.kept = (0..events.len()).filter(|i| !popped[*i] && matches!(events[*i].kind, Kind::Text { .. })).collect();
    r
}

fn flush(events: &[Ev], p: &mut Pending, r: &mut Replay) {
    let mut run = std::mem::take(&mut p.run);
    let after = std::mem::take(&mut p.after);
    if run.is_empty() {
        return;
    }
    run.sort_unstable();
    if after.is_empty() {
        r.no_retry += run.len();
        return;
    }
    if run.len() == 2 && after.len() >= 2 {
        let s = |i: usize| events[i].stroke.as_str();
        if s(run[0]) == s(after[1]) && s(run[1]) == s(after[0]) && s(run[0]) != s(run[1]) {
            r.reordered += 1;
            return;
        }
    }
    let window = &after[..after.len().min(run.len())];
    for (k, &struck) in run.iter().enumerate() {
        let (s, order) = (&events[struck], std::iter::once(k.min(window.len() - 1)).chain(0..window.len()));
        let mut outcome = None;
        for j in order {
            let intended = &events[window[j]];
            let Kind::Text { word, .. } = &intended.kind else { continue };
            if intended.stroke == s.stroke {
                outcome = Some(Err(0));
                break;
            }
            if matches!(&s.kind, Kind::Text { word: w, .. } if w == word) {
                outcome = Some(Err(1));
                break;
            }
            if let Some(sl) = slip(&s.stroke, &intended.stroke) {
                let struck_word = if let Kind::Text { word, .. } = &s.kind { Some(word.clone()) } else { None };
                outcome = Some(Ok(Fix { day: s.day, struck: s.stroke.clone(), struck_word, intended: intended.stroke.clone(), word: word.clone(), slip: sl }));
                break;
            }
        }
        match outcome {
            Some(Ok(fix)) => r.fixes.push(fix),
            Some(Err(0)) => r.restruck += 1,
            Some(Err(_)) => r.same_text += 1,
            None => r.unrelated += 1,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The dictionary

#[derive(Default)]
struct Dict {
    /// word -> its outlines, the words file's own first
    words: HashMap<String, Vec<String>>,
    /// outline -> the word it types
    outlines: HashMap<String, String>,
}

impl Dict {
    fn add(&mut self, outline: &str, word: &str) {
        let word = word.to_lowercase();
        self.words.entry(word.clone()).or_default().push(outline.to_owned());
        self.outlines.entry(outline.to_owned()).or_insert(word);
    }

    fn load(dir: &Path) -> Result<Self> {
        let mut d = Self::default();
        for file in ["02-lyk-words.json", "02-lyk-short.json", "02-lyk-alternates.json", "02-lyk-extras.json"] {
            let path = dir.join(file);
            let text = std::fs::read_to_string(&path).with_context(|| format!("{} (run `build` first)", path.display()))?;
            let map: BTreeMap<String, String> = serde_json::from_str(&text).with_context(|| path.display().to_string())?;
            for (outline, word) in &map {
                d.add(outline, word);
            }
        }
        Ok(d)
    }

    /// An ordinary word that may appear in output: letters and apostrophes, in the dictionary.
    fn ordinary(&self, word: &str) -> bool {
        word.len() <= 24 && word.chars().all(|c| c.is_alphabetic() || c == '\'') && self.words.contains_key(word)
    }

    /// The outline to drill a word with: the intended stroke when the dictionary types the word
    /// with it, else the word's first outline.
    fn outline_for(&self, word: &str, seen: &str) -> Option<String> {
        if self.outlines.get(seen).is_some_and(|w| w == word) {
            return Some(seen.to_owned());
        }
        self.words.get(word)?.first().cloned()
    }
}

// ---------------------------------------------------------------------------------------------
// The report

#[derive(Serialize, Debug, Default)]
struct Report {
    pub thresholds: Thresholds,
    pub window: WindowInfo,
    pub counts: Counts,
    pub classes: Vec<(String, usize)>,
    pub words: Vec<WordRow>,
    pub words_by_rate: Vec<WordRow>,
    pub outlines_by_rate: Vec<OutlineRow>,
    pub keys: Vec<KeyRow>,
    pub fingers: Vec<GroupRow>,
    pub rows: Vec<GroupRow>,
    pub hands: Vec<GroupRow>,
    pub swaps: Vec<(String, String, usize)>,
    pub pairs: Vec<PairRow>,
    pub days: Vec<DayRow>,
    pub halves: Option<(f64, f64)>,
}

#[derive(Serialize, Debug, Default)]
struct Thresholds {
    pub min_errors: usize,
    pub min_attempts: usize,
    pub min_day_strokes: usize,
    pub gap_seconds: i64,
}

#[derive(Serialize, Debug, Default)]
struct WindowInfo {
    pub days: usize,
    /// Approximate calendar dates (the tape has only the time of day)
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Serialize, Debug, Default)]
struct Counts {
    pub strokes_kept: usize,
    pub undos: usize,
    pub undos_empty_stack: usize,
    pub undo_count_matched_percent: f64,
    pub mistakes: usize,
    pub restruck: usize,
    pub same_text: usize,
    pub unrelated: usize,
    pub no_retry: usize,
    pub reordered: usize,
}

#[derive(Serialize, Debug, Clone)]
struct WordRow {
    pub word: String,
    pub errors: usize,
    pub attempts: usize,
    pub rate: f64,
    pub outline: String,
    /// The slip made most often with it
    pub watch: String,
}

#[derive(Serialize, Debug)]
struct OutlineRow {
    pub outline: String,
    pub word: String,
    pub errors: usize,
    pub attempts: usize,
    pub rate: f64,
}

#[derive(Serialize, Debug)]
struct KeyRow {
    pub key: String,
    pub wanted: usize,
    pub missed: usize,
    pub miss_rate: f64,
    pub added: usize,
}

#[derive(Serialize, Debug)]
struct GroupRow {
    pub name: String,
    pub presses: usize,
    pub errors: usize,
    pub rate: f64,
}

#[derive(Serialize, Debug)]
struct PairRow {
    pub struck: String,
    pub intended: String,
    pub word: String,
    pub struck_word: Option<String>,
    pub count: usize,
}

#[derive(Serialize, Debug)]
struct DayRow {
    pub day: String,
    pub strokes: usize,
    pub mistakes: usize,
    pub rate: f64,
}

fn rate(errors: usize, attempts: usize) -> f64 {
    if attempts == 0 { 0.0 } else { errors as f64 / attempts as f64 }
}

fn key_label(bit: u32) -> String {
    NAMES[bit as usize].to_owned()
}

fn note_for(counts: &HashMap<String, usize>) -> String {
    let mut all: Vec<_> = counts.iter().collect();
    all.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    all.first().map_or_else(String::new, |(k, _)| (*k).clone())
}

/// What the slip says to watch for: `you often add -T`, `you often miss R`, `you often hit X for Y`.
fn watch_note(s: &Slip, struck: &str) -> String {
    match (s.added.as_slice(), s.missed.as_slice()) {
        ([a], []) => format!("you often add {}", NAMES[*a as usize]),
        ([], [m]) => format!("you often miss {}", NAMES[*m as usize]),
        ([a], [m]) => format!("you often hit {} for {}", NAMES[*a as usize], NAMES[*m as usize]),
        _ => format!("you often strike {struck}"),
    }
}

fn analyse(events: &[Ev], days: &[(u32, Option<i64>)], dict: &Dict, opt: &Options) -> Report {
    let r = replay(events);
    let mut rep = Report {
        thresholds: Thresholds { min_errors: opt.min_errors, min_attempts: opt.min_attempts, min_day_strokes: MIN_DAY_STROKES, gap_seconds: GAP },
        window: WindowInfo {
            days: days.len(),
            from: days.first().and_then(|d| d.1).map(date_string),
            to: days.last().and_then(|d| d.1).map(date_string),
        },
        ..Report::default()
    };
    rep.counts = Counts {
        strokes_kept: r.kept.len(),
        undos: r.undos,
        undos_empty_stack: r.undos_with_empty_stack,
        undo_count_matched_percent: 100.0 * rate(r.undo_matched, r.undo_checked),
        mistakes: r.fixes.len(),
        restruck: r.restruck,
        same_text: r.same_text,
        unrelated: r.unrelated,
        no_retry: r.no_retry,
        reordered: r.reordered,
    };
    // Successes: strokes that stayed typed, by word and by stroke
    let mut ok_word: HashMap<&str, usize> = HashMap::new();
    let mut ok_stroke: HashMap<&str, usize> = HashMap::new();
    let mut ok_day: HashMap<u32, usize> = HashMap::new();
    let mut wanted = [0usize; 24];
    for &i in &r.kept {
        let Kind::Text { word, .. } = &events[i].kind else { continue };
        *ok_word.entry(word).or_default() += 1;
        *ok_stroke.entry(&events[i].stroke).or_default() += 1;
        *ok_day.entry(events[i].day).or_default() += 1;
        if let Some(s) = Stroke::parse(&events[i].stroke) {
            bits(s.0).for_each(|b| wanted[b as usize] += 1);
        }
    }
    let mut classes: HashMap<&str, usize> = HashMap::new();
    let mut by_word: HashMap<&str, WordTally> = HashMap::new();
    let mut by_stroke: HashMap<&str, (usize, &str)> = HashMap::new();
    let mut added = [0usize; 24];
    let mut missed = [0usize; 24];
    let mut swaps: HashMap<(u32, u32), usize> = HashMap::new();
    let mut pairs: HashMap<(&str, &str), usize> = HashMap::new();
    let mut err_day: HashMap<u32, usize> = HashMap::new();
    for f in &r.fixes {
        *classes.entry(f.slip.class).or_default() += 1;
        let w = by_word.entry(&f.word).or_default();
        w.0 += 1;
        *w.1.entry(watch_note(&f.slip, &f.struck)).or_default() += 1;
        *w.2.entry(&f.intended).or_default() += 1;
        by_stroke.entry(&f.intended).or_insert((0, &f.word)).0 += 1;
        for &a in &f.slip.added {
            added[a as usize] += 1;
        }
        for &m in &f.slip.missed {
            missed[m as usize] += 1;
        }
        if let ([a], [m]) = (f.slip.added.as_slice(), f.slip.missed.as_slice()) {
            *swaps.entry((*a, *m)).or_default() += 1;
        }
        *pairs.entry((&f.struck, &f.intended)).or_default() += 1;
        *err_day.entry(f.day).or_default() += 1;
        if let Some(s) = Stroke::parse(&f.intended) {
            bits(s.0).for_each(|b| wanted[b as usize] += 1);
        }
    }
    let mut classes: Vec<(String, usize)> = classes.into_iter().map(|(k, v)| (k.to_owned(), v)).collect();
    classes.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    rep.classes = classes;

    let mut rows: Vec<WordRow> = by_word
        .iter()
        .filter(|(w, _)| dict.ordinary(w))
        .map(|(w, (errors, notes, strokes))| {
            let attempts = errors + ok_word.get(*w).copied().unwrap_or(0);
            let seen = note_for(&strokes.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect());
            WordRow {
                word: (*w).to_owned(),
                errors: *errors,
                attempts,
                rate: rate(*errors, attempts),
                outline: dict.outline_for(w, &seen).unwrap_or(seen),
                watch: note_for(notes),
            }
        })
        .collect();
    rows.sort_by(|a, b| b.errors.cmp(&a.errors).then(a.word.cmp(&b.word)));
    rep.words = rows.iter().filter(|w| w.errors >= opt.min_errors).take(opt.top).cloned().collect();
    rows.sort_by(|a, b| b.rate.total_cmp(&a.rate).then(b.errors.cmp(&a.errors)).then(a.word.cmp(&b.word)));
    rep.words_by_rate = rows.iter().filter(|w| w.errors >= opt.min_errors && w.attempts >= opt.min_attempts).take(opt.top).cloned().collect();

    let mut orows: Vec<OutlineRow> = by_stroke
        .iter()
        .filter(|(_, (_, w))| dict.ordinary(w))
        .map(|(o, (errors, w))| {
            let attempts = errors + ok_stroke.get(*o).copied().unwrap_or(0);
            OutlineRow { outline: (*o).to_owned(), word: (*w).to_owned(), errors: *errors, attempts, rate: rate(*errors, attempts) }
        })
        .filter(|o| o.errors >= opt.min_errors && o.attempts >= opt.min_attempts)
        .collect();
    orows.sort_by(|a, b| b.rate.total_cmp(&a.rate).then(b.errors.cmp(&a.errors)).then(a.outline.cmp(&b.outline)));
    orows.truncate(opt.top);
    rep.outlines_by_rate = orows;

    rep.keys = (0..24u32)
        .filter(|b| added[*b as usize] + missed[*b as usize] > 0)
        .map(|b| KeyRow {
            key: key_label(b),
            wanted: wanted[b as usize],
            missed: missed[b as usize],
            miss_rate: rate(missed[b as usize], wanted[b as usize]),
            added: added[b as usize],
        })
        .collect();
    rep.keys.sort_by(|a, b| (b.missed + b.added).cmp(&(a.missed + a.added)).then(a.key.cmp(&b.key)));
    let group = |name_of: fn(u32) -> &'static str| {
        let mut m: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
        for b in 1..23u32 {
            let e = m.entry(name_of(b)).or_default();
            e.0 += wanted[b as usize];
            e.1 += added[b as usize] + missed[b as usize];
        }
        let mut v: Vec<GroupRow> = m.into_iter().map(|(n, (p, e))| GroupRow { name: n.to_owned(), presses: p, errors: e, rate: rate(e, p) }).collect();
        v.sort_by(|a, b| b.rate.total_cmp(&a.rate).then(a.name.cmp(&b.name)));
        v
    };
    rep.fingers = group(finger);
    rep.rows = group(row);
    rep.hands = group(hand);
    let mut sw: Vec<_> = swaps.into_iter().map(|((a, m), n)| (key_label(a), key_label(m), n)).filter(|s| s.2 >= opt.min_errors).collect();
    sw.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)).then(a.1.cmp(&b.1)));
    sw.truncate(opt.top);
    rep.swaps = sw;

    let mut pr: Vec<PairRow> = pairs
        .into_iter()
        .filter(|(_, n)| *n >= opt.min_errors)
        .filter_map(|((s, i), n)| {
            let f = r.fixes.iter().find(|f| f.struck == s && f.intended == i)?;
            dict.ordinary(&f.word).then(|| PairRow {
                struck: s.to_owned(),
                intended: i.to_owned(),
                word: f.word.clone(),
                struck_word: f.struck_word.clone().filter(|w| dict.ordinary(w)),
                count: n,
            })
        })
        .collect();
    pr.sort_by(|a, b| b.count.cmp(&a.count).then(a.struck.cmp(&b.struck)).then(a.intended.cmp(&b.intended)));
    pr.truncate(opt.top);
    rep.pairs = pr;

    // Trend: per active day, mistakes against strokes attempted
    let mut tally: Vec<(u32, usize, usize)> = days
        .iter()
        .map(|(d, _)| {
            let errs = err_day.get(d).copied().unwrap_or(0);
            (*d, ok_day.get(d).copied().unwrap_or(0) + errs, errs)
        })
        .collect();
    tally.retain(|t| t.1 >= MIN_DAY_STROKES);
    if tally.len() >= 2 {
        let mid = tally.len() / 2;
        let half = |t: &[(u32, usize, usize)]| rate(t.iter().map(|x| x.2).sum(), t.iter().map(|x| x.1).sum());
        rep.halves = Some((half(&tally[..mid]), half(&tally[mid..])));
    }
    rep.days = tally
        .iter()
        .map(|(d, n, e)| DayRow {
            day: days.iter().find(|x| x.0 == *d).and_then(|x| x.1).map_or_else(|| format!("day {d}"), date_string),
            strokes: *n,
            mistakes: *e,
            rate: rate(*e, *n),
        })
        .collect();
    rep
}

// ---------------------------------------------------------------------------------------------
// Printing

fn pc(x: f64) -> String {
    format!("{:.1}%", 100.0 * x)
}

fn show(rep: &Report) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    let t = &rep.thresholds;
    let c = &rep.counts;
    let _ = writeln!(
        s,
        "Window: {} active days{}. Thresholds: at least {} mistakes (rates: and {} attempts); a retry within {} s; trend days with {}+ strokes.",
        rep.window.days,
        rep.window.from.as_ref().zip(rep.window.to.as_ref()).map_or_else(String::new, |(a, b)| format!(" (about {a} to {b})")),
        t.min_errors,
        t.min_attempts,
        t.gap_seconds,
        t.min_day_strokes
    );
    let _ = writeln!(
        s,
        "{} strokes kept, {} undos ({} with nothing to undo; {:.0}% matched the popped stroke's length).",
        c.strokes_kept, c.undos, c.undos_empty_stack, c.undo_count_matched_percent
    );
    let _ = writeln!(
        s,
        "{} mistakes found (near-miss then retry). Set aside: {} struck again unchanged, {} retyped another way, {} reordered pairs, {} not a near-miss, {} with no retry.\n",
        c.mistakes, c.restruck, c.same_text, c.reordered, c.unrelated, c.no_retry
    );
    let _ = writeln!(s, "How they miss");
    for (k, n) in &rep.classes {
        let _ = writeln!(s, "  {n:>6}  {k}");
    }
    let _ = writeln!(s, "\nWords with the most mistakes\n  {:<16} {:>6} {:>9} {:>7}  {:<14} watch", "word", "errors", "attempts", "rate", "outline");
    for w in &rep.words {
        let _ = writeln!(s, "  {:<16} {:>6} {:>9} {:>7}  {:<14} {}", w.word, w.errors, w.attempts, pc(w.rate), w.outline, w.watch);
    }
    let _ = writeln!(s, "\nWords by error rate\n  {:<16} {:>6} {:>9} {:>7}  {:<14} watch", "word", "errors", "attempts", "rate", "outline");
    for w in &rep.words_by_rate {
        let _ = writeln!(s, "  {:<16} {:>6} {:>9} {:>7}  {:<14} {}", w.word, w.errors, w.attempts, pc(w.rate), w.outline, w.watch);
    }
    let _ = writeln!(s, "\nStrokes by error rate (the intended stroke)\n  {:<14} {:<14} {:>6} {:>9} {:>7}", "stroke", "word", "errors", "attempts", "rate");
    for o in &rep.outlines_by_rate {
        let _ = writeln!(s, "  {:<14} {:<14} {:>6} {:>9} {:>7}", o.outline, o.word, o.errors, o.attempts, pc(o.rate));
    }
    let _ = writeln!(s, "\nKeys\n  {:<6} {:>8} {:>8} {:>7} {:>8}", "key", "wanted", "missed", "miss%", "added");
    for k in rep.keys.iter().take(14) {
        let _ = writeln!(s, "  {:<6} {:>8} {:>8} {:>7} {:>8}", k.key, k.wanted, k.missed, pc(k.miss_rate), k.added);
    }
    for (title, rows) in [("Fingers", &rep.fingers), ("Rows", &rep.rows), ("Hands", &rep.hands)] {
        let _ = writeln!(s, "\n{title} (key errors per key press)");
        for g in rows {
            let _ = writeln!(s, "  {:<16} {:>8} presses {:>6} errors {:>7}", g.name, g.presses, g.errors, pc(g.rate));
        }
    }
    let _ = writeln!(s, "\nKeys confused (struck for wanted)");
    for (a, m, n) in &rep.swaps {
        let _ = writeln!(s, "  {n:>6}  {a} for {m}");
    }
    let _ = writeln!(s, "\nCommonest near-misses (struck -> intended)");
    for p in &rep.pairs {
        let got = p.struck_word.as_ref().map_or_else(|| "no entry".to_owned(), |w| format!("\"{w}\""));
        let _ = writeln!(s, "  {:>6}  {:<14} {} -> {:<14} \"{}\"", p.count, p.struck, got, p.intended, p.word);
    }
    let _ = writeln!(s, "\nPer day (mistakes per stroke attempted)");
    for d in rep.days.iter().rev().take(14).rev() {
        let _ = writeln!(s, "  {}  {:>7} strokes {:>6} mistakes {:>7}", d.day, d.strokes, d.mistakes, pc(d.rate));
    }
    if let Some((a, b)) = rep.halves {
        let _ = writeln!(s, "  earlier half of the days {}, later half {}", pc(a), pc(b));
    }
    s
}

// ---------------------------------------------------------------------------------------------
// Drills

#[derive(Serialize, Debug)]
struct Item {
    p: String,
    o: Vec<String>,
    w: String,
    n: String,
}

#[derive(Serialize, Debug)]
struct PairDrill {
    title: String,
    items: Vec<Item>,
}

#[derive(Serialize, Debug)]
struct Drills {
    v: u32,
    title: String,
    items: Vec<Item>,
    pairs: Vec<PairDrill>,
}

/// Weighted by error rate times how often the word is typed (its attempts): a word gets one to
/// three copies in the set, in proportion, so the commonest slips are met most.
fn drills(rep: &Report, dict: &Dict, all: &[WordRow]) -> Drills {
    let weight = |w: &WordRow| w.rate * w.attempts as f64;
    let mut ranked: Vec<&WordRow> = all.iter().filter(|w| w.errors >= rep.thresholds.min_errors).collect();
    ranked.sort_by(|a, b| weight(b).total_cmp(&weight(a)).then(a.word.cmp(&b.word)));
    ranked.truncate(40);
    let top = ranked.first().map_or(1.0, |w| weight(w)).max(f64::MIN_POSITIVE);
    let mut items = Vec::new();
    for w in ranked {
        let copies = 1 + (2.0 * weight(w) / top).floor() as usize;
        for _ in 0..copies.min(3) {
            items.push(Item { p: w.word.clone(), o: vec![w.outline.clone()], w: w.word.clone(), n: format!("{} ({} of {})", w.watch, w.errors, w.attempts) });
        }
    }
    let mut pairs = Vec::new();
    for p in &rep.pairs {
        let Some(sw) = p.struck_word.as_ref().filter(|w| **w != p.word) else { continue };
        let (Some(a), Some(b)) = (dict.outline_for(sw, &p.struck), dict.outline_for(&p.word, &p.intended)) else { continue };
        pairs.push(PairDrill {
            title: format!("{sw} / {}", p.word),
            items: vec![
                Item { p: p.word.clone(), o: vec![b.clone()], w: p.word.clone(), n: format!("not {sw} ({a}): struck {} times", p.count) },
                Item { p: sw.clone(), o: vec![a], w: sw.clone(), n: format!("not {} ({b})", p.word) },
            ],
        });
    }
    Drills { v: 1, title: "My drills".to_owned(), items, pairs }
}

/// The drill words as plain text for Monkeytype's Custom text box (Custom -> Change -> paste or
/// import): `n` words in lines of ten, the weighted words cycled, with every fourth word a
/// confusable partner (`late lead late lead` runs, so the two are struck against each other).
fn monkeytype(d: &Drills, n: usize) -> String {
    let pool: Vec<&str> = d.items.iter().map(|i| i.w.as_str()).collect();
    let partners: Vec<&str> = d.pairs.iter().flat_map(|p| p.items.iter().cycle().take(4).map(|i| i.w.as_str())).collect();
    if pool.is_empty() {
        return String::new();
    }
    let (mut a, mut b) = (0usize, 0usize);
    let mut words = Vec::with_capacity(n);
    for i in 0..n {
        if i % 4 == 3 && !partners.is_empty() {
            words.push(partners[b % partners.len()]);
            b += 1;
        } else {
            // a stride through the list, so a word's copies are not side by side
            words.push(pool[(a * 7 + a / pool.len()) % pool.len()]);
            a += 1;
        }
    }
    let mut out: String = words.chunks(10).map(|c| c.join(" ")).collect::<Vec<_>>().join("\n");
    out.push('\n');
    out
}

// ---------------------------------------------------------------------------------------------

pub fn run(opt: &Options) -> Result<()> {
    let text = std::fs::read_to_string(opt.tape).with_context(|| opt.tape.display().to_string())?;
    let since = opt.since.map(|s| parse_date(s).with_context(|| format!("--since wants YYYY-MM-DD, got {s}"))).transpose()?;
    let all = parse_tape(&text);
    let Some(last) = all.last() else { bail!("no strokes on the tape") };
    let anchor = anchor_day(opt.tape, last.sec);
    if since.is_some() && anchor.is_none() {
        bail!("cannot date the tape; use --last N");
    }
    let (events, days) = window(all, opt.last, since, anchor);
    let dict = Dict::load(opt.dict)?;
    let rep = analyse(&events, &days, &dict, opt);
    let out_dir = crate::paths::mistakes_dir();
    let write = |path: &Path, body: &str| -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, body).with_context(|| path.display().to_string())
    };
    if opt.drill {
        let every = analyse(&events, &days, &dict, &Options { top: usize::MAX, min_errors: 1, min_attempts: 0, ..*opt });
        let mut rows = every.words;
        rows.retain(|w| w.errors >= opt.min_errors);
        let d = drills(&rep, &dict, &rows);
        let path = opt.out.map_or_else(|| out_dir.join("drills.json"), Path::to_path_buf);
        write(&path, &serde_json::to_string_pretty(&d)?)?;
        println!("{} drill items and {} confusable pairs written to {}", d.items.len(), d.pairs.len(), path.display());
        let text = monkeytype(&d, opt.words);
        if !text.is_empty() {
            let mt = path.with_file_name("monkeytype-drill.txt");
            write(&mt, &text)?;
            println!(
                "{} words for Monkeytype written to {}\nMonkeytype: Custom -> Change -> paste the text or Import file (words only, no pipes needed).",
                text.split_whitespace().count(),
                mt.display()
            );
        }
        return Ok(());
    }
    if opt.json {
        println!("{}", serde_json::to_string_pretty(&rep)?);
    } else {
        print!("{}", show(&rep));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dict() -> Dict {
        let mut d = Dict::default();
        for (o, w) in [("HRAET", "late"), ("HRAED", "lead"), ("-T", "the"), ("-F", "of"), ("KAT", "cat"), ("TKOG", "dog")] {
            d.add(o, w);
        }
        d
    }

    fn opts() -> Options<'static> {
        Options { tape: Path::new(""), dict: Path::new(""), last: None, since: None, min_errors: 1, min_attempts: 0, top: 20, json: false, drill: false, out: None, words: 40 }
    }

    fn run_tape(tape: &str) -> (Report, Replay) {
        let events = parse_tape(tape);
        let days: Vec<(u32, Option<i64>)> = {
            let mut d: Vec<u32> = events.iter().map(|e| e.day).collect();
            d.dedup();
            d.into_iter().map(|x| (x, None)).collect()
        };
        (analyse(&events, &days, &dict(), &opts()), replay(&events))
    }

    #[test]
    fn an_undone_near_miss_pairs_with_the_retry() {
        let (rep, r) = run_tape("10:00:00  HRAET        \" late\"\n10:00:01  *            -5 \"\"\n10:00:02  HRAED        \" lead\"\n10:00:03  KAT          \" cat\"\n");
        assert_eq!(r.fixes.len(), 1, "{:?}", r.fixes);
        assert_eq!(r.fixes[0].struck, "HRAET");
        assert_eq!(r.fixes[0].word, "lead");
        assert_eq!(rep.words[0].word, "lead");
        assert_eq!(rep.words[0].errors, 1);
        // the retry and the cat stayed typed; the undone stroke did not
        assert_eq!(rep.counts.strokes_kept, 2);
        assert_eq!(rep.words[0].attempts, 2);
        assert_eq!(rep.words[0].watch, "you often hit -T for -D");
    }

    #[test]
    fn a_miss_with_no_undo_pairs_with_the_next_word() {
        let (_, r) = run_tape("10:00:00  KATS         ?\n10:00:01  KAT          \" cat\"\n");
        assert_eq!(r.fixes.len(), 1, "{:?}", r.fixes);
        assert_eq!(r.fixes[0].slip.class, "one key added");
        assert_eq!(r.fixes[0].slip.added, vec![20]);
    }

    #[test]
    fn a_chain_of_undos_pairs_each_with_the_final_retry() {
        let (_, r) = run_tape(
            "10:00:00  KATS         \" cats\"\n10:00:01  *            -5 \"\"\n10:00:02  KAT          \" cat\"\n10:00:03  *            -4 \"\"\n10:00:04  KAT          \" cat\"\n",
        );
        // KATS -> KAT once; the second KAT was undone and struck again unchanged
        assert_eq!(r.fixes.len(), 1, "{:?}", r.fixes);
        assert_eq!(r.restruck, 1);
    }

    #[test]
    fn an_unrelated_word_after_an_undo_is_not_a_mistake() {
        let (_, r) = run_tape("10:00:00  KAT          \" cat\"\n10:00:01  *            -4 \"\"\n10:00:02  TKOG         \" dog\"\n");
        assert!(r.fixes.is_empty(), "{:?}", r.fixes);
        assert_eq!(r.unrelated, 1);
    }

    #[test]
    fn a_late_retry_is_not_a_retry() {
        let (_, r) = run_tape("10:00:00  HRAET        \" late\"\n10:00:01  *            -5 \"\"\n10:05:00  HRAED        \" lead\"\n");
        assert!(r.fixes.is_empty(), "{:?}", r.fixes);
        assert_eq!(r.no_retry, 1);
    }

    #[test]
    fn typed_lines_are_a_barrier_and_never_read() {
        let (_, r) = run_tape("10:00:00  HRAET        \" late\"\n10:00:01  typed        \"hunter2\"\n10:00:01  *            -5 \"\"\n10:00:02  HRAED        \" lead\"\n");
        assert!(r.fixes.is_empty(), "{:?}", r.fixes);
        let events = parse_tape("10:00:01  typed        \"hunter2\"\n");
        assert_eq!(events[0].kind, Kind::Typed);
        assert!(events[0].stroke.is_empty(), "the typed text must not be kept: {:?}", events[0]);
    }

    #[test]
    fn near_miss_classes() {
        let class = |a: &str, b: &str| slip(a, b).map(|s| s.class);
        assert_eq!(class("KATS", "KAT"), Some("one key added"));
        assert_eq!(class("KA", "KAT"), Some("one key missing"));
        assert_eq!(class("HRAET", "HRAED"), Some("a neighbour key instead"));
        assert_eq!(class("S", "-S"), Some("the other hand's twin key"));
        assert_eq!(class("TAT", "KAT"), Some("a neighbour key instead"));
        assert_eq!(class("KAT", "KAT"), None);
        assert_eq!(class("KAT", "TKOG"), None);
        assert_eq!(class("KAT", "KAB"), Some("a distant key instead"));
        assert_eq!(class("KPAT", "KAT"), Some("one key added"));
        assert_eq!(class("KAPLT", "KAT"), Some("two keys added"));
    }

    #[test]
    fn reordered_strokes_are_one_reordered_pair() {
        let (_, r) = run_tape("10:00:00  -F           \" of\"\n10:00:00  -T           \" the\"\n10:00:01  *            -4 \"\"\n10:00:01  *            -3 \"\"\n10:00:02  -T           \" the\"\n10:00:02  -F           \" of\"\n");
        assert_eq!(r.reordered, 1, "{r:?}");
        assert!(r.fixes.is_empty(), "{:?}", r.fixes);
    }

    #[test]
    fn rates_divide_errors_by_attempts() {
        let tape = "10:00:00  HRAET        \" late\"\n10:00:01  *            -5 \"\"\n10:00:02  HRAED        \" lead\"\n10:00:03  HRAED        \" lead\"\n10:00:04  HRAED        \" lead\"\n";
        let (rep, _) = run_tape(tape);
        let w = &rep.words[0];
        assert_eq!((w.errors, w.attempts), (1, 4), "{w:?}");
        assert!((w.rate - 0.25).abs() < 1e-9, "{}", w.rate);
        assert!((rate(0, 0)).abs() < 1e-9);
    }

    #[test]
    fn key_tallies_name_the_key_added() {
        let (rep, _) = run_tape("10:00:00  KATS         ?\n10:00:01  KAT          \" cat\"\n");
        let s = rep.keys.iter().find(|k| k.key == "-S").expect("-S in the key table");
        assert_eq!((s.added, s.missed), (1, 0));
        assert_eq!(rep.fingers[0].name, "right ring");
    }

    #[test]
    fn only_ordinary_dictionary_words_reach_the_output() {
        // `hunter2` and `zzqx` are not dictionary words: no word row, though the mistake is counted
        let mut d = dict();
        d.add("KATS", "hunter2");
        let events = parse_tape("10:00:00  KATS         \" zzqx\"\n10:00:01  *            -5 \"\"\n10:00:02  KAT          \" zzqx\"\n10:00:03  KATS         ?\n10:00:04  TKOG         \" dog\"\n");
        let days = vec![(0, None)];
        let rep = analyse(&events, &days, &d, &opts());
        assert!(rep.words.is_empty(), "{:?}", rep.words);
        assert!(d.ordinary("late") && !d.ordinary("hunter2") && !d.ordinary("zzqx"));
        let blob = serde_json::to_string(&rep).unwrap();
        assert!(!blob.contains("zzqx") && !blob.contains("hunter2"), "{blob}");
    }

    #[test]
    fn days_split_where_the_clock_goes_back_and_windows_cut_them() {
        let events = parse_tape("23:59:00  KAT          \" cat\"\n00:00:10  KAT          \" cat\"\n00:00:20  KAT          \" cat\"\n");
        assert_eq!(events.iter().map(|e| e.day).collect::<Vec<_>>(), vec![0, 1, 1]);
        let anchor = parse_date("2026-11-20");
        let (kept, days) = window(events.clone(), Some(1), None, anchor);
        assert_eq!((kept.len(), days.len()), (2, 1));
        assert_eq!(days[0].1.map(date_string).as_deref(), Some("2026-11-20"));
        let (kept, _) = window(events.clone(), None, parse_date("2026-11-20"), anchor);
        assert_eq!(kept.len(), 2);
        let (kept, days) = window(events, None, parse_date("2026-11-19"), anchor);
        assert_eq!((kept.len(), days.len()), (3, 2));
        assert_eq!(date_string(days_from_civil(2026, 2, 28) + 1), "2026-03-01");
    }

    #[test]
    fn the_monkeytype_text_is_words_only_with_partners_woven_in() {
        let tape = "10:00:00  HRAET        \" late\"\n10:00:01  *            -5 \"\"\n10:00:02  HRAED        \" lead\"\n";
        let (rep, _) = run_tape(tape);
        let d = drills(&rep, &dict(), &rep.words);
        let text = monkeytype(&d, 24);
        assert_eq!(text.split_whitespace().count(), 24, "{text}");
        assert!(text.split_whitespace().all(|w| w == "late" || w == "lead"), "{text}");
        assert_eq!(text.lines().count(), 3, "{text}");
        assert!(!text.contains('|') && !text.contains("HRAED"), "{text}");
    }

    #[test]
    fn drills_carry_the_outline_and_the_slip() {
        let tape = "10:00:00  HRAET        \" late\"\n10:00:01  *            -5 \"\"\n10:00:02  HRAED        \" lead\"\n";
        let (rep, _) = run_tape(tape);
        let d = drills(&rep, &dict(), &rep.words);
        assert_eq!(d.items[0].o, vec!["HRAED".to_owned()]);
        assert!(d.items[0].n.starts_with("you often hit -T for -D"), "{}", d.items[0].n);
        assert_eq!(d.pairs.len(), 1, "{:?}", d.pairs);
        assert_eq!(d.pairs[0].items.len(), 2);
    }
}

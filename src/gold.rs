//! What Plover and Lapwing write for each word: the answers the rules are scored against.

use crate::data::load_dictionary;
use crate::lookup_cache::{Reader, Writer};
use crate::reverse::{Composition, Glue, compose};
use crate::stroke::{Stroke, parse_outline};
use anyhow::Result;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

/// Every outline a dictionary has for each plain lowercase word, shortest first.
pub fn all_outlines(dict: &BTreeMap<String, String>) -> HashMap<String, Vec<String>> {
    let mut words: HashMap<String, Vec<String>> = HashMap::new();
    for (outline, word) in dict {
        let plain = !word.is_empty()
            && word.chars().all(|c| c.is_ascii_lowercase() || c == '\'')
            && word.chars().any(|c| c.is_ascii_lowercase());
        // Only outlines that are strokes (no numbers)
        if plain && parse_outline(outline).is_some() {
            words.entry(word.clone()).or_default().push(outline.clone());
        }
    }
    for outlines in words.values_mut() {
        outlines.sort_by_key(|o| (o.split('/').count(), o.len(), o.clone()));
    }
    words
}

/// Every word a dictionary writes with a capital first letter and the rest lowercase (`Matt`),
/// lowercased.
fn capitalized(dict: &BTreeMap<String, String>) -> impl Iterator<Item = String> + '_ {
    dict.values().filter_map(|word| {
        let mut letters = word.chars();
        let first = letters.next()?;
        (first.is_ascii_uppercase() && letters.clone().all(|c| c.is_ascii_lowercase()) && letters.next().is_some())
            .then(|| word.to_ascii_lowercase())
    })
}

/// Which of the two gold dictionaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theory {
    Plover,
    Lapwing,
}

/// What a gold dictionary offers to write a word it lacks: its standalone suffix strokes
/// (`{^'s}`, `{^ing}`) by the text they add, and its `{^}` attach stroke. The same pieces lyk
/// composes from, so a word missing from all three is judged the same way in each.
#[derive(Default)]
struct Pieces {
    suffixes: Vec<(String, Vec<Stroke>)>,
    glue: Option<Vec<Stroke>>,
}

/// The shortest (strokes, then keys) of `a` and `b`.
fn cheaper(a: Vec<Stroke>, b: &[Stroke]) -> Vec<Stroke> {
    let cost = |o: &[Stroke]| (o.len(), crate::generate::keys(o));
    if cost(b) < cost(&a) { b.to_vec() } else { a }
}

fn pieces(dict: &BTreeMap<String, String>) -> Pieces {
    let mut suffixes: HashMap<String, Vec<Stroke>> = HashMap::new();
    let mut glue: Option<Vec<Stroke>> = None;
    for (outline, word) in dict {
        let Some(strokes) = parse_outline(outline) else { continue };
        if word == "{^}" {
            glue = Some(match glue {
                Some(held) => cheaper(held, &strokes),
                None => strokes,
            });
            continue;
        }
        let Some(text) = word.strip_prefix("{^").and_then(|w| w.strip_suffix('}')) else { continue };
        if text.is_empty() || !text.chars().all(|c| c.is_ascii_lowercase() || c == '\'') {
            continue;
        }
        let held = suffixes.remove(text);
        suffixes.insert(text.to_owned(), held.map_or_else(|| strokes.clone(), |h| cheaper(h, &strokes)));
    }
    Pieces { suffixes: suffixes.into_iter().collect(), glue }
}

pub struct Gold {
    pub plover: HashMap<String, Vec<String>>,
    pub lapwing: HashMap<String, Vec<String>>,
    plover_pieces: Pieces,
    lapwing_pieces: Pieces,
    /// Words either dictionary writes capitalized and neither writes lowercase: names
    names: HashSet<String>,
}

impl Gold {
    pub fn load(root: &Path) -> Result<Self> {
        let dicts = root.join("dictionaries");
        let (plover, lapwing) =
            (load_dictionary(&dicts.join("plover-main.json"))?, load_dictionary(&dicts.join("lapwing-base.json"))?);
        let mut gold = Self {
            plover: all_outlines(&plover),
            lapwing: all_outlines(&lapwing),
            plover_pieces: pieces(&plover),
            lapwing_pieces: pieces(&lapwing),
            names: HashSet::new(),
        };
        gold.names = capitalized(&plover)
            .chain(capitalized(&lapwing))
            .filter(|word| !gold.plover.contains_key(word) && !gold.lapwing.contains_key(word))
            .collect();
        Ok(gold)
    }

    /// Whether the word is a name: neither dictionary writes it lowercase, and one of them writes
    /// it capitalized (`Matt`, never `matt`).
    pub fn is_name(&self, word: &str) -> bool {
        self.names.contains(word)
    }

    /// Every such name, lowercase.
    pub fn names(&self) -> impl Iterator<Item = &String> {
        self.names.iter()
    }

    /// The words either dictionary writes.
    pub fn words(&self) -> HashSet<&str> {
        self.plover
            .keys()
            .chain(self.lapwing.keys())
            .map(String::as_str)
            .collect()
    }

    /// Whether Plover and Lapwing write the word with this outline, comparing shapes: the join
    /// key, and the `KWR` or star that Lapwing and Plover put on a continuation stroke to mark it
    /// as one, are not counted.
    pub fn writes(&self, word: &str, outline: &str) -> (bool, bool) {
        let wanted = shape(outline);
        let has = |map: &HashMap<String, Vec<String>>| {
            map.get(word)
                .is_some_and(|all| all.iter().any(|o| shape(o) == wanted))
        };
        (has(&self.plover), has(&self.lapwing))
    }

    /// A word `theory` lacks, written from the pieces it does have: one of its words plus a
    /// `{^suffix}` stroke, or two of its words with its `{^}` attach stroke between. The same
    /// idea as lyk's composition (`reverse::compose`), so no theory is judged on a word only the
    /// others were allowed to compose.
    pub fn compose(&self, theory: Theory, word: &str) -> Option<Composition> {
        let (words, pieces) = match theory {
            Theory::Plover => (&self.plover, &self.plover_pieces),
            Theory::Lapwing => (&self.lapwing, &self.lapwing_pieces),
        };
        let suffixes: Vec<(&str, Vec<Stroke>)> = pieces.suffixes.iter().map(|(t, s)| (t.as_str(), s.clone())).collect();
        let glue = pieces.glue.as_deref().map_or(Glue::Unavailable, Glue::Stroke);
        compose(|w| words.get(w).and_then(|all| crate::generate::shortest(all)), &suffixes, glue, word)
    }

    pub fn shortest(&self, word: &str) -> (Option<&str>, Option<&str>) {
        (
            self.plover
                .get(word)
                .and_then(|v| v.first())
                .map(String::as_str),
            self.lapwing
                .get(word)
                .and_then(|v| v.first())
                .map(String::as_str),
        )
    }

    /// Both dictionaries as `lookup`'s cache keeps them (see `crate::lookup_cache`).
    pub fn write(&self, w: &mut Writer) {
        for words in [&self.plover, &self.lapwing] {
            w.map(words, |w, outlines| w.strs(outlines));
        }
        for pieces in [&self.plover_pieces, &self.lapwing_pieces] {
            w.len(pieces.suffixes.len());
            for (text, strokes) in &pieces.suffixes {
                w.str(text);
                w.strokes(strokes);
            }
            w.len(usize::from(pieces.glue.is_some()));
            w.strokes(pieces.glue.as_deref().unwrap_or_default());
        }
        let mut names: Vec<&String> = self.names.iter().collect();
        names.sort_unstable();
        w.len(names.len());
        for name in names {
            w.str(name);
        }
    }

    /// What [`Gold::write`] wrote.
    pub fn read(r: &mut Reader<'_>) -> Option<Self> {
        let plover = r.map(Reader::strings)?;
        let lapwing = r.map(Reader::strings)?;
        let mut pieces = || -> Option<Pieces> {
            let n = r.count()?;
            let suffixes = (0..n).map(|_| Some((r.string()?, r.strokes()?))).collect::<Option<_>>()?;
            let has_glue = r.len()? == 1;
            let glue = r.strokes()?;
            Some(Pieces { suffixes, glue: has_glue.then_some(glue) })
        };
        let (plover_pieces, lapwing_pieces) = (pieces()?, pieces()?);
        let names = r.strings()?.into_iter().collect();
        Some(Self { plover, lapwing, plover_pieces, lapwing_pieces, names })
    }
}

/// An outline with the markers of a continuation stroke taken off: `+`, a leading `KWR` before a
/// vowel, and a star.
fn shape(outline: &str) -> String {
    outline
        .split('/')
        .enumerate()
        .map(|(n, stroke)| {
            if n == 0 {
                return stroke.to_owned();
            }
            let stroke = stroke.strip_prefix('+').unwrap_or(stroke);
            let stroke = match stroke.strip_prefix("KWR") {
                Some(rest) if rest.starts_with(['A', 'O', 'E', 'U', '*']) => rest,
                _ => stroke,
            };
            stroke.replace('*', "")
        })
        .collect::<Vec<_>>()
        .join("/")
}

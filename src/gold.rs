//! What Plover and Lapwing write for each word: the answers the rules are scored against.

use crate::data::load_dictionary;
use crate::stroke::parse_outline;
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

pub struct Gold {
    pub plover: HashMap<String, Vec<String>>,
    pub lapwing: HashMap<String, Vec<String>>,
    /// Words either dictionary writes capitalized and neither writes lowercase: names
    names: HashSet<String>,
}

impl Gold {
    pub fn load(root: &Path) -> Result<Self> {
        let dicts = root.join("dictionaries");
        let (plover, lapwing) =
            (load_dictionary(&dicts.join("plover-main.json"))?, load_dictionary(&dicts.join("lapwing-base.json"))?);
        let mut gold = Self { plover: all_outlines(&plover), lapwing: all_outlines(&lapwing), names: HashSet::new() };
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

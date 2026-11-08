//! The solver: a run of strokes that missed the dictionary, read as the word it most likely meant.
//!
//! [`core`] is the decoder, self-contained so the keymux plugin runs the very same file; this
//! module packs what it reads (the rules' chord tables, every word's sounds and spelling, the
//! known outlines) into one blob ([`pack`]), and holds the `solve` command's eval. See
//! `plans/09-wasm-fallback-resolver.md` and `site/reference/fallback.json`.

pub mod core;
pub mod plugin;

pub use self::core::{Model, Reading};

use super::orthography::{self, Phone};
use super::resolve::default_spelling;
use super::{Rules, plain};
use crate::data::Pronunciations;
use crate::generate::{Entry, Origin};
use crate::stroke::Stroke;
use std::collections::HashMap;

/// The ARPAbet sounds, vowels first: every sound the lexicon or the chord tables use.
const VOWELS: [&str; 15] = ["AA", "AE", "AH", "AO", "AW", "AY", "EH", "ER", "EY", "IH", "IY", "OW", "OY", "UH", "UW"];
const CONSONANTS: [&str; 24] = [
    "B", "CH", "D", "DH", "F", "G", "HH", "JH", "K", "L", "M", "N", "NG", "P", "R", "S", "SH", "T", "TH", "V", "W", "Y", "Z", "ZH",
];
/// Consonants a typist (or a chord) may write for each other at a small cost.
const CONFUSABLE: [(&str, &str); 10] = [
    ("P", "B"),
    ("T", "D"),
    ("K", "G"),
    ("F", "V"),
    ("TH", "DH"),
    ("S", "Z"),
    ("SH", "ZH"),
    ("CH", "JH"),
    ("N", "NG"),
    ("M", "N"),
];

/// What skipping a sound of the word costs, by what it is.
const DROP_UNSTRESSED: u8 = 1;
const DROP_SECONDARY: u8 = 3;
const DROP_STRESSED: u8 = 8;
const DROP_CONSONANT: u8 = 4;
/// `HH` and the `Y` glide are often not struck at all.
const DROP_WEAK_CONSONANT: u8 = 2;
/// A word reached through its spelling rather than its sounds.
const SPELLED_PATH: u8 = 2;

struct Sounds {
    ids: HashMap<String, u8>,
    names: Vec<String>,
}

impl Sounds {
    fn new() -> Self {
        let mut s = Self { ids: HashMap::new(), names: Vec::new() };
        for name in VOWELS.iter().chain(CONSONANTS.iter()) {
            s.id(name);
        }
        s
    }
    fn id(&mut self, sound: &str) -> u8 {
        let name = plain(sound);
        if let Some(&id) = self.ids.get(name) {
            return id;
        }
        let id = u8::try_from(self.names.len()).expect("fewer than 64 sounds");
        assert!(id < 64, "the solver's edge masks hold 64 sounds");
        self.ids.insert(name.to_owned(), id);
        self.names.push(name.to_owned());
        id
    }
}

/// The vowel letters in `text`, one bit each: `a e i o u y`.
fn letters(text: &str) -> u8 {
    text.chars()
        .map(|c| match c {
            'a' => 1,
            'e' => 2,
            'i' => 4,
            'o' => 8,
            'u' => 16,
            'y' => 32,
            _ => 0,
        })
        .fold(0, |a, b| a | b)
}

/// The letters a vowel sound is commonly spelled with: what lets the aligner below tell which
/// group of vowel letters in a word spells which vowel sound.
fn spellable(sound: &str) -> u8 {
    letters(match sound {
        "AA" => "ao",
        "AE" => "a",
        "AH" => "aeiou",
        "AO" => "aou",
        "AW" => "ou",
        "AY" => "iey",
        "EH" => "ea",
        "ER" => "eiuoy",
        "EY" => "aeiy",
        "IH" => "iey",
        "IY" => "eiya",
        "OW" => "oeau",
        "OY" => "oiy",
        "UH" => "ou",
        "UW" => "oeiu",
        _ => "",
    })
}

/// Which vowel letters spell each vowel sound of `sounds` in `word` (0 for consonants and for a
/// vowel no letters could be given to): the word's runs of vowel letters matched to its vowel
/// sounds in order, a run skipped where it is silent (a final `e` freely).
fn align(word: &str, sounds: &[String]) -> Vec<u8> {
    let chars: Vec<char> = word.chars().collect();
    let mut groups: Vec<(u8, bool)> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let is_v = |j: usize| {
            let c = chars[j];
            "aeiou".contains(c) || (c == 'y' && j > 0)
        };
        if is_v(i) {
            let start = i;
            while i < chars.len() && is_v(i) {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            let silent_e = text == "e" && i == chars.len() && start > 0;
            groups.push((letters(&text), silent_e));
        } else {
            i += 1;
        }
    }
    let vowels: Vec<usize> = (0..sounds.len()).filter(|&i| sounds[i].ends_with(|c: char| c.is_ascii_digit())).collect();
    let (n, m) = (vowels.len(), groups.len());
    const INF: u32 = u32::MAX / 4;
    let mut dp = vec![vec![INF; m + 1]; n + 1];
    dp[0][0] = 0;
    for a in 0..=n {
        for b in 0..=m {
            let here = dp[a][b];
            if here >= INF {
                continue;
            }
            if b < m {
                let skip = if groups[b].1 { 0 } else { 2 };
                dp[a][b + 1] = dp[a][b + 1].min(here + skip);
            }
            if a < n {
                dp[a + 1][b] = dp[a + 1][b].min(here + 3);
                if b < m {
                    let fits = spellable(plain(&sounds[vowels[a]])) & groups[b].0 != 0;
                    dp[a + 1][b + 1] = dp[a + 1][b + 1].min(here + if fits { 0 } else { 1 });
                }
            }
        }
    }
    let mut out = vec![0u8; sounds.len()];
    let (mut a, mut b) = (n, m);
    while a > 0 || b > 0 {
        let here = dp[a][b];
        if a > 0 && b > 0 {
            let fits = spellable(plain(&sounds[vowels[a - 1]])) & groups[b - 1].0 != 0;
            if dp[a - 1][b - 1] + if fits { 0 } else { 1 } == here {
                out[vowels[a - 1]] = groups[b - 1].0;
                a -= 1;
                b -= 1;
                continue;
            }
        }
        if a > 0 && dp[a - 1][b] + 3 == here {
            a -= 1;
            continue;
        }
        b -= 1;
    }
    out
}

/// The letters an orthographic reading's vowel came from, recovered from its sounds.
fn phone_letters(phone: &Phone) -> u8 {
    let heard = plain(&phone.heard);
    if phone.heard == phone.long && phone.long == phone.weak {
        // A vowel pair (`ea`, `oo`) or `-tio-`: every pair that reads as this sound
        let mut mask = 0;
        for (pair, sound) in orthography::VOWEL_PAIRS {
            if sound == heard {
                mask |= letters(pair);
            }
        }
        return if mask == 0 { letters("io") } else { mask };
    }
    let mut mask = 0;
    for c in ['a', 'e', 'i', 'o', 'u'] {
        let (short, _) = orthography::short_long(c);
        if short == heard {
            mask |= letters(&c.to_string());
        }
    }
    if heard == "IH" {
        mask |= letters("y");
    }
    mask
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct EdgeKey {
    mask: u64,
    letters: u8,
    drop: u8,
    vowel: bool,
}

#[derive(Default)]
struct Trie {
    children: Vec<HashMap<EdgeKey, u32>>,
    ends: Vec<Vec<(u32, u8)>>,
}

impl Trie {
    fn insert(&mut self, path: &[EdgeKey], word: u32, extra: u8) {
        if self.children.is_empty() {
            self.children.push(HashMap::new());
            self.ends.push(Vec::new());
        }
        let mut at = 0usize;
        for key in path {
            at = match self.children[at].get(key) {
                Some(&child) => child as usize,
                None => {
                    let child = self.children.len();
                    self.children.push(HashMap::new());
                    self.ends.push(Vec::new());
                    self.children[at].insert(*key, child as u32);
                    child
                }
            };
        }
        match self.ends[at].iter_mut().find(|(w, _)| *w == word) {
            Some(end) => end.1 = end.1.min(extra),
            None => self.ends[at].push((word, extra)),
        }
    }
}

struct Blob(Vec<u8>);

impl Blob {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn len(&mut self, n: usize) {
        self.u32(u32::try_from(n).expect("table fits"));
    }
    fn str(&mut self, s: &str) {
        self.u16(u16::try_from(s.len()).expect("short text"));
        self.0.extend_from_slice(s.as_bytes());
    }
    fn bytes(&mut self, b: &[u8]) {
        self.u8(u8::try_from(b.len()).expect("short token list"));
        self.0.extend_from_slice(b);
    }
}

/// What the lexicon holds: every word ranked among the `common` most frequent that has a
/// pronunciation, its sounds, and its rank.
pub struct Lexicon<'a> {
    pub pron: &'a Pronunciations,
    pub rank: &'a HashMap<String, usize>,
    pub common: usize,
}

/// The known outlines a run may contain as exact pieces: every entry of the dictionary that is a
/// word's own outline (not a backup, a phrase or a standalone suffix).
pub fn stems(entries: &[Entry]) -> Vec<(Vec<Stroke>, String)> {
    entries
        .iter()
        .filter(|e| !matches!(e.origin, Origin::Backup | Origin::Phrase | Origin::Suffix))
        .map(|e| (e.outline.clone(), e.word.clone()))
        .collect()
}

/// Everything [`Model::load`] reads, packed.
#[must_use]
pub fn pack(rules: &Rules, lexicon: &Lexicon, stems: &[(Vec<Stroke>, String)]) -> Vec<u8> {
    let mut sounds = Sounds::new();
    let chords = |table: &[(&HashMap<String, Stroke>)], sounds: &mut Sounds| -> Vec<(u32, Vec<u8>)> {
        let mut out: Vec<(u32, Vec<u8>)> = table
            .iter()
            .flat_map(|t| t.iter())
            .map(|(label, stroke)| (stroke.0, label.split_whitespace().map(|s| sounds.id(s)).collect()))
            .collect();
        out.sort();
        out.dedup();
        out
    };
    let onset = chords(&[&rules.onset, &rules.onset_cluster], &mut sounds);
    let coda = chords(&[&rules.coda, &rules.coda_cluster], &mut sounds);
    let mut vowel_readings: Vec<(u32, u8, u8)> = rules
        .vowel
        .iter()
        .filter(|(label, _)| !label.ends_with(|c: char| c.is_ascii_digit()))
        .map(|(label, stroke)| {
            let spelled = label.split_once('.').map_or("", |(_, s)| s);
            (stroke.0, sounds.id(label), letters(spelled))
        })
        .collect();
    vowel_readings.sort();
    vowel_readings.dedup();
    let mut vowel_coda: Vec<(u32, u8, u8, u8)> = rules
        .vowel_coda
        .iter()
        .filter_map(|(label, stroke)| {
            let (v, c) = label.split_once(' ')?;
            let spelled = v.split_once('.').map_or("", |(_, s)| s);
            Some((stroke.0, sounds.id(v), letters(spelled), sounds.id(c)))
        })
        .collect();
    vowel_coda.sort();
    vowel_coda.dedup();

    // The lexicon: each word's sounds, and each reading of its spelling, as a path of the trie
    let mut words: Vec<(&String, usize)> = lexicon
        .rank
        .iter()
        .filter(|(w, r)| **r < lexicon.common && lexicon.pron.contains_key(*w))
        .map(|(w, r)| (w, *r))
        .collect();
    words.sort_by(|a, b| (a.1, a.0).cmp(&(b.1, b.0)));
    let mut trie = Trie::default();
    for (id, (word, _)) in words.iter().enumerate() {
        let id = id as u32;
        for variant in &lexicon.pron[*word] {
            let spelled = align(word, variant);
            let path: Vec<EdgeKey> = variant
                .iter()
                .zip(&spelled)
                .map(|(sound, &letters)| {
                    let vowel = sound.ends_with(|c: char| c.is_ascii_digit());
                    let drop = if vowel {
                        match sound.chars().last() {
                            Some('0') => DROP_UNSTRESSED,
                            Some('2') => DROP_SECONDARY,
                            _ => DROP_STRESSED,
                        }
                    } else if matches!(plain(sound), "HH" | "Y") {
                        DROP_WEAK_CONSONANT
                    } else {
                        DROP_CONSONANT
                    };
                    EdgeKey { mask: 1 << sounds.id(sound), letters, drop, vowel }
                })
                .collect();
            trie.insert(&path, id, 0);
        }
        for reading in orthography::readings(word, 1) {
            let path: Vec<EdgeKey> = reading
                .iter()
                .map(|phone| {
                    let vowel = phone.heard.ends_with(|c: char| c.is_ascii_digit());
                    if vowel {
                        let mask = (1u64 << sounds.id(&phone.heard)) | (1 << sounds.id(&phone.long)) | (1 << sounds.id(&phone.weak));
                        let drop = if phone.weak.ends_with('0') { DROP_UNSTRESSED } else { DROP_SECONDARY };
                        EdgeKey { mask, letters: phone_letters(phone), drop, vowel }
                    } else {
                        EdgeKey { mask: 1 << sounds.id(&phone.heard), letters: 0, drop: DROP_CONSONANT, vowel }
                    }
                })
                .collect();
            trie.insert(&path, id, SPELLED_PATH);
        }
    }

    let mut b = Blob(Vec::new());
    b.0.extend_from_slice(core::MAGIC);
    b.u32(core::VERSION);
    b.u16(sounds.names.len() as u16);
    for name in &sounds.names {
        b.str(name);
        b.str(default_spelling(name));
        b.u8(u8::from(VOWELS.contains(&name.as_str())));
    }
    let id = |s: &str| sounds.ids[s];
    b.u64((1 << id("R")) | (1 << id("L")));
    for (i, _) in sounds.names.iter().enumerate() {
        let mut mask = 0u64;
        for (a, c) in CONFUSABLE {
            if id(a) as usize == i {
                mask |= 1 << id(c);
            }
            if id(c) as usize == i {
                mask |= 1 << id(a);
            }
        }
        b.u64(mask);
    }
    for table in [&onset, &coda] {
        b.len(table.len());
        for (keys, toks) in table {
            b.u32(*keys);
            b.bytes(toks);
        }
    }
    b.len(vowel_readings.len());
    for &(keys, sound, letters) in &vowel_readings {
        b.u32(keys);
        b.u8(sound);
        b.u8(letters);
    }
    b.len(vowel_coda.len());
    for &(keys, v, letters, c) in &vowel_coda {
        b.u32(keys);
        b.u8(v);
        b.u8(letters);
        b.u8(c);
    }
    let mut suffixes: Vec<(&Stroke, &String)> = rules.suffixes.iter().collect();
    suffixes.sort();
    b.len(suffixes.len());
    for (stroke, text) in suffixes {
        b.u32(stroke.0);
        b.str(text);
    }
    b.len(rules.two_stroke_suffixes.len());
    for (text, strokes) in &rules.two_stroke_suffixes {
        b.u32(strokes[0].0);
        b.u32(strokes[1].0);
        b.str(text);
    }
    let mut spelling: Vec<(&Stroke, &String)> = rules.spelling.iter().collect();
    spelling.sort();
    b.len(spelling.len());
    for (stroke, letter) in spelling {
        b.u32(stroke.0);
        b.str(letter);
    }
    let mut stems: Vec<&(Vec<Stroke>, String)> = stems.iter().filter(|(o, _)| !o.is_empty() && o.len() <= 6).collect();
    stems.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    b.len(stems.len());
    for (outline, text) in stems {
        b.u8(outline.len() as u8);
        for s in outline {
            b.u32(s.0 & !Stroke::JOIN);
        }
        b.str(text);
    }
    b.len(words.len());
    for (word, rank) in &words {
        b.str(word);
        b.u32(*rank as u32);
    }
    // The trie, flattened: each node's edges and words contiguous
    let n = trie.children.len();
    let mut edge_rows: Vec<(EdgeKey, u32)> = Vec::new();
    let mut end_rows: Vec<(u32, u8)> = Vec::new();
    b.len(n);
    for node in 0..n {
        let start = edge_rows.len() as u32;
        let mut kids: Vec<(EdgeKey, u32)> = trie.children[node].iter().map(|(k, v)| (*k, *v)).collect();
        kids.sort_by_key(|(k, c)| (k.mask, k.letters, k.drop, *c));
        edge_rows.extend(kids);
        let ends_start = end_rows.len() as u32;
        let mut ends = trie.ends[node].clone();
        ends.sort_by_key(|&(w, extra)| (extra, w));
        end_rows.extend(ends);
        b.u32(start);
        b.u32(edge_rows.len() as u32);
        b.u32(ends_start);
        b.u32(end_rows.len() as u32);
    }
    b.len(edge_rows.len());
    for (key, child) in &edge_rows {
        b.u64(key.mask);
        b.u32(*child);
        b.u8(key.letters);
        b.u8(key.drop);
        b.u8(u8::from(key.vowel));
    }
    b.len(end_rows.len());
    for (word, extra) in &end_rows {
        b.u32(*word);
        b.u8(*extra);
    }
    b.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vowel_letters_line_up_with_vowel_sounds() {
        let s = |t: &str| t.split(' ').map(str::to_owned).collect::<Vec<_>>();
        // m-ea-n: the IY is spelled `ea`
        assert_eq!(align("mean", &s("M IY1 N")), vec![0, letters("ea"), 0]);
        // a final silent e is skipped
        assert_eq!(align("make", &s("M EY1 K")), vec![0, letters("a"), 0]);
        // computer: o, u, e
        assert_eq!(
            align("computer", &s("K AH0 M P Y UW1 T ER0")),
            vec![0, letters("o"), 0, 0, 0, letters("u"), 0, letters("e")]
        );
    }
}

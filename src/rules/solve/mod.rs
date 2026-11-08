//! The solver: a run of strokes that missed the dictionary, read as the word it most likely meant.
//!
//! [`core`] is the decoder, self-contained so the keymux plugin runs the very same file; this
//! module packs what it reads (the rules' chord tables, every word's sounds and spelling, the
//! known outlines) into one blob ([`pack`]), and holds the `solve` command's eval. See
//! `plans/09-wasm-fallback-resolver.md` and `site/reference/fallback.json`.

// The decoder works in `u32` indices and small `u8` sound ids that the packer bounds (a trie of
// under a million nodes, under 64 sounds), and is spliced into a plugin that may use nothing but
// `std`: its casts between those widths are deliberate, not a loss.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::too_many_lines
)]
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

/// The `ARPAbet` sounds, vowels first: every sound the lexicon or the chord tables use.
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
const SPELLED_PATH: u8 = 3;
/// A consonant chord whose sound the rules tie to a spelling (`T.ed` on `-D`).
const SPELLED_CONSONANT: u8 = 2;

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
        "AW" | "UH" => "ou",
        "AY" | "IH" => "iey",
        "EH" => "ea",
        "ER" => "eiuoy",
        "EY" => "aeiy",
        "IY" => "eiya",
        "OW" => "oeau",
        "OY" => "oiy",
        "UW" => "oeiu",
        _ => "",
    })
}

/// Which vowel letters spell each vowel sound of `sounds` in `word` (0 for consonants and for a
/// vowel no letters could be given to): the word's runs of vowel letters matched to its vowel
/// sounds in order, a run skipped where it is silent (a final `e` freely).
fn align(word: &str, sounds: &[String]) -> Vec<u8> {
    /// A vowel sound left with no letters, a run of letters skipped (not a silent `e`), a sound
    /// matched to letters that do not usually spell it.
    const NO_LETTERS: u32 = 3;
    const SKIP_LETTERS: u32 = 2;
    const ODD_LETTERS: u32 = 1;
    const UNREACHED: u32 = u32::MAX / 4;
    let chars: Vec<char> = word.chars().collect();
    let is_vowel_letter = |at: usize| "aeiou".contains(chars[at]) || (chars[at] == 'y' && at > 0);
    // Each run of vowel letters, and whether it is a silent final `e`
    let mut groups: Vec<(u8, bool)> = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        if is_vowel_letter(at) {
            let start = at;
            while at < chars.len() && is_vowel_letter(at) {
                at += 1;
            }
            let text: String = chars[start..at].iter().collect();
            groups.push((letters(&text), text == "e" && at == chars.len() && start > 0));
        } else {
            at += 1;
        }
    }
    let vowels: Vec<usize> = (0..sounds.len()).filter(|&i| sounds[i].ends_with(|c: char| c.is_ascii_digit())).collect();
    let odd = |sound: usize, group: usize| u32::from(spellable(plain(&sounds[vowels[sound]])) & groups[group].0 == 0) * ODD_LETTERS;
    // cost[s][g]: the cheapest way to line the first `s` vowel sounds up with the first `g` runs
    let mut cost = vec![vec![UNREACHED; groups.len() + 1]; vowels.len() + 1];
    cost[0][0] = 0;
    for s in 0..=vowels.len() {
        for g in 0..=groups.len() {
            let here = cost[s][g];
            if here >= UNREACHED {
                continue;
            }
            if g < groups.len() {
                let skip = if groups[g].1 { 0 } else { SKIP_LETTERS };
                cost[s][g + 1] = cost[s][g + 1].min(here + skip);
            }
            if s < vowels.len() {
                cost[s + 1][g] = cost[s + 1][g].min(here + NO_LETTERS);
                if g < groups.len() {
                    cost[s + 1][g + 1] = cost[s + 1][g + 1].min(here + odd(s, g));
                }
            }
        }
    }
    let mut out = vec![0u8; sounds.len()];
    let (mut s, mut g) = (vowels.len(), groups.len());
    while s > 0 || g > 0 {
        let here = cost[s][g];
        if s > 0 && g > 0 && cost[s - 1][g - 1] + odd(s - 1, g - 1) == here {
            out[vowels[s - 1]] = groups[g - 1].0;
            s -= 1;
            g -= 1;
        } else if s > 0 && cost[s - 1][g] + NO_LETTERS == here {
            s -= 1;
        } else {
            g -= 1;
        }
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
    /// Where each word's pronunciations end, by word: where a known outline of it enters the trie
    sound_ends: HashMap<u32, Vec<u32>>,
}

impl Trie {
    /// Add `path` for `word`; the node it ends at.
    fn insert(&mut self, path: &[EdgeKey], word: u32, extra: u8) -> u32 {
        if self.children.is_empty() {
            self.children.push(HashMap::new());
            self.ends.push(Vec::new());
        }
        let mut at = 0usize;
        for key in path {
            at = if let Some(&child) = self.children[at].get(key) {
                child as usize
            } else {
                let child = self.children.len();
                self.children.push(HashMap::new());
                self.ends.push(Vec::new());
                self.children[at].insert(*key, index(child));
                child
            };
        }
        match self.ends[at].iter_mut().find(|(w, _)| *w == word) {
            Some(end) => end.1 = end.1.min(extra),
            None => self.ends[at].push((word, extra)),
        }
        index(at)
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

/// Write the keymux plugin, its source and its blob, into `dir` beside the dictionary files.
/// Returns the blob's size in bytes.
pub fn write_plugin(rules: &Rules, lexicon: &Lexicon, entries: &[Entry], dir: &std::path::Path) -> anyhow::Result<usize> {
    let blob = pack(rules, lexicon, &stems(entries));
    crate::output::write_if_different(&dir.join(plugin::BLOB_FILE), &blob)?;
    crate::output::write_if_different(&dir.join(plugin::SOURCE_FILE), plugin::source(&blob).as_bytes())?;
    Ok(blob.len())
}

/// A count or position as the `u32` the blob stores it as. Every table here is far below 2^32.
fn index(n: usize) -> u32 {
    u32::try_from(n).expect("a solver table fits in u32")
}

/// One way of saying a prefix: each sound, and whether it is a vowel.
type PrefixSounds = Vec<(u8, bool)>;

/// The chord tables the decoder reads strokes with, its sounds interned.
struct Tables {
    onset: Vec<(u32, Vec<u8>, u8)>,
    coda: Vec<(u32, Vec<u8>, u8)>,
    vowel_readings: Vec<(u32, u8, u8)>,
    vowel_coda: Vec<(u32, u8, u8, u8)>,
    /// `[[prefix]]` keys and each distinct way of saying the prefix
    prefixes: Vec<(u32, Vec<PrefixSounds>)>,
}

impl Tables {
    fn new(rules: &Rules, sounds: &mut Sounds) -> Self {
        let mut vowel_readings: Vec<(u32, u8, u8)> = rules
            .vowel
            .iter()
            .filter(|(label, _)| !label.ends_with(|c: char| c.is_ascii_digit()))
            .map(|(label, stroke)| {
                let spelled = label.split_once('.').map_or("", |(_, s)| s);
                (stroke.0, sounds.id(label), letters(spelled))
            })
            .collect();
        vowel_readings.sort_unstable();
        vowel_readings.dedup();
        let mut vowel_coda: Vec<(u32, u8, u8, u8)> = rules
            .vowel_coda
            .iter()
            .filter_map(|(label, stroke)| {
                let (vowel, coda) = label.split_once(' ')?;
                let spelled = vowel.split_once('.').map_or("", |(_, s)| s);
                Some((stroke.0, sounds.id(vowel), letters(spelled), sounds.id(coda)))
            })
            .collect();
        vowel_coda.sort_unstable();
        vowel_coda.dedup();
        let prefixes = rules
            .prefixes
            .iter()
            .map(|(prefix, keys)| {
                let mut ways: Vec<PrefixSounds> = prefix
                    .sounds
                    .iter()
                    .map(|way| way.split_whitespace().map(|s| (sounds.id(s), s.ends_with(|c: char| c.is_ascii_digit()))).collect())
                    .collect();
                ways.sort_unstable();
                ways.dedup();
                (keys.0, ways)
            })
            .collect();
        Self {
            prefixes,
            onset: chords(&[&rules.onset, &rules.onset_cluster], sounds),
            coda: chords(&[&rules.coda, &rules.coda_cluster], sounds),
            vowel_readings,
            vowel_coda,
        }
    }
}

/// One side's chord tables as `(keys, sounds, cost)`: bigger chords first, so the decoder's
/// capped search for covers tries them before the single keys; a chord whose sound depends on a
/// spelling (`T.ed`) costs a little.
fn chords(tables: &[&HashMap<String, Stroke>], sounds: &mut Sounds) -> Vec<(u32, Vec<u8>, u8)> {
    let mut out: Vec<(u32, Vec<u8>, u8)> = tables
        .iter()
        .flat_map(|t| t.iter())
        .map(|(label, stroke)| {
            let spelled = label.split_whitespace().any(|s| s.contains('.'));
            (stroke.0, label.split_whitespace().map(|s| sounds.id(s)).collect(), u8::from(spelled) * SPELLED_CONSONANT)
        })
        .collect();
    out.sort_by(|a, b| {
        (std::cmp::Reverse(a.0.count_ones()), a.0, &a.1, a.2).cmp(&(std::cmp::Reverse(b.0.count_ones()), b.0, &b.1, b.2))
    });
    out.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
    out
}

/// What skipping a sound of a word's pronunciation costs.
fn drop_cost(sound: &str) -> u8 {
    if sound.ends_with(|c: char| c.is_ascii_digit()) {
        match sound.chars().last() {
            Some('0') => DROP_UNSTRESSED,
            Some('2') => DROP_SECONDARY,
            _ => DROP_STRESSED,
        }
    } else if matches!(plain(sound), "HH" | "Y") {
        DROP_WEAK_CONSONANT
    } else {
        DROP_CONSONANT
    }
}

/// The lexicon's words, most common first with their ranks, and the trie over each one's
/// pronunciations and the readings of its spelling.
fn lexicon_trie<'a>(lexicon: &'a Lexicon, sounds: &mut Sounds) -> (Vec<(&'a String, usize)>, Trie) {
    let mut words: Vec<(&String, usize)> = lexicon
        .rank
        .iter()
        .filter(|(w, r)| **r < lexicon.common && lexicon.pron.contains_key(*w))
        .map(|(w, r)| (w, *r))
        .collect();
    words.sort_by(|a, b| (a.1, a.0).cmp(&(b.1, b.0)));
    let mut trie = Trie::default();
    for (id, (word, _)) in words.iter().enumerate() {
        let id = index(id);
        for variant in &lexicon.pron[*word] {
            let path: Vec<EdgeKey> = variant
                .iter()
                .zip(align(word, variant))
                .map(|(sound, letters)| EdgeKey {
                    mask: 1 << sounds.id(sound),
                    letters,
                    drop: drop_cost(sound),
                    vowel: sound.ends_with(|c: char| c.is_ascii_digit()),
                })
                .collect();
            let end = trie.insert(&path, id, 0);
            trie.sound_ends.entry(id).or_default().push(end);
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
    (words, trie)
}

/// Everything [`Model::load`] reads, packed.
#[must_use]
pub fn pack(rules: &Rules, lexicon: &Lexicon, stems: &[(Vec<Stroke>, String)]) -> Vec<u8> {
    let mut sounds = Sounds::new();
    let tables = Tables::new(rules, &mut sounds);
    let (words, trie) = lexicon_trie(lexicon, &mut sounds);
    let mut b = Blob(Vec::new());
    b.0.extend_from_slice(core::MAGIC);
    b.u32(core::VERSION);
    write_sounds(&mut b, &sounds);
    write_tables(&mut b, &tables, rules);
    let ids: HashMap<&str, u32> = words.iter().enumerate().map(|(i, (w, _))| (w.as_str(), index(i))).collect();
    write_stems(&mut b, stems, lexicon, |text| ids.get(text).and_then(|id| trie.sound_ends.get(id)).cloned().unwrap_or_default());
    b.len(words.len());
    for (word, rank) in &words {
        b.str(word);
        b.u32(index(*rank));
    }
    write_trie(&mut b, &trie);
    b.0
}

fn write_sounds(b: &mut Blob, sounds: &Sounds) {
    b.u16(u16::try_from(sounds.names.len()).expect("fewer than 64 sounds"));
    for name in &sounds.names {
        b.str(name);
        b.str(default_spelling(name));
        b.u8(u8::from(VOWELS.contains(&name.as_str())));
    }
    let id = |s: &str| sounds.ids[s];
    b.u64((1 << id("R")) | (1 << id("L")));
    b.u8(id("R"));
    b.u8(id("ER"));
    for i in 0..sounds.names.len() {
        let mut mask = 0u64;
        for (one, other) in CONFUSABLE {
            if usize::from(id(one)) == i {
                mask |= 1 << id(other);
            }
            if usize::from(id(other)) == i {
                mask |= 1 << id(one);
            }
        }
        b.u64(mask);
    }
}

fn write_tables(b: &mut Blob, tables: &Tables, rules: &Rules) {
    for table in [&tables.onset, &tables.coda] {
        b.len(table.len());
        for (keys, toks, cost) in table {
            b.u32(*keys);
            b.bytes(toks);
            b.u8(*cost);
        }
    }
    b.len(tables.vowel_readings.len());
    for &(keys, sound, letters) in &tables.vowel_readings {
        b.u32(keys);
        b.u8(sound);
        b.u8(letters);
    }
    b.len(tables.vowel_coda.len());
    for &(keys, vowel, letters, coda) in &tables.vowel_coda {
        b.u32(keys);
        b.u8(vowel);
        b.u8(letters);
        b.u8(coda);
    }
    let mut suffixes: Vec<(&Stroke, &String)> = rules.suffixes.iter().collect();
    suffixes.sort_unstable();
    b.len(suffixes.len());
    for (stroke, text) in suffixes {
        b.u32(stroke.0);
        b.str(text);
    }
    // In a fixed order: the rules hold them in a hash map's
    let mut two_stroke: Vec<&(String, Vec<Stroke>)> = rules.two_stroke_suffixes.iter().collect();
    two_stroke.sort_by(|a, b| (&a.1, &a.0).cmp(&(&b.1, &b.0)));
    b.len(two_stroke.len());
    for (text, strokes) in two_stroke {
        b.u32(strokes[0].0);
        b.u32(strokes[1].0);
        b.str(text);
    }
    let mut spelling: Vec<(&Stroke, &String)> = rules.spelling.iter().collect();
    spelling.sort_unstable();
    b.len(spelling.len());
    for (stroke, letter) in spelling {
        b.u32(stroke.0);
        b.str(letter);
    }
    b.len(tables.prefixes.len());
    for (keys, ways) in &tables.prefixes {
        b.u32(*keys);
        b.u8(u8::try_from(ways.len()).expect("few ways to say a prefix"));
        for way in ways {
            b.u8(u8::try_from(way.len()).expect("a short prefix"));
            for &(sound, vowel) in way {
                b.u8(sound);
                b.u8(u8::from(vowel));
            }
        }
    }
}

/// The known outlines, six strokes or fewer, each with its word's rank (the lexicon's bound for a
/// word outside it).
fn write_stems(b: &mut Blob, stems: &[(Vec<Stroke>, String)], lexicon: &Lexicon, sound_ends: impl Fn(&str) -> Vec<u32>) {
    // An outline with an empty stroke in it is no outline a keyboard sends
    let mut stems: Vec<&(Vec<Stroke>, String)> = stems
        .iter()
        .filter(|(o, _)| !o.is_empty() && o.len() <= 6 && o.iter().all(|s| s.0 & !Stroke::JOIN != 0))
        .collect();
    stems.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    b.len(stems.len());
    for (outline, text) in stems {
        b.u8(u8::try_from(outline.len()).expect("six strokes or fewer"));
        for s in outline {
            b.u32(s.0 & !Stroke::JOIN);
        }
        // Lowercase, as ranks and the lexicon are and as the dictionary reads back: the generator
        // writes the pronoun `I`, which the decoder capitalizes again on the way out
        let lower = text.to_lowercase();
        b.str(&lower);
        let rank = lexicon.rank.get(lower.as_str()).copied().unwrap_or(lexicon.common);
        b.u32(index(rank.min(lexicon.common)));
        // Where the word's own sounds end in the trie, so later strokes can carry on into a
        // longer word (`SKEL` *cancel*, then `+EUPBG`, *canceling*)
        let ends = sound_ends(&lower);
        b.u8(u8::try_from(ends.len().min(8)).expect("eight or fewer"));
        for node in ends.iter().take(8) {
            b.u32(*node);
        }
    }
}

/// The trie, flattened: each node's edges and words contiguous, in a fixed order, as offset
/// arrays (`n + 1` each, so a node's range ends where the next one's starts); each edge eight
/// bytes, its sounds an index into a table of the few distinct masks.
fn write_trie(b: &mut Blob, trie: &Trie) {
    let n = trie.children.len();
    let mut edge_rows: Vec<(EdgeKey, u32)> = Vec::new();
    let mut end_rows: Vec<(u32, u8)> = Vec::new();
    let mut edge_start = Vec::with_capacity(n + 1);
    let mut end_start = Vec::with_capacity(n + 1);
    for node in 0..n {
        edge_start.push(index(edge_rows.len()));
        let mut kids: Vec<(EdgeKey, u32)> = trie.children[node].iter().map(|(k, v)| (*k, *v)).collect();
        kids.sort_by_key(|(k, c)| (k.mask, k.letters, k.drop, *c));
        edge_rows.extend(kids);
        end_start.push(index(end_rows.len()));
        let mut ends = trie.ends[node].clone();
        ends.sort_by_key(|&(w, extra)| (extra, w));
        end_rows.extend(ends);
    }
    edge_start.push(index(edge_rows.len()));
    end_start.push(index(end_rows.len()));
    let mut masks: Vec<u64> = edge_rows.iter().map(|(k, _)| k.mask).collect();
    masks.sort_unstable();
    masks.dedup();
    b.len(n);
    for at in edge_start.iter().chain(&end_start) {
        b.u32(*at);
    }
    b.len(masks.len());
    for mask in &masks {
        b.u64(*mask);
    }
    b.len(edge_rows.len());
    for (key, child) in &edge_rows {
        let class = masks.binary_search(&key.mask).expect("every mask is listed");
        b.u32(*child);
        b.u16(u16::try_from(class).expect("fewer than 65,536 distinct masks"));
        b.u8(key.letters);
        assert!(key.drop < 0x80, "a drop cost fits in seven bits");
        b.u8(key.drop | if key.vowel { 0x80 } else { 0 });
    }
    b.len(end_rows.len());
    for (word, extra) in &end_rows {
        b.u32(*word);
        b.u8(*extra);
    }
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

    /// The model `build` ships, from the full fixture dictionary, packed and loaded once.
    fn model() -> &'static Model {
        static MODEL: std::sync::LazyLock<Model> = std::sync::LazyLock::new(|| {
            let setup = crate::dictionary_fixture::setup();
            let lexicon = Lexicon { pron: &setup.pron, rank: &setup.rank, common: 100_000 };
            let blob = pack(&setup.rules, &lexicon, &stems(&crate::dictionary_fixture::generated().entries));
            Model::load(&blob).expect("the packed blob loads")
        });
        &MODEL
    }

    #[test]
    fn a_run_reads_as_the_word_its_keys_sound_like() {
        for (outline, word) in [
            // one syllable a stroke, and folded
            ("KOPL/+PU/+TER", "computer"),
            ("S/+TA/+SHUPB", "station"),
            ("TPO/+TPHE/+TEUBG", "phonetic"),
            ("TKE/+TER/+PHU/+TPHEUFPL", "determinism"),
            // the `-R` key for ER, consonants out of steno order
            ("PHART/+TPHUL", "maternal"),
            ("TOLT", "total"),
            // a known outline (`OER` other) and a word read by its spelling
            ("OER/+WEU/+-S/+E", "otherwise"),
            // a whole word beats a stem and a suffix that spell something else
            ("HROBG/+-L", "local"),
            // a two-stroke suffix
            ("KUB/+EUZ/+-PL", "cubism"),
        ] {
            assert_eq!(model().answer(outline).as_deref(), Some(word), "{outline}");
        }
    }

    #[test]
    fn pieces_join_by_english_spelling() {
        // a silent e drops before a vowel suffix; a known outline and a suffix that spell no word
        // still read, as the pieces
        assert_eq!(model().answer("SR/+-G").as_deref(), Some("having"));
        assert_eq!(model().answer("SKP/+-R").as_deref(), Some("ander"));
    }

    #[test]
    fn spelling_mode_is_letter_by_letter() {
        assert_eq!(model().answer("~K/+A/+T").as_deref(), Some("kat"));
    }

    #[test]
    fn a_run_far_from_every_word_types_its_keys_read_back() {
        // nothing the lexicon has sounds like this: the plain read-back, not some far-off word
        let answer = model().answer("STKPWHRAO*EUFRPBLGTSDZ/+STKPWHRAO*EUFRPBLGTSDZ");
        let floor = model().read_back(&[core::parse_stroke("STKPWHRAO*EUFRPBLGTSDZ").unwrap(), core::parse_stroke("+STKPWHRAO*EUFRPBLGTSDZ").unwrap()]);
        assert_eq!(answer, floor);
    }

    #[test]
    fn the_plugin_reads_strokes_exactly_as_lyk_does() {
        // `parse_stroke` is the plugin's own copy of `Stroke::parse`: every outline the dictionary
        // has must read to the same bits
        for entry in &crate::dictionary_fixture::generated().entries {
            for stroke in &entry.outline {
                let text = stroke.to_string();
                assert_eq!(core::parse_stroke(&text), Stroke::parse(&text).map(|s| s.0), "{text}");
            }
        }
    }

    #[test]
    fn the_plugin_source_carries_the_decoder_and_the_interface() {
        let source = plugin::source(b"LYKS");
        for needle in ["pub fn solve_with", "extern \"C\" fn alloc", "extern \"C\" fn fallback", "extern \"C\" fn joins", "include_bytes!(\"05-lyk-solver.bin\")"] {
            assert!(source.contains(needle), "{needle}");
        }
        assert!(Model::load(b"nope").is_none());
    }
}

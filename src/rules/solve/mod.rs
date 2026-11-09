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
use crate::num::ToF64;
use crate::generate::{Entry, Origin};
use crate::stroke::Stroke;
use std::collections::{HashMap, HashSet};

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
/// Leaving out the consonant a word ends on, beyond what a consonant costs anywhere else.
const DROP_FINAL_CONSONANT: u8 = 1;
/// `HH` and the `Y` glide are often not struck at all.
const DROP_WEAK_CONSONANT: u8 = 2;
/// Leaving out a word's last vowel, beyond what that vowel costs anywhere else.
const DROP_LAST: u8 = 2;
/// A word reached through its spelling rather than its sounds.
const SPELLED_PATH: u8 = 3;
/// A word reached through its spelling with its silent final `e` struck as a vowel (`TKA/+-PB/+SE`
/// *dance*): the stroke says which word on purpose.
const SPELLED_FINAL_E: u8 = 2;
/// A known outline goes on into a longer word spelled from it (*liquid*, *liquidation*) when it is
/// at least this many letters: a shorter one starts too many words to mean any.
const STEM_START_LETTERS: usize = 3;
/// The most places in the trie one known outline goes on from: its own sounds' ends first.
const STEM_NODES: usize = 8;
/// Real words pieces may spell past the lexicon: ranked below this, and this many letters or more.
const SPELLINGS: usize = 250_000;
const SPELLING_LETTERS: usize = 4;
/// (tuning) a plural morpheme's last vowel kept, never folded away
const PLURAL_VOWEL_KEPT: bool = false;

/// A packing cost, or (natively, while tuning) `LYK_SOLVE_<NAME>` overriding it.
fn tuned_pack(name: &str, default: u8) -> u8 {
    std::env::var(format!("LYK_SOLVE_{name}")).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}
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
    let (groups, of) = align_runs(word, sounds);
    of.iter().map(|g| g.map_or(0, |g| groups[g].0)).collect()
}

/// A run of vowel letters: its letters, whether it is a silent final `e`, and where it starts.
type VowelRun = (u8, bool, usize);

/// [`align`]'s workings: each run of vowel letters, and which run each sound is spelled with.
fn align_runs(word: &str, sounds: &[String]) -> (Vec<VowelRun>, Vec<Option<usize>>) {
    /// A vowel sound left with no letters, a run of letters skipped (not a silent `e`), a sound
    /// matched to letters that do not usually spell it.
    const NO_LETTERS: u32 = 3;
    const SKIP_LETTERS: u32 = 2;
    const ODD_LETTERS: u32 = 1;
    const UNREACHED: u32 = u32::MAX / 4;
    let chars: Vec<char> = word.chars().collect();
    let is_vowel_letter = |at: usize| "aeiou".contains(chars[at]) || (chars[at] == 'y' && at > 0);
    // Each run of vowel letters, and whether it is a silent final `e`
    let mut groups: Vec<VowelRun> = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        if is_vowel_letter(at) {
            let start = at;
            while at < chars.len() && is_vowel_letter(at) {
                at += 1;
            }
            let text: String = chars[start..at].iter().collect();
            groups.push((letters(&text), text == "e" && at == chars.len() && start > 0, start));
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
    let mut out = vec![None; sounds.len()];
    let (mut s, mut g) = (vowels.len(), groups.len());
    while s > 0 || g > 0 {
        let here = cost[s][g];
        if s > 0 && g > 0 && cost[s - 1][g - 1] + odd(s - 1, g - 1) == here {
            out[vowels[s - 1]] = Some(g - 1);
            s -= 1;
            g -= 1;
        } else if s > 0 && cost[s - 1][g] + NO_LETTERS == here {
            s -= 1;
        } else {
            g -= 1;
        }
    }
    (groups, out)
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
    /// Where the sounds of a word's first letters end, by those letters and the word's rank:
    /// where a known outline spelled that way enters a longer word (*liquid*, *liquidation*)
    spelled_starts: HashMap<String, Vec<(usize, u32)>>,
}

impl Trie {
    /// Add `path` for `word`; the node it ends at.
    fn insert(&mut self, path: &[EdgeKey], word: u32, extra: u8) -> u32 {
        self.insert_along(path, word, extra).last().copied().unwrap_or(0)
    }

    /// [`Self::insert`], with every node along the path: `nodes[i]` is where the first `i` sounds end.
    fn insert_along(&mut self, path: &[EdgeKey], word: u32, extra: u8) -> Vec<u32> {
        if self.children.is_empty() {
            self.children.push(HashMap::new());
            self.ends.push(Vec::new());
        }
        let mut at = 0usize;
        let mut nodes = vec![0];
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
            nodes.push(index(at));
        }
        match self.ends[at].iter_mut().find(|(w, _)| *w == word) {
            Some(end) => end.1 = end.1.min(extra),
            None => self.ends[at].push((word, extra)),
        }
        nodes
    }

    /// A node's children, in the order the packed trie keeps them.
    fn kids(&self, node: usize) -> Vec<(EdgeKey, u32)> {
        let mut kids: Vec<(EdgeKey, u32)> = self.children[node].iter().map(|(k, v)| (*k, *v)).collect();
        kids.sort_by_key(|(k, c)| (k.mask, k.letters, k.drop, *c));
        kids
    }

    /// The nodes breadth first, each node's children numbered together as its edges are laid out:
    /// `(order, id)`, `order[new]` the node numbered `new` and `id[old]` its new number. Packed so,
    /// edge `k` leads to node `k + 1`, and what the decoder reads about a node's children (their
    /// lookahead, what ends below them, its working memory for them) sits side by side.
    fn breadth_first(&self) -> (Vec<u32>, Vec<u32>) {
        let mut order = vec![0u32];
        let mut id = vec![0u32; self.children.len()];
        let mut at = 0;
        while at < order.len() {
            for (_, child) in self.kids(order[at] as usize) {
                id[child as usize] = index(order.len());
                order.push(child);
            }
            at += 1;
        }
        (order, id)
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
    /// Real spellings past `rank`, with their line in the frequency list (`Setup::spellings`)
    pub spellings: &'a [(String, usize)],
    /// The web's bigram counts (`Setup::web_bigrams`), for the context rule
    pub bigrams: &'a [(String, String, u64)],
    pub common: usize,
}

/// A pair the bigram list lacks was seen fewer times than its cut (100,000): taken as half that.
/// It only ever counts against a pair (the decoder caps it at 0), and only against a pair of words
/// common enough that the list would have held them together had they gone together.
const CONTEXT_UNSEEN_COUNT: f64 = 50_000.0;

/// What a reading's first word is looked up as in the bigram table: as the decoder's
/// `context_points` takes it: up to a space, up to an apostrophe, without a last character that is
/// no letter or digit, lowercase.
fn context_first_word(text: &str) -> String {
    let first = text.split(' ').next().unwrap_or(text);
    let first = first.split(['\'', '\u{2019}']).next().unwrap_or(first);
    first.trim_end_matches(|c: char| !c.is_alphanumeric()).to_lowercase()
}

/// log2 of `x`, in quarter bits.
fn quarter_bits(x: f64) -> f64 {
    (4.0 * x.log2()).round()
}

/// The bigram table the context rule reads ([`core::Context`]): every pair's counts summed over
/// the case they were written in, each word's count as a first word and as a second, and every pair
/// whose second word a reading may start with (`outputs`), with its pointwise mutual information.
/// Quarter bits of counts at least 100,000 and at most 2^40 are 66 to 160, and of a pair's
/// information within ±32 bits once clamped: the casts to `u8` and `i8` are exact.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn write_context(b: &mut Blob, bigrams: &[(String, String, u64)], outputs: &HashSet<String>) {
    let mut pairs: HashMap<(&str, &str), u64> = HashMap::new();
    let mut left: HashMap<&str, u64> = HashMap::new();
    let mut right: HashMap<&str, u64> = HashMap::new();
    let mut total = 0u64;
    for (first, second, count) in bigrams {
        *pairs.entry((first.as_str(), second.as_str())).or_default() += count;
        *left.entry(first.as_str()).or_default() += count;
        *right.entry(second.as_str()).or_default() += count;
        total += count;
    }
    let total_f = total.to_f64();
    let q = |count: u64| quarter_bits(count.to_f64()) as u8;
    let mut words: Vec<&str> = left.keys().copied().chain(right.keys().copied().filter(|w| outputs.contains(*w))).collect();
    words.sort_unstable();
    words.dedup();
    let id: HashMap<&str, u32> = words.iter().enumerate().map(|(i, w)| (*w, index(i))).collect();
    let mut rows: Vec<Vec<(u32, i8)>> = vec![Vec::new(); words.len()];
    for (&(first, second), &count) in &pairs {
        if !outputs.contains(second) {
            continue;
        }
        let pmi = (count.to_f64() * total_f / (left[first].to_f64() * right[second].to_f64())).log2();
        rows[id[first] as usize].push((id[second], (4.0 * pmi).round().clamp(-128.0, 127.0) as i8));
    }
    // Natively, while tuning, `LYK_SOLVE_CONTEXT_UNSEEN` overrides it
    let unseen = std::env::var("LYK_SOLVE_CONTEXT_UNSEEN").ok().and_then(|v| v.parse::<f64>().ok()).unwrap_or(CONTEXT_UNSEEN_COUNT);
    b.u32(quarter_bits(unseen * total_f) as u32);
    b.len(words.len());
    for (word, row) in words.iter().zip(&mut rows) {
        row.sort_unstable();
        b.str(word);
        b.u8(left.get(word).map_or(0, |&c| q(c)));
        b.u8(if outputs.contains(*word) { right.get(word).map_or(0, |&c| q(c)) } else { 0 });
        b.len(row.len());
    }
    for row in &rows {
        for &(second, pmi) in row {
            b.u32(second);
            b.u8(pmi as u8);
        }
    }
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
        .flat_map(|(label, stroke)| {
            let spelled = label.split_whitespace().any(|s| s.contains('.'));
            let mut ways = vec![(stroke.0, label.split_whitespace().map(|s| sounds.id(s)).collect::<Vec<u8>>(), u8::from(spelled) * SPELLED_CONSONANT)];
            // A chord that shares the vowel's R key (`V.rv` `-FRB`, *carve*): also the R it holds
            if let Some((sound, spelling)) = label.split_once('.')
                && spelling.starts_with('r')
                && plain(sound) != "R"
                && !label.contains(' ')
            {
                ways.push((stroke.0, vec![sounds.id("R"), sounds.id(sound)], 0));
            }
            ways
        })
        .collect();
    out.sort_by(|a, b| {
        (std::cmp::Reverse(a.0.count_ones()), a.0, &a.1, a.2).cmp(&(std::cmp::Reverse(b.0.count_ones()), b.0, &b.1, b.2))
    });
    out.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
    out
}

/// What skipping a sound of a word's pronunciation costs.
/// Only a reduced vowel (`[reduce]`'s AH, IH, ER) is cheap to leave out: any other vowel is a
/// syllable the theory strikes, and a full vowel that ends the word all the more (*probably*'s
/// final IY is what tells it from *probable*, whose reduced *-ble* is left out freely). A consonant
/// that ends the word is what tells it apart too: 1 more (*problem*'s M, beside *probably*).
fn drop_cost(sound: &str, last: bool, ends_word: bool) -> u8 {
    if sound.ends_with(|c: char| c.is_ascii_digit()) {
        let reduced = matches!(plain(sound), "AH" | "IH" | "ER");
        let drop = match sound.chars().last() {
            Some('0') if reduced => DROP_UNSTRESSED,
            Some('0' | '2') => DROP_SECONDARY,
            _ => DROP_STRESSED,
        };
        if last && !reduced { (drop + tuned_pack("DROP_LAST", DROP_LAST)).min(DROP_STRESSED) } else { drop }
    } else if matches!(plain(sound), "HH" | "Y") {
        DROP_WEAK_CONSONANT
    } else if ends_word {
        DROP_CONSONANT + tuned_pack("DROP_FINAL_CONSONANT", DROP_FINAL_CONSONANT)
    } else {
        DROP_CONSONANT
    }
}

/// One way of saying `word` as a path down the trie: each sound, the vowel letters it is spelled
/// with, and what skipping it costs.
fn sound_path(word: &str, variant: &[String], sounds: &mut Sounds) -> Vec<EdgeKey> {
    let last_vowel = variant.iter().rposition(|s| s.ends_with(|c: char| c.is_ascii_digit()));
    variant
        .iter()
        .zip(align(word, variant))
        .enumerate()
        .map(|(at, (sound, letters))| EdgeKey {
            mask: 1 << sounds.id(sound),
            letters,
            drop: drop_cost(sound, Some(at) == last_vowel, at + 1 == variant.len()),
            vowel: sound.ends_with(|c: char| c.is_ascii_digit()),
        })
        .collect()
}

const MORPHEMES: &str = include_str!("../../../rules/morphemes.toml");

/// A piece a giant word is built from (`rules/morphemes.toml`): its spelling, whether it is a
/// prefix (else a suffix), and every way it is said.
pub struct Morpheme {
    pub text: String,
    pub prefix: bool,
    pub sounds: Vec<String>,
}

/// The morpheme table, prefixes then suffixes, each in spelling order.
pub fn morphemes() -> Vec<Morpheme> {
    #[derive(serde::Deserialize)]
    struct Table {
        prefix: std::collections::BTreeMap<String, Vec<String>>,
        suffix: std::collections::BTreeMap<String, Vec<String>>,
    }
    let table: Table = toml::from_str(MORPHEMES).expect("rules/morphemes.toml parses");
    let prefixes = table.prefix.into_iter().map(|(text, sounds)| Morpheme { text, prefix: true, sounds });
    prefixes.chain(table.suffix.into_iter().map(|(text, sounds)| Morpheme { text, prefix: false, sounds })).collect()
}

/// Whether a word is said ending on a consonant, its final `e` silent (*dance*, not *coffee*).
fn pron_has_no_final_vowel(variants: &[Vec<String>]) -> bool {
    variants.iter().any(|v| v.last().is_some_and(|s| !s.ends_with(|c: char| c.is_ascii_digit())))
}

/// One reading of a word's spelling ([`orthography::readings`]) as a path down the trie.
fn spelled_path(reading: &[Phone], sounds: &mut Sounds) -> Vec<EdgeKey> {
    let last_vowel = reading.iter().rposition(|p| p.heard.ends_with(|c: char| c.is_ascii_digit()));
    reading
        .iter()
        .enumerate()
        .map(|(at, phone)| {
            let vowel = phone.heard.ends_with(|c: char| c.is_ascii_digit());
            if vowel {
                let mask = (1u64 << sounds.id(&phone.heard)) | (1 << sounds.id(&phone.long)) | (1 << sounds.id(&phone.weak));
                // The last vowel letter is a syllable struck (*probably*'s *y*), as on the
                // sounds' own path; not an *e*, which ends a word as often reduced or silent
                let drop = if Some(at) == last_vowel && phone_letters(phone) != letters("e") {
                    (DROP_SECONDARY + tuned_pack("DROP_LAST", DROP_LAST)).min(DROP_STRESSED)
                } else if phone.weak.ends_with('0') {
                    DROP_UNSTRESSED
                } else {
                    DROP_SECONDARY
                };
                EdgeKey { mask, letters: phone_letters(phone), drop, vowel }
            } else {
                EdgeKey { mask: 1 << sounds.id(&phone.heard), letters: 0, drop: DROP_CONSONANT, vowel }
            }
        })
        .collect()
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
    for (id, (word, rank)) in words.iter().enumerate() {
        let id = index(id);
        for variant in &lexicon.pron[*word] {
            // An abbreviation said in full (*tv* as *television*): those sounds are the full word's
            if variant.len() > 2 * word.chars().count() + 1 {
                continue;
            }
            let (runs, run_of) = align_runs(word, variant);
            let path = sound_path(word, variant, sounds);
            let nodes = trie.insert_along(&path, id, 0);
            trie.sound_ends.entry(id).or_default().push(nodes[path.len()]);
            // Before each vowel spelled after the first, when every vowel sound before it is
            // spelled by the vowel letters before it in turn: the word's letters up to there, said
            let mut vowels_before = 0;
            for (at, run) in run_of.iter().enumerate() {
                let vowel = variant[at].ends_with(|c: char| c.is_ascii_digit());
                if !vowel {
                    continue;
                }
                let Some(run) = *run else { break };
                if run != vowels_before {
                    break;
                }
                vowels_before += 1;
                let start = runs[run].2;
                if run > 0 && start >= STEM_START_LETTERS {
                    let letters: String = word.chars().take(start).collect();
                    trie.spelled_starts.entry(letters).or_default().push((*rank, nodes[at]));
                }
            }
        }
        for reading in orthography::readings(word, 1) {
            let last_vowel = reading.iter().rposition(|p| p.heard.ends_with(|c: char| c.is_ascii_digit()));
            let path = spelled_path(&reading, sounds);
            trie.insert(&path, id, tuned_pack("SPELLED_PATH", SPELLED_PATH));
            // A silent final `e` struck as an *e* (EH or IY, not a schwa any vowel key reads as) is
            // the word's spelling on purpose (*dance*'s, not *dancer*'s ER): a cheaper spelled path
            let chars: Vec<char> = word.chars().collect();
            let silent_e_struck = chars.len() >= 3
                && chars[chars.len() - 1] == 'e'
                && !"aeiouy".contains(chars[chars.len() - 2])
                && Some(reading.len() - 1) == last_vowel
                && pron_has_no_final_vowel(&lexicon.pron[*word]);
            if silent_e_struck {
                let e = &reading[reading.len() - 1];
                let mut path = path;
                if let Some(last) = path.last_mut() {
                    last.mask = (1u64 << sounds.id(&e.heard)) | (1 << sounds.id(&e.long));
                }
                trie.insert(&path, id, tuned_pack("SPELLED_FINAL_E", SPELLED_FINAL_E));
            }
        }
    }
    (words, trie)
}

/// The known outlines, and then the outlines the rules teach each lexicon word (`explain`'s "ours":
/// its outline and its homophone variant, the join key on every stroke after the first) when two
/// strokes or more: what the lessons have a writer strike syllable by syllable, which the shipped
/// dictionary no longer holds now the backups are gone. An outline an entry holds stays that
/// entry's; two words taught the same outline give it to the commoner.
fn with_taught(rules: &Rules, lexicon: &Lexicon, words: &[(&String, usize)], stems: &[(Vec<Stroke>, String)]) -> Vec<(Vec<Stroke>, String)> {
    let bare = |outline: &[Stroke]| outline.iter().map(|s| s.0 & !Stroke::JOIN).collect::<Vec<u32>>();
    let mut held: HashSet<Vec<u32>> = stems.iter().map(|(o, _)| bare(o)).collect();
    let mut out = stems.to_vec();
    for (word, _) in words {
        for sounds in &lexicon.pron[*word] {
            for outline in [rules.outline(word, sounds), rules.variant(word, sounds)].into_iter().flatten() {
                if outline.len() >= 2 && held.insert(bare(&outline)) {
                    out.push((outline, (*word).clone()));
                }
            }
        }
    }
    out
}

/// Real words past the lexicon that pieces may spell (*look* + *up*, *lookup*): every word of
/// [`SPELLING_LETTERS`] letters or more ranked below [`SPELLINGS`] in the frequency list, as a
/// sorted table of [`core::spelling_hash`]es with their ranks. No sounds: only what a run of pieces
/// spells is checked against it.
fn write_spellings(b: &mut Blob, lexicon: &Lexicon, lexicon_words: &HashMap<&str, u32>) {
    let mut table: Vec<(u64, u32)> = lexicon
        .rank
        .iter()
        .chain(lexicon.spellings.iter().map(|(w, r)| (w, r)))
        .filter(|(w, r)| {
            **r < SPELLINGS && w.len() >= SPELLING_LETTERS && w.bytes().all(|c| c.is_ascii_lowercase()) && !lexicon_words.contains_key(w.as_str())
        })
        .map(|(w, r)| (core::spelling_hash(w), index(*r)))
        .collect();
    table.sort_unstable();
    table.dedup_by_key(|e| e.0);
    b.len(table.len());
    for (hash, rank) in table {
        b.u64(hash);
        b.u32(rank);
    }
}

/// Everything [`Model::load`] reads, packed.
#[must_use]
pub fn pack(rules: &Rules, lexicon: &Lexicon, stems: &[(Vec<Stroke>, String)]) -> Vec<u8> {
    let mut sounds = Sounds::new();
    let tables = Tables::new(rules, &mut sounds);
    let (words, mut trie) = lexicon_trie(lexicon, &mut sounds);
    // The morphemes in the same trie, numbered after the words, each way it is said, every
    // consonant struck: a morpheme is short and cheap to join, and with its consonants skippable it
    // would match half of any stroke (`UD`, *hood*). An unstressed vowel may go, as the rules fold
    // it (`EUFPL`, *-ism*), but not a morpheme's only vowel (`HE/+-D` is no *hood*), nor a plural's
    // last (*-nesses* without it is *-ness* + *-s*)
    let morphemes = morphemes();
    for (j, m) in morphemes.iter().enumerate() {
        for way in &m.sounds {
            let variant: Vec<String> = way.split_whitespace().map(str::to_owned).collect();
            let vowels = variant.iter().filter(|s| s.ends_with(|c: char| c.is_ascii_digit())).count();
            let plural = variant.len() >= 2 && matches!(variant[variant.len() - 1].as_str(), "S" | "Z");
            let droppable = |at: usize| {
                variant[at].ends_with('0') && vowels > 1 && !(PLURAL_VOWEL_KEPT && plural && at + 2 == variant.len())
            };
            let path: Vec<EdgeKey> = sound_path(&m.text, &variant, &mut sounds)
                .into_iter()
                .enumerate()
                .map(|(at, k)| EdgeKey { drop: if droppable(at) { k.drop } else { 0x7f }, ..k })
                .collect();
            trie.insert(&path, index(words.len() + j), 0);
        }
        // And as it is spelled, every sound struck: `TPHESZ` is *-ness* letter by letter
        for reading in orthography::readings(&m.text, 1) {
            let path: Vec<EdgeKey> = spelled_path(&reading, &mut sounds).into_iter().map(|k| EdgeKey { drop: 0x7f, ..k }).collect();
            trie.insert(&path, index(words.len() + j), tuned_pack("SPELLED_PATH", SPELLED_PATH));
        }
    }
    let mut b = Blob(Vec::new());
    b.0.extend_from_slice(core::MAGIC);
    b.u32(core::VERSION);
    write_sounds(&mut b, &sounds);
    write_tables(&mut b, &tables, rules);
    let ids: HashMap<&str, u32> = words.iter().enumerate().map(|(i, (w, _))| (w.as_str(), index(i))).collect();
    let stems = with_taught(rules, lexicon, &words, stems);
    let (order, id) = trie.breadth_first();
    write_stems(&mut b, &stems, lexicon, |text| {
        let mut ends = ids.get(text).and_then(|id| trie.sound_ends.get(id)).cloned().unwrap_or_default();
        // Then where longer words spelled the same way go on from it, most common first
        let mut starts = trie.spelled_starts.get(text).cloned().unwrap_or_default();
        starts.sort_unstable();
        for (_, node) in starts {
            if ends.len() >= STEM_NODES {
                break;
            }
            if !ends.contains(&node) {
                ends.push(node);
            }
        }
        ends.iter().map(|&node| id[node as usize]).collect()
    });
    b.len(words.len());
    for (word, rank) in &words {
        b.str(word);
        b.u32(index(*rank));
    }
    write_spellings(&mut b, lexicon, &ids);
    b.len(morphemes.len());
    for m in &morphemes {
        b.str(&m.text);
        b.u8(u8::from(m.prefix));
    }
    write_trie(&mut b, &trie, &order, &id);
    // What a reading may start with: a word, a known outline's word, a real spelling pieces spell
    let outputs: HashSet<String> = words
        .iter()
        .map(|(w, _)| context_first_word(w))
        .chain(stems.iter().map(|(_, text)| context_first_word(text)))
        .chain(lexicon.spellings.iter().filter(|(_, r)| *r < SPELLINGS).map(|(w, _)| context_first_word(w)))
        .collect();
    write_context(&mut b, lexicon.bigrams, &outputs);
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
    // Each suffix chord twice: as the rules write it, one reading of the stroke among others, and
    // with the star on its first stroke, the suffix sheet's hard entry (see `crate::output`); a
    // chord with its star already (`+*T`, the TH sound too) has no hard form
    let plain: Vec<u32> = rules.suffixes.keys().map(|s| s.0).collect();
    let mut suffixes: Vec<(u32, &String, bool)> = Vec::new();
    for (stroke, text) in &rules.suffixes {
        let hard = stroke.0 | Stroke::STAR;
        suffixes.push((stroke.0, text, false));
        if hard != stroke.0 && !plain.contains(&hard) {
            suffixes.push((hard, text, true));
        }
    }
    suffixes.sort_unstable();
    b.len(suffixes.len());
    for (stroke, text, hard) in suffixes {
        b.u32(stroke);
        b.str(text);
        b.u8(u8::from(hard));
    }
    // In a fixed order: the rules hold them in a hash map's
    let mut two_stroke: Vec<(u32, u32, &String, bool)> = Vec::new();
    for (text, strokes) in &rules.two_stroke_suffixes {
        let (first, second) = (strokes[0].0, strokes[1].0);
        two_stroke.push((first, second, text, first & Stroke::STAR != 0));
        if first & Stroke::STAR == 0 {
            two_stroke.push((first | Stroke::STAR, second, text, true));
        }
    }
    two_stroke.sort_unstable();
    b.len(two_stroke.len());
    for (first, second, text, hard) in two_stroke {
        b.u32(first);
        b.u32(second);
        b.str(text);
        b.u8(u8::from(hard));
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
/// bytes, its sounds an index into a table of the few distinct masks. The nodes are numbered
/// [`Trie::breadth_first`] (`order`, `id`).
fn write_trie(b: &mut Blob, trie: &Trie, order: &[u32], id: &[u32]) {
    let n = trie.children.len();
    let mut edge_rows: Vec<(EdgeKey, u32)> = Vec::new();
    let mut end_rows: Vec<(u32, u8)> = Vec::new();
    let mut edge_start = Vec::with_capacity(n + 1);
    let mut end_start = Vec::with_capacity(n + 1);
    for &node in order {
        let node = node as usize;
        edge_start.push(index(edge_rows.len()));
        edge_rows.extend(trie.kids(node).into_iter().map(|(k, child)| (k, id[child as usize])));
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
            let lexicon = Lexicon { pron: &setup.pron, rank: &setup.rank, spellings: &setup.spellings, bigrams: &setup.web_bigrams, common: 100_000 };
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

    /// A word family by three rules a typist can hold in mind, in order: (a) a brief is its word;
    /// (b) a word you know followed by ending chords, when they spell a real word, is that word;
    /// (c) otherwise the run is read by its sounds. The table is the design: when a rule changes,
    /// it changes here first.
    #[test]
    fn a_word_family_follows_three_rules() {
        for (outline, word, rule) in [
            ("PROBL", "probable", "a: brief"),
            ("PROEBL", "probably", "a: brief"),
            ("PROB/+ABL", "probable", "c: prob-able"),
            ("PRO/+PWABL", "probable", "c: pro-bable"),
            ("PROB/+-BL", "probable", "b: prob + -able"),
            ("PRO/+-BL", "probably", "c: pro + able is no word, so sounds: pro-bly"),
            ("PROB/+-BL/+-L", "probably", "b: prob + -able + -ly"),
            ("PROBL/+-L", "probably", "b: probable + -ly"),
            ("PROB/+ABL/+HREU", "probably", "c: prob-ab-ly"),
            ("PROBL/+U/+TEU", "probability", "b: probable + -ity"),
            ("PROB/+-BL/+U/+TEU", "probability", "b: prob + -able + -ity"),
            ("EUPL/+PROBL", "improbable", "c: im + probable's sounds"),
            ("EUPL/+PROB/+-BL", "improbable", "c: im-prob + -able"),
            ("EUPL/+PROBL/+-L", "improbably", "c: improbable + -ly"),
        ] {
            assert_eq!(model().answer(outline).as_deref(), Some(word), "{outline} ({rule})");
        }
    }

    #[test]
    fn only_a_struck_outline_plus_endings_comes_first_never_a_misstroke_guess() {
        for (outline, word, why) in [
            ("KHRO/+-S", "encloses", "KHRO is enclose: enclose + s, rule b"),
            ("SKRAEPL/+-Z", "screams", "not cream + s, one key off SKRAEPL"),
            ("STAOUP/+-D", "stupid", "not stew + ed, one key off STAOUP"),
        ] {
            assert_eq!(model().answer(outline).as_deref(), Some(word), "{outline} ({why})");
        }
    }

    #[test]
    fn star_t_ending_a_run_is_the_th_sound_or_the_th_ending_whichever_spells_the_word() {
        for (outline, word) in [("H/+E/+HR/+*T", "health"), ("TPOUR/+*T", "fourth"), ("WARPL/+*T", "warmth")] {
            assert_eq!(model().answer(outline).as_deref(), Some(word), "{outline}");
        }
    }

    /// A word struck piece by piece as the rules teach each piece: a lexicon word's own outline,
    /// a morpheme's outline from its sounds in `rules/morphemes.toml`, joined.
    fn struck_by_pieces(pieces: &[&str]) -> String {
        let setup = crate::dictionary_fixture::setup();
        let table = morphemes();
        let mut strokes: Vec<Stroke> = Vec::new();
        for piece in pieces {
            let sounds: Vec<String> =
                table.iter().find(|m| m.text == *piece).map_or_else(|| setup.pron[*piece][0].clone(), |m| m.sounds[0].split_whitespace().map(str::to_owned).collect());
            strokes.extend(setup.rules.outline(piece, &sounds).unwrap_or_else(|| panic!("{piece} has an outline")));
        }
        setup.rules.joined(&mut strokes);
        crate::stroke::render_outline(&strokes)
    }

    #[test]
    fn a_giant_word_is_prefixes_then_a_word_then_suffixes() {
        for (word, pieces) in [
            ("antidisestablishmentarianism", &["anti", "dis", "establish", "ment", "arian", "ism"][..]),
            ("counterrevolutionaries", &["counter", "revolutionaries"]),
            ("hyperventilation", &["hyper", "ventilation"]),
            ("superconductivity", &["super", "conductivity"]),
            ("multiculturalism", &["multi", "cultural", "ism"]),
            ("neurotransmitters", &["neuro", "transmitters"]),
            ("psychotherapists", &["psycho", "therapists"]),
            ("overcompensation", &["over", "compensation"]),
            ("semiconductors", &["semi", "conductors"]),
            ("pseudoscientific", &["pseudo", "scientific"]),
            ("internationalization", &["international", "ization"]),
        ] {
            let outline = struck_by_pieces(pieces);
            assert_eq!(model().answer(&outline).as_deref(), Some(word), "{outline}");
        }
    }

    #[test]
    fn morphemes_go_only_where_they_build_a_word() {
        for (outline, word, rule) in [
            ("TKPWAO/+TK/+TPHESZ", "goodness", "a word you know beats one built of morphemes (goodnesses)"),
            ("TAOLS/+HE/+-D", "toolshed", "a morpheme's only vowel is struck (tools + hood)"),
            ("RAOURPG/+UD", "regrouped", "a morpheme's consonants are all struck (regroup + hood)"),
            ("A/+HR/+U/+-S", "alice", "no morpheme on a word under four letters (a + less)"),
            ("R/+EU/+-T/+EU", "righty", "no morpheme on a word under four letters (are + ity)"),
            ("PHAOEU/+-BG/+R/+O/+-PB", "micron", "no suffix chord after a prefix (micro + en)"),
            ("THAO*E/+TPH/+KEU/+-PBG", "thinking", "no path into a morpheme that cannot follow (the + -ness...)"),
            ("KHAOEURBD/+TPHESZ", "childishness", "a morpheme struck as spelled, -ness letter by letter (not childish + nesses)"),
            ("APB/+TAOE/+KOPB/+SREPB/+-GS/+-L", "anticonventional", "the word and its chords spell a word you know (not convention + ly)"),
            ("APB/+TAOE/+KRAOE/+AEUGS/+-FT", "anticreationist", "only suffixes after the word's suffixes (not curry + ation + est)"),
            ("ABS/+HRAOUT/+EUFT/+EU/+KHREU", "absolutistically", "a chord that writes a suffix morpheme takes the morphemes after it (-ist + -ically)"),
            ("KAEUR/+-LS/+-PBS", "carelessness", "and the chords after it (-less + -ness)"),
        ] {
            assert_eq!(model().answer(outline).as_deref(), Some(word), "{outline}: {rule}");
        }
    }

    #[test]
    fn a_plural_folds_on_a_word_s_last_stroke() {
        for (outline, word, why) in [
            ("A/+PWAPBD/+-PB/+PHUPBTS", "abandonments", "abandonment + a folded -s, not abandonment with an S left over"),
            ("STOP/+OEFRZ", "stopovers", "on -Z too"),
            ("PWROUPB/+OUTS", "brownouts", "a compound's plural"),
            ("TKPWAO/+TK/+TPHESZ", "goodness", "-SZ spelling a double s is still the word you know"),
        ] {
            assert_eq!(model().answer(outline).as_deref(), Some(word), "{outline} ({why})");
        }
    }

    #[test]
    fn suffixes_join_by_english_spelling_at_the_seam() {
        for (outline, word, rule) in [
            ("ABGSD/+-PBT/+OL/+SKWREU", "accidentology", "-logy takes its o"),
            ("APB/+TAOE/+KHREUPL/+ABGS/+-S", "anticlimaxes", "-s is -es after x"),
            ("ABG/+KHAOU/+AEUR/+EU/+SHEUP", "actuaryship", "y stays before -ship"),
        ] {
            assert_eq!(model().answer(outline).as_deref(), Some(word), "{outline}: {rule}");
        }
    }

    #[test]
    fn plus_star_puts_the_star_on_the_stroke_before() {
        // `+*` is the star struck late: `+-T` then `+*` is `+*T`, the TH of *health*
        assert_eq!(model().answer("H/+E/+HR/+-T/+*"), model().answer("H/+E/+HR/+*T"));
        assert_eq!(model().answer("H/+E/+HR/+-T/+*").as_deref(), Some("health"));
    }

    #[test]
    fn plus_alone_locks_the_text_so_far_and_what_follows_adds_to_it() {
        // `+` alone keeps what the run says now; the strokes after only add to it, and are read
        // as what the whole would then spell
        for (outline, word) in [
            ("KAT/+", "cat"),
            ("HRAOBG/+/+UP", "lookup"),
            ("PROB/+/+-BL", "probable"),
            ("PROB/+/+-BL/+-L", "probably"),
            ("PROBL/+/+-L", "probably"),
        ] {
            assert_eq!(model().answer(outline).as_deref(), Some(word), "{outline}");
        }
        // Never rewritten: whatever follows, the locked *cat* stays at the front
        for tail in ["+-L/+OG", "+AS/+TROF/+EBG", "+-S"] {
            let answer = model().answer(&format!("KAT/+/{tail}")).unwrap_or_default();
            assert!(answer.starts_with("cat"), "KAT/+/{tail} gave {answer}");
        }
        // Locks stack: each one keeps all that came before it
        let answer = model().answer("KAT/+/+-S/+").unwrap_or_default();
        assert_eq!(answer, "cats");
    }

    #[test]
    fn a_run_read_stroke_by_stroke_reads_as_it_does_from_scratch() {
        // keymux asks again on every stroke: the solver goes on from the run it read last, and must
        // answer exactly as it would reading the whole run fresh
        let runs = [
            "KOPL/+PU/+TER",
            "PROB/+-BL/+-L",
            "TPHRAGS/+-L/+AOEUZ/+AEUGS",
            "EUPL/+PROB/+-BL",
            "TP/+EUT/+*",
            "PROB/+AEUGS/+-L/+EUZ/+-L",
            "KAT/+KAT/+KAT/+KAT/+KAT/+KAT/+KAT/+KAT/+KAT/+KAT/+KAT/+KAT/+KAT",
        ];
        let mut going = model().scratch();
        for run in runs {
            let strokes: Vec<u32> = run.split('/').map(|s| core::parse_stroke(s).unwrap()).collect();
            for n in 1..=strokes.len() {
                let fresh = model().solve_with(&mut model().scratch(), &strokes[..n], 5);
                let stepped = model().solve_with(&mut going, &strokes[..n], 5);
                let texts = |r: &[Reading]| r.iter().map(|r| (r.text.clone(), r.cost)).collect::<Vec<_>>();
                assert_eq!(texts(&stepped), texts(&fresh), "{run} at {n} strokes");
            }
        }
        // and going back: a stroke undone, then another struck
        let a: Vec<u32> = "KOPL/+PU/+TER".split('/').map(|s| core::parse_stroke(s).unwrap()).collect();
        let b: Vec<u32> = "KOPL/+PU/+TEUPBG".split('/').map(|s| core::parse_stroke(s).unwrap()).collect();
        model().solve_with(&mut going, &a, 5);
        let fresh = model().solve_with(&mut model().scratch(), &b, 5);
        let stepped = model().solve_with(&mut going, &b, 5);
        assert_eq!(stepped.first().map(|r| &r.text), fresh.first().map(|r| &r.text));
    }

    #[test]
    fn a_long_run_costs_one_stroke_s_work_per_stroke() {
        // Re-reading a 12-stroke run from scratch on every stroke is 78 steps; going on is 12
        let strokes: Vec<u32> = std::iter::repeat_n("KAT", 12).map(|s| core::parse_stroke(s).unwrap()).collect();
        let mut going = model().scratch();
        for n in 1..=strokes.len() {
            model().solve_with(&mut going, &strokes[..n], 1);
        }
        assert_eq!(going.steps_taken(), 12);
        // keymux also asks for the newest stroke alone (`+KAT`) before the whole run: that must not
        // lose the run read so far, so the work stays one stroke's per stroke
        let joined: Vec<u32> = "KAT/+KAT/+KAT/+KAT/+KAT/+KAT/+KAT/+KAT/+KAT/+KAT/+KAT/+KAT".split('/').map(|s| core::parse_stroke(s).unwrap()).collect();
        let mut going = model().scratch();
        for n in 1..=joined.len() {
            if n > 1 {
                model().solve_with(&mut going, &joined[n - 1..n], 1);
            }
            model().solve_with(&mut going, &joined[..n], 1);
        }
        assert!(going.steps_taken() <= 12 + 11, "{} steps", going.steps_taken());
    }

    #[test]
    fn pieces_that_spell_a_real_word_past_the_lexicon_are_that_word() {
        // *lookup* is rank 121,405: no pronunciation in the lexicon, but a real spelling, so
        // *look* + *up* is *lookup*, not *lockup* or *loop*
        for outline in ["HRAOBG/+UP", "HRAOBG/+-P"] {
            assert_eq!(model().answer(outline).as_deref(), Some("lookup"), "{outline}");
        }
    }

    #[test]
    fn no_stroke_of_a_long_run_is_dropped() {
        // A giant word struck syllable by syllable: the 13th stroke and on still count
        let run = |n: usize| vec!["KAT"; n].join("/+");
        let twelve = model().answer(&run(12));
        let thirteen = model().answer(&run(13));
        assert_ne!(twelve, thirteen, "a 13th stroke changed nothing");
        let thirty = model().answer(&run(30)).unwrap_or_default();
        assert!(thirty.len() > thirteen.unwrap_or_default().len(), "strokes past the solver's reach are dropped: {thirty}");
    }

    #[test]
    fn a_prefix_chord_in_the_first_stroke_is_always_read() {
        // `STK` *dis-* in a stroke with dozens of free readings: never cut by the cap on them
        for (outline, word) in [("STKPAOER/+UT/+-D", "dispirited"), ("STKERT/+AEU/+-GS", "dissertation"), ("STKPHEPL/+ED", "dismembered")] {
            assert_eq!(model().answer(outline).as_deref(), Some(word), "{outline}");
        }
    }

    #[test]
    fn a_chord_that_shares_the_vowels_r_says_the_r() {
        // `-FRB` is `V.rv`: the R of *starve*, *curve* struck with the V, not left out
        for (outline, word) in [("-S/+TAFRBZ", "starves"), ("-FT/+AFRBZ", "starves")] {
            assert_eq!(model().answer(outline).as_deref(), Some(word), "{outline}");
        }
    }

    #[test]
    fn the_outline_the_rules_teach_for_a_word_types_it() {
        // `explain`'s "ours": struck syllable by syllable as the lessons teach, never a near word
        // (*wait* + *-er*), however a known word plus an ending might also spell it
        for (outline, word) in [
            ("WAUT/+-R", "water"),
            ("PWRAEBG/+-G", "breaking"),
            ("TKAEUPBLG/+-S", "dangerous"),
            ("TKPWRAET/+-R", "greater"),
            ("A/+PAEURPBLT", "apparently"),
        ] {
            assert_eq!(model().answer(outline).as_deref(), Some(word), "{outline}");
        }
    }

    #[test]
    fn a_left_hand_stroke_ending_a_word_is_its_consonants_in_any_order() {
        // The left bank strikes Z before L (`STKPW` then `HR`): *curls* ends L Z all the same
        for (outline, word) in [("KUR/+STKPWHR", "curls"), ("KOEU/+STKPWHR", "coils")] {
            assert_eq!(model().answer(outline).as_deref(), Some(word), "{outline}");
        }
    }

    #[test]
    fn a_tie_goes_to_the_more_common_word() {
        // Each pair costs the same once rarity is rounded to whole points: the commoner word wins
        for (outline, word, over) in [("AEU/+T", "eight", "ate"), ("PWO/+-R/+-G", "boring", "baring"), ("TA/+P", "tape", "tap")] {
            assert_eq!(model().answer(outline).as_deref(), Some(word), "{outline}: {word} over {over}");
        }
    }

    #[test]
    fn an_abbreviation_said_in_full_leaves_those_sounds_to_the_full_word() {
        // *tv* is also said *television*: those sounds type *television*, however common *tv* is
        assert_eq!(model().answer("TE/+HRO/+SREURB/+UPB").as_deref(), Some("television"));
    }

    #[test]
    fn the_search_is_wide_enough_for_a_long_run_of_loose_sounds() {
        // Each of these falls out of a 256-state beam before its last stroke settles it
        for (outline, word) in [("THAO*E/+TPH/+KEU/+-PBG", "thinking"), ("-FL/+EU/+-T", "flight"), ("R/+EU/+-F/+E/+-R", "river")] {
            assert_eq!(model().answer(outline).as_deref(), Some(word), "{outline}");
        }
    }

    #[test]
    fn striking_a_words_silent_final_e_spells_that_word() {
        // The written *e* struck as a vowel: *dance*, not the *-er* word that sounds the same
        for (outline, word) in [("TKA/+-PB/+SE", "dance"), ("PW/+EU/+K/+E", "bike"), ("HA*PB/+TK/+HRE", "handle")] {
            assert_eq!(model().answer(outline).as_deref(), Some(word), "{outline}");
        }
    }

    #[test]
    fn a_known_outline_goes_on_into_a_longer_word_spelled_from_it() {
        // *liquid* says IH where *liquidation* says AH: the letters, not the sounds, line them up
        assert_eq!(model().answer("KWEULD/+ARB/+UPB").as_deref(), Some("liquidation"));
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
        for needle in ["pub fn solve_with", "extern \"C\" fn alloc", "extern \"C\" fn fallback", "extern \"C\" fn joins", "extern \"C\" fn init", "extern \"C\" fn context", "include_bytes!(\"05-lyk-solver.bin\")"] {
            assert!(source.contains(needle), "{needle}");
        }
        assert!(Model::load(b"nope").is_none());
    }

    #[test]
    fn the_previous_word_is_the_last_word_or_a_sentence_start() {
        use super::core::previous_word;
        let full = "x".repeat(core::CONTEXT_BYTES);
        for (context, word) in [
            ("", None),
            ("   ", None),
            ("the cat ", Some("cat")),
            ("The Cat", Some("cat")),
            ("I don't", Some("t")),
            ("the dogs' ", Some("dogs")),
            ("It ended. ", Some("<s>")),
            ("\"Stop!\" ", Some("<s>")),
            ("Why?)\n", Some("<s>")),
            ("well, ", None),
            ("one; two:", None),
            ("\"yes\" ", None),
            (full.as_str(), None),
        ] {
            assert_eq!(previous_word(context).as_deref(), word, "{context:?}");
        }
        // A full context's word is whole when a space comes before it, wherever that is
        assert_eq!(previous_word(&format!(" {}", &full[1..])).as_deref(), Some(&full[1..]));
        assert_eq!(previous_word(&format!("{} cat", &full[4..])).as_deref(), Some("cat"));
    }

    #[test]
    fn an_empty_or_unknown_context_changes_nothing() {
        let m = model();
        let mut sc = m.scratch();
        for outline in ["-PB/+OE/+*", "WAOE/+RE", "AEU/+T", "KOPL/+PU/+TER", "-B/+AOT/+-S", "HRAOBG/+/+UP", "PROB/+-BL"] {
            let run: Vec<u32> = outline.split('/').map(|s| core::parse_stroke(s).expect("a stroke")).collect();
            let plain: Vec<(String, u32)> = m.solve_with(&mut sc, &run, 8).into_iter().map(|r| (r.text, r.cost)).collect();
            for context in ["", "well, ", "zzqxj ", "\"yes\" "] {
                let after: Vec<(String, u32)> = m.solve_in(&mut sc, &run, 8, context).into_iter().map(|r| (r.text, r.cost)).collect();
                assert_eq!(after, plain, "{outline} after {context:?}");
                assert_eq!(m.answer_in(&mut sc, outline, context), m.answer(outline), "{outline} after {context:?}");
            }
        }
    }

    #[test]
    fn the_word_before_breaks_a_tie_between_words_that_sound_alike() {
        let m = model();
        for (outline, context, alone, after) in [("-PB/+OE/+*", "I don't ", "no", "know"), ("WAOE/+RE", "they ", "where", "were")] {
            assert_eq!(m.answer(outline).as_deref(), Some(alone), "{outline} alone");
            assert_eq!(m.answer_after(outline, context).as_deref(), Some(after), "{outline} after {context:?}");
        }
        // After a sentence's end the word before is the start itself, not the word before the stop
        assert_eq!(m.answer_after("-PB/+OE/+*", "I don't.  ").as_deref(), m.answer_after("-PB/+OE/+*", "Fine. ").as_deref());
    }

    #[test]
    fn the_word_before_never_moves_a_reading_past_a_whole_outline() {
        let m = model();
        let mut sc = m.scratch();
        let run: Vec<u32> = "WAUT/+-R".split('/').map(|s| core::parse_stroke(s).expect("a stroke")).collect();
        for context in ["", "they ", "I ", "to ", "the "] {
            let first = m.solve_in(&mut sc, &run, 5, context).into_iter().next().expect("a reading");
            assert!(first.exact && first.text == "water", "after {context:?}: {}", first.text);
        }
    }
}

//! The word pool of the adaptive source: lyk's own frequency list and Monkeytype's English lists,
//! merged, deduplicated and put in one order that the adaptive engine walks down.
//!
//! The order. A word's *key* is its rank, and the pool is sorted by key:
//!
//! - a word in lyk's frequency list (`data/frequency/en_full.txt`, filtered as `--source common`
//!   filters: plain words, no names unless `--names`) has its rank in that filtered list;
//! - a word only in one of Monkeytype's frequency-ordered lists (`english`, `english_1k`,
//!   `english_5k`, `english_10k`) has its position in that list times the list's *scale* (the
//!   median of lyk rank / list position over the words both have), so the two rankings are on one
//!   axis; the smallest key over the lists it is in is used;
//! - a word in any of those four lists has its key multiplied by the Monkeytype weight (default
//!   0.7), so it is met earlier than its rank alone says;
//! - then two tiers of words in no ordered list and past lyk's window: first the words only in
//!   `english_25k`, then those only in `english_450k`. Both lists are unordered (their files say
//!   `orderedByFrequency: false`), so each tier is in a fixed scrambled order (a hash of the
//!   word), not alphabetical.
//!
//! The head (everything above the tiers) is built when the pool opens, from the first
//! `LYK_WINDOW` ranks of lyk's list and the four ordered lists: tens of thousands of words, a few
//! milliseconds. A tier is read only when the engine walks past what is loaded, and with
//! `--wordlist lyk` the frequency list is read on in chunks, so no part of the long tail is out of
//! reach and none is loaded before it is needed. A lyk-only word below rank `LYK_WINDOW` is not in
//! the mixed pool: past that rank the subtitle counts are mostly misspellings and fragments; with
//! `--wordlist lyk` they are all kept.

use super::feed::is_plain_word;
use super::sources;
use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use std::collections::{HashMap, HashSet};
use std::io::BufRead;
use std::path::{Path, PathBuf};

/// Which lists make up the pool.
#[derive(Clone, Copy, ValueEnum, PartialEq, Eq, Debug)]
pub enum Wordlist {
    /// Monkeytype's English lists only
    Monkeytype,
    /// lyk's frequency list only
    Lyk,
    /// Both, merged (the default)
    Both,
}

/// The default `--monkeytype-weight`.
pub const DEFAULT_WEIGHT: f64 = 0.7;
/// How many ranks of lyk's list the mixed pool reads (see the module note).
pub const LYK_WINDOW: usize = 50_000;
/// How many lyk ranks `--wordlist lyk` reads at a time.
const LYK_CHUNK: usize = 20_000;
/// Monkeytype lists whose words get the weight.
const BONUS: [&str; 4] = ["english", "english_1k", "english_5k", "english_10k"];
/// The frequency-ordered lists (all but the 450k dictionary).
const ORDERED: [&str; 4] = ["english", "english_1k", "english_5k", "english_10k"];
/// The unordered lists, each a tier after the head and read when the engine walks past it: a 25k
/// word list (`orderedByFrequency` is false in the file) and the 450k dictionary.
const TIERS: [&str; 2] = ["english_25k", "english_450k"];
/// Most positions of a list looked at when scaling it.
const SCALE_SPAN: usize = 5000;
/// Fewest shared words to trust a scale; fewer and the scale is 1.
const SCALE_MIN_SHARED: usize = 20;

#[derive(Clone, Debug)]
pub struct Opts {
    pub wordlist: Wordlist,
    pub weight: f64,
    pub names: bool,
}

/// Where a Monkeytype list is saved by `lyk fetch`.
#[must_use]
pub fn list_path(data: &Path, name: &str) -> PathBuf {
    data.join("wordlists").join(format!("monkeytype-{name}.json"))
}

/// The words of a Monkeytype language file: `{"name": .., "words": [..]}` (or a bare array), in
/// file order.
pub fn parse_monkeytype(json: &str) -> Result<Vec<String>> {
    let value: serde_json::Value = serde_json::from_str(json)?;
    let words = match &value {
        serde_json::Value::Array(a) => a,
        serde_json::Value::Object(o) => match o.get("words") {
            Some(serde_json::Value::Array(a)) => a,
            _ => bail!("no \"words\" array"),
        },
        _ => bail!("not a word list"),
    };
    Ok(words.iter().filter_map(|w| w.as_str().map(str::to_owned)).collect())
}

/// An ordered list's usable words (plain, not a skipped name), in order, each once.
fn usable(words: Vec<String>, names: bool, skip: &HashSet<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    words
        .into_iter()
        .map(|w| if w == "I" { "i".to_owned() } else { w })
        .filter(|w| is_plain_word(w, names) && !skip.contains(w) && seen.insert(w.clone()))
        .collect()
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

/// The scale of a list: the median of lyk rank over list position for the words both have.
fn scale(list: &[String], lyk_rank: &HashMap<&str, usize>) -> f64 {
    let ratios: Vec<f64> = list
        .iter()
        .take(SCALE_SPAN)
        .enumerate()
        .filter_map(|(i, w)| lyk_rank.get(w.as_str()).map(|&r| r as f64 / (i + 1) as f64))
        .collect();
    if ratios.len() < SCALE_MIN_SHARED { 1.0 } else { median(ratios).clamp(0.25, 4.0) }
}

/// The head of the pool in order: lyk's ranked words (if `use_lyk`) and the ordered Monkeytype
/// lists (`lists`, name and words, if any), by key.
#[must_use]
pub fn head(lyk: &[String], lists: &[(String, Vec<String>)], weight: f64, use_lyk: bool) -> Vec<String> {
    let lyk_rank: HashMap<&str, usize> =
        if use_lyk { lyk.iter().enumerate().map(|(i, w)| (w.as_str(), i + 1)).collect() } else { HashMap::new() };
    let mut key: HashMap<&str, f64> = lyk_rank.iter().map(|(w, r)| (*w, *r as f64)).collect();
    let mut bonus: HashSet<&str> = HashSet::new();
    for (name, words) in lists {
        let s = scale(words, &lyk_rank);
        let weighted = BONUS.contains(&name.as_str());
        for (i, w) in words.iter().enumerate() {
            if weighted {
                bonus.insert(w);
            }
            if lyk_rank.contains_key(w.as_str()) {
                continue;
            }
            let e = (i + 1) as f64 * s;
            key.entry(w).and_modify(|k| *k = k.min(e)).or_insert(e);
        }
    }
    let mut ranked: Vec<(f64, &str)> = key.iter().map(|(w, k)| (if bonus.contains(w) { k * weight } else { *k }, *w)).collect();
    ranked.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(b.1)));
    ranked.into_iter().map(|(_, w)| w.to_owned()).collect()
}

/// A fixed scrambling of a word for the unordered tail (FNV-1a).
fn scramble(word: &str) -> u64 {
    word.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// The unordered tail: `words` not already `taken`, scrambled.
#[must_use]
pub fn tail(words: Vec<String>, taken: &HashSet<String>) -> Vec<String> {
    let mut v: Vec<String> = words.into_iter().filter(|w| !taken.contains(w)).collect();
    v.sort_by(|a, b| scramble(a).cmp(&scramble(b)).then(a.cmp(b)));
    v.dedup();
    v
}

/// Words in pool order, fetched by index; the pool loads more behind the call as it is walked.
pub trait Candidates {
    fn get(&mut self, i: usize) -> Option<&str>;
}

impl Candidates for Vec<String> {
    fn get(&mut self, i: usize) -> Option<&str> {
        self.as_slice().get(i).map(String::as_str)
    }
}

enum Stage {
    /// Head built; the unordered tiers (index of the next) are read as the pool is walked
    Tiers(usize),
    /// `--wordlist lyk`: the frequency file is read on in chunks
    LykStream(std::io::Lines<std::io::BufReader<std::fs::File>>),
    Done,
}

pub struct Pool {
    words: Vec<String>,
    stage: Stage,
    opts: Opts,
    data: PathBuf,
    skip: HashSet<String>,
    /// Lists that `--wordlist both` wanted and `lyk fetch` has not downloaded
    pub missing: Vec<String>,
}

impl Pool {
    /// Opens the pool: reads the head only.
    pub fn open(data: &Path, opts: &Opts) -> Result<Self> {
        let skip = if opts.names { HashSet::new() } else { sources::names_in(data) };
        let mut missing = Vec::new();
        let mut lists = Vec::new();
        if opts.wordlist != Wordlist::Lyk {
            for name in ORDERED {
                match std::fs::read_to_string(list_path(data, name)) {
                    Ok(text) => {
                        let words = parse_monkeytype(&text).with_context(|| list_path(data, name).display().to_string())?;
                        lists.push((name.to_owned(), usable(words, opts.names, &skip)));
                    }
                    Err(_) => missing.push(name.to_owned()),
                }
            }
            if opts.wordlist == Wordlist::Monkeytype && lists.is_empty() {
                bail!("{}: no Monkeytype word lists; run `lyk fetch` to download them (`lyk where` shows the folder)", data.join("wordlists").display());
            }
        }
        let freq = data.join("frequency/en_full.txt");
        let mut pool = Self { words: Vec::new(), stage: Stage::Done, opts: opts.clone(), data: data.to_path_buf(), skip, missing };
        match opts.wordlist {
            Wordlist::Lyk => {
                let file = std::fs::File::open(&freq).with_context(|| {
                    format!("{}: no word list; run `lyk fetch` to download the data (`lyk where` shows the folder)", freq.display())
                })?;
                pool.stage = Stage::LykStream(std::io::BufReader::new(file).lines());
                pool.more();
            }
            Wordlist::Both | Wordlist::Monkeytype => {
                let lyk = if opts.wordlist == Wordlist::Both {
                    sources::common(&freq, LYK_WINDOW, opts.names, &pool.skip)
                        .or_else(|e| if lists.is_empty() { Err(e) } else { Ok(Vec::new()) })?
                } else {
                    Vec::new()
                };
                pool.words = head(&lyk, &lists, opts.weight, opts.wordlist == Wordlist::Both);
                pool.stage = Stage::Tiers(0);
            }
        }
        Ok(pool)
    }

    /// Loads the next part of the pool; false when there is none.
    fn more(&mut self) -> bool {
        match std::mem::replace(&mut self.stage, Stage::Done) {
            Stage::Done => false,
            Stage::Tiers(first) => {
                for (i, name) in TIERS.iter().enumerate().skip(first) {
                    let Ok(text) = std::fs::read_to_string(list_path(&self.data, name)) else {
                        self.missing.push((*name).to_owned());
                        continue;
                    };
                    let Ok(words) = parse_monkeytype(&text) else { continue };
                    let taken: HashSet<String> = self.words.iter().cloned().collect();
                    let tier = tail(usable(words, self.opts.names, &self.skip), &taken);
                    if tier.is_empty() {
                        continue;
                    }
                    self.words.extend(tier);
                    self.stage = Stage::Tiers(i + 1);
                    return true;
                }
                false
            }
            Stage::LykStream(mut lines) => {
                let mut chunk = 0;
                while chunk < LYK_CHUNK {
                    let Some(Ok(line)) = lines.next() else { break };
                    let Some(word) = line.split_whitespace().next() else { continue };
                    if is_plain_word(word, self.opts.names) && !self.skip.contains(word) {
                        self.words.push(word.to_owned());
                        chunk += 1;
                    }
                }
                if chunk == LYK_CHUNK {
                    self.stage = Stage::LykStream(lines);
                }
                chunk > 0
            }
        }
    }

    /// Words loaded so far.
    #[cfg(test)]
    #[must_use]
    pub const fn loaded(&self) -> usize {
        self.words.len()
    }
}

impl Candidates for Pool {
    fn get(&mut self, i: usize) -> Option<&str> {
        while i >= self.words.len() {
            if !self.more() {
                return None;
            }
        }
        self.words.as_slice().get(i).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| (*w).to_owned()).collect()
    }

    #[test]
    fn both_monkeytype_formats_parse() {
        let object = r#"{"name":"english","noLazyMode":true,"orderedByFrequency":true,"words":["the","be","of"]}"#;
        assert_eq!(parse_monkeytype(object).unwrap(), ["the", "be", "of"]);
        assert_eq!(parse_monkeytype(r#"["a","b"]"#).unwrap(), ["a", "b"]);
        assert_eq!(parse_monkeytype(r#"{"words":["x",3,"y"]}"#).unwrap(), ["x", "y"], "non-strings are dropped");
        assert!(parse_monkeytype(r#"{"name":"x"}"#).is_err());
        assert!(parse_monkeytype("not json").is_err());
        assert!(parse_monkeytype("3").is_err());
    }

    #[test]
    fn usable_words_are_plain_unskipped_and_unique() {
        let got = usable(v(&["the", "Matt", "NASA", "the", "co2", "don't", "ben"]), false, &HashSet::from(["ben".to_owned()]));
        assert_eq!(got, ["the", "don't"]);
        assert_eq!(usable(v(&["Matt"]), true, &HashSet::new()), ["Matt"]);
    }

    #[test]
    fn the_merge_ranks_by_lyk_then_by_scaled_monkeytype_position() {
        // lyk: 100 words r1..r100; the list repeats r1..r40 at the same positions, so its scale is 1
        let lyk: Vec<String> = (1..=100).map(|i| format!("r{i}")).collect();
        let mut list: Vec<String> = lyk.iter().take(40).cloned().collect();
        list.extend(v(&["mtonly"]));
        let merged = head(&lyk, &[("other".into(), list)], 0.7, true);
        assert_eq!(&merged[..3], ["r1", "r2", "r3"]);
        // not a bonus list: mtonly is the 41st position at scale 1, so after r40 and before r42
        let at = |w: &str| merged.iter().position(|x| x == w).unwrap();
        assert!(at("r40") < at("mtonly") && at("mtonly") < at("r42"), "{:?}", &merged[38..45]);
        assert_eq!(merged.len(), 101, "the union has each word once");
    }

    #[test]
    fn a_scale_other_than_one_stretches_monkeytype_positions() {
        // the list holds every second lyk word: position p is lyk rank 2p, so its scale is 2
        let lyk: Vec<String> = (1..=200).map(|i| format!("r{i}")).collect();
        let mut list: Vec<String> = (1..=50).map(|p| format!("r{}", 2 * p)).collect();
        list.push("mtonly".into());
        assert!((scale(&list, &lyk.iter().enumerate().map(|(i, w)| (w.as_str(), i + 1)).collect()) - 2.0).abs() < 1e-9);
        let merged = head(&lyk, &[("other".into(), list)], 1.0, true);
        let at = |w: &str| merged.iter().position(|x| x == w).unwrap();
        // 51 * 2 = 102: after r101, before r103
        assert!(at("r101") < at("mtonly") && at("mtonly") < at("r103"), "{:?}", &merged[99..105]);
    }

    #[test]
    fn the_weight_moves_bonus_list_words_earlier_and_only_those() {
        let lyk: Vec<String> = (1..=100).map(|i| format!("r{i}")).collect();
        let bonus = ("english_10k".to_owned(), v(&["r50"]));
        let plain = ("other".to_owned(), v(&["r60"]));
        let merged = head(&lyk, &[bonus, plain], 0.7, true);
        let at = |w: &str| merged.iter().position(|x| x == w).unwrap();
        // r50 * 0.7 = 35: before r36; r60 is not weighted
        assert!(at("r50") < at("r36") && at("r50") > at("r34"), "r50 sits at {}", at("r50"));
        assert_eq!(at("r60"), 59);
        let none = head(&lyk, &[("english_10k".to_owned(), v(&["r50"]))], 1.0, true);
        assert_eq!(none.iter().position(|x| x == "r50"), Some(49), "weight 1 changes nothing");
    }

    #[test]
    fn monkeytype_only_ranks_by_position_and_lyk_only_by_lyk() {
        let lyk = v(&["a1", "b1"]);
        let lists = [("english".to_owned(), v(&["x1", "y1", "a1"]))];
        assert_eq!(head(&lyk, &lists, 1.0, false), ["x1", "y1", "a1"]);
        assert_eq!(head(&lyk, &[], 1.0, true), ["a1", "b1"]);
        // mixed: a1 is in both and keeps its lyk rank 1; x1 and y1 are at positions 1 and 2 (scale 1)
        let merged = head(&lyk, &lists, 1.0, true);
        assert_eq!(merged.len(), 4);
        assert_eq!(merged[0], "a1");
    }

    #[test]
    fn the_unordered_tail_leaves_out_the_head_and_is_not_alphabetical() {
        let words: Vec<String> = (0..200).map(|i| format!("w{i:03}")).collect();
        let taken: HashSet<String> = words.iter().take(10).cloned().collect();
        let t = tail(words.clone(), &taken);
        assert_eq!(t.len(), 190);
        assert!(t.iter().all(|w| !taken.contains(w)));
        let mut sorted = t.clone();
        sorted.sort();
        assert_ne!(t, sorted, "scrambled, not alphabetical");
        assert_eq!(t, tail(words, &taken), "and the same every time");
    }

    fn data_dir(name: &str, with_lists: bool) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lyk-pool-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("frequency")).unwrap();
        std::fs::create_dir_all(dir.join("wordlists")).unwrap();
        std::fs::write(dir.join("frequency/en_full.txt"), "the 9\nof 8\nand 7\nll 6\ncat 5\ndog 4\nzzz 1\n").unwrap();
        if with_lists {
            std::fs::write(list_path(&dir, "english"), r#"{"name":"english","words":["the","dog","apple"]}"#).unwrap();
            std::fs::write(list_path(&dir, "english_450k"), r#"{"name":"english_450k","words":["Aaron","apple","zebra","yak","the","cat"]}"#).unwrap();
            std::fs::write(list_path(&dir, "english_25k"), r#"{"name":"english_25k","orderedByFrequency":false,"words":["quokka","yak","cat"]}"#).unwrap();
        }
        dir
    }

    #[test]
    fn the_pool_walks_head_then_the_lazy_tail_and_never_repeats() {
        let dir = data_dir("walk", true);
        let mut pool = Pool::open(&dir, &Opts { wordlist: Wordlist::Both, weight: 0.7, names: false }).unwrap();
        let head_len = pool.loaded();
        // the, of, and, cat, dog, zzz (lyk) + apple (monkeytype only): 7 words; the tiers add quokka, then yak and zebra
        assert_eq!(head_len, 7, "the 450k list is not read yet");
        let mut all = Vec::new();
        let mut i = 0;
        while let Some(w) = pool.get(i) {
            all.push(w.to_owned());
            i += 1;
        }
        assert_eq!(all.len(), 10, "{all:?}");
        let unique: HashSet<&String> = all.iter().collect();
        assert_eq!(unique.len(), all.len(), "{all:?}");
        let at = |w: &str| all.iter().position(|x| x == w).unwrap();
        assert!(at("quokka") < at("yak") && at("quokka") < at("zebra"), "the 25k tier comes before the 450k one: {all:?}");
        assert!(!all.contains(&"Aaron".to_owned()) && !all.contains(&"ll".to_owned()));
        // the weighted words are early: the and dog are in the weighted english list
        assert_eq!(all[0], "the");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore = "diagnostic: LYK_DATA=dir with the fetched data, run with --ignored --nocapture"]
    fn dump_the_real_pool() {
        let data = std::env::var_os("LYK_DATA").map(PathBuf::from).expect("LYK_DATA");
        for (wordlist, weight) in [(Wordlist::Both, 0.7), (Wordlist::Both, 1.0), (Wordlist::Monkeytype, 0.7), (Wordlist::Lyk, 0.7)] {
            let mut pool = Pool::open(&data, &Opts { wordlist, weight, names: false }).unwrap();
            let head = pool.loaded();
            let mut n = 0;
            while pool.get(n).is_some() {
                n += 1;
                if n >= 600_000 {
                    break;
                }
            }
            let first: Vec<&str> = (0..30).filter_map(|i| pool.get(i).map(|w| &*Box::leak(w.to_owned().into_boxed_str()))).collect();
            eprintln!("{wordlist:?} weight {weight}: head {head}, total {n}, first {first:?}");
        }
    }

    #[test]
    fn modes_and_missing_files() {
        let dir = data_dir("modes", true);
        let mut mt = Pool::open(&dir, &Opts { wordlist: Wordlist::Monkeytype, weight: 0.7, names: false }).unwrap();
        assert_eq!(mt.get(0), Some("the"));
        let mut lyk = Pool::open(&dir, &Opts { wordlist: Wordlist::Lyk, weight: 0.7, names: false }).unwrap();
        let first: Vec<String> = (0..6).filter_map(|i| lyk.get(i).map(str::to_owned)).collect();
        assert_eq!(first, ["the", "of", "and", "cat", "dog", "zzz"], "lyk order, fragments dropped");
        assert_eq!(lyk.get(6), None);
        let _ = std::fs::remove_dir_all(&dir);

        // no lists downloaded: both falls back to lyk and says which lists are missing; monkeytype alone fails
        let bare = data_dir("bare", false);
        let std::result::Result::Ok(mut both) = Pool::open(&bare, &Opts { wordlist: Wordlist::Both, weight: 0.7, names: false }) else { panic!("both falls back") };
        assert_eq!(both.get(0), Some("the"));
        assert_eq!(both.missing.len(), ORDERED.len());
        let err = Pool::open(&bare, &Opts { wordlist: Wordlist::Monkeytype, weight: 0.7, names: false }).err().expect("fails").to_string();
        assert!(err.contains("lyk fetch"), "{err}");
        let _ = std::fs::remove_dir_all(&bare);
    }

    #[test]
    fn the_lyk_stream_keeps_reading_in_chunks_without_a_cap() {
        let dir = std::env::temp_dir().join(format!("lyk-pool-stream-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("frequency")).unwrap();
        let n = LYK_CHUNK * 2 + 5;
        let letters = |mut i: usize| -> String { let mut s = String::from("zz"); for _ in 0..4 { s.push((b'a' + (i % 26) as u8) as char); i /= 26; } s };
        let text: String = (0..n).fold(String::new(), |mut s, i| {
            std::fmt::Write::write_fmt(&mut s, format_args!("{} {}\n", letters(i), n - i)).unwrap();
            s
        });
        std::fs::write(dir.join("frequency/en_full.txt"), text).unwrap();
        let mut pool = Pool::open(&dir, &Opts { wordlist: Wordlist::Lyk, weight: 0.7, names: false }).unwrap();
        assert_eq!(pool.loaded(), LYK_CHUNK, "one chunk at open");
        assert!(pool.get(LYK_CHUNK * 2 + 4).is_some(), "the pool reads on as it is walked");
        assert_eq!(pool.get(n), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

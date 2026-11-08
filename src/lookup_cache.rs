//! What `lookup` reads, kept in [`DIR`] in a form it reads back in milliseconds.
//!
//! A lookup needs both reference dictionaries, every pronunciation and the dictionary `build`
//! wrote, and parsing them from their text files takes about two seconds, most of it the 130 MB
//! backups file read twice. The first lookup after any of them changes, or after the program is
//! rebuilt, parses them as ever and writes them here; every later one reads them back. The
//! backups are written sorted by word, and a lookup reads only the outlines of the words it shows.
//!
//! A cache is keyed on the size and modification time of the program and of every file it was
//! made from, so a stale one is never read: it is made again. Only what a lookup reads is kept
//! (see [`Setup::for_lookup`]); every other command loads its data as before.

use crate::analysis::Setup;
use crate::built::{self, Backups};
use crate::data::Pronunciations;
use crate::generate::{self, Entry, Origin};
use crate::gold::Gold;
use crate::output::{FAMILY_FILE, Role};
use crate::rules::Step;
use crate::stroke::Stroke;
use anyhow::Result;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Where the caches live: beside the built dictionary, never under `data/`.
pub const DIR: &str = "out/cache";
const LOOKUP_FILE: &str = "lookup.bin";
const BACKUPS_FILE: &str = "backups.bin";
const LOOKUP_MAGIC: &[u8] = b"lyk lookup cache 1\n";
const BACKUPS_MAGIC: &[u8] = b"lyk backups cache 1\n";

/// The files a lookup's data is made from: the pronunciations and reference dictionaries
/// [`Setup::load`] and [`Gold::load`] read, and the dictionary `build` wrote to `built_dir`.
fn inputs(data: &Path, built_dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = [
        "pronunciation/cmudict.dict",
        "pronunciation/english_us_mfa.dict",
        "pronunciation/wikipron_us_broad.tsv",
        "dictionaries/plover-main.json",
        "dictionaries/lapwing-base.json",
    ]
    .iter()
    .map(|file| data.join(file))
    .collect();
    files.extend(Role::ALL.map(|role| built_dir.join(role.file_name())));
    files.push(built_dir.join(FAMILY_FILE));
    files
}

/// What a cache made now from `files` by this program is keyed on: a line for the program and
/// each file, with its size and modification time (a missing one as `0 0`).
fn key(files: &[PathBuf]) -> String {
    let mut key = String::new();
    for path in std::env::current_exe().ok().iter().chain(files) {
        let (size, modified) = std::fs::metadata(path).map_or((0, 0), |meta| {
            let modified = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
            (meta.len(), modified.map_or(0, |d| d.as_nanos()))
        });
        let _ = writeln!(key, "{} {size} {modified}", path.display());
    }
    key
}

/// What a lookup reads.
pub struct Loaded {
    pub setup: Setup,
    /// The dictionary `build` wrote, read back
    pub entries: Vec<Entry>,
    pub backups: Option<Backups>,
    /// Each word's cheapest entry (`generate::cheapest`), by its place in `entries`, when the
    /// cache had it
    pub cheapest: Option<Vec<u32>>,
}

/// What `lookup` reads, the dictionary read back from `built_dir`: from the cache in `cache_dir`
/// when it is current, else loaded as every other command loads it and written there for the
/// next lookup. A cache that cannot be written is only slower.
pub fn load(data: &Path, rules: Option<&Path>, common: usize, built_dir: &Path, cache_dir: &Path) -> Result<Loaded> {
    let key = key(&inputs(data, built_dir));
    if let Some(Cached { gold, pron, entries, cheapest, backups }) = read(cache_dir, &key) {
        let setup = Setup::for_lookup(crate::analysis::load_rules(rules)?, pron, gold);
        return Ok(Loaded { setup, entries, backups: Some(backups), cheapest: Some(cheapest) });
    }
    let setup = Setup::load(data, rules, Some(common))?;
    let entries = built::load(built_dir)?;
    let backups = Backups::load(built_dir).ok();
    if let Some(backups) = &backups {
        let _ = write(cache_dir, &key, &setup.gold, &setup.pron, &entries, backups);
    }
    Ok(Loaded { setup, entries, backups, cheapest: None })
}

/// Everything the cache holds, read back.
struct Cached {
    gold: Gold,
    pron: Pronunciations,
    entries: Vec<Entry>,
    cheapest: Vec<u32>,
    backups: Backups,
}

/// [`generate::cheapest`] as places in `entries`.
fn cheapest_at(entries: &[Entry]) -> Vec<u32> {
    let at: HashMap<*const Stroke, u32> = entries.iter().zip(0..).map(|(e, n)| (e.outline.as_ptr(), n)).collect();
    let mut cheapest: Vec<u32> = generate::cheapest(entries).values().map(|o| at[&o.as_ptr()]).collect();
    cheapest.sort_unstable();
    cheapest
}

/// How many sections the pronunciations are written in, to be decoded side by side: a few
/// hundred thousand words of a few sounds each, every sound its own string, are most of the cache.
const PRON_PARTS: usize = 4;

/// The sections before the pronunciations: the gold dictionaries, the entries, the cheapest.
const SECTIONS: usize = 3;

/// Every cached part, if both files are there and were made from `key`.
fn read(cache_dir: &Path, key: &str) -> Option<Cached> {
    let backups = SortedBackups::open(&cache_dir.join(BACKUPS_FILE), key)?;
    let bytes = std::fs::read(cache_dir.join(LOOKUP_FILE)).ok()?;
    let mut header = Reader::new(bytes.strip_prefix(LOOKUP_MAGIC)?);
    if header.str()? != key {
        return None;
    }
    let lengths: Vec<usize> = (0..SECTIONS + PRON_PARTS).map(|_| header.len()).collect::<Option<_>>()?;
    let mut rest = header.rest();
    let mut sections = Vec::with_capacity(lengths.len());
    for &len in &lengths {
        let (section, after) = rest.split_at_checked(len)?;
        sections.push(section);
        rest = after;
    }
    if !rest.is_empty() {
        return None;
    }
    // Independent sections, each a few tens of milliseconds to decode: side by side
    let (gold, entries, cheapest, parts) = std::thread::scope(|scope| {
        let gold = scope.spawn(|| Gold::read(&mut Reader::new(sections[0])));
        // Every part's thread started before any is waited on
        #[allow(clippy::needless_collect)]
        let parts: Vec<_> =
            sections[SECTIONS..].iter().map(|&part| scope.spawn(move || read_pron(&mut Reader::new(part)))).collect();
        let entries = read_entries(&mut Reader::new(sections[1]));
        let cheapest = Reader::new(sections[2]).numbers();
        let parts: Option<Vec<_>> = parts.into_iter().map(|part| part.join().ok().flatten()).collect();
        (gold.join().ok().flatten(), entries, cheapest, parts)
    });
    let parts = parts?;
    let mut pron = Pronunciations::with_capacity(parts.iter().map(Vec::len).sum());
    pron.extend(parts.into_iter().flatten());
    let (entries, cheapest) = (entries?, cheapest?);
    if cheapest.iter().any(|&at| at as usize >= entries.len()) {
        return None;
    }
    Some(Cached { gold: gold?, pron, entries, cheapest, backups: Backups::sorted(backups) })
}

/// Write both files, each to a temporary name first so a reader never sees half of one.
fn write(
    cache_dir: &Path,
    key: &str,
    gold: &Gold,
    pron: &Pronunciations,
    entries: &[Entry],
    backups: &Backups,
) -> std::io::Result<()> {
    let mut sections: Vec<Writer> = (0..SECTIONS + PRON_PARTS).map(|_| Writer::default()).collect();
    gold.write(&mut sections[0]);
    if !write_entries(&mut sections[1], entries) {
        return Ok(());
    }
    let cheapest = cheapest_at(entries);
    sections[2].len(cheapest.len());
    for at in cheapest {
        sections[2].u32(at);
    }
    let mut words: Vec<&String> = pron.keys().collect();
    words.sort_unstable();
    for (part, words) in sections[SECTIONS..].iter_mut().zip(words.chunks(words.len().div_ceil(PRON_PARTS).max(1)).chain(std::iter::repeat(&[][..]))) {
        write_pron(part, pron, words);
    }
    let mut file = Writer(LOOKUP_MAGIC.to_vec());
    file.str(key);
    for section in &sections {
        file.len(section.0.len());
    }
    for section in &sections {
        file.0.extend_from_slice(&section.0);
    }
    let Some(sorted) = backups.sorted_bytes(key) else { return Ok(()) };
    std::fs::create_dir_all(cache_dir)?;
    replace(&cache_dir.join(BACKUPS_FILE), &sorted)?;
    replace(&cache_dir.join(LOOKUP_FILE), &file.0)
}

fn replace(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temporary = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(&temporary, path)
}

/// Lengths and numbers little-endian in four bytes, a string as its length then its bytes.
#[derive(Default)]
pub struct Writer(Vec<u8>);

impl Writer {
    pub fn u32(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    pub fn len(&mut self, len: usize) {
        self.u32(u32::try_from(len).expect("a cache section under 4 GB"));
    }

    pub fn str(&mut self, text: &str) {
        self.len(text.len());
        self.0.extend_from_slice(text.as_bytes());
    }

    pub fn strs(&mut self, all: &[String]) {
        self.len(all.len());
        for text in all {
            self.str(text);
        }
    }

    pub fn strokes(&mut self, strokes: &[Stroke]) {
        self.len(strokes.len());
        for stroke in strokes {
            self.u32(stroke.0);
        }
    }

    /// A map's entries sorted by key, so the same map always writes the same bytes.
    pub fn map<V>(&mut self, map: &HashMap<String, V>, mut value: impl FnMut(&mut Self, &V)) {
        let mut keys: Vec<&String> = map.keys().collect();
        keys.sort_unstable();
        self.len(keys.len());
        for key in keys {
            self.str(key);
            value(self, &map[key]);
        }
    }
}

/// Reads what [`Writer`] wrote; `None` on anything short or malformed.
pub struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let (taken, rest) = self.bytes.split_at_checked(n)?;
        self.bytes = rest;
        Some(taken)
    }

    const fn rest(self) -> &'a [u8] {
        self.bytes
    }

    pub fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    pub fn len(&mut self) -> Option<usize> {
        self.u32().and_then(|n| usize::try_from(n).ok())
    }

    /// A length that sizes an allocation, no more than the bytes left could hold.
    pub fn count(&mut self) -> Option<usize> {
        self.len().filter(|&n| n <= self.bytes.len())
    }

    pub fn str(&mut self) -> Option<&'a str> {
        let n = self.len()?;
        std::str::from_utf8(self.take(n)?).ok()
    }

    pub fn string(&mut self) -> Option<String> {
        self.str().map(str::to_owned)
    }

    pub fn strings(&mut self) -> Option<Vec<String>> {
        let n = self.count()?;
        (0..n).map(|_| self.string()).collect()
    }

    pub fn strokes(&mut self) -> Option<Vec<Stroke>> {
        self.numbers().map(|n| n.into_iter().map(Stroke).collect())
    }

    pub fn numbers(&mut self) -> Option<Vec<u32>> {
        let n = self.count()?;
        (0..n).map(|_| self.u32()).collect()
    }

    pub fn map<V>(&mut self, mut value: impl FnMut(&mut Self) -> Option<V>) -> Option<HashMap<String, V>> {
        let n = self.count()?;
        let mut map = HashMap::with_capacity(n);
        for _ in 0..n {
            let key = self.string()?;
            map.insert(key, value(self)?);
        }
        Some(map)
    }
}

/// The pronunciations of `words`, in that order.
fn write_pron(w: &mut Writer, pron: &Pronunciations, words: &[&String]) {
    w.len(words.len());
    for &word in words {
        w.str(word);
        let variants = &pron[word];
        w.len(variants.len());
        for sounds in variants {
            w.strs(sounds);
        }
    }
}

fn read_pron(r: &mut Reader<'_>) -> Option<Vec<(String, Vec<Vec<String>>)>> {
    let n = r.count()?;
    (0..n)
        .map(|_| {
            let word = r.string()?;
            let variants = r.count()?;
            Some((word, (0..variants).map(|_| r.strings()).collect::<Option<_>>()?))
        })
        .collect()
}

/// Every origin an entry read back from `out/lyk` can have (see `built::load`).
const ORIGINS: [Origin; 8] = [
    Origin::Rules,
    Origin::Short(Step::Skeleton),
    Origin::Alternate,
    Origin::Backup,
    Origin::Extra,
    Origin::Phrase,
    Origin::Suffix,
    Origin::Family,
];

/// `false`, writing nothing usable, when an entry has an origin reading back never gives.
fn write_entries(w: &mut Writer, entries: &[Entry]) -> bool {
    w.len(entries.len());
    for entry in entries {
        let Some(origin) = ORIGINS.iter().position(|&o| o == entry.origin) else { return false };
        w.str(&entry.word);
        w.strokes(&entry.outline);
        w.len(origin);
    }
    true
}

fn read_entries(r: &mut Reader<'_>) -> Option<Vec<Entry>> {
    let n = r.count()?;
    (0..n)
        .map(|_| {
            let word = r.string()?;
            let outline = r.strokes()?;
            let origin = *ORIGINS.get(r.len()?)?;
            Some(Entry { word, outline, origin })
        })
        .collect()
}

/// The backups file's outlines by word, sorted, as [`SortedBackups::bytes`] writes them: the
/// magic and key, the number of words, a table of each word's place in the text that follows
/// (where it starts, its length, the length of its outlines after it), then the text: each word
/// followed by its outlines, a line each. Opening it reads only the header; finding a word reads
/// the few table rows and words a binary search needs.
pub struct SortedBackups {
    file: std::sync::Mutex<std::fs::File>,
    words: u64,
    table: u64,
}

/// A table row's bytes: where its word starts, its length, its outlines' length
const ROW: usize = 12;

impl SortedBackups {
    /// The file the outlines of `index` (a word to its outlines, in the order the backups file
    /// lists them) are written as.
    pub fn bytes<'a>(key: &str, index: impl IntoIterator<Item = (&'a str, Vec<&'a str>)>) -> Option<Vec<u8>> {
        let mut words: Vec<(&str, Vec<&str>)> = index.into_iter().collect();
        words.sort_unstable_by(|a, b| a.0.cmp(b.0));
        let mut file = Writer(BACKUPS_MAGIC.to_vec());
        file.str(key);
        file.len(words.len());
        let mut text = Vec::new();
        for (word, outlines) in &words {
            let lines = outlines.join("\n");
            file.len(text.len());
            file.len(word.len());
            file.len(lines.len());
            text.extend_from_slice(word.as_bytes());
            text.extend_from_slice(lines.as_bytes());
        }
        u32::try_from(text.len()).ok()?;
        file.0.extend_from_slice(&text);
        Some(file.0)
    }

    fn open(path: &Path, key: &str) -> Option<Self> {
        let mut file = std::fs::File::open(path).ok()?;
        let mut head = vec![0; BACKUPS_MAGIC.len() + 4 + key.len() + 4];
        file.read_exact(&mut head).ok()?;
        let mut header = Reader::new(head.strip_prefix(BACKUPS_MAGIC)?);
        if header.str()? != key {
            return None;
        }
        let words = u64::from(header.u32()?);
        Some(Self { file: std::sync::Mutex::new(file), words, table: head.len() as u64 })
    }

    fn read_at(file: &mut std::fs::File, at: u64, n: usize) -> Option<Vec<u8>> {
        file.seek(SeekFrom::Start(at)).ok()?;
        let mut bytes = vec![0; n];
        file.read_exact(&mut bytes).ok()?;
        Some(bytes)
    }

    /// The outlines of `word` (lowercase), as text, in the backups file's order.
    pub fn outlines(&self, word: &str) -> Vec<String> {
        let Ok(mut file) = self.file.lock() else { return Vec::new() };
        let text = self.table + self.words * ROW as u64;
        let (mut low, mut high) = (0, self.words);
        while low < high {
            let middle = low + (high - low) / 2;
            let Some(row) = Self::read_at(&mut file, self.table + middle * ROW as u64, ROW) else { return Vec::new() };
            let mut row = Reader::new(&row);
            let (Some(at), Some(len), Some(lines)) = (row.u32(), row.len(), row.len()) else { return Vec::new() };
            let Some(here) = Self::read_at(&mut file, text + u64::from(at), len + lines) else { return Vec::new() };
            match here[..len].cmp(word.as_bytes()) {
                std::cmp::Ordering::Less => low = middle + 1,
                std::cmp::Ordering::Greater => high = middle,
                std::cmp::Ordering::Equal => {
                    let lines = String::from_utf8_lossy(&here[len..]);
                    return lines.split('\n').filter(|l| !l.is_empty()).map(str::to_owned).collect();
                }
            }
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stroke::{parse_outline, render_outline};

    fn fresh_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lyk-cache-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn built_dir(root: &Path) -> PathBuf {
        let dir = root.join("lyk");
        std::fs::create_dir_all(&dir).unwrap();
        for role in Role::ALL {
            std::fs::write(dir.join(role.file_name()), "{}").unwrap();
        }
        std::fs::write(dir.join(Role::Words.file_name()), "{\n  \"KAT\": \"cat\",\n  \"TKOG\": \"dog\"\n}").unwrap();
        std::fs::write(
            dir.join(Role::Backups.file_name()),
            "{\n  \"KA/+T\": \"cat\",\n  \"TKO/+G\": \"dog\",\n  \"TK/+O/+G\": \"dog\",\n  \"S\": \"I\"\n}",
        )
        .unwrap();
        dir
    }

    fn rendered(entries: &[Entry]) -> Vec<(String, String, Origin)> {
        entries.iter().map(|e| (e.word.clone(), render_outline(&e.outline), e.origin)).collect()
    }

    fn pron() -> Pronunciations {
        HashMap::from([
            ("cat".to_owned(), vec![vec!["K".to_owned(), "AE1".to_owned(), "T".to_owned()]]),
            ("read".to_owned(), vec![vec!["R".to_owned(), "IY1".to_owned(), "D".to_owned()], vec!["R".to_owned(), "EH1".to_owned(), "D".to_owned()]]),
        ])
    }

    #[test]
    fn what_is_written_reads_back_the_same() {
        let words = |p: &Pronunciations| { let mut w: Vec<String> = p.keys().cloned().collect(); w.sort(); w };
        let all = pron();
        let sorted = words(&all);
        let mut w = Writer::default();
        write_pron(&mut w, &all, &sorted.iter().collect::<Vec<_>>());
        assert_eq!(read_pron(&mut Reader::new(&w.0)).map(|p| p.into_iter().collect::<Pronunciations>()), Some(pron()));

        let gold = &crate::dictionary_fixture::setup().gold;
        let mut w = Writer::default();
        gold.write(&mut w);
        let back = Gold::read(&mut Reader::new(&w.0)).unwrap();
        let mut again = Writer::default();
        back.write(&mut again);
        assert_eq!(w.0, again.0, "the gold dictionaries read back write the same bytes");

        let entries = &crate::dictionary_fixture::generated().entries;
        let read_back: Vec<Entry> = entries.iter().filter(|e| ORIGINS.contains(&e.origin)).map(|e| Entry { word: e.word.clone(), outline: e.outline.clone(), origin: e.origin }).collect();
        let mut w = Writer::default();
        assert!(write_entries(&mut w, &read_back));
        assert_eq!(rendered(&read_entries(&mut Reader::new(&w.0)).unwrap()), rendered(&read_back));
        assert!(!write_entries(&mut Writer::default(), &[Entry { word: "x".into(), outline: vec![], origin: Origin::Brief }]), "an origin reading back never gives is not cached");
    }

    #[test]
    fn something_short_or_malformed_reads_as_nothing() {
        let all = pron();
        let mut w = Writer::default();
        write_pron(&mut w, &all, &all.keys().collect::<Vec<_>>());
        for cut in 0..w.0.len() {
            assert_eq!(read_pron(&mut Reader::new(&w.0[..cut])), None, "cut at {cut}");
        }
        assert_eq!(read_pron(&mut Reader::new(&u32::MAX.to_le_bytes())), None, "a huge count allocates nothing");
    }

    #[test]
    fn the_sorted_backups_give_every_word_the_outlines_the_backups_file_does() {
        let root = fresh_dir("sorted");
        let dir = built_dir(&root);
        let backups = Backups::load(&dir).unwrap();
        let bytes = backups.sorted_bytes("key").unwrap();
        std::fs::write(root.join("b.bin"), bytes).unwrap();
        assert!(SortedBackups::open(&root.join("b.bin"), "other key").is_none(), "another key is another cache");
        let sorted = Backups::sorted(SortedBackups::open(&root.join("b.bin"), "key").unwrap());
        for word in ["cat", "dog", "i", "a", "zebra", ""] {
            let text = |b: &Backups| b.outlines(word).iter().map(|o| render_outline(o)).collect::<Vec<_>>();
            assert_eq!(text(&sorted), text(&backups), "{word}");
        }
        assert_eq!(sorted.outlines("dog"), vec![parse_outline("TKO/+G").unwrap(), parse_outline("TK/+O/+G").unwrap()]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_cache_is_read_back_until_a_file_it_was_made_from_changes() {
        let root = fresh_dir("key");
        let dir = built_dir(&root);
        let cache = root.join("cache");
        let files = inputs(&root.join("data"), &dir);
        let made = key(&files);
        let gold = &crate::dictionary_fixture::setup().gold;
        let entries = built::load(&dir).unwrap();
        write(&cache, &made, gold, &pron(), &entries, &Backups::load(&dir).unwrap()).unwrap();

        let cached = read(&cache, &key(&files)).expect("a current cache reads back");
        assert_eq!(cached.pron, pron());
        assert_eq!(rendered(&cached.entries), rendered(&entries));
        assert_eq!(cached.backups.outlines("dog").len(), 2);
        let entry = |at: u32| &cached.entries[at as usize];
        let mut cheapest: Vec<(&str, &[Stroke])> =
            cached.cheapest.iter().map(|&at| (entry(at).word.as_str(), entry(at).outline.as_slice())).collect();
        let mut want: Vec<(&str, &[Stroke])> = generate::cheapest(&entries).into_iter().collect();
        cheapest.sort_unstable();
        want.sort_unstable();
        assert_eq!(cheapest, want, "each word's cheapest entry is kept");

        std::fs::write(dir.join(Role::Words.file_name()), "{\n  \"KAT\": \"cat\"\n}").unwrap();
        assert!(read(&cache, &key(&files)).is_none(), "a changed dictionary file makes the cache stale");
        assert!(read(&cache, &made).is_some(), "the key alone decides");
        std::fs::write(cache.join(LOOKUP_FILE), b"lyk lookup cache 1\n").unwrap();
        assert!(read(&cache, &made).is_none(), "a cut-short cache is no cache");
        let _ = std::fs::remove_dir_all(&root);
    }
}

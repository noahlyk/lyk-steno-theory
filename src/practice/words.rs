//! Which words are practice words: the one predicate behind the pool, the seeding of `vocab.json`,
//! `lyk vocab clean` and the adaptive feed.
//!
//! A word qualifies when all of these hold ([`WordFilter::accepts`]):
//!
//! 1. **Shape.** Letters only (an inner apostrophe only in a real contraction of [`CONTRACTIONS`]),
//!    two letters or more except `a` and `I`.
//! 2. **A real, common-enough word.** One of: a contraction of the list; a word of Monkeytype's
//!    `english`, `english_1k`, `_5k`, `_10k` or `_25k` lists; a word of [`SPOKEN_WORDS`] (ordinary
//!    words the lists leave out: `ok`, the profanity of speech); or a word within
//!    [`LYK_RANK_LIMIT`] of that list *and* in Monkeytype's `english_450k` dictionary with four
//!    letters or more, or three letters within rank [`SHORT_RANK_LIMIT`] (a rank alone is not enough there: the subtitle counts rank names, fragments
//!    and misspellings). `--deep` lifts the rank and length limits: any word of the 450k dictionary.
//! 3. **Not a morpheme fragment.** No text of a suffix or prefix entry of lyk's rules
//!    (`rules/suffixes.toml`, `rules/morphemes.toml`, the family endings and prefixes) and none of
//!    [`EXTRA_FRAGMENTS`], unless it is also a real word (Monkeytype's lists up to the 10k, or
//!    [`REAL_WORDS`]: `age`, `less`, `over`).
//! 4. **Not noise.** Not a vocal noise or exclamation (`crate::prose::noise_letters`).
//! 5. **Not a name** that Plover and Lapwing only write capitalised, unless `--names`.
//!
//! `--deep` keeps 3, 4 and 5; only the frequency limits of 2 lift. When Monkeytype's lists are not
//! downloaded the rank alone decides (the 450k dictionary and the length rule need the lists), and
//! with `--deep` it does not decide at all.

use super::feed::is_plain_word;
use super::pool::{list_path, parse_monkeytype};
use super::sources;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

/// Version of this filter: stored as `filter_version` in `vocab.json`; a file with a lower one is
/// cleaned once by `lyk practice`. Bump it when the predicate changes what it accepts.
pub const FILTER_VERSION: u32 = 1;
/// A word of lyk's frequency list qualifies by rank up to this one (with the dictionary check).
pub const LYK_RANK_LIMIT: usize = 20_000;
/// Ordinary words of speech that Monkeytype's lists leave out (its lists are filtered) and that the
/// typist's strokes and subtitle counts have in plenty: they qualify without a list.
pub const SPOKEN_WORDS: &[&str] = &[
    "ok", "shit", "fuck", "fucking", "fucked", "ass", "asshole", "bitch", "bastard", "bullshit", "sex", "goddamn", "dick", "butt", "whore",
    "motherfucker", "pissed", "piss", "till", "tv", "dna", "mama", "papa", "doc", "lab", "bro", "nah", "yep", "yup", "thief", "darling", "christmas",
    "sergeant", "honour", "tip", "highness", "english", "spanish", "mum", "mums", "moms",
];
/// The fewest letters a word needs when only the dictionary and the rank vouch for it, unless its
/// rank is within `SHORT_RANK_LIMIT` (`owe`, `beg`, `hug`; not `abb`, `ack`, `ahn`).
pub const MIN_LETTERS_BY_RANK: usize = 4;
/// A three-letter word qualifies by the dictionary and the rank within this one.
pub const SHORT_RANK_LIMIT: usize = 5_000;
/// Monkeytype's lists that vouch for a word on their own, and the ones that also make it real
/// enough to be a word where a fragment shares its spelling.
const CORE_LISTS: [&str; 4] = ["english", "english_1k", "english_5k", "english_10k"];
const WIDE_LIST: &str = "english_25k";
const DICTIONARY: &str = "english_450k";

/// The contractions that may be typed: an inner apostrophe in any other word disqualifies it.
pub const CONTRACTIONS: &[&str] = &[
    "i'm", "i've", "i'll", "i'd", "you're", "you've", "you'll", "you'd", "he's", "he'll", "he'd", "she's", "she'll", "she'd", "it's",
    "it'll", "we're", "we've", "we'll", "we'd", "they're", "they've", "they'll", "they'd", "that's", "that'll", "there's", "here's",
    "what's", "who's", "where's", "how's", "let's", "don't", "doesn't", "didn't", "can't", "couldn't", "won't", "wouldn't", "shouldn't",
    "isn't", "aren't", "wasn't", "weren't", "hasn't", "haven't", "hadn't", "mustn't", "needn't", "ain't", "o'clock", "ma'am", "y'all",
];

/// Pieces of words and non-words that lyk's own rules do not list but the tape and the frequency
/// counts carry: suffix and prefix sounds, letter pairs, syllables.
pub const EXTRA_FRAGMENTS: &[&str] = &[
    "er", "ed", "ly", "le", "en", "ent", "st", "ta", "kon", "eer", "ing", "ion", "ism", "ist", "ive", "ous", "ish", "ful", "est", "ity", "ize",
    "ize", "ify", "ious", "ary", "logy", "ness", "ment", "tion", "sion", "tive", "ance", "ence", "ible", "able", "ation", "al", "ic", "cy", "th",
    "re", "un", "pre", "dis", "mis", "de", "ex", "ing", "ings", "ers", "eds", "ly", "ie", "ee", "oo", "uh", "um", "ah", "hm", "sh", "ch", "ck",
    "mm", "em", "ma", "ya", "da", "pa", "jo", "la", "aw", "ow", "ha", "erm", "mr", "mrs", "dr", "lt", "lf", "ls", "ll", "ve", "gee", "ter", "und",
];

/// Prefix and suffix texts that are also whole words of ordinary use: never a fragment.
pub const REAL_WORDS: &[&str] = &[
    "age", "less", "some", "let", "over", "under", "cross", "extra", "post", "ship", "hood", "ward", "able", "super", "mini", "auto", "counter",
    "or", "ever", "out", "up", "self", "off", "full", "free", "fore", "mid", "half", "back", "down", "high", "low", "end", "way", "wise", "like",
];

const SUFFIXES: &str = include_str!("../../rules/suffixes.toml");
const MORPHEMES: &str = include_str!("../../rules/morphemes.toml");

/// The texts of lyk's suffix and prefix entries (lowercase, no braces or carets), and the extras.
#[must_use]
pub fn fragment_texts() -> HashSet<String> {
    let mut out: HashSet<String> = EXTRA_FRAGMENTS.iter().map(|s| (*s).to_owned()).collect();
    if let Ok(table) = toml::from_str::<BTreeMap<String, String>>(SUFFIXES) {
        out.extend(table.into_values().map(|v| v.trim_matches(|c| c == '{' || c == '}' || c == '^').to_lowercase()));
    }
    if let Ok(table) = toml::from_str::<BTreeMap<String, BTreeMap<String, toml::Value>>>(MORPHEMES) {
        for section in ["prefix", "suffix"] {
            out.extend(table.get(section).into_iter().flat_map(|t| t.keys().cloned()));
        }
    }
    out.extend(crate::family::BACKUP_ENDINGS.iter().map(|s| (*s).to_owned()));
    out.extend(crate::family::PREFIXES.iter().chain(crate::family::BACKUP_PREFIXES).map(|(p, _)| (*p).to_owned()));
    out
}

/// Whether the spelling is a practice word's shape: plain letters, a contraction from the list when
/// there is an apostrophe, two letters or more except `a` and `I`.
#[must_use]
pub fn shape_ok(word: &str, names: bool) -> bool {
    let lower = word.to_lowercase();
    // `won` is a contraction piece for the feed (`won't`) and also the past of `win`
    if !is_plain_word(word, names) && lower != "won" {
        return false;
    }
    if lower.contains('\'') {
        return CONTRACTIONS.contains(&lower.as_str());
    }
    lower.len() >= 2 || lower == "a" || lower == "i"
}

/// The data behind the predicate. Build it once with [`WordFilter::load`] and ask it about words.
#[derive(Clone, Debug, Default)]
pub struct WordFilter {
    /// Monkeytype's english, 1k, 5k and 10k words
    core: HashSet<String>,
    /// Monkeytype's 25k words
    wide: HashSet<String>,
    /// Monkeytype's 450k dictionary (empty when not downloaded)
    dictionary: HashSet<String>,
    /// Rank in lyk's frequency list (plain words, names left out), up to `LYK_RANK_LIMIT`
    rank: HashMap<String, usize>,
    names: HashSet<String>,
    fragments: HashSet<String>,
    /// `--deep`: the frequency limits are lifted
    pub deep: bool,
    /// `--names`: capitalised names are allowed
    pub allow_names: bool,
}

impl WordFilter {
    /// Reads what the predicate needs from the data folder; missing files only mean less is known.
    #[must_use]
    pub fn load(data: &Path, deep: bool, allow_names: bool) -> Self {
        let names = if allow_names { HashSet::new() } else { sources::names_in(data) };
        let read = |name: &str| -> HashSet<String> {
            std::fs::read_to_string(list_path(data, name))
                .ok()
                .and_then(|t| parse_monkeytype(&t).ok())
                // only the entries written lowercase: the lists keep names capitalised (`Sam`)
                .map_or_else(HashSet::new, |w| w.into_iter().filter(|w| !w.chars().any(char::is_uppercase) || w == "I").map(|w| w.to_lowercase()).collect())
        };
        let mut core = HashSet::new();
        for name in CORE_LISTS {
            core.extend(read(name));
        }
        let rank = sources::common(&data.join("frequency/en_full.txt"), LYK_RANK_LIMIT, allow_names, &names)
            .map_or_else(|_| HashMap::new(), |ws| ws.into_iter().enumerate().map(|(i, w)| (w.to_lowercase(), i + 1)).collect());
        Self { core, wide: read(WIDE_LIST), dictionary: read(DICTIONARY), rank, names, fragments: fragment_texts(), deep, allow_names }
    }

    #[cfg(test)]
    /// A filter from given sets, for tests: `core` words, `wide` (25k) words, `dictionary` words,
    /// lyk's ranked words in order and the names.
    #[must_use]
    pub fn from_parts(core: &[&str], wide: &[&str], dictionary: &[&str], ranked: &[&str], names: &[&str], deep: bool) -> Self {
        let set = |w: &[&str]| w.iter().map(|w| (*w).to_owned()).collect();
        Self {
            core: set(core),
            wide: set(wide),
            dictionary: set(dictionary),
            rank: ranked.iter().enumerate().map(|(i, w)| ((*w).to_owned(), i + 1)).collect(),
            names: set(names),
            fragments: fragment_texts(),
            deep,
            allow_names: false,
        }
    }

    /// Whether the word has the shape of a practice word but is a morpheme fragment.
    #[must_use]
    pub fn is_fragment(&self, word: &str) -> bool {
        let w = word.to_lowercase();
        self.fragments.contains(&w) && !self.core.contains(&w) && !REAL_WORDS.contains(&w.as_str())
    }

    /// Whether `word` is a practice word (see the module note).
    #[must_use]
    pub fn accepts(&self, word: &str) -> bool {
        if !shape_ok(word, self.allow_names) {
            return false;
        }
        let w = word.to_lowercase();
        if self.is_fragment(&w) || crate::prose::noise_letters(&w) || (!self.allow_names && self.names.contains(&w)) {
            return false;
        }
        if matches!(w.as_str(), "a" | "i") || w.contains('\'') || self.core.contains(&w) || self.wide.contains(&w) {
            return true;
        }
        if SPOKEN_WORDS.contains(&w.as_str()) {
            return true;
        }
        let have_dictionary = !self.dictionary.is_empty();
        if self.deep {
            return !have_dictionary || self.dictionary.contains(&w);
        }
        let Some(&rank) = self.rank.get(&w) else { return false };
        !have_dictionary || (self.dictionary.contains(&w) && (w.chars().count() >= MIN_LETTERS_BY_RANK || (w.chars().count() == 3 && rank <= SHORT_RANK_LIMIT)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter(deep: bool) -> WordFilter {
        // ranks: `the`, then fillers up to `SHORT_RANK_LIMIT`, then the rest: those are past the short-word limit
        let filler: Vec<String> = (0..SHORT_RANK_LIMIT).map(|i| format!("filler{i}")).collect();
        let mut ranked: Vec<&str> = vec!["the"];
        ranked.extend(filler.iter().map(String::as_str));
        ranked.extend(["ordinary", "ben", "tash2", "ode", "ash", "ish", "hmm", "er", "ed", "ing", "abb", "ack"]);
        WordFilter::from_parts(
            &["the", "age", "less", "over", "or", "don't", "it's", "i'm", "a", "see"],
            &["quokka"],
            &["tash", "eer", "er", "ed", "ing", "ly", "le", "en", "st", "kon", "ent", "abb", "ack", "ordinary", "rare", "ben", "ish", "hmm", "zebra", "ash", "ode"],
            &ranked,
            &["ben"],
            deep,
        )
    }

    #[test]
    #[ignore = "diagnostic: LYK_DATA=dir with the fetched data, run with --ignored --nocapture"]
    fn dump_the_tiers() {
        let data = std::env::var_os("LYK_DATA").map(std::path::PathBuf::from).expect("LYK_DATA");
        let f = WordFilter::load(&data, false, false);
        let mt: HashSet<&String> = f.core.iter().chain(f.wide.iter()).collect();
        let by_rank: Vec<&String> = f.rank.iter().filter(|(w, _)| !mt.contains(w) && f.accepts(w)).map(|(w, _)| w).collect();
        eprintln!("core {} wide {} dictionary {} rank-map {}", f.core.len(), f.wide.len(), f.dictionary.len(), f.rank.len());
        eprintln!("accepted only by rank: {}", by_rank.len());
        let mut short: Vec<&&String> = by_rank.iter().filter(|w| w.len() <= 4).collect();
        short.sort();
        eprintln!("rank route, up to 4 letters: {} {:?}", short.len(), short);
        let deep = WordFilter::load(&data, true, false);
        let n = deep.dictionary.iter().filter(|w| deep.accepts(w)).count();
        eprintln!("deep: dictionary words accepted {n}");
    }

    #[test]
    #[ignore = "diagnostic: LYK_DATA and LYK_DICT (the built dictionary folder); lists one-stroke outlines held by non-practice words"]
    fn dump_one_stroke_outlines_held_by_non_practice_words() {
        let data = std::env::var_os("LYK_DATA").map(std::path::PathBuf::from).expect("LYK_DATA");
        let dict = std::env::var_os("LYK_DICT").map(std::path::PathBuf::from).expect("LYK_DICT");
        let f = WordFilter::load(&data, false, false);
        let read = |file: &str| -> BTreeMap<String, String> {
            serde_json::from_str(&std::fs::read_to_string(dict.join(file)).unwrap()).unwrap()
        };
        let words = read("02-lyk-words.json");
        let mut one: HashMap<String, Vec<String>> = HashMap::new();
        for file in ["02-lyk-words.json", "02-lyk-short.json", "02-lyk-alternates.json", "02-lyk-extras.json"] {
            for (o, w) in read(file) {
                if !o.contains('/') {
                    one.entry(w.to_lowercase()).or_default().push(o);
                }
            }
        }
        let freq: HashMap<String, usize> = f.rank.clone();
        let held: Vec<(&String, &String)> = words.iter().filter(|(o, w)| !o.contains('/') && !f.accepts(w)).collect();
        let starred = held.iter().filter(|(o, _)| o.contains('*')).count();
        let mut core: Vec<&String> = f.core.iter().filter(|w| f.accepts(w) && !one.contains_key(*w)).collect();
        core.sort_by_key(|w| freq.get(*w).copied().unwrap_or(usize::MAX));
        eprintln!("one-stroke entries in 02-lyk-words.json: {}", words.keys().filter(|o| !o.contains('/')).count());
        eprintln!("held by words that fail the predicate: {} ({starred} with a star)", held.len());
        eprintln!("practice words of Monkeytype's english..10k with no one-stroke outline: {}", core.len());
        let mut sample: Vec<String> = held.iter().map(|(o, w)| format!("{o}={w}")).collect();
        sample.sort_by_key(|e| e.split('=').nth(1).and_then(|w| freq.get(w)).copied().unwrap_or(usize::MAX));
        eprintln!("held, first 60 by rank: {:?}", &sample[..sample.len().min(60)]);
        eprintln!("held, every 40th: {:?}", sample.iter().step_by(40).take(25).collect::<Vec<_>>());
        eprintln!("practice words without one-stroke, first 40 by rank: {:?}", &core[..core.len().min(40)]);
        eprintln!("every 25th: {:?}", core.iter().step_by(25).take(25).collect::<Vec<_>>());
    }

    #[test]
    fn real_words_pass_and_the_junk_of_a_real_vocab_fails() {
        let f = filter(false);
        assert!(f.accepts("it's") && f.accepts("don't"), "contractions are not in any list");
        for ok in ["the", "a", "I", "i", "ok", "shit", "age", "less", "over", "or", "don't", "it's", "I'm", "quokka", "ordinary", "see"] {
            assert!(f.accepts(ok), "{ok} should qualify");
        }
        for bad in [
            "tash", "eer", "er", "ed", "ing", "ly", "le", "en", "st", "ta", "kon", "ent", "ab", "abb", "ack", "ahn", "ben", "hmm", "ish", "zzz", "ugh",
            "", "x", "co2", "NASA", "Matt", "can'tt", "o'", "'s", "john's", "ode", "ash",
        ] {
            assert!(!f.accepts(bad), "{bad} should not qualify");
        }
    }

    #[test]
    fn a_rank_alone_is_not_enough_and_the_dictionary_and_four_letters_are() {
        let f = filter(false);
        assert!(f.accepts("ordinary"), "ranked, in the dictionary, long enough");
        assert!(!f.accepts("tash"), "in the dictionary but past the rank limit");
        assert!(!f.accepts("ash") && !f.accepts("ode"), "three letters past rank 5,000 need a Monkeytype list");
        let near = WordFilter::from_parts(&[], &[], &["owe", "abb"], &["owe"], &[], false);
        assert!(near.accepts("owe") && !near.accepts("abb"), "a three-letter word needs the rank within 5,000");
        assert!(!f.accepts("rare"), "in the dictionary, no rank");
        assert!(WordFilter::from_parts(&[], &[], &[], &["common"], &[], false).accepts("common"), "no dictionary downloaded: the rank decides");
    }

    #[test]
    fn deep_lifts_the_frequency_limits_and_keeps_fragments_names_and_noise_out() {
        let f = filter(true);
        assert!(f.accepts("rare") && f.accepts("zebra") && f.accepts("ash"), "any dictionary word");
        assert!(f.accepts("tash"), "deep reaches the dictionary tail");
        for bad in ["er", "ed", "ing", "ly", "le", "en", "st", "kon", "ent", "eer", "ish", "hmm", "ben"] {
            assert!(!f.accepts(bad), "{bad} is still out with --deep");
        }
        assert!(!f.accepts("ahn"), "not in the dictionary");
    }

    #[test]
    fn contractions_are_a_fixed_list() {
        for c in CONTRACTIONS {
            assert!(shape_ok(c, false), "{c}");
        }
        for bad in ["abc'd", "dogs'", "tash's", "it'ss", "'tis"] {
            assert!(!shape_ok(bad, false), "{bad}");
        }
        assert!(shape_ok("I'm", false) && shape_ok("I", false) && shape_ok("a", false) && !shape_ok("b", false));
        assert!(shape_ok("won", false) && !shape_ok("don", false), "won is a word; don is a piece of don't");
    }

    #[test]
    fn every_suffix_and_prefix_text_is_a_fragment_unless_it_is_a_real_word() {
        let f = filter(false);
        let texts = fragment_texts();
        for t in ["ing", "ed", "ly", "er", "en", "ent", "ness", "ment", "tion", "ism", "ist", "anti", "inter", "trans", "dis", "mis", "non", "ish", "ize"] {
            assert!(texts.contains(t), "{t} is a fragment text");
            assert!(f.is_fragment(t), "{t}");
        }
        for t in ["age", "less", "over", "under", "or", "some", "let"] {
            assert!(!f.is_fragment(t), "{t} is a real word too");
        }
    }
}

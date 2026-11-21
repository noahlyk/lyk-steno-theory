//! The word feed: which words, in which order. Pure and deterministic for a seed.
//!
//! A source gives *units* in bands: a unit is a word, or a phrase or sentence of several words
//! that stay together in the feed. Bands are the rank ranges of `common`, or one band for the
//! other sources. Without `weighted` the bands are one pool; with it every draw picks a band
//! first (each band equally likely, so a narrow band of common words is met as often as a wide
//! one of rare words) and then a unit in it.

use anyhow::{Result, bail};

/// A small seedable generator (splitmix64): the feed must repeat for a seed, so no `rand`.
pub struct Rng(u64);

impl Rng {
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub const fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number below `n` (`n` > 0).
    pub const fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            items.swap(i, self.below(i + 1));
        }
    }
}

#[derive(Clone, Debug)]
pub struct Opts {
    pub count: usize,
    pub seed: u64,
    pub no_repeat: bool,
    pub lowercase_only: bool,
    pub min_len: usize,
    pub max_len: usize,
    pub weighted: bool,
}

/// Contraction pieces the subtitle counts keep once the apostrophe is stripped (`don't` is
/// `don`, `t`), which are not words anyone types.
const FRAGMENTS: &[&str] = &[
    "ll", "ve", "re", "don", "didn", "isn", "wasn", "couldn", "wouldn", "shouldn", "doesn", "aren", "weren", "hasn",
    "haven", "hadn", "ain", "mustn", "needn", "won",
];

/// A plain word: letters with at most inner apostrophes, lowercase (`I` and `I'm` too);
/// a capitalised word only with `names`. Not a lone letter other than `a` and `I`.
#[must_use]
pub fn is_plain_word(word: &str, names: bool) -> bool {
    let mut chars = word.chars();
    let Some(first) = chars.next() else { return false };
    if !word.chars().all(|c| c.is_ascii_alphabetic() || c == '\'') || word.starts_with('\'') || word.ends_with('\'') {
        return false;
    }
    if word.len() == 1 && !matches!(word, "a" | "I" | "i") {
        return false;
    }
    if FRAGMENTS.contains(&word) {
        return false;
    }
    let rest_lower = !word.chars().skip(1).any(|c| c.is_ascii_uppercase());
    if first.is_ascii_lowercase() {
        return rest_lower;
    }
    // Capitals: the pronoun I and its contractions always; any other capitalised word is a name.
    let pronoun = word == "I" || word.starts_with("I'");
    rest_lower && (pronoun || names)
}

/// `1-200,201-1000` as inclusive 1-based rank ranges.
pub fn parse_bands(text: &str) -> Result<Vec<(usize, usize)>> {
    let mut bands = Vec::new();
    for part in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let Some((lo, hi)) = part.split_once('-') else { bail!("band {part:?}: write it as MIN-MAX, such as 1-200") };
        let (Ok(lo), Ok(hi)) = (lo.trim().parse::<usize>(), hi.trim().parse::<usize>()) else {
            bail!("band {part:?}: MIN and MAX must be whole numbers")
        };
        if lo == 0 || hi < lo {
            bail!("band {part:?}: ranks start at 1 and MAX must not be below MIN");
        }
        bands.push((lo, hi));
    }
    if bands.is_empty() {
        bail!("no bands given");
    }
    Ok(bands)
}

/// How many ranked words the bands reach.
#[must_use]
pub fn reach(bands: &[(usize, usize)]) -> usize {
    bands.iter().map(|b| b.1).max().unwrap_or(0)
}

/// The units of each band, out of `ranked` (the commonest word first).
#[must_use]
pub fn band_units(ranked: &[String], bands: &[(usize, usize)]) -> Vec<Vec<String>> {
    bands
        .iter()
        .map(|&(lo, hi)| ranked.iter().skip(lo - 1).take(hi + 1 - lo).cloned().collect::<Vec<_>>())
        .filter(|b| !b.is_empty())
        .collect()
}

fn words_ok(unit: &str, o: &Opts) -> bool {
    unit.split_whitespace().all(|w| {
        let n = w.chars().count();
        n >= o.min_len && n <= o.max_len && !(o.lowercase_only && w.chars().any(char::is_uppercase))
    })
}

/// The feed: units drawn until `count` words, cut to exactly `count`. With `no_repeat` a unit is
/// used once, so a pool smaller than `count` gives a shorter feed; otherwise the same unit never
/// comes twice in a row (unless it is the only one).
#[must_use]
pub fn build(bands: Vec<Vec<String>>, o: &Opts) -> Vec<String> {
    let mut rng = Rng::new(o.seed);
    let mut seen = std::collections::HashSet::new();
    let mut bands: Vec<Vec<String>> = bands
        .into_iter()
        .map(|b| b.into_iter().filter(|u| words_ok(u, o) && (!o.no_repeat || seen.insert(u.clone()))).collect::<Vec<_>>())
        .filter(|b| !b.is_empty())
        .collect();
    if !o.weighted {
        bands = vec![bands.into_iter().flatten().collect()];
    }
    if bands.iter().all(Vec::is_empty) {
        return Vec::new();
    }
    for b in &mut bands {
        rng.shuffle(b);
    }
    let mut next = vec![0_usize; bands.len()];
    let mut out: Vec<String> = Vec::new();
    let mut words = 0;
    let mut last: Option<&String> = None;
    while words < o.count {
        let live: Vec<usize> = (0..bands.len()).filter(|&i| !o.no_repeat || next[i] < bands[i].len()).collect();
        if live.is_empty() {
            break;
        }
        let b = live[rng.below(live.len())];
        let unit = if o.no_repeat {
            next[b] += 1;
            &bands[b][next[b] - 1]
        } else {
            let mut pick = rng.below(bands[b].len());
            if bands[b].len() > 1 && last == Some(&bands[b][pick]) {
                pick = (pick + 1) % bands[b].len();
            }
            &bands[b][pick]
        };
        last = Some(unit);
        words += unit.split_whitespace().count();
        out.extend(unit.split_whitespace().map(str::to_owned));
    }
    out.truncate(o.count);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> Opts {
        Opts { count: 20, seed: 7, no_repeat: false, lowercase_only: false, min_len: 1, max_len: 30, weighted: false }
    }

    fn units(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| (*w).to_owned()).collect()
    }

    #[test]
    fn the_same_seed_gives_the_same_feed_and_another_a_different_one() {
        let pool: Vec<String> = (0..200).map(|i| format!("w{i}")).collect();
        let a = build(vec![pool.clone()], &opts());
        let b = build(vec![pool.clone()], &opts());
        let c = build(vec![pool], &Opts { seed: 8, ..opts() });
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 20);
    }

    #[test]
    fn no_repeat_uses_each_unit_once_and_runs_out() {
        let pool = units(&["a1", "b1", "a1", "c1"]);
        let feed = build(vec![pool], &Opts { no_repeat: true, ..opts() });
        let mut sorted = feed.clone();
        sorted.sort();
        assert_eq!(sorted, units(&["a1", "b1", "c1"]), "feed {feed:?}");
    }

    #[test]
    fn a_word_never_repeats_straight_away() {
        let feed = build(vec![units(&["x1", "y1"])], &Opts { count: 200, ..opts() });
        assert!(feed.windows(2).all(|w| w[0] != w[1]), "{feed:?}");
    }

    #[test]
    fn length_and_case_filters() {
        let pool = units(&["a", "bb", "ccc", "Dddd", "eeeee"]);
        let o = Opts { min_len: 2, max_len: 4, lowercase_only: true, no_repeat: true, ..opts() };
        let mut feed = build(vec![pool], &o);
        feed.sort();
        assert_eq!(feed, units(&["bb", "ccc"]));
    }

    #[test]
    fn phrases_stay_together_and_the_feed_is_cut_to_count() {
        let feed = build(vec![units(&["it does not", "he is"])], &Opts { count: 4, no_repeat: true, ..opts() });
        assert_eq!(feed.len(), 4, "{feed:?}");
        let text = feed.join(" ");
        assert!(text.contains("it does not") || text.contains("he is"), "{text}");
    }

    #[test]
    fn weighted_draws_a_band_first() {
        let rare: Vec<String> = (0..1000).map(|i| format!("r{i}")).collect();
        let common = units(&["c1", "c2"]);
        let o = Opts { count: 600, weighted: true, ..opts() };
        let feed = build(vec![common, rare], &o);
        let c = feed.iter().filter(|w| w.starts_with('c')).count();
        assert!((200..=400).contains(&c), "common words {c} of {}", feed.len());
        let flat = build(vec![units(&["c1", "c2"]), (0..1000).map(|i| format!("r{i}")).collect()], &Opts { weighted: false, ..o });
        assert!(flat.iter().filter(|w| w.starts_with('c')).count() < 20, "unweighted should rarely draw the two common words");
    }

    #[test]
    fn plain_words() {
        for ok in ["the", "don't", "I", "I'm", "a"] {
            assert!(is_plain_word(ok, false), "{ok}");
        }
        for bad in ["", "x", "t", "ll", "don", "Matt", "hello.", "a-b", "'s", "co2", "NASA", "its'"] {
            assert!(!is_plain_word(bad, false), "{bad}");
        }
        assert!(is_plain_word("Matt", true));
        assert!(!is_plain_word("NASA", true));
    }

    #[test]
    fn bands_parse_and_reject() {
        assert_eq!(parse_bands("1-200, 201-1000").unwrap(), vec![(1, 200), (201, 1000)]);
        for bad in ["", "5", "0-5", "9-3", "a-b"] {
            assert!(parse_bands(bad).is_err(), "{bad}");
        }
        assert_eq!(reach(&[(1, 5), (6, 50)]), 50);
    }

    #[test]
    fn band_units_slice_the_ranking() {
        let ranked: Vec<String> = (1..=10).map(|i| format!("w{i}")).collect();
        let bands = band_units(&ranked, &[(1, 2), (9, 20)]);
        assert_eq!(bands, vec![units(&["w1", "w2"]), units(&["w9", "w10"])]);
    }
}

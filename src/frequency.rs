//! How common each word is: the subtitle counts (`data/frequency/en_full.txt`), with each name's
//! count cut to a tenth.
//!
//! Subtitles are scripted dialogue, and dialogue addresses people by name constantly: 435 of the
//! 5,000 commonest subtitle words are names (`matt` outranked `mat`, `mike` outranked `mic`). lyk
//! writes a name as its ordinary outline after the capitalize stroke (`KPA`), so a name and the
//! word it sounds like compete for one stroke, and the word should win it. A name here is a word
//! Plover and Lapwing only ever write capitalized ([`crate::gold::Gold::is_name`]).
//!
//! Blending in written English (the Google web bigrams) was measured and rejected: it moved 36 of
//! the 1,000 commonest words' outlines on photo-finishes between two very common words
//! (`thank`/`than`, `love`/`off`), and demoted the conversational words people do type
//! (`okay`, `honey`, `nah`, said 100-500 times as often as written on the web). See
//! `docs/decisions.md`.

use std::collections::HashMap;

/// A name's count is divided by this: `matt` is said 7.6 times as often as `mat`, and a tenth
/// puts the name behind the word while a common name still outranks a rare word.
pub const NAME_SHARE: u64 = 10;

/// Each word's count and rank.
pub struct Counts {
    /// How often each word is said, names cut to a tenth
    pub count: HashMap<String, u64>,
    /// 0 is the most common word, by `count`; ties keep the list's order
    pub rank: HashMap<String, usize>,
}

/// Counts and ranks `spoken` (most common first, as the subtitle file lists them; a repeated
/// word keeps its first count), dividing each name's count by [`NAME_SHARE`].
pub fn count(spoken: Vec<(String, u64)>, is_name: impl Fn(&str) -> bool) -> Counts {
    let mut seen = std::collections::HashSet::with_capacity(spoken.len());
    let mut order: Vec<(String, u64)> = spoken
        .into_iter()
        .filter(|(word, _)| seen.insert(word.clone()))
        .map(|(word, said)| {
            let said = if is_name(&word) { said.div_ceil(NAME_SHARE) } else { said };
            (word, said)
        })
        .collect();
    // A stable sort: equal counts keep the list's order.
    order.sort_by_key(|&(_, said)| std::cmp::Reverse(said));
    let mut counts = Counts { count: HashMap::with_capacity(order.len()), rank: HashMap::with_capacity(order.len()) };
    for (n, (word, said)) in order.into_iter().enumerate() {
        counts.rank.insert(word.clone(), n);
        counts.count.insert(word, said);
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spoken(words: &[(&str, u64)]) -> Vec<(String, u64)> {
        words.iter().map(|&(w, c)| (w.to_owned(), c)).collect()
    }

    #[test]
    fn a_name_yields_to_the_word_it_sounds_like() {
        let words = spoken(&[("matt", 300), ("mat", 100)]);
        let plain = count(words.clone(), |_| false);
        assert!(plain.rank["matt"] < plain.rank["mat"]);
        let damped = count(words, |w| w == "matt");
        assert_eq!(damped.count["matt"], 30);
        assert!(damped.rank["mat"] < damped.rank["matt"]);
    }

    #[test]
    fn ties_keep_the_list_order_and_a_repeated_word_its_first_count() {
        let counts = count(spoken(&[("a", 100), ("b", 100), ("a", 1)]), |_| false);
        assert_eq!(counts.rank["a"], 0);
        assert_eq!(counts.rank["b"], 1);
        assert_eq!(counts.count["a"], 100);
    }

    #[test]
    fn a_name_said_once_is_still_said() {
        assert_eq!(count(spoken(&[("zed", 1)]), |_| true).count["zed"], 1);
    }
}

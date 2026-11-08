//! Judging the rules against Plover and Lapwing, and learning which rules are missing: the data
//! every command loads ([`Setup`]), the score and README tables ([`score`]), and the per-word
//! views ([`explain`]).

mod explain;
mod score;

pub use explain::{explain, learn, mismatches};
pub use score::{check_readme, score};


use crate::data::{
    Pronunciations, fallback_pronunciations, load_bigrams, load_cmudict, load_frequency_bounded,
    load_mfa, load_wikipron,
};
use crate::gold::Gold;
use crate::rules::Rules;
use crate::stroke::render_outline;
use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

/// The frequency corpus (`data/frequency/en_full.txt`) strips every apostrophe, so a contraction
/// is counted there under its bare, misspelled form (`dont`) while `CMUdict`, the only source of
/// pronunciation, keeps the real spelling (`don't`). [`Setup::load`]'s word list is built from the
/// *intersection* of a rank and a pronunciation - `dont` has no pronunciation, `don't` has no
/// rank - so every contraction below was silently invisible to the whole generator, no matter how
/// common it actually is, until this table backfills `don't`'s rank from `dont`'s. Hand-curated,
/// not derived by stripping every apostrophe `CMUdict` has, because most of those are unrelated
/// possessive-noun pronunciation entries (`thing's`) that would otherwise wrongly borrow their
/// plural twin's rank (`things`) - this list is closed-class English contractions only, verified
/// against both files. A few (`he'll`/`hell`, `we'll`/`well`, `who're`/`whore`) share their
/// stripped form with a real, unrelated word already at that rank; the contraction borrows an
/// approximate rank from it rather than its own true frequency, an accepted, minor imprecision far
/// better than being entirely absent from the dictionary. See `docs/decisions.md`.
const CONTRACTIONS: &[(&str, &str)] = &[
    ("aint", "ain't"),
    ("arent", "aren't"),
    ("cant", "can't"),
    ("couldnt", "couldn't"),
    ("didnt", "didn't"),
    ("doesnt", "doesn't"),
    ("dont", "don't"),
    ("hadnt", "hadn't"),
    ("hasnt", "hasn't"),
    ("havent", "haven't"),
    ("hed", "he'd"),
    ("hell", "he'll"),
    ("hes", "he's"),
    ("heres", "here's"),
    ("howd", "how'd"),
    ("howre", "how're"),
    ("hows", "how's"),
    ("id", "i'd"),
    ("ill", "i'll"),
    ("im", "i'm"),
    ("ive", "i've"),
    ("isnt", "isn't"),
    ("itd", "it'd"),
    ("itll", "it'll"),
    ("its", "it's"),
    ("lets", "let's"),
    ("maam", "ma'am"),
    ("mustnt", "mustn't"),
    ("neednt", "needn't"),
    ("oclock", "o'clock"),
    ("shed", "she'd"),
    ("shell", "she'll"),
    ("shes", "she's"),
    ("shouldnt", "shouldn't"),
    ("thatd", "that'd"),
    ("thatll", "that'll"),
    ("thats", "that's"),
    ("thered", "there'd"),
    ("therell", "there'll"),
    ("therere", "there're"),
    ("theres", "there's"),
    ("thereve", "there've"),
    ("theyd", "they'd"),
    ("theyll", "they'll"),
    ("theyre", "they're"),
    ("theyve", "they've"),
    ("wasnt", "wasn't"),
    ("wed", "we'd"),
    ("well", "we'll"),
    ("were", "we're"),
    ("weve", "we've"),
    ("werent", "weren't"),
    ("whatd", "what'd"),
    ("whatll", "what'll"),
    ("whatre", "what're"),
    ("whats", "what's"),
    ("whatve", "what've"),
    ("whod", "who'd"),
    ("wholl", "who'll"),
    ("whore", "who're"),
    ("whos", "who's"),
    ("whove", "who've"),
    ("wont", "won't"),
    ("wouldnt", "wouldn't"),
    ("yall", "y'all"),
    ("youd", "you'd"),
    ("youll", "you'll"),
    ("youre", "you're"),
    ("youve", "you've"),
];

pub struct Setup {
    pub rules: Rules,
    pub pron: Pronunciations,
    pub gold: Gold,
    /// 0 is the most common word, by `count`
    pub rank: HashMap<String, usize>,
    /// How often each word is said, a name's count cut to a tenth (see [`crate::frequency`])
    pub count: HashMap<String, u64>,
    /// How often one word is immediately followed by another (Norvig's Google Books Ngrams,
    /// `data/frequency/bigrams_en.txt`), kept only where both words are in `rank` - this
    /// generator's own vocabulary, not the raw corpus's (see `data::load_bigrams` for what that
    /// filters out). Consumed by `generate`'s phrase-briefing pass (`enumerate_phrase_candidates`)
    /// to rank which bigrams are worth attempting to merge into a single stroke - see
    /// `site/reference/phrases.json`.
    pub bigrams: HashMap<(String, String), u64>,
    /// The frequency list's rows past those ranked here, down to [`SPELLINGS`], with their line:
    /// real spellings, unranked and unpronounced, that the solver lets pieces spell (*lookup*)
    pub spellings: Vec<(String, usize)>,
}

/// How far down the frequency list real spellings are kept for the solver (see `Setup::spellings`).
pub const SPELLINGS: usize = 250_000;

impl Setup {
    /// `common` bounds how much of the frequency list is worth ranking at all: `None` for every
    /// command that needs a real rank for any word regardless of commonness (`explain`,
    /// `learn`, `mismatches`), `Some(n)` for one that already only looks at the `n` most common
    /// words anyway (`build`, `score`, `quote`, `resolve`), padded past `n` so nothing a caller
    /// still reads near that edge comes up unranked that would not have been before.
    pub fn load(data: &Path, rules: Option<&Path>, common: Option<usize>) -> Result<Self> {
        let rules = match rules {
            Some(path) => Rules::load(path)?,
            None => Rules::built_in()?,
        };
        let freq_limit =
            common.map_or(usize::MAX, |c| c.saturating_add(c / 5).max(c.saturating_add(10_000)));
        let freq_path = data.join("frequency/en_full.txt");
        let pron_path = data.join("pronunciation/cmudict.dict");
        let mfa_path = data.join("pronunciation/english_us_mfa.dict");
        let wikipron_path = data.join("pronunciation/wikipron_us_broad.tsv");
        let bigrams_path = data.join("frequency/bigrams_en.txt");
        // Six independent files, none needed before another: load them on their own threads
        // instead of one after another.
        let (frequency, pron, mfa, wikipron, gold, bigrams) = std::thread::scope(|scope| {
            let frequency = scope.spawn(|| load_frequency_bounded(&freq_path, freq_limit.max(SPELLINGS)));
            let pron = scope.spawn(|| load_cmudict(&pron_path));
            let mfa = scope.spawn(|| load_mfa(&mfa_path));
            let wikipron = scope.spawn(|| load_wikipron(&wikipron_path));
            let gold = scope.spawn(|| Gold::load(data));
            let bigrams = scope.spawn(|| load_bigrams(&bigrams_path));
            (
                frequency.join().expect("frequency load panicked"),
                pron.join().expect("pronunciation load panicked"),
                mfa.join().expect("mfa pronunciation load panicked"),
                wikipron.join().expect("wikipron pronunciation load panicked"),
                gold.join().expect("gold dictionary load panicked"),
                bigrams.join().expect("bigram load panicked"),
            )
        });
        let mut pron = pron?;
        // Every word CMUdict has no entry for at all gets a translated ARPAbet pronunciation from
        // MFA or WikiPron instead, if either has one that translates - a genuine fallback, never a
        // replacement: `or_insert` only ever fires for a key `pron` does not already have, so a
        // CMUdict-covered word is completely unaffected. See `data::fallback_pronunciations` and
        // `crate::ipa`.
        for (word, variants) in fallback_pronunciations(&pron, &mfa?, &wikipron?) {
            pron.entry(word).or_insert(variants);
        }
        let gold = gold?;
        let mut spoken = frequency?;
        let spellings: Vec<(String, usize)> =
            spoken.iter().enumerate().skip(freq_limit).map(|(line, (w, _))| (w.clone(), line)).collect();
        spoken.truncate(freq_limit);
        // See `CONTRACTIONS`: a word CMUdict has a real pronunciation for but the frequency
        // corpus only counted under its apostrophe-stripped spelling. It is listed right after it.
        let listed: std::collections::HashSet<String> = spoken.iter().map(|(w, _)| w.clone()).collect();
        for &(stripped, real) in CONTRACTIONS {
            if pron.contains_key(real)
                && !listed.contains(real)
                && let Some(at) = spoken.iter().position(|(w, _)| w == stripped)
            {
                spoken.insert(at + 1, (real.to_owned(), spoken[at].1));
            }
        }
        let crate::frequency::Counts { count, rank } = crate::frequency::count(spoken, |w| gold.is_name(w));
        // Vocabulary-filtered here, not in `load_bigrams`: "vocabulary" means this generator's
        // own ranked words, which only exist once `rank` above is built.
        let bigrams = bigrams?
            .into_iter()
            .filter(|(first, second, _)| rank.contains_key(first) && rank.contains_key(second))
            .map(|(first, second, times)| ((first, second), times))
            .collect();
        Ok(Self {
            rules,
            pron,
            gold,
            rank,
            count,
            bigrams,
            spellings,
        })
    }

    /// Every outline the rules give a word, one per pronunciation that can be written, and the
    /// form its spelling gives it when it clashes with a word that sounds the same.
    pub fn candidates(&self, word: &str) -> Vec<String> {
        let mut found: Vec<String> = Vec::new();
        for sounds in self.pron.get(word).into_iter().flatten() {
            for mut outline in [
                self.rules.outline(word, sounds),
                self.rules.variant(word, sounds),
            ]
            .into_iter()
            .flatten()
            {
                // The join key written out on every continuation stroke, as the dictionary files do
                self.rules.joined(&mut outline);
                let text = render_outline(&outline);
                if !found.contains(&text) {
                    found.push(text);
                }
            }
        }
        found
    }

    fn syllable_count(&self, word: &str) -> usize {
        self.pron
            .get(word)
            .and_then(|all| all.first())
            .map_or(0, |sounds| {
                sounds
                    .iter()
                    .filter(|s| s.ends_with(|c: char| c.is_ascii_digit()))
                    .count()
            })
    }

    /// Words either dictionary writes that have a pronunciation, most common first.
    fn words(&self) -> Vec<&str> {
        let mut words: Vec<&str> = self
            .gold
            .words()
            .into_iter()
            .filter(|w| self.pron.contains_key(*w))
            .collect();
        words.sort_by_key(|w| (self.rank.get(*w).copied().unwrap_or(usize::MAX), *w));
        words
    }
}

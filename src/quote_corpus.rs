//! A harsh, hardcoded-text regression benchmark: a hundred short passages nobody picked to
//! flatter lyk, scored exactly the way the live `quote` command scores one (`quote::rows_for`,
//! `quote::cost`), aggregated into per-word and per-passage win/loss counts against Plover and
//! Lapwing. Run this after any rule change - a drop in these numbers against the thresholds below
//! is a real regression in how terse the theory actually is, not noise, since the corpus itself
//! never changes.
//!
//! The passages are a mix of public-domain lines (old enough that attribution doesn't matter for
//! a test fixture) and plain, unremarkable English sentences written for this file - ordinary
//! prose nobody picked to make lyk look good, which is the entire point: a benchmark curated to
//! win would be measuring the curation, not the theory.

use crate::dictionary_fixture;
use crate::quote::{cost, rows_for};

const PASSAGES: &[&str] = &[
    "Four score and seven years ago our fathers brought forth on this continent a new nation.",
    "A house divided against itself cannot stand.",
    "Government of the people, by the people, for the people, shall not perish from the earth.",
    "To be or not to be, that is the question.",
    "All that glitters is not gold.",
    "Something is rotten in the state of Denmark.",
    "The lady doth protest too much, methinks.",
    "We hold these truths to be self evident, that all men are created equal.",
    "Give me liberty or give me death.",
    "I only regret that I have but one life to lose for my country.",
    "Early to bed and early to rise makes a man healthy, wealthy, and wise.",
    "A penny saved is a penny earned.",
    "Energy and persistence conquer all things.",
    "If you would not be forgotten as soon as you are dead, either write something worth reading or do something worth writing.",
    "I went to the woods because I wished to live deliberately.",
    "Our life is frittered away by detail, simplify, simplify.",
    "Not until we are lost do we begin to understand ourselves.",
    "Trust thyself, every heart vibrates to that iron string.",
    "A foolish consistency is the hobgoblin of little minds.",
    "Nothing great was ever achieved without enthusiasm.",
    "To be yourself in a world that is constantly trying to make you something else is a great accomplishment.",
    "It is not length of life, but depth of life that matters.",
    "The only true wisdom is in knowing you know nothing.",
    "An unexamined life is not worth living.",
    "Know thyself.",
    "He who has a why to live can bear almost any how.",
    "That which does not kill us makes us stronger.",
    "Without music, life would be a mistake.",
    "Patience is bitter, but its fruit is sweet.",
    "We are what we repeatedly do, excellence then is not an act but a habit.",
    "The whole is greater than the sum of its parts.",
    "It is during our darkest moments that we must focus to see the light.",
    "You have power over your mind, not outside events, realize this and you will find strength.",
    "Waste no more time arguing about what a good man should be, be one.",
    "The impediment to action advances action, what stands in the way becomes the way.",
    "It is not death that a man should fear, but he should fear never beginning to live.",
    "When I despair, I remember that all through history the way of truth and love has always won.",
    "Live as if you were to die tomorrow, learn as if you were to live forever.",
    "The weak can never forgive, forgiveness is the attribute of the strong.",
    "It does not matter how slowly you go as long as you do not stop.",
    "The journey of a thousand miles begins with a single step.",
    "Life is really simple, but we insist on making it complicated.",
    "Real knowledge is to know the extent of one's ignorance.",
    "Our greatest glory is not in never falling, but in rising every time we fall.",
    "The mind is everything, what you think you become.",
    "Three things cannot be long hidden, the sun, the moon, and the truth.",
    "Peace comes from within, do not seek it without.",
    "Happiness is not something ready made, it comes from your own actions.",
    "A room without books is like a body without a soul.",
    "Fall seven times and stand up eight.",
    "A smooth sea never made a skilled sailor.",
    "A ship in harbor is safe, but that is not what ships are built for.",
    "When one door of happiness closes, another opens.",
    "The best and most beautiful things in the world cannot be seen or touched.",
    "Alone we can do so little, together we can do so much.",
    "Life is either a daring adventure or nothing at all.",
    "If you tell the truth, you don't have to remember anything.",
    "The secret of getting ahead is getting started.",
    "Kindness is a language which the deaf can hear and the blind can see.",
    "Do what you can, with what you have, where you are.",
    "Nothing in life is to be feared, it is only to be understood.",
    "I am among those who think that science has great beauty.",
    "In three words I can sum up everything I have learned about life, it goes on.",
    "Two roads diverged in a wood, and I took the one less traveled by.",
    "The woods are lovely, dark and deep, but I have promises to keep.",
    "Do not go gentle into that good night.",
    "Hope is the thing with feathers that perches in the soul.",
    "It always seems impossible until it is done.",
    "Education is the most powerful weapon which you can use to change the world.",
    "The greatest glory in living lies not in never falling, but in rising every time we fall.",
    "Genius is one percent inspiration and ninety nine percent perspiration.",
    "I have not failed, I have just found ten thousand ways that will not work.",
    "The best way to predict the future is to create it.",
    "If you want to go fast, go alone, if you want to go far, go together.",
    "Not everything that is faced can be changed, but nothing can be changed until it is faced.",
    "Whether you think you can, or you think you can't, you're right.",
    "The future belongs to those who believe in the beauty of their dreams.",
    "Imagination is more important than knowledge.",
    "In the middle of difficulty lies opportunity.",
    "Float like a butterfly, sting like a bee.",
    "Not all those who wander are lost.",
    "There is some good in this world, and it's worth fighting for.",
    "Even the smallest person can change the course of the future.",
    "All we have to decide is what to do with the time that is given us.",
    "I don't know where we are going, but we can't be late.",
    "She wasn't sure if it would rain, so she didn't bring an umbrella.",
    "He couldn't find his keys, and he hadn't checked the kitchen table yet.",
    "They're planning to repaint the fence before it gets any colder outside.",
    "We've been waiting for almost an hour and the bus still hasn't arrived.",
    "I'm not certain what time the meeting starts, but it's probably after lunch.",
    "You shouldn't leave the window open when it's raining this hard.",
    "It's getting late, and we haven't finished packing for the trip yet.",
    "The children were laughing so loudly that the neighbors could hear them from next door.",
    "A quiet morning walk along the river can clear your mind before a busy day.",
    "The recipe calls for two cups of flour, a pinch of salt, and three eggs.",
    "After the storm passed, the whole garden smelled like wet grass and flowers.",
    "She spent the afternoon reorganizing her bookshelf by color instead of by author.",
    "The old clock in the hallway hasn't kept accurate time in nearly a decade.",
    "He finally finished the crossword puzzle after three cups of coffee and a long nap.",
    "The train was delayed again, so everyone on the platform just sat down and waited.",
    "Learning to type without looking at the keyboard takes a surprising amount of practice.",
    "A good night's sleep often matters more than any other single habit.",
    "The library closes early on weekends, which nobody seems to remember until they arrive.",
];

/// Running totals and win/loss counts for a passage corpus, next to Plover and Lapwing.
#[derive(Default)]
struct Stats {
    passages: u32,
    passage_wins_plover: u32,
    passage_losses_plover: u32,
    passage_wins_lapwing: u32,
    passage_losses_lapwing: u32,
    words: u32,
    word_fewer_keys_plover: u32,
    word_more_keys_plover: u32,
    word_fewer_keys_lapwing: u32,
    word_more_keys_lapwing: u32,
    word_fewer_strokes_plover: u32,
    word_fewer_strokes_lapwing: u32,
    /// Lyk's key total over *every* word it resolves a cost for, regardless of whether Plover or
    /// Lapwing have an entry at all - a "full coverage" figure, not comparable on its own to either
    /// theory's total below (see `total_keys_lyk_vs_plover`/`_vs_lapwing` for the numbers that are).
    total_keys_lyk: u64,
    /// Lyk's key total, but only summed over the words Plover also has an entry for - the same
    /// population `total_keys_plover` sums over, so the two are a fair, apples-to-apples
    /// comparison. (Summing lyk's cost over every word it resolves while summing Plover's only
    /// over words Plover has was a real bug here once: on a corpus dense with proper nouns neither
    /// gold dictionary carries, lyk's uncounterweighted fallback cost made it look worse in
    /// aggregate even when it won on every shared word - see `docs/decisions.md`.)
    total_keys_lyk_vs_plover: u64,
    total_keys_plover: u64,
    /// As `total_keys_lyk_vs_plover`, but scoped to the words Lapwing has an entry for instead.
    total_keys_lyk_vs_lapwing: u64,
    total_keys_lapwing: u64,
    /// Lyk's full-coverage *stroke* total, word by word (the same population as `total_keys_lyk`,
    /// just counting strokes instead of keys) - the baseline `total_strokes_lyk_with_phrases` is
    /// compared against, since strokes (not keys) are what phrase-briefing actually exists to
    /// reduce. See `quote::total_cost_with_phrases`'s own doc for why keys alone can be a
    /// misleading way to see this mechanism's real effect.
    total_strokes_lyk: u64,
    /// Lyk's full-coverage stroke total, walked phrase-aware (`quote::total_cost_with_phrases`): a
    /// phrase entry covering two consecutive words wins over scoring them separately, dropping two
    /// strokes to one wherever it fires. A genuinely separate, additive figure - not folded into
    /// `total_strokes_lyk`, `total_keys_lyk`, or either `_vs_plover`/`_vs_lapwing` total above, none
    /// of which this benchmark's own thresholds (calibrated word-by-word) should ever move because
    /// of phrase entries existing. See `docs/phrases.md`.
    total_strokes_lyk_with_phrases: u64,
    /// Lyk's full-coverage key total, walked the same phrase-aware way - reported alongside the
    /// stroke figures above, but see `quote::total_cost_with_phrases`'s own doc for why this number
    /// can legitimately come out roughly flat (or very slightly worse) even when the stroke total
    /// clearly improves: a firing phrase is built from both words' own *primaries*, while
    /// `total_keys_lyk` already credits each word with its own globally cheapest entry of any
    /// origin, which can occasionally be a cheaper non-primary backup.
    total_keys_lyk_with_phrases: u64,
    /// How many times a phrase entry actually fired across the whole corpus - zero is itself a
    /// real, honest result worth seeing directly, not just the key total it would imply.
    phrase_hits: u32,
}

impl Stats {
    fn pct(num: u32, den: u32) -> f64 {
        if den == 0 { 0.0 } else { f64::from(num) * 100.0 / f64::from(den) }
    }

    fn report(&self) {
        println!("passages: {}", self.passages);
        println!(
            "  vs plover:  wins {} ({:.1}%)  losses {}",
            self.passage_wins_plover,
            Self::pct(self.passage_wins_plover, self.passages),
            self.passage_losses_plover
        );
        println!(
            "  vs lapwing: wins {} ({:.1}%)  losses {}",
            self.passage_wins_lapwing,
            Self::pct(self.passage_wins_lapwing, self.passages),
            self.passage_losses_lapwing
        );
        println!("words: {}", self.words);
        println!(
            "  fewer keys    vs plover:  {} ({:.1}%)   more keys: {} ({:.1}%)",
            self.word_fewer_keys_plover,
            Self::pct(self.word_fewer_keys_plover, self.words),
            self.word_more_keys_plover,
            Self::pct(self.word_more_keys_plover, self.words)
        );
        println!(
            "  fewer keys    vs lapwing: {} ({:.1}%)   more keys: {} ({:.1}%)",
            self.word_fewer_keys_lapwing,
            Self::pct(self.word_fewer_keys_lapwing, self.words),
            self.word_more_keys_lapwing,
            Self::pct(self.word_more_keys_lapwing, self.words)
        );
        println!(
            "  fewer strokes vs plover:  {} ({:.1}%)",
            self.word_fewer_strokes_plover,
            Self::pct(self.word_fewer_strokes_plover, self.words)
        );
        println!(
            "  fewer strokes vs lapwing: {} ({:.1}%)",
            self.word_fewer_strokes_lapwing,
            Self::pct(self.word_fewer_strokes_lapwing, self.words)
        );
        println!(
            "total keys (shared vocabulary only, fair comparison): lyk-vs-plover {} plover {}   \
             lyk-vs-lapwing {} lapwing {}",
            self.total_keys_lyk_vs_plover,
            self.total_keys_plover,
            self.total_keys_lyk_vs_lapwing,
            self.total_keys_lapwing
        );
        println!(
            "lyk full-coverage total (every word lyk resolves, including ones neither gold \
             dictionary has at all): {}",
            self.total_keys_lyk
        );
        let signed = |n: u64| i64::try_from(n).unwrap_or(i64::MAX);
        let stroke_savings = signed(self.total_strokes_lyk) - signed(self.total_strokes_lyk_with_phrases);
        let key_savings = signed(self.total_keys_lyk) - signed(self.total_keys_lyk_with_phrases);
        println!(
            "phrase-aware walk (greedy longest-match over word pairs - see docs/phrases.md; a \
             separate, additive figure, never folded into any total above): {} phrase entries \
             fired over {} words",
            self.phrase_hits, self.words
        );
        println!(
            "  strokes: {} with phrases vs {} word-by-word ({} fewer strokes) - this is the real \
             number phrase-briefing exists to move",
            self.total_strokes_lyk_with_phrases, self.total_strokes_lyk, stroke_savings
        );
        println!(
            "  keys:    {} with phrases vs {} word-by-word ({} fewer keys) - expected to stay \
             roughly flat, see docs/phrases.md",
            self.total_keys_lyk_with_phrases, self.total_keys_lyk, key_savings
        );
    }
}

fn score(passages: &[&str]) -> Stats {
    let setup = dictionary_fixture::setup();
    let lyk = dictionary_fixture::cheapest();
    let phrases = crate::quote::phrase_map(dictionary_fixture::generated());
    let mut stats = Stats::default();

    for &passage in passages {
        let (phrase_strokes, phrase_keys, phrase_hits) =
            crate::quote::total_cost_with_phrases(setup, &lyk, &phrases, passage);
        stats.total_strokes_lyk_with_phrases += phrase_strokes;
        stats.total_keys_lyk_with_phrases += phrase_keys;
        stats.phrase_hits += phrase_hits;

        let rows = rows_for(setup, &lyk, passage);
        let mut p_keys_full = 0u64;
        let (mut p_keys_vs_plover, mut l_keys) = (0u64, 0u64);
        let (mut p_keys_vs_lapwing, mut w_keys) = (0u64, 0u64);
        for row in &rows {
            stats.words += 1;
            let ours = cost(&row.lyk);
            if let Some((s, k)) = ours {
                p_keys_full += u64::from(k);
                stats.total_strokes_lyk += s as u64;
            }
            if let (Some((os, ok)), Some((ps, pk))) = (ours, cost(&row.plover)) {
                l_keys += u64::from(pk);
                p_keys_vs_plover += u64::from(ok);
                match ok.cmp(&pk) {
                    std::cmp::Ordering::Less => stats.word_fewer_keys_plover += 1,
                    std::cmp::Ordering::Greater => stats.word_more_keys_plover += 1,
                    std::cmp::Ordering::Equal => {}
                }
                if os < ps {
                    stats.word_fewer_strokes_plover += 1;
                }
            }
            if let (Some((os, ok)), Some((ws, wk))) = (ours, cost(&row.lapwing)) {
                w_keys += u64::from(wk);
                p_keys_vs_lapwing += u64::from(ok);
                match ok.cmp(&wk) {
                    std::cmp::Ordering::Less => stats.word_fewer_keys_lapwing += 1,
                    std::cmp::Ordering::Greater => stats.word_more_keys_lapwing += 1,
                    std::cmp::Ordering::Equal => {}
                }
                if os < ws {
                    stats.word_fewer_strokes_lapwing += 1;
                }
            }
        }
        stats.passages += 1;
        stats.total_keys_lyk += p_keys_full;
        stats.total_keys_lyk_vs_plover += p_keys_vs_plover;
        stats.total_keys_plover += l_keys;
        stats.total_keys_lyk_vs_lapwing += p_keys_vs_lapwing;
        stats.total_keys_lapwing += w_keys;
        // Compared against each theory's own total over the *same* shared-vocabulary words, not
        // lyk's full-coverage total - otherwise a passage with a proper noun neither gold
        // dictionary knows would silently charge lyk for it without ever charging the other side.
        match p_keys_vs_plover.cmp(&l_keys) {
            std::cmp::Ordering::Less => stats.passage_wins_plover += 1,
            std::cmp::Ordering::Greater => stats.passage_losses_plover += 1,
            std::cmp::Ordering::Equal => {}
        }
        match p_keys_vs_lapwing.cmp(&w_keys) {
            std::cmp::Ordering::Less => stats.passage_wins_lapwing += 1,
            std::cmp::Ordering::Greater => stats.passage_losses_lapwing += 1,
            std::cmp::Ordering::Equal => {}
        }
    }
    stats
}

/// Every threshold here was set just under what a real run against `PASSAGES` actually measured
/// (see `docs/decisions.md`'s quote-corpus entry for the exact numbers it was calibrated from), not
/// a loose, anything-passes sanity floor - deliberately harsh, so an incoming change that makes the
/// theory even slightly less terse fails this test immediately instead of drifting unnoticed. A
/// *real* improvement should raise these numbers right back up along with the thresholds, in the
/// same commit, not leave them stranded below what the theory now actually does.
mod threshold {
    /// Share of the hundred-odd passages whose total keys lyk beats outright.
    ///
    /// Raised across the board here (`generate()`'s candidate-enumeration-and-solver rewrite -
    /// see `docs/decisions.md`): a real, measured improvement, same practice as every other
    /// threshold change this project has made - raise the number in the same commit as the win,
    /// not leave it stranded low. Even `WORD_MORE_KEYS_PCT_PLOVER_MAX` (the one number this rewrite
    /// was expected to cost something on, since the solver trades a handful of individual words for
    /// a better aggregate - see its own doc below) ended up *lower* than its pre-rewrite value, not
    /// merely back at par, once two real bugs were found and fixed: a peeling bug, and a blanket
    /// fragment-safety filter that briefly regressed `which`/`on`/`from`/`no`/`then`/`day` back to
    /// their full phonetic readings before being reverted as contradicting the day-4 philosophy
    /// reversal (both in `docs/decisions.md`).
    ///
    /// Raised again here, substantially, after fixing a real asymmetry bug in how `score()` summed
    /// lyk's total against each theory's: lyk's side was summed over *every* word it resolves, the
    /// other theory's only over words it has an entry for, so a passage with even one word neither
    /// gold dictionary knows charged lyk without ever charging the other side. Fixing that raised
    /// the real measured win rate from 76.7%/89.3% to 85.4%/97.1% - see `docs/decisions.md`.
    pub const PASSAGE_WIN_PCT_PLOVER: f64 = 82.0;
    pub const PASSAGE_WIN_PCT_LAPWING: f64 = 95.0;
    /// Share of individual words where lyk strictly beats the other theory on keys.
    pub const WORD_FEWER_KEYS_PCT_PLOVER: f64 = 23.0;
    pub const WORD_FEWER_KEYS_PCT_LAPWING: f64 = 38.0;
    /// Share of individual words where lyk strictly *loses* on keys - capped, so a change that
    /// quietly makes more words worse doesn't hide behind an unrelated set of words getting better.
    ///
    /// The solver now genuinely minimizes total frequency-weighted cost across the *whole*
    /// dictionary at once, not "how many individual words tie or beat Plover" - a handful more rare
    /// words individually costing a star or a key more than Plover's own brief would be an accepted
    /// price for a strictly better aggregate, if it actually came to that. Measured today it's
    /// actually *better* than the pre-rewrite baseline (9.5 -> 5.7), not just flat, once a blanket
    /// fragment-safety filter that briefly regressed `which`/`on`/`from`/`no`/`then`/`day` back to
    /// their full phonetic readings was found and reverted (see `docs/decisions.md`) - removing an
    /// over-strict filter freed up real wins the solver couldn't reach before.
    pub const WORD_MORE_KEYS_PCT_PLOVER_MAX: f64 = 6.2;
    pub const WORD_MORE_KEYS_PCT_LAPWING_MAX: f64 = 2.8;
    /// How much cheaper lyk's aggregate key total must be, as a percentage of the other theory's
    /// own total - the single hardest number to fake, since it is not swayed by lots of tiny common
    /// words tying (`a`, `the`, `is` cost the same everywhere) the way a per-word percentage is.
    ///
    /// Computed from `total_keys_lyk_vs_plover`/`_vs_lapwing`, not `total_keys_lyk` - see those
    /// fields' own doc comments on `Stats` for why using lyk's unscoped full-coverage total here
    /// was a real bug, not a stricter-but-valid alternative metric. Raised from 7.0/13.0 to the real
    /// fixed numbers (9.97%/18.02% measured) once that bug was found - see `docs/decisions.md`.
    pub const AGGREGATE_SAVINGS_PCT_PLOVER: f64 = 9.0;
    pub const AGGREGATE_SAVINGS_PCT_LAPWING: f64 = 17.0;
}

/// Prints the full breakdown (`cargo test --release quote_corpus -- --nocapture` to see it) and
/// holds the theory to the thresholds above.
#[test]
fn lyk_beats_plover_and_lapwing_across_a_hundred_unpicked_passages() {
    use threshold as t;
    let stats = score(PASSAGES);
    stats.report();

    let passage_win_pct_plover = Stats::pct(stats.passage_wins_plover, stats.passages);
    let passage_win_pct_lapwing = Stats::pct(stats.passage_wins_lapwing, stats.passages);
    let word_fewer_keys_pct_plover = Stats::pct(stats.word_fewer_keys_plover, stats.words);
    let word_fewer_keys_pct_lapwing = Stats::pct(stats.word_fewer_keys_lapwing, stats.words);
    let word_more_keys_pct_plover = Stats::pct(stats.word_more_keys_plover, stats.words);
    let word_more_keys_pct_lapwing = Stats::pct(stats.word_more_keys_lapwing, stats.words);
    let savings_pct_plover = (stats.total_keys_plover as f64 - stats.total_keys_lyk_vs_plover as f64) * 100.0
        / stats.total_keys_plover as f64;
    let savings_pct_lapwing = (stats.total_keys_lapwing as f64 - stats.total_keys_lyk_vs_lapwing as f64) * 100.0
        / stats.total_keys_lapwing as f64;

    assert!(
        passage_win_pct_plover >= t::PASSAGE_WIN_PCT_PLOVER,
        "only {passage_win_pct_plover:.1}% of passages beat Plover outright, want >= {}%",
        t::PASSAGE_WIN_PCT_PLOVER
    );
    assert!(
        passage_win_pct_lapwing >= t::PASSAGE_WIN_PCT_LAPWING,
        "only {passage_win_pct_lapwing:.1}% of passages beat Lapwing outright, want >= {}%",
        t::PASSAGE_WIN_PCT_LAPWING
    );
    assert!(
        word_fewer_keys_pct_plover >= t::WORD_FEWER_KEYS_PCT_PLOVER,
        "only {word_fewer_keys_pct_plover:.1}% of words beat Plover on keys, want >= {}%",
        t::WORD_FEWER_KEYS_PCT_PLOVER
    );
    assert!(
        word_fewer_keys_pct_lapwing >= t::WORD_FEWER_KEYS_PCT_LAPWING,
        "only {word_fewer_keys_pct_lapwing:.1}% of words beat Lapwing on keys, want >= {}%",
        t::WORD_FEWER_KEYS_PCT_LAPWING
    );
    assert!(
        word_more_keys_pct_plover <= t::WORD_MORE_KEYS_PCT_PLOVER_MAX,
        "{word_more_keys_pct_plover:.1}% of words cost more than Plover's, want <= {}%",
        t::WORD_MORE_KEYS_PCT_PLOVER_MAX
    );
    assert!(
        word_more_keys_pct_lapwing <= t::WORD_MORE_KEYS_PCT_LAPWING_MAX,
        "{word_more_keys_pct_lapwing:.1}% of words cost more than Lapwing's, want <= {}%",
        t::WORD_MORE_KEYS_PCT_LAPWING_MAX
    );
    assert!(
        savings_pct_plover >= t::AGGREGATE_SAVINGS_PCT_PLOVER,
        "lyk is only {savings_pct_plover:.2}% cheaper than Plover in aggregate, want >= {}%",
        t::AGGREGATE_SAVINGS_PCT_PLOVER
    );
    assert!(
        savings_pct_lapwing >= t::AGGREGATE_SAVINGS_PCT_LAPWING,
        "lyk is only {savings_pct_lapwing:.2}% cheaper than Lapwing in aggregate, want >= {}%",
        t::AGGREGATE_SAVINGS_PCT_LAPWING
    );
}

/// The five Gutenberg books `fetch` downloads for this corpus (see `src/fetch.rs`), in the order
/// their sentences get concatenated before sampling - public domain, pre-1929, across different
/// registers and eras the same way the 103 hand-picked passages above already lean on in
/// miniature (a novel of manners, frontier vernacular, a historical novel, a whaling epic, and the
/// same transcendentalist essay tradition `PASSAGES` already draws several lines from).
const CORPUS_BOOKS: &[&str] = &[
    "data/corpus/pride-and-prejudice.txt",
    "data/corpus/huckleberry-finn.txt",
    "data/corpus/tale-of-two-cities.txt",
    "data/corpus/moby-dick.txt",
    "data/corpus/walden.txt",
];

/// Roughly how many passages `corpus_extract::sample` thins the five books' combined sentences
/// down to.
const EXTRACTED_TARGET: usize = 400;

/// Reads and extracts the larger corpus from the books `fetch` downloaded, sampling down to
/// [`EXTRACTED_TARGET`] - deterministic, so repeated runs against the same downloaded files always
/// score the exact same set of passages.
fn extracted_passages() -> Vec<String> {
    let mut all = Vec::new();
    for path in CORPUS_BOOKS {
        let raw = std::fs::read_to_string(path).unwrap_or_else(|e| {
            panic!("{path}: {e} - run `cargo run --release -- fetch` first")
        });
        all.extend(crate::corpus_extract::extract(&raw));
    }
    crate::corpus_extract::sample(&all, EXTRACTED_TARGET)
}

/// Thresholds for the larger, extracted-prose corpus below, calibrated the same way as
/// [`threshold`] above: set a few points under a real, measured run - see `docs/decisions.md`'s
/// entry for this corpus for the full numbers. Kept as a separate module, and deliberately not
/// merged with [`threshold`], because this corpus's numbers are not expected to match the
/// hand-picked 103's exactly - measured: 416 passages, lyk beats Plover outright on 80.8% (vs. the
/// 103's 85.4%) and Lapwing on 95.0% (vs. 97.1%); of 5,595 words, lyk costs fewer keys than Plover
/// on 21.9% and Lapwing on 34.5% (close to the 103's own 24.8%/40.2%); in aggregate, over the words
/// each theory actually has an entry for, lyk is cheaper than both: 18,916 keys vs. Plover's 20,384
/// (7.2% cheaper) and 18,254 vs. Lapwing's 21,313 (14.4% cheaper) - consistent with the hand-picked
/// 103's own 9.97%/18.02%, which is the real point of running a larger, unpicked corpus at all:
/// confirming the win isn't an artifact of which 103 passages got chosen.
///
/// An earlier version of this corpus's numbers looked like a flip (lyk *more* expensive in
/// aggregate) because `score()` had a real asymmetry bug: lyk's total was summed over every word it
/// resolves, while each gold theory's total was summed only over words it has an entry for. Real
/// public-domain fiction is dense with proper nouns neither gold dictionary carries at all
/// (`netherfield`, `pemberley`, `queequeg`) - lyk's phonetic fallback resolves nearly all of them,
/// and that cost landed in lyk's aggregate with no counterweight on the other side (322 such words,
/// 3,769 keys on this corpus). The hand-picked 103 never hit this because nobody picked a passage
/// with a proper noun absent from Plover's own dictionary. Fixed by scoring each side over the
/// *same* shared vocabulary (`total_keys_lyk_vs_plover`/`_vs_lapwing`, see their doc comments on
/// `Stats`) - see `docs/decisions.md` for the full before/after. `total_keys_lyk` (lyk's
/// full-coverage total, uncomparable on its own) is kept and still printed by `report()` as a
/// separate, clearly-labeled figure, since "lyk resolves words the others can't at all" is a real
/// strength worth keeping visible, just not one that belongs mixed into a "which costs less" ratio.
mod extracted_threshold {
    pub const PASSAGE_WIN_PCT_PLOVER: f64 = 76.0;
    pub const PASSAGE_WIN_PCT_LAPWING: f64 = 90.0;
    pub const WORD_FEWER_KEYS_PCT_PLOVER: f64 = 20.0;
    pub const WORD_FEWER_KEYS_PCT_LAPWING: f64 = 32.0;
    pub const WORD_MORE_KEYS_PCT_PLOVER_MAX: f64 = 7.2;
    pub const WORD_MORE_KEYS_PCT_LAPWING_MAX: f64 = 3.4;
    pub const AGGREGATE_SAVINGS_PCT_PLOVER: f64 = 6.0;
    pub const AGGREGATE_SAVINGS_PCT_LAPWING: f64 = 13.0;
}

/// The same benchmark as `lyk_beats_plover_and_lapwing_across_a_hundred_unpicked_passages`, over a
/// larger, non-hand-picked sample of real connected prose instead of a hand-picked list - see
/// `docs/decisions.md` for why both run side by side rather than one replacing the other. Needs
/// `cargo run --release -- fetch` to have downloaded the books first; panics with that instruction
/// if they're missing rather than silently skipping.
#[test]
fn lyk_beats_plover_and_lapwing_across_extracted_gutenberg_prose() {
    use extracted_threshold as t;
    let passages = extracted_passages();
    let refs: Vec<&str> = passages.iter().map(String::as_str).collect();
    let stats = score(&refs);
    stats.report();

    let passage_win_pct_plover = Stats::pct(stats.passage_wins_plover, stats.passages);
    let passage_win_pct_lapwing = Stats::pct(stats.passage_wins_lapwing, stats.passages);
    let word_fewer_keys_pct_plover = Stats::pct(stats.word_fewer_keys_plover, stats.words);
    let word_fewer_keys_pct_lapwing = Stats::pct(stats.word_fewer_keys_lapwing, stats.words);
    let word_more_keys_pct_plover = Stats::pct(stats.word_more_keys_plover, stats.words);
    let word_more_keys_pct_lapwing = Stats::pct(stats.word_more_keys_lapwing, stats.words);
    let savings_pct_plover = (stats.total_keys_plover as f64 - stats.total_keys_lyk_vs_plover as f64) * 100.0
        / stats.total_keys_plover as f64;
    let savings_pct_lapwing = (stats.total_keys_lapwing as f64 - stats.total_keys_lyk_vs_lapwing as f64)
        * 100.0
        / stats.total_keys_lapwing as f64;

    assert!(
        passage_win_pct_plover >= t::PASSAGE_WIN_PCT_PLOVER,
        "only {passage_win_pct_plover:.1}% of passages beat Plover outright, want >= {}%",
        t::PASSAGE_WIN_PCT_PLOVER
    );
    assert!(
        passage_win_pct_lapwing >= t::PASSAGE_WIN_PCT_LAPWING,
        "only {passage_win_pct_lapwing:.1}% of passages beat Lapwing outright, want >= {}%",
        t::PASSAGE_WIN_PCT_LAPWING
    );
    assert!(
        word_fewer_keys_pct_plover >= t::WORD_FEWER_KEYS_PCT_PLOVER,
        "only {word_fewer_keys_pct_plover:.1}% of words beat Plover on keys, want >= {}%",
        t::WORD_FEWER_KEYS_PCT_PLOVER
    );
    assert!(
        word_fewer_keys_pct_lapwing >= t::WORD_FEWER_KEYS_PCT_LAPWING,
        "only {word_fewer_keys_pct_lapwing:.1}% of words beat Lapwing on keys, want >= {}%",
        t::WORD_FEWER_KEYS_PCT_LAPWING
    );
    assert!(
        word_more_keys_pct_plover <= t::WORD_MORE_KEYS_PCT_PLOVER_MAX,
        "{word_more_keys_pct_plover:.1}% of words cost more than Plover's, want <= {}%",
        t::WORD_MORE_KEYS_PCT_PLOVER_MAX
    );
    assert!(
        word_more_keys_pct_lapwing <= t::WORD_MORE_KEYS_PCT_LAPWING_MAX,
        "{word_more_keys_pct_lapwing:.1}% of words cost more than Lapwing's, want <= {}%",
        t::WORD_MORE_KEYS_PCT_LAPWING_MAX
    );
    assert!(
        savings_pct_plover >= t::AGGREGATE_SAVINGS_PCT_PLOVER,
        "lyk is only {savings_pct_plover:.2}% cheaper than Plover in aggregate, want >= {}%",
        t::AGGREGATE_SAVINGS_PCT_PLOVER
    );
    assert!(
        savings_pct_lapwing >= t::AGGREGATE_SAVINGS_PCT_LAPWING,
        "lyk is only {savings_pct_lapwing:.2}% cheaper than Lapwing in aggregate, want >= {}%",
        t::AGGREGATE_SAVINGS_PCT_LAPWING
    );
}

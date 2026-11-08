//! Putting the dictionary together: fixed entries first, then every word's primary settled at
//! once, then every other way in, each only where nobody holds the text. See [`generate`] for the
//! order and `site/reference/collisions.json` for why.

mod backup;
mod candidates;
mod fill;
mod report;
mod settle;
mod solver;
#[cfg(test)]
mod tests;

pub use candidates::{Candidate, STROKE_WEIGHT, candidate_cost, enumerate_candidates};
pub use backup::print_paths;
pub use report::build;

use crate::analysis::Setup;
use crate::rules::Step;
use crate::ergonomics;
use crate::stroke::{Stroke, parse_outline, render_outline};
use anyhow::{Context, Result};
use candidates::{enumerate_in_parallel, ranked_words, rules_texts};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

const BUILT_IN_BRIEFS: &str = include_str!("../../rules/briefs.toml");

const BUILT_IN_SUFFIXES: &str = include_str!("../../rules/suffixes.toml");

/// Every stroke that isn't a word at all: the commands, punctuation, and fingerspelling/OS-key
/// dictionaries deployed alongside the generated dictionary in a real keymux config. These load
/// at *higher* priority than the generated words there, so a word that happens to land on the
/// same stroke as one of these is never reachable — a real stroke, that was never a bug in the
/// deployed config, just unknown to the generator. Reserved the same way a brief's outline is
/// (see `generate`), so the collision can't resurface on the next regeneration.
const RESERVED: &[(&str, &str)] = &[
    ("a command", include_str!("../../rules/reserved/commands.json")),
    ("a punctuation mark", include_str!("../../rules/reserved/punctuation.json")),
    ("a fingerspelled letter or OS shortcut", include_str!("../../rules/reserved/emilys-modifiers.json")),
];

#[derive(Deserialize, Default)]
pub struct Briefs {
    /// Vestigial since the solver replaced `best_brief`: every word's own gold-brief candidates
    /// now compete on cost regardless of rank, so there is no "window" left to size (see
    /// `enumerate_candidates`). Kept parseable so `rules/briefs.toml`'s own header does not need
    /// editing just to stop meaning anything; `#[allow(dead_code)]` is honest about that, not a
    /// bug to fix by wiring it back in.
    #[allow(dead_code)]
    #[serde(default)]
    count: usize,
    /// Vestigial for the same reason: a word with no `[word]` entry now merges *both* gold
    /// dictionaries unconditionally (see `enumerate_candidates`'s own doc) rather than trying one
    /// default source first.
    #[allow(dead_code)]
    #[serde(default = "plover")]
    default: String,
    /// Per word: a literal outline (forces exactly one candidate, never reopened to competition)
    /// or `"plover"`/`"lapwing"` (narrows which gold dictionary this word's own brief candidates
    /// come from, still fully competing on cost). See `enumerate_candidates`.
    #[serde(default)]
    word: HashMap<String, String>,
}

fn plover() -> String {
    "plover".into()
}

impl Briefs {
    /// The briefs in `path`, or the built-in `rules/briefs.toml` when there is none.
    pub fn from_path(path: Option<&Path>) -> Result<Self> {
        path.map_or_else(Self::built_in, Self::load)
    }

    pub fn built_in() -> Result<Self> {
        Ok(toml::from_str(BUILT_IN_BRIEFS)?)
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).with_context(|| path.display().to_string())?;
        Ok(toml::from_str(&text)?)
    }
}

/// How a dictionary entry came about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Origin {
    /// A standalone suffix: a continuation stroke that stands for it on its own, not folded onto
    /// any particular word (see `rules/suffixes.toml`)
    Suffix,
    Brief,
    Rules,
    /// A long word written shorter by one of the shortening steps
    Short(Step),
    /// The spelling told it from a word that sounds the same
    Spelling,
    /// Another pronunciation of a word
    Alternate,
    /// The same candidate with its last stroke's `-Z` respelled `-S`, offered wherever the word's
    /// own spelling ends in a literal `s` - competes on equal footing with the `-Z` form it was
    /// copied from, unlike [`Origin::Alternate`] (see [`enumerate_candidates`])
    Voicing,
    /// The ladder step that settled a clash: 1 is the star on the first stroke
    Ladder(u8),
    /// Composed from one or two already-placed words' own outlines: a stem plus a bare
    /// continuation stroke for a reduced trailing syllable, a leading consonant folded onto
    /// another word's first stroke, or two whole words back to back — struck on top of however
    /// each piece is actually briefed or folded, so a learner who knows the pieces can always
    /// reach the word that way too (see the backup-entry pass in [`generate`]).
    Backup,
    /// A family member's root's own settled outline with the member's endings' keys added
    /// (`build` `PWHR` -> `builds` `PWHRZ`), offered after a first settling pass so a family can
    /// share one shape - see `family_offers` and `site/reference/syllables.json`. Competes on cost like any
    /// other candidate.
    Family,
    /// Two already-placed words' own settled single-stroke *primaries*, merged into one new,
    /// previously-unclaimed stroke when their key-bitmasks are disjoint — a real 2-strokes-to-1
    /// compression for a common bigram (`setup.bigrams`), not a cosmetic one. The entry's `word`
    /// field holds the two-word phrase text ("of the"), never a real single word competing for
    /// anything. Never a stem a continuation stroke can follow — see `single_stroke_stems`'s own
    /// exclusion filter and `site/reference/phrases.json`, which this whole mechanism is documented in.
    Phrase,
    /// Another single-stroke way to write a word, added last of all: a candidate (a gold brief,
    /// the plain rules reading, a spelling variant, an `-S` respelling, a family shape) that lost
    /// its word's primary slot but whose stroke nobody else ended up needing. It never takes a
    /// stroke from anything and never becomes the outline `cheapest` reports - see
    /// `site/reference/briefs.json`.
    Extra,
}

pub struct Entry {
    pub word: String,
    pub outline: Vec<Stroke>,
    pub origin: Origin,
}

pub struct Generated {
    pub entries: Vec<Entry>,
    pub unresolved: Vec<String>,
    /// A backup entry (see [`Origin::Backup`]) whose composed outline another entry already had.
    pub backup_skipped: usize,
    /// A word whose own plain-rules outline costs less than its actual entry, and is sitting
    /// completely unclaimed: a regression guard, not something expected to ever hold more than a
    /// handful of words (see where it is built in [`generate`]).
    pub stranded: Vec<String>,
    /// How many words survived fixed-point peeling still holding more than one live candidate -
    /// the real input `solve_contested` ran over. The whole algorithm choice (successive shortest
    /// augmenting paths over this subgraph, not dense Hungarian over the full dictionary) rests on
    /// this staying small; see `quality::the_contested_set_after_peeling_stays_small`.
    pub contested: usize,
    /// How many [`Origin::Phrase`] entries the phrase-briefing pass actually committed - see
    /// `site/reference/phrases.json`.
    pub phrases_added: usize,
    /// A phrase candidate that lost its merged stroke, either to something `taken` already held
    /// or to another eligible bigram this same pass arbitrated away (see `arbitrate_phrase_round`).
    pub phrases_skipped: usize,
    /// Of the phrase entries added, how many took their stroke from a rarer word.
    pub phrases_displaced: usize,
    /// How many [`Origin::Extra`] entries were added after everything else.
    pub extras_added: usize,
    /// Every word that is another word plus a regular ending, by word - see `crate::family`.
    pub links: HashMap<String, crate::family::Link>,
    /// Every word that is a prefix plus another word, by word - see `crate::family`.
    pub prefix_links: HashMap<String, crate::family::PrefixLink>,
}

const KEY_WEIGHT: u32 = 1_000;

/// How much cheaper a family-shaped candidate (`Origin::Family`) counts than its raw cost: two
/// keys, so a member takes its root's shape (`building` `PWHR-G`, not Plover's `PW-LG`) unless
/// that costs more than two extra keys. See `site/reference/syllables.json`.
pub const FAMILY_DISCOUNT: u32 = 2_000;

/// Every outline a real keymux config already gives to a command, a punctuation mark, or
/// fingerspelling - never available to a word, whatever the rules would have generated for it.
/// Pulled out of [`generate`] so a test can confirm the invariant it exists to guarantee (no
/// generated word ever lands on one of these) without re-deriving this list by hand.
pub fn reserved_outlines() -> Result<HashMap<String, String>> {
    let mut taken = HashMap::new();
    for (kind, json) in RESERVED {
        let strokes: HashMap<String, String> = serde_json::from_str(json)
            .with_context(|| format!("rules/reserved: {kind} dictionary"))?;
        for outline_text in strokes.keys() {
            let Some(outline) = parse_outline(outline_text) else {
                continue;
            };
            let text = render_outline(&outline);
            taken.entry(text).or_insert_with(|| format!("reserved ({kind})"));
        }
    }
    Ok(taken)
}


/// The dictionary as it is built: every entry so far, which word holds each outline's text, and
/// the words nothing could be found for.
pub struct Book {
    pub entries: Vec<Entry>,
    pub taken: HashMap<String, String>,
    pub unresolved: Vec<String>,
}

/// A point in a [`Book`] to go back to: see [`Book::reset`].
pub struct Mark {
    pub entries: usize,
    taken: HashMap<String, String>,
    unresolved: usize,
}

impl Book {
    /// Adds `outline` for `word` if its text is free; whether it was.
    pub fn claim(&mut self, word: &str, outline: Vec<Stroke>, origin: Origin) -> bool {
        let text = render_outline(&outline);
        if self.taken.contains_key(&text) {
            return false;
        }
        self.place(word, outline, origin, text);
        true
    }

    /// Adds `outline` for `word`, `text` being its rendered text, which the caller knows is free.
    pub fn place(&mut self, word: &str, outline: Vec<Stroke>, origin: Origin, text: String) {
        self.taken.insert(text, word.to_owned());
        self.entries.push(Entry { word: word.to_owned(), outline, origin });
    }

    /// Adds `outline` for `word` ahead of every word: an error naming the holder if its text is not
    /// free, since a fixed entry colliding is a mistake in the rules files, not a contest.
    fn fix(&mut self, word: &str, outline: Vec<Stroke>, origin: Origin, what: &str) -> Result<()> {
        let text = render_outline(&outline);
        if let Some(holder) = self.taken.get(&text) {
            anyhow::bail!("{what} wants the outline {text:?}, which {holder:?} already has");
        }
        self.place(word, outline, origin, text);
        Ok(())
    }

    pub fn mark(&self) -> Mark {
        Mark { entries: self.entries.len(), taken: self.taken.clone(), unresolved: self.unresolved.len() }
    }

    /// Everything added since `mark` taken back out.
    pub fn reset(&mut self, mark: &Mark) {
        self.entries.truncate(mark.entries);
        self.taken.clone_from(&mark.taken);
        self.unresolved.truncate(mark.unresolved);
    }
}

/// The whole dictionary, in the order every choice is made (see `site/reference/collisions.json`):
///
/// 1. reserved strokes (commands, punctuation, fingerspelling), the standalone suffixes, the
///    pronoun "I", and every literal `[word]` outline from `rules/briefs.toml`, fixed first;
/// 2. every other word's primary, settled at once over every word's candidates ([`settle`]);
/// 3. other pronunciations, then the backup joins ([`backup`]), then bonus short forms, each only
///    where nobody holds the text;
/// 4. phrases (`crate::phrases`), which may take a rarer word's stroke;
/// 5. extra one-stroke entries, in whatever strokes nobody needed.
pub fn generate(setup: &Setup, briefs: &Briefs, common: usize) -> Result<Generated> {
    let mut book = Book { entries: Vec::new(), taken: reserved_outlines()?, unresolved: Vec::new() };

    // A standalone suffix is struck on its own far more often than any one rare word wants its
    // outline, so it comes first, the same as a brief.
    let suffixes: BTreeMap<String, String> = toml::from_str(BUILT_IN_SUFFIXES)?;
    for (text, word) in &suffixes {
        let outline = parse_outline(text).with_context(|| format!("{text:?} in rules/suffixes.toml"))?;
        book.fix(word, outline, Origin::Suffix, &format!("suffix {word:?}"))?;
    }
    // The pronoun "I": the frequency list has it lowercase, and nearly all of "i"'s count is the
    // pronoun. The bare letter is not a word (fingerspelling writes it), so it can't take `AOEU`
    // from "eye". See `written` for how "i'm" and "i think" come out.
    book.fix("I", parse_outline("EU").context("\"EU\" for the pronoun \"I\"")?, Origin::Brief, "the pronoun \"I\"")?;

    let words = ranked_words(setup, common);
    // The texts any word's plain rules give, which a brevity candidate never takes, and the
    // one-syllable strokes' stars it pays to take (see `RulesTexts`).
    let texts = rules_texts(setup, &words);

    // A literal `[word]` outline forces exactly one candidate and is never reopened: a human
    // decision, fixed before any other word competes. A `"plover"`/`"lapwing"` entry only narrows
    // which gold dictionary the word's briefs come from, and still competes.
    let is_literal_override = |word: &str| briefs.word.get(word).is_some_and(|s| s != "plover" && s != "lapwing");
    for (word, _) in words.iter().filter(|(w, _)| is_literal_override(w)) {
        let candidate = enumerate_candidates(setup, briefs, word, &texts)
            .into_iter()
            .next()
            .with_context(|| format!("{word:?}'s brief in rules/briefs.toml does not parse as an outline"))?;
        if !typable(&candidate.outline) {
            anyhow::bail!(
                "{word:?}'s brief (in rules/briefs.toml) is {:?}, which cannot be typed: the right pinky \
                 cannot press a diagonal pair (-TZ, -SD) or three of -T -S -D -Z",
                render_outline(&candidate.outline)
            );
        }
        book.fix(word, candidate.outline, candidate.origin, &format!("{word:?}'s brief (in rules/briefs.toml)"))?;
    }

    let competing: Vec<(&str, usize)> =
        words.iter().map(|(w, r)| (w.as_str(), *r)).filter(|(w, _)| !is_literal_override(w)).collect();
    let names: Vec<&str> = competing.iter().map(|(w, _)| *w).collect();
    let candidates = enumerate_in_parallel(&names, |word| enumerate_candidates(setup, briefs, word, &texts));
    let pools = settle::Pools::new(competing, candidates);

    let vocabulary = || words.iter().map(|(w, _)| w.as_str());
    let links = crate::family::links(&setup.pron, &setup.count, vocabulary());
    let prefix_links = crate::family::prefix_links(&setup.pron, &setup.count, vocabulary());
    // The wider tables the backup joins use, never the primaries (see `BACKUP_ENDINGS`).
    let join_endings = crate::family::all_links(&setup.pron, &setup.count, vocabulary(), crate::family::BACKUP_ENDINGS);
    let join_prefixes =
        crate::family::prefix_links_with(&setup.pron, &setup.count, vocabulary(), crate::family::BACKUP_PREFIXES);

    let settled = settle::settle(setup, &pools, &settle::Families { links: &links, prefixes: &prefix_links }, &mut book);
    fill::alternates(setup, &words, &mut book);
    let ranked: Vec<&str> = words.iter().map(|(w, _)| w.as_str()).collect();
    let backup_skipped = backup::place(
        setup,
        &mut book,
        &backup::JoinLinks { endings: &join_endings, prefixes: &join_prefixes },
        &ranked,
    );
    fill::short_forms(&pools, &mut book);

    // Phrases see every backup in `taken`, and run before extras so a word that lost its stroke
    // can still be given another.
    let phrases = {
        let pool_index: HashMap<&str, &[Candidate]> = pools.iter().collect();
        let stems: HashSet<&str> = links.values().map(|l| l.stem.as_str()).collect();
        let natural = |word: &str| {
            let sounds = setup.pron.get(word)?.first()?;
            let mut outline = setup.rules.outline(word, sounds)?;
            setup.rules.joined(&mut outline);
            Some(outline)
        };
        crate::phrases::place(setup, &mut book.entries, &mut book.taken, &pool_index, &stems, &natural, &is_literal_override)
    };
    let extras_added = fill::extras(&pools, &settled.offers, &mut book);
    let stranded = fill::stranded(setup, &book);

    Ok(Generated {
        entries: book.entries,
        unresolved: book.unresolved,
        backup_skipped,
        stranded,
        contested: settled.contested,
        phrases_added: phrases.added,
        phrases_skipped: phrases.skipped,
        phrases_displaced: phrases.displaced,
        extras_added,
        links,
        prefix_links,
    })
}

/// Each word's single best entry among a list, respecting `Origin`: an `Origin::Alternate` or
/// `Origin::Extra` entry can never outrank any other entry for the same word on raw stroke/key
/// cost alone - its own doc
/// comment already claims it "never competes with the primary"; this is what actually makes that
/// true downstream, in both places that used to compare entries by `(len, keys)` alone.
pub fn best_entry_per_word<'a>(entries: impl Iterator<Item = &'a Entry>) -> HashMap<&'a str, &'a Entry> {
    let mut best: HashMap<&str, &Entry> = HashMap::new();
    for entry in entries {
        match best.get(entry.word.as_str()) {
            Some(&current) if priority(current) <= priority(entry) => {}
            _ => {
                best.insert(entry.word.as_str(), entry);
            }
        }
    }
    best
}

/// How good an entry is as its word's outline, lowest best: any other origin before another
/// pronunciation or an extra, then strokes, then keys and ergonomics as one number (a family
/// shape's `FAMILY_DISCOUNT` applied as the solver applied it, so a bonus short form that merely
/// ties it doesn't shadow it), then the outline itself, so the answer never depends on the order
/// entries were pushed in.
pub fn priority(e: &Entry) -> (bool, usize, u32, &[Stroke]) {
    let discount = if e.origin == Origin::Family { FAMILY_DISCOUNT } else { 0 };
    (
        matches!(e.origin, Origin::Alternate | Origin::Extra),
        e.outline.len(),
        (KEY_WEIGHT * keys(&e.outline) + ergonomics::weighted_outline(&e.outline)).saturating_sub(discount),
        e.outline.as_slice(),
    )
}

/// Each word's cheapest outline (fewest strokes, then fewest keys) among a list of entries.
///
/// A standalone suffix's own entry (`rules/suffixes.toml`) never counts, even when its own
/// display text happens to also be a real word (`less`, `age`, `some`, `ism`, and 17 others): its
/// outline is a continuation-stroke fragment that is never a word by itself when struck alone
/// (the join key it carries refuses that, by design — see [the join key](site/reference/join-key.json)),
/// so it is never really "the outline" for that word even when it numerically looks cheapest. An
/// `Origin::Alternate` entry (another pronunciation's outline) is excluded from winning against
/// any other origin for the same word by [`best_entry_per_word`]'s own priority tier - see its
/// doc comment.
pub fn cheapest(entries: &[Entry]) -> HashMap<&str, &[Stroke]> {
    best_entry_per_word(entries.iter().filter(|e| e.origin != Origin::Suffix))
        .into_iter()
        .map(|(word, entry)| (word, entry.outline.as_slice()))
        .collect()
}

/// Every outline in a generated dictionary, by its rendered text, to the word it writes.
pub fn index(generated: &Generated) -> HashMap<String, &str> {
    index_of(&generated.entries)
}

/// [`index`] over bare entries, such as a dictionary read back from disk (`crate::built`).
pub fn index_of(entries: &[Entry]) -> HashMap<String, &str> {
    entries
        .iter()
        .map(|e| (render_outline(&e.outline), e.word.as_str()))
        .collect()
}

/// Whether an entry made in this way is a word's own primary outline (not a short form, backup,
/// alternate, extra, phrase or suffix).
const fn is_primary_origin(origin: Origin) -> bool {
    matches!(origin, Origin::Brief | Origin::Rules | Origin::Spelling | Origin::Voicing | Origin::Ladder(_) | Origin::Family)
}

/// Every single-stroke word's primary outline, by its own bare stroke — what
/// `Rules::resolve` checks before reading an unmatched run's first stroke as
/// raw phonemes, so a continuation after a known word (`SKP/+-R`, "and" plus "-er") reads as that
/// word's own real text ("ander") instead of garbling the word itself ("sper", "and"'s own `SKP`
/// read as if it meant nothing). A standalone suffix's own entry is excluded the same way
/// [`cheapest`] excludes it everywhere else — its outline is a continuation-stroke fragment, never
/// a real word struck alone. An [`Origin::Phrase`] entry is excluded for the same reason as a
/// suffix, for the opposite reason: its `word` field is two words' own text ("of the"), not a real
/// single word a continuation stroke could sensibly follow at all (`"of the" + "-s"` would read
/// back as "of thes", nonsense) — see `site/reference/phrases.json`. Collision-free by construction: `taken`
/// already guarantees one word per outline text, so there is never more than one candidate for a
/// given stroke here. Only a word's primary counts, not its short forms, backups, extras or
/// alternates: the same word under four strokes would only repeat a reading it already gives.
/// Over bare entries, such as a dictionary read back from disk
/// (`crate::built`), where every word file entry stands in as a primary.
pub fn single_stroke_stems_of(entries: &[Entry]) -> HashMap<Stroke, String> {
    entries
        .iter()
        .filter(|e| is_primary_origin(e.origin))
        .filter_map(|e| match e.outline[..] {
            [stroke] => Some((stroke, e.word.clone())),
            _ => None,
        })
        .collect()
}

/// The shortest outline (fewest strokes, then fewest keys) from a list.
pub fn shortest(outlines: &[String]) -> Option<Vec<Stroke>> {
    outlines
        .iter()
        .filter_map(|o| parse_outline(o))
        .min_by_key(|o| (o.len(), keys(o)))
}

/// How many keys an outline takes to press.
pub fn keys(outline: &[Stroke]) -> u32 {
    outline.iter().map(|s| s.0.count_ones()).sum()
}

/// The tail of a contraction the frequency list's tokenizer split off on its own ("I've" counted
/// as "i" plus "'ve"). Never a word by itself - struck alone it would type " 've" with a space -
/// but with the full contraction's count it out-arbitrates real words for short strokes (it once
/// pushed "you" off `U`). Colloquial clipped words ("'cause", "'em", "'til") are real words and stay.
pub const CONTRACTION_TAILS: &[&str] = &["'s", "'d", "'ll", "'re", "'m", "'ve", "'t", "'n", "'a"];

/// The top-`top_n` bigrams by raw count (`setup.bigrams`), each paired with its candidate merged
/// outline where one exists - not yet checked against `taken` or against each other, mirroring
/// `compose_backup`'s own "found, not yet committed" boundary. `top_n` bounds the raw bigram list
/// scanned, ranked by count, *before* the eligibility filter - not the number of phrase entries
/// that end up committed, which is smaller once ineligible pairs, round-internal collisions, and
/// `taken` are all accounted for. See `site/reference/phrases.json`'s threshold section for how the real
/// eligible-set size was measured and why raw count (not a PMI/collocation ratio) is what ranks
/// bigrams here.
/// The pronoun's own entry is the capitalized "I" (see `generate`), while the bigram corpus and
/// the frequency list only ever spell it lowercase.
pub fn entry_word(word: &str) -> &str {
    if word == "i" { "I" } else { word }
}

/// How a word is actually written out: the pronoun "I", alone or leading a contraction ("i'm") or
/// as either half of a phrase entry ("i think"), is capitalized; everything else is as-is. Words
/// are keyed lowercase everywhere inside the generator, so this is applied only on the way out.
pub fn written(word: &str) -> String {
    word.split(' ')
        .map(|token| {
            if token == "i" || token.starts_with("i'") {
                format!("I{}", &token[1..])
            } else {
                token.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether every stroke of `outline` can be pressed (see [`Stroke::typable`]).
pub fn typable(outline: &[Stroke]) -> bool {
    outline.iter().all(|stroke| stroke.typable())
}

/// `outline` with every untypable stroke written as two (see [`Stroke::split_untypable`]).
pub fn split_untypable(outline: &[Stroke]) -> Option<Vec<Stroke>> {
    let mut out = Vec::with_capacity(outline.len() + 1);
    for stroke in outline {
        match stroke.split_untypable() {
            Some(pair) => out.extend(<[Stroke; 2]>::from(pair)),
            None => out.push(*stroke),
        }
    }
    typable(&out).then_some(out)
}


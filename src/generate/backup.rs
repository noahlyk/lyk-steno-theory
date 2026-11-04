//! Backup joins: every `+` way into a word a learner can work out from pieces they already know,
//! added beside the word's primary once every primary is settled. None of them can change a
//! primary; they only ever take a text nobody holds. See `docs/syllables.md`.
//!
//! Each round indexes everything placed so far (every primary, and from round 1 on every earlier
//! round's backups too, which is how *building* + `+-S` reaches *buildings*), collects every
//! direction's every composition for every word, gives a text two words both want to the commoner
//! one, and commits the winners. It repeats until a round adds nothing. The directions:
//!
//! - **suffix**: a placed stem's outline plus a reduced trailing syllable as one continuation stroke
//!   (*build* + "-ing"), or plus a two-stroke suffix from `rules/suffixes.toml` found by spelling
//!   (*cube* + "-ism");
//! - **lead**: a leading consonant cluster folded onto a placed word's first stroke (*ever* -> *never*);
//! - **compound**: two placed words back to back, when the spelling is the two joined (*myself*);
//! - **ending**: a family link's stem plus its ending's stroke (*call* + "-ed", *fool* + "-ish");
//! - **prefix**: a prefix's stroke, then a placed stem's outline (*un* + *clear*);
//! - **syllables**: the word itself one syllable per stroke (`Rules::syllable_paths`), round 0 only.
//!
//! A stem joins through its fastest outline and through its own plain rules reading when that
//! differs (*profitable* `PROFBLT` and `PROFT/+-BL`), with any star-ladder marker stroke dropped: the
//! marker only tells a word from a homophone, and inside a longer join it is noise. Every
//! composition of three strokes or more whose last stroke fits on the one before it, as a reduced
//! syllable would, is also offered folded (`SEPB/+SU/+-BL` and `SEPB/+SUBL`).

use crate::analysis::Setup;
use crate::family::{Link, PrefixLink, PrefixStroke};
use super::{Book, Entry, Origin, priority, typable};
use crate::stroke::{Stroke, render_outline};
use std::collections::{HashMap, HashSet};

/// Rounds at most. Each round only builds on the previous round's new entries, so growth stops
/// well before this; hitting it would mean something matches too permissively.
const MAX_ROUNDS: usize = 5;

/// Where the family links a backup join may use come from: see `crate::family::all_links`.
pub struct JoinLinks<'a> {
    /// Every link each word has, over `crate::family::BACKUP_ENDINGS`.
    pub endings: &'a HashMap<String, Vec<Link>>,
    /// Each word's prefix link, over `crate::family::BACKUP_PREFIXES`.
    pub prefixes: &'a HashMap<String, PrefixLink>,
}

/// One composition: the word, its outline (join keys set), and the outline's text.
type Join = (String, Vec<Stroke>, String);

/// Adds every backup join it can to `entries`, claiming each text in `taken`. Returns how many
/// compositions were skipped because their text was already held or went to a commoner word.
pub fn place(setup: &Setup, book: &mut Book, links: &JoinLinks) -> usize {
    let mut stems = Stems::index(setup, &book.entries, links);
    let mut skipped = 0;
    for round in 0..MAX_ROUNDS {
        let mut words: Vec<&str> = stems.outlines.keys().copied().collect();
        words.sort_unstable();
        let joins = collect_in_parallel(&words, |word| stems.joins(word, round == 0));
        let winners = arbitrate(&joins, |word| setup.count.get(word).copied().unwrap_or(0));
        let first_new = book.entries.len();
        for ((word, outline, text), won) in joins.into_iter().zip(winners) {
            if !won || book.taken.contains_key(&text) {
                skipped += 1;
                continue;
            }
            book.place(&word, outline, Origin::Backup, text);
        }
        if book.entries.len() == first_new {
            break;
        }
        stems.update(&book.entries, first_new);
    }
    skipped
}

/// `compose` run over `words` on every core, results in `words` order.
fn collect_in_parallel<F>(words: &[&str], compose: F) -> Vec<Join>
where
    F: Fn(&str) -> Vec<Join> + Sync,
{
    let threads = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    let chunk = words.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> =
            words.chunks(chunk).map(|part| scope.spawn(|| part.iter().flat_map(|w| compose(w)).collect::<Vec<_>>())).collect();
        handles.into_iter().flat_map(|h| h.join().expect("a backup-join thread panicked")).collect()
    })
}

/// Which compositions win their text: one nobody else composed, or the one whose word is said most
/// often among those that share it (ties to the word that sorts first). A backup is optional, so
/// losing one costs nothing but the shortcut; the shortcut goes to the word typed more often.
pub fn arbitrate(joins: &[Join], weight_of: impl Fn(&str) -> u64) -> Vec<bool> {
    let mut best: HashMap<&str, usize> = HashMap::with_capacity(joins.len());
    for (i, (word, _, text)) in joins.iter().enumerate() {
        let key = |j: usize| (weight_of(&joins[j].0), std::cmp::Reverse(joins[j].0.as_str()));
        match best.get(text.as_str()) {
            Some(&held) if key(held) >= key(i) || joins[held].0 == *word => {}
            _ => {
                best.insert(text.as_str(), i);
            }
        }
    }
    (0..joins.len()).map(|i| best.get(joins[i].2.as_str()) == Some(&i)).collect()
}

/// The index every round composes from, built once and updated as each round adds entries.
struct Stems<'a> {
    setup: &'a Setup,
    links: &'a JoinLinks<'a>,
    /// Each placed word's best entry so far, by its position in the entries.
    best: HashMap<&'a str, usize>,
    /// Each placed word's outlines to join through: its fastest, then its plain rules reading
    /// when that is a different, longer form, star-ladder markers dropped from both.
    outlines: HashMap<&'a str, Vec<Vec<Stroke>>>,
    /// Each placed word's plain rules reading, whatever its length: what a join through an ending
    /// past the family set builds on, since that ending may match by coincidence and a brief stem
    /// would then type nothing like the word (`andy` is not `SKP` + "-y").
    plain: HashMap<&'a str, Vec<Stroke>>,
    /// `rules/families.toml`'s `not_family`: words whose ending is part of the word, never joined
    /// as a stem plus that ending (`wither` is not `with` + "-er").
    not_family: HashSet<String>,
    /// Placed words by their first pronunciation, exactly.
    by_sounds: HashMap<&'a [String], Vec<&'a str>>,
    /// The same, stress ignored: a word keeps primary stress alone (`my` `M AY1`) but often has
    /// secondary stress inside a compound (`myself` `M AY2 S EH1 L F`). Compounds only.
    by_sounds_loose: HashMap<Vec<&'a str>, Vec<&'a str>>,
}

impl<'a> Stems<'a> {
    fn index(setup: &'a Setup, entries: &[Entry], links: &'a JoinLinks<'a>) -> Self {
        // A standalone suffix's entry is a continuation shape (`ed` is `+-D`), never a stem. Words
        // are keyed by the pronunciation table's own strings, so the index borrows nothing from
        // `entries` and can be kept across rounds while they grow.
        let mut stems = Self {
            setup,
            links,
            best: HashMap::new(),
            outlines: HashMap::new(),
            plain: HashMap::new(),
            not_family: crate::family::not_family(),
            by_sounds: HashMap::new(),
            by_sounds_loose: HashMap::new(),
        };
        stems.update(entries, 0);
        stems
    }

    /// Takes the entries from `first_new` on into account: a word whose best entry changed joins
    /// through its new one from the next round.
    fn update(&mut self, entries: &[Entry], first_new: usize) {
        for (at, entry) in entries.iter().enumerate().skip(first_new).filter(|(_, e)| e.origin != Origin::Suffix) {
            if self.best.get(entry.word.as_str()).is_none_or(|&held| priority(entry) < priority(&entries[held])) {
                self.set(entries, at);
            }
        }
    }

    /// Makes `entries[at]` its word's best entry: the outlines the word joins through, and the
    /// word in the sound indexes the first time it is seen.
    fn set(&mut self, entries: &[Entry], at: usize) {
        let setup = self.setup;
        let Some((word, prons)) = setup.pron.get_key_value(entries[at].word.as_str()) else { return };
        let word = word.as_str();
        let sounds = prons.first();
        let mut own = vec![without_markers(&entries[at].outline)];
        if let Some(mut plain) = sounds.and_then(|s| setup.rules.outline(word, s)) {
            setup.rules.joined(&mut plain);
            if typable(&plain) {
                if plain.len() >= 2 && !own.contains(&plain) {
                    own.push(plain.clone());
                }
                self.plain.insert(word, plain);
            }
        }
        own.retain(|o| !o.is_empty());
        if self.best.insert(word, at).is_none()
            && let Some(sounds) = sounds
        {
            self.by_sounds.entry(sounds.as_slice()).or_default().push(word);
            self.by_sounds_loose.entry(stress_stripped(sounds)).or_default().push(word);
        }
        self.outlines.insert(word, own);
    }

    /// Every composition for `word`, finished: join keys set, typable, fragment-checked where the
    /// pieces are not already whole outlines, and each foldable one offered folded too.
    fn joins(&self, word: &str, first_round: bool) -> Vec<Join> {
        let Some(sounds) = self.setup.pron.get(word).and_then(|p| p.first()) else { return Vec::new() };
        let mut out: Vec<Join> = Vec::new();
        let mut push = |outline: Vec<Stroke>, sounds: Option<&[String]>| {
            let mut outline = outline;
            self.setup.rules.joined(&mut outline);
            if !typable(&outline) || sounds.is_some_and(|s| !self.setup.rules.matches_real_consonants(&outline, word, s)) {
                return;
            }
            let folded = folded(&outline).filter(|o| typable(o));
            for outline in std::iter::once(outline).chain(folded) {
                let text = render_outline(&outline);
                if !out.iter().any(|(_, _, t)| *t == text) {
                    out.push((word.to_owned(), outline, text));
                }
            }
        };
        // Pieces that are not whole outlines (a bare syllable, a folded-on cluster) are checked
        // against the word's sounds; whole outlines joined together need no check.
        for outline in self.suffixed(word, sounds) {
            push(outline, Some(sounds));
        }
        for outline in self.led(word, sounds) {
            push(outline, Some(sounds));
        }
        for outline in self.compounds(word, sounds) {
            push(outline, Some(sounds));
        }
        for (outline, family) in self.ended(word) {
            push(outline, (!family).then_some(sounds.as_slice()));
        }
        for outline in self.prefixed(word) {
            push(outline, None);
        }
        if first_round {
            for pron in &self.setup.pron[word] {
                for outline in self.setup.rules.syllable_paths(word, pron) {
                    push(outline, Some(pron));
                }
            }
        }
        out
    }

    /// The outlines of `stem`, or none when it is `word` itself or not placed.
    fn of(&self, stem: &str, word: &str) -> &[Vec<Stroke>] {
        if stem == word { &[] } else { self.outlines.get(stem).map_or(&[], Vec::as_slice) }
    }

    /// A stem plus a reduced trailing syllable's continuation stroke, and a stem found by spelling
    /// plus one of `rules/suffixes.toml`'s two-stroke suffixes (whose second syllable's onset can
    /// sit on the first stroke, so it is reused as written rather than re-derived).
    fn suffixed(&self, word: &str, sounds: &[String]) -> Vec<Vec<Stroke>> {
        let mut out = Vec::new();
        let rules = &self.setup.rules;
        if !self.not_family.contains(word)
            && let Some((split, bare)) = rules.suffix_split(word, sounds)
            && let Some(stem) = self.by_sounds.get(&sounds[..split]).and_then(|s| spelled_stem(word, s, |w, s| w.starts_with(s)))
        {
            out.extend(self.of(stem, word).iter().map(|o| [o.as_slice(), &[bare]].concat()));
        }
        if let Some((suffix, strokes)) = rules.two_stroke_suffix(word) {
            let stem = &word[..word.len() - suffix.len()];
            out.extend(self.of(stem, word).iter().map(|o| [o.as_slice(), strokes].concat()));
        }
        out
    }

    /// A leading consonant cluster folded onto a placed word's first stroke (`ever` -> `never`):
    /// the join key only continues forward, so the cluster has to land on that stroke, and it
    /// can't when they share a key.
    fn led(&self, word: &str, sounds: &[String]) -> Vec<Vec<Stroke>> {
        let mut out = Vec::new();
        for split in 1..sounds.len().min(4) {
            let Some(stem) = self.by_sounds.get(&sounds[split..]).and_then(|s| spelled_stem(word, s, |w, s| w.ends_with(s))) else {
                continue;
            };
            let Some(leading) = self.setup.rules.prefix_keys(word, sounds, split) else { continue };
            for outline in self.of(stem, word) {
                if let Some((first, rest)) = outline.split_first()
                    && leading.0 & first.0 == 0
                {
                    out.push([&[Stroke(leading.0 | first.0)], rest].concat());
                }
            }
        }
        out
    }

    /// Two placed words back to back, at every split where the word's spelling is exactly the two
    /// spellings joined (`myself` is `my` + `self`; `mice` + `elf` sounds the same but is not it).
    fn compounds(&self, word: &str, sounds: &[String]) -> Vec<Vec<Stroke>> {
        let mut out = Vec::new();
        if self.not_family.contains(word) {
            return out;
        }
        let loose = stress_stripped(sounds);
        for split in 1..loose.len() {
            let (Some(firsts), Some(seconds)) = (self.by_sounds_loose.get(&loose[..split]), self.by_sounds_loose.get(&loose[split..]))
            else {
                continue;
            };
            let (Some(first), Some(second)) =
                (spelled_stem(word, firsts, |w, s| w.starts_with(s)), spelled_stem(word, seconds, |w, s| w.ends_with(s)))
            else {
                continue;
            };
            if first.len() + second.len() != word.len() {
                continue;
            }
            for a in self.of(first, word) {
                out.extend(self.of(second, word).iter().map(|b| [a.as_slice(), b].concat()));
            }
        }
        out
    }

    /// Every family link's stem plus its ending's stroke (`call` + "-ed", `fool` + "-ish").
    /// Each with whether its ending is a family one (see `Link::is_family`).
    fn ended(&self, word: &str) -> Vec<(Vec<Stroke>, bool)> {
        let links = self.links.endings.get(word).map_or(&[][..], Vec::as_slice);
        links
            .iter()
            .flat_map(|link| {
                let stems: &[Vec<Stroke>] = if link.is_family() {
                    self.of(&link.stem, word)
                } else {
                    self.plain.get(link.stem.as_str()).filter(|_| link.stem != word).map_or(&[], std::slice::from_ref)
                };
                stems.iter().map(|o| ([o.as_slice(), &link.strokes].concat(), link.is_family()))
            })
            .collect()
    }

    /// The prefix's stroke, then the stem's outline (`un` + `clear` `UPB/+KHRAOER`).
    fn prefixed(&self, word: &str) -> Vec<Vec<Stroke>> {
        let Some(link) = self.links.prefixes.get(word) else { return Vec::new() };
        let head = match link.stroke {
            PrefixStroke::Fixed(text) => crate::stroke::parse_outline(text),
            PrefixStroke::Word(prefix) => self.outlines.get(prefix).and_then(|o| o.first().cloned()),
        };
        let Some(head) = head else { return Vec::new() };
        self.of(&link.stem, word).iter().map(|o| [head.as_slice(), o].concat()).collect()
    }
}

/// `outline` without its star-ladder marker strokes.
fn without_markers(outline: &[Stroke]) -> Vec<Stroke> {
    outline.iter().copied().filter(|s| !s.is_marker()).collect()
}

/// `outline` with its last stroke folded onto the one before, when that leaves two strokes or more
/// and the last stroke fits there (see [`Stroke::fold`]).
fn folded(outline: &[Stroke]) -> Option<Vec<Stroke>> {
    let [head @ .., before, last] = outline else { return None };
    if head.is_empty() {
        return None;
    }
    let merged = before.fold(*last)?;
    Some([head, &[merged]].concat())
}

/// `sounds` with every stress digit removed.
fn stress_stripped(sounds: &[String]) -> Vec<&str> {
    sounds.iter().map(|s| s.trim_end_matches(|c: char| c.is_ascii_digit())).collect()
}

/// Of the placed words that share the sounds `word` needs, the one whose spelling also sits at
/// the boundary (`building` starts with `build`, not `billed`); `None` when no single one does.
fn spelled_stem<'s>(word: &str, stems: &[&'s str], boundary: fn(&str, &str) -> bool) -> Option<&'s str> {
    if let [stem] = *stems { Some(stem) } else {
        let mut matches = stems.iter().copied().filter(|s| boundary(word, s));
        let first = matches.next()?;
        matches.next().is_none().then_some(first)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stroke::parse_outline;

    fn join(word: &str, text: &str) -> Join {
        (word.to_owned(), parse_outline(text).unwrap(), text.to_owned())
    }

    #[test]
    fn a_shared_text_goes_to_the_commoner_word_whatever_the_order() {
        let weight = |w: &str| match w {
            "spade" => 1_796,
            "spayed" => 76,
            _ => 0,
        };
        for joins in [
            vec![join("spade", "SPAEUD/+-S"), join("spayed", "SPAEUD/+-S")],
            vec![join("spayed", "SPAEUD/+-S"), join("spade", "SPAEUD/+-S")],
        ] {
            let won = arbitrate(&joins, weight);
            let winner: Vec<&str> = joins.iter().zip(&won).filter(|(_, w)| **w).map(|(j, _)| j.0.as_str()).collect();
            assert_eq!(winner, ["spade"]);
        }
        // A tie goes to the word that sorts first; a text nobody shares always wins.
        let won = arbitrate(&[join("b", "PW/+-S"), join("a", "PW/+-S"), join("c", "KR/+-S")], |_| 0);
        assert_eq!(won, [false, true, true]);
    }

    #[test]
    fn a_last_stroke_that_fits_on_the_one_before_is_also_offered_folded() {
        let text = |o: &str| folded(&parse_outline(o).unwrap()).map(|o| render_outline(&o));
        assert_eq!(text("SEPB/+SU/+-BL").as_deref(), Some("SEPB/+SUBL"));
        // Two strokes would fold to one: that is the one-stroke outlines' business, not a join's
        assert_eq!(text("SEPBS/+-BL"), None);
        // Out of steno order, or a key already down: no fold
        assert_eq!(text("A/+PWE/+-PB"), Some("A/+PWEPB".to_owned()));
        assert_eq!(text("A/+PWEL/+-PB"), None);
        assert_eq!(text("A/+PWEPB/+-PB"), None);
    }

    #[test]
    fn a_ladder_marker_never_ends_up_inside_a_join() {
        let o = parse_outline("SOR/+*").unwrap();
        assert_eq!(render_outline(&without_markers(&o)), "SOR");
    }
}

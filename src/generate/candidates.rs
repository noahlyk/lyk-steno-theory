//! Every word's candidate pool: every legal way to write its primary outline, from every source
//! at once (gold briefs from Plover and Lapwing, the rules reading, the stem-plus-suffix reading,
//! a spelling variant, brevity steps, the star ladder), each with the one cost every choice in
//! this theory is made on: strokes, then keys, then comfort. See `docs/collisions.md`.

use super::{Briefs, CONTRACTION_TAILS, KEY_WEIGHT, Origin, keys, split_untypable, typable};
use crate::analysis::Setup;
use crate::ergonomics;
use crate::stroke::{Stroke, parse_outline, render_outline};
use std::collections::{HashMap, HashSet};

/// One legal way to write a word's *primary* outline, considered on equal footing against every
/// other legal way regardless of where it came from - a gold brief, lyk's own plain rules reading,
/// a spelling-told-apart variant, a brevity-ladder compression, or a star-ladder step. See
/// [`enumerate_candidates`], which builds these, and `generate`'s own peeling/solver step, which
/// picks among them.
#[derive(Clone)]
pub struct Candidate {
    pub outline: Vec<Stroke>,
    pub origin: Origin,
    pub cost: u32,
}

pub const STROKE_WEIGHT: u32 = 25_000;

/// Intrinsic cost of an outline alone, independent of word or frequency: fewer strokes dominates,
/// fewer keys breaks ties within that, ergonomics only breaks ties within *that*. `STROKE_WEIGHT`
/// is 25 keys - more than the 24 keys one stroke can hold - so no word ever prefers a longer
/// outline for itself; it is finite, rather than effectively infinite, so that once frequency is
/// folded in, one extra stroke for a rare word is weighed against extra keys for a common one
/// instead of always winning (see `docs/briefs.md`). Ergonomics stays bounded under `KEY_WEIGHT`.
///
/// Frequency does NOT belong here - it is a property of a *word* wanting a candidate, not of the
/// candidate itself, reusable as-is by any other word considering the exact same outline. Frequency
/// only enters arbitration as the solver's own edge weight (`generate`'s own `edge_cost`), making
/// the global optimum the solver finds literally the same metric `build` already reports as "keys
/// per word, weighted by how often it is said" - the objective is the headline quality number
/// itself, not a proxy for it.
#[must_use]
pub fn candidate_cost(outline: &[Stroke]) -> u32 {
    let strokes = u32::try_from(outline.len()).unwrap_or(u32::MAX);
    STROKE_WEIGHT * strokes + KEY_WEIGHT * keys(outline) + ergonomics::weighted_outline(outline)
}

/// Every raw and drop-repaired candidate from one gold dictionary's own listed outlines for
/// `word`, in list order: the raw outline as itself, plus its drop-repair whenever
/// `drop_fragments` finds one to offer alongside it. A raw outline that only holds *part* of a
/// real key combination (`OB` for "on", missing half its real `-PB` coda) is still offered here,
/// not screened out - under the post-reversal philosophy (see `docs/decisions.md`'s "core
/// philosophy reversed" entry) a candidate holding no trace, or only part of a trace, of a word's
/// own sound is not disqualifying by itself, only its cost is what decides. An earlier version of
/// this function rejected any raw candidate `matches_real_consonants` flagged, which was the right
/// call under the old "every outline should be sound-derivable" philosophy but directly
/// contradicted the reversal once it landed - it cost `which` its own `KH` (confirmed: the user's
/// own, explicitly favorite example) and five other words a real key-count regression for no
/// benefit, since the thing it was guarding against (a result that doesn't sound like the word) is
/// no longer something this project avoids. Both the raw and the repaired candidate still compete
/// on cost like anything else - `drop_fragments`'s own, more aggressive strip (dropping the
/// fragment's bits entirely rather than keeping even the partial chord) remains worth offering
/// alongside the raw form, since it's sometimes shorter still (`make` -> `AEUBG`).
fn gold_candidates(
    rules: &crate::rules::Rules,
    outlines: Option<&Vec<String>>,
    word: &str,
    sounds: &[String],
) -> Vec<Vec<Stroke>> {
    let mut found = Vec::new();
    for text in outlines.into_iter().flatten() {
        let Some(raw) = parse_outline(text) else { continue };
        if let Some(repaired) = rules.drop_fragments(&raw, word, sounds) {
            found.push(repaired);
        }
        found.push(raw);
    }
    found
}

/// Every legal, fragment-safe primary-outline candidate for `word` - backups excluded (composing
/// from an already-placed word's own outline is inherently iterative, the opposite of this
/// function's whole point; see the backup-entry pass further down in `generate`, which is
/// untouched by this). A pure function: no `taken`, no mutable state, no dependence on any other
/// word's decision, so it can be called in any order, any number of times, for a unit test or for
/// the real solver alike. Multiple candidates may render to the same outline text; callers dedupe,
/// keeping the cheaper one (see `generate`'s own peeling step) - this never happens itself.
///
/// Sources merged into one list, every one gated exactly as the mechanism it reuses already gates
/// it:
/// 1. **Gold-brief candidates.** A `[word]` literal override in `rules/briefs.toml` still forces
///    exactly one candidate and nothing else - a human decision, not something to reopen to
///    competition even against this word's *own* rules-derived or brevity candidates, the same way
///    a briefed word kept exactly one entry before this existed. Short of a literal override,
///    *both* `setup.gold.plover` and `setup.gold.lapwing` are merged unconditionally (today only
///    one source is ever tried, gated by `briefs.default`/`briefs.word`'s `"plover"`/`"lapwing"`
///    selector - that gating only ever narrows *which* dictionaries to merge now, never whether a
///    brief competes at all). Every raw candidate competes regardless of whether it holds a real,
///    complete sound or only a fragment of one - see `gold_candidates`'s own doc for why a blanket
///    `matches_real_consonants` filter was tried here and reverted.
/// 2. **Rules-derived candidate**: `setup.rules.outline(word, sounds)` - lyk's own plain reading,
///    unconditionally, for every word with a brief or not. This is the piece missing from the old
///    brief pool entirely, and the whole reason lyk's own derivation used to never get a chance to
///    compete against a common word's gold brief.
/// 3. **Homophone resolution, folded into legality rather than cost**: `rules.variant(word,
///    sounds)`, exactly the same call the old pre-solver homophone pass made per word, added
///    alongside the plain rules candidate rather than replacing it. `variant` only ever returns
///    `Some` for the member of a sound-alike group whose own spelling needs telling apart from the
///    others (`billed` against `build`); the member that already matches the default convention
///    gets `None` and simply keeps competing on its plain reading alone. This answers "what text
///    may this word even propose," not "who should win it" - a homophone fight is then just an
///    ordinary contested-text fight the solver settles on cost and frequency weight, same as any
///    other collision.
/// 4. **Brevity-ladder candidates**: identical eligibility gate to before (`setup.rank[word] <
///    step.rank && first.len() > 1`), every `Shortening` from `setup.rules.brevity()`, every
///    `setup.rules.shorten(...)` result shorter than the full form, plus its two star-ladder
///    variants (`Origin::Short(step.step)`) - mirrors the old asymmetry where short forms get fewer
///    ladder steps than full outlines.
/// 5. **Full-outline ladder candidates**: the rules-derived outline *and* the spelling variant (if
///    it has one), each with all four `ladder(outline, join)` steps (`Origin::Ladder(n)`) - a
///    generalization of the old behavior, which only ever laddered a word that fell through to the
///    main loop, never one that had already claimed a spelling variant.
/// 6. **S-for-Z coda alternates** (`Origin::Voicing`): every candidate built by steps 1-5 whose
///    last stroke's coda is a voiced `-Z`, offered again with that stroke's `-Z` respelled `-S`,
///    gated on the word's own spelling ending in a literal `s` (a word spelled with `z`/`zz`
///    never qualifies, since it doesn't end in `s`). Brief, Rules, Short, and Ladder candidates all
///    derive their coda from the word's real pronunciation, which is phonetically correct but
///    leaves a cheaper `-S` reading unable to compete just because nothing ever proposed it at
///    that stroke length (`girls`'s phonetic `-Z` coda never raced against Plover's own `-S`
///    brief, because the brevity-ladder only shortens sound-derived candidates, never a raw brief
///    string - see `docs/decisions.md`). This step closes that gap the same way for every source
///    at once, by respelling whatever already won the coda rather than re-deriving it.
///
/// Every word is guaranteed at least one candidate (source 2) unless its own rules genuinely
/// cannot write it in a single stroke at all (a real, pre-existing failure mode -
/// `setup.rules.outline` already returns `None` for that today - not a new one introduced here); a
/// pathological word with neither a gold brief nor a rules outline has zero candidates and is
/// routed to `unresolved` by peeling, exactly as it would have been unresolved before.
pub fn enumerate_candidates(
    setup: &Setup,
    briefs: &Briefs,
    word: &str,
    simple: &HashSet<&str>,
    wanted: &HashSet<&str>,
) -> Vec<Candidate> {
    let Some(prons) = setup.pron.get(word) else { return Vec::new() };
    let Some(sounds) = prons.first() else { return Vec::new() };
    let mut pool = Pool { setup, word, sounds, out: Vec::new() };

    // 1. Gold briefs; a literal override is the one candidate and nothing else competes with it.
    match briefs.word.get(word).map(String::as_str) {
        Some("plover") => pool.briefs(setup.gold.plover.get(word)),
        Some("lapwing") => pool.briefs(setup.gold.lapwing.get(word)),
        Some(literal) => return pool.literal(literal),
        None => {
            pool.briefs(setup.gold.plover.get(word));
            pool.briefs(setup.gold.lapwing.get(word));
        }
    }
    // 2. The plain rules reading, every time, brief or not; 2b. the stem-plus-suffix readings.
    let base = setup.rules.outline(word, sounds);
    if let Some(outline) = &base {
        pool.push(outline.clone(), Origin::Rules);
    }
    pool.suffixed();
    // 3. The spelling variant that tells a homophone apart, beside the plain reading.
    let variant = setup.rules.variant(word, sounds);
    if let Some(outline) = &variant {
        pool.push(outline.clone(), Origin::Spelling);
    }
    // 4. Brevity steps, gated by rank.
    if let Some(first) = base.as_ref().filter(|b| b.len() > 1) {
        pool.shortened(prons, first.len(), simple, wanted);
    }
    // 5. The star ladder on the plain reading and on the spelling variant.
    for outline in [base.as_ref(), variant.as_ref()].into_iter().flatten() {
        for (n, laddered) in ladder(outline, setup.rules.join_key()) {
            pool.push(laddered, Origin::Ladder(n));
        }
    }
    // 6. S-for-Z coda alternates.
    for outline in voicing_alternates(word, &pool.out) {
        pool.push(outline, Origin::Voicing);
    }
    pool.out
}

/// One word's candidates as they are gathered.
struct Pool<'a> {
    setup: &'a Setup,
    word: &'a str,
    sounds: &'a [String],
    out: Vec<Candidate>,
}

impl Pool<'_> {
    /// Adds `outline`, join keys set. One that can't be typed as written is offered with one pinky
    /// key swapped for its partner (still one stroke) and with the offending pinky keys moved onto
    /// a continuation stroke: never just dropped.
    fn push(&mut self, mut outline: Vec<Stroke>, origin: Origin) {
        let rules = &self.setup.rules;
        rules.joined(&mut outline);
        if !typable(&outline) {
            if let [bad] = outline.iter().filter(|s| !s.typable()).collect::<Vec<_>>()[..] {
                let at = outline.iter().position(|s| s == bad).expect("it is in the outline");
                for swapped in bad.typable_swaps() {
                    let mut repaired = outline.clone();
                    repaired[at] = swapped;
                    let cost = candidate_cost(&repaired);
                    self.out.push(Candidate { outline: repaired, origin, cost });
                }
            }
            let Some(split) = split_untypable(&outline) else { return };
            outline = split;
            rules.joined(&mut outline);
        }
        let cost = candidate_cost(&outline);
        self.out.push(Candidate { outline, origin, cost });
    }

    /// A gold dictionary's outlines for the word, each raw and with its fragments dropped.
    fn briefs(&mut self, outlines: Option<&Vec<String>>) {
        for outline in gold_candidates(&self.setup.rules, outlines, self.word, self.sounds) {
            self.push(outline, Origin::Brief);
        }
    }

    /// A literal `[word]` outline: the user's own choice, so never filtered here (it is checked,
    /// and reported, where literals are placed). The word's only candidate.
    fn literal(mut self, literal: &str) -> Vec<Candidate> {
        if let Some(mut outline) = parse_outline(literal) {
            self.setup.rules.joined(&mut outline);
            let cost = candidate_cost(&outline);
            self.out.push(Candidate { outline, origin: Origin::Brief, cost });
        }
        self.out
    }

    /// The stem-plus-suffix readings: the trailing reduced syllable struck as its own continuation
    /// after the rules reading of the rest (`pearly` `PERL/+EU` when `PERL` is taken), up to two
    /// syllables peeled. Never fewer strokes than the plain reading, so it is the predictable way
    /// down when every one-stroke form is gone, ahead of a bare `/+*` or a two-stroke gold brief.
    fn suffixed(&mut self) {
        let rules = &self.setup.rules;
        let mut stem_sounds: Vec<String> = self.sounds.to_vec();
        let mut suffix_strokes: Vec<Stroke> = Vec::new();
        for _ in 0..2 {
            let Some((split, suffix)) = rules.suffix_split(self.word, &stem_sounds) else { break };
            if split == 0 {
                break;
            }
            suffix_strokes.insert(0, suffix);
            stem_sounds.truncate(split);
            if let Some(mut outline) = rules.outline(self.word, &stem_sounds) {
                outline.extend(suffix_strokes.iter().copied());
                self.push(outline, Origin::Rules);
            }
        }
    }

    /// Every brevity step the word's rank allows, over every pronunciation: each form shorter than
    /// the plain reading (`full` strokes) and its first two star rungs, unless a one-syllable word
    /// (`simple`) or any word (`wanted`) has that text by its plain rules.
    fn shortened(&mut self, prons: &[Vec<String>], full: usize, simple: &HashSet<&str>, wanted: &HashSet<&str>) {
        let setup = self.setup;
        let own = setup.rank.get(self.word).copied().unwrap_or(usize::MAX);
        for short in setup.rules.brevity().iter().filter(|s| own < s.rank) {
            let mut seen: HashSet<Vec<Stroke>> = HashSet::new();
            for sounds in prons {
                for outline in setup.rules.shorten(*short, self.word, sounds).into_iter().filter(|o| o.len() < full) {
                    let starred =
                        ladder(&outline, setup.rules.join_key()).into_iter().filter(|(n, _)| *n <= 2).map(|(_, o)| o);
                    for candidate in std::iter::once(outline.clone()).chain(starred) {
                        let text = render_outline(&candidate);
                        if !simple.contains(text.as_str()) && !wanted.contains(text.as_str()) && seen.insert(candidate.clone()) {
                            self.push(candidate, Origin::Short(short.step));
                        }
                    }
                }
            }
        }
    }
}

/// Every candidate in `out` whose last stroke's coda is a voiced `-Z`, respelled with that `-Z`
/// swapped for `-S` - offered only when `word`'s own spelling ends in a literal `s` (never `z` or
/// `zz`, which simply don't end in `s`). A word's own spelling outranks a re-derived sound for
/// safety, the same reasoning as the `-ism`/`-ify` match in `rules/suffixes.toml` (see
/// `docs/decisions.md`): `girls` strokes its coda as a voiced `-Z` (`TKPWEURLZ`) because that's how
/// it really sounds, which is phonetically correct but leaves the word's own cheaper `-S` spelling
/// unable to compete just because nothing had ever proposed it at that stroke length. This offers
/// it again, every time, so the two can compete on cost like any other candidate-origin ambiguity
/// instead of `-Z` winning by default.
fn voicing_alternates(word: &str, out: &[Candidate]) -> Vec<Vec<Stroke>> {
    const CODA_S: u32 = 1 << 20;
    const CODA_Z: u32 = 1 << 22;
    if !word.ends_with('s') {
        return Vec::new();
    }
    let mut seen: HashSet<Vec<Stroke>> = out.iter().map(|c| c.outline.clone()).collect();
    let mut additions: Vec<Vec<Stroke>> = Vec::new();
    for candidate in out {
        let Some(&last) = candidate.outline.last() else { continue };
        if last.0 & CODA_Z == 0 || last.0 & CODA_S != 0 {
            continue;
        }
        let mut respelled = candidate.outline.clone();
        let i = respelled.len() - 1;
        respelled[i] = Stroke((last.0 & !CODA_Z) | CODA_S);
        if seen.insert(respelled.clone()) {
            additions.push(respelled);
        }
    }
    additions
}

/// The other outlines to try for a word whose outline is taken, in the order they are tried.
pub(super) fn ladder(outline: &[Stroke], join: bool) -> Vec<(u8, Vec<Stroke>)> {
    let mut steps = Vec::new();
    let star = |outline: &[Stroke], at: usize| -> Option<Vec<Stroke>> {
        let stroke = outline.get(at)?;
        (stroke.0 & Stroke::STAR == 0).then(|| {
            let mut next = outline.to_vec();
            next[at] = Stroke(stroke.0 | Stroke::STAR);
            next
        })
    };
    if let Some(next) = star(outline, 0) {
        steps.push((1, next));
    }
    if outline.len() > 1
        && let Some(next) = star(outline, outline.len() - 1)
    {
        steps.push((2, next));
    }
    // A stroke of just the star after the word marks it as the other one
    let marker = Stroke(Stroke::STAR | if join { Stroke::JOIN } else { 0 });
    for (n, extra) in [(3u8, 1usize), (4, 2)] {
        let mut next = outline.to_vec();
        next.extend(std::iter::repeat_n(marker, extra));
        steps.push((n, next));
    }
    steps
}

/// Every word `generate` is willing to consider at all, most common first: a real pronunciation on
/// record, inside the requested frequency window, plain lowercase (apostrophes aside), with "a" the
/// only single-letter word and no bare contraction tail (`CONTRACTION_TAILS`). Factored out so a test can build the exact same `simple`/`wanted` sets `generate` does
/// (see [`group_outline_sets`]) without re-deriving this filter by hand.
pub(super) fn ranked_words(setup: &Setup, common: usize) -> Vec<(&String, usize)> {
    let mut words: Vec<(&String, usize)> = setup
        .rank
        .iter()
        .filter(|(w, r)| {
            **r < common
                && setup.pron.contains_key(w.as_str())
                && (w.len() >= 2 || w.as_str() == "a")
                && !CONTRACTION_TAILS.contains(&w.as_str())
                && w.bytes().all(|b| b.is_ascii_lowercase() || b == b'\'')
        })
        .map(|(w, r)| (w, *r))
        .collect();
    words.sort_by_key(|(w, r)| (*r, w.as_str()));
    words
}

/// The texts a one-syllable word's own plain rules would produce (`simple`) and the texts *any*
/// word's plain rules would produce (`wanted`) - the same two sets `generate` has always built
/// before settling a single word, now feeding [`enumerate_candidates`]'s own brevity-ladder gate
/// instead of a dedicated pre-pass: a brevity candidate is never offered an outline that one of
/// these texts already names, the same way a learner could already work either one out by the
/// plain rules without the brief being guessable at all.
pub(super) fn group_outline_sets(setup: &Setup, words: &[(&String, usize)]) -> (HashSet<String>, HashSet<String>) {
    let mut groups: HashMap<String, Vec<&str>> = HashMap::new();
    for (word, _) in words {
        if let Some(outline) = setup.pron[word.as_str()]
            .iter()
            .find_map(|sounds| setup.rules.outline(word, sounds))
        {
            groups.entry(render_outline(&outline)).or_default().push(word.as_str());
        }
    }
    let mut simple: HashSet<String> = HashSet::new();
    for (text, members) in &groups {
        let one_syllable = |w: &&str| {
            setup.pron[*w][0]
                .iter()
                .filter(|s| s.ends_with(|c: char| c.is_ascii_digit()))
                .count()
                == 1
        };
        if members.iter().any(one_syllable) {
            simple.insert(text.clone());
        }
    }
    let wanted: HashSet<String> = groups.keys().cloned().collect();
    (simple, wanted)
}

/// `enumerate` applied to every word, in order, split across the machine's cores: candidate
/// enumeration is a pure function of the word, so the result is identical to a sequential map.
pub(super) fn enumerate_in_parallel<F>(words: &[&str], enumerate: F) -> Vec<Vec<Candidate>>
where
    F: Fn(&str) -> Vec<Candidate> + Sync,
{
    let threads = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    let chunk = words.len().div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> = words
            .chunks(chunk)
            .map(|part| scope.spawn(|| part.iter().map(|w| enumerate(w)).collect::<Vec<_>>()))
            .collect();
        handles.into_iter().flat_map(|h| h.join().expect("a candidate-enumeration thread panicked")).collect()
    })
}

//! Every way to write a word as joined strokes: a lattice over its sounds.
//!
//! A word's sounds are positions `0..=n`; a stroke is an edge from one position to a later one,
//! covering the sounds between them with at most one vowel. Each edge has every chord that can write
//! its sounds (the vowel as it is heard or as it is spelled, a reduced syllable as a bare
//! continuation, consonants sharing keys: *m-p-r* is `PHR`, the *p* already down in `PH`). A path
//! from the first position to the last is one way to type the word, from two strokes up to one
//! sound per stroke, and the paths are listed cheapest first by a best-first search with an exact
//! lower bound, so asking for the best forty costs forty paths, not all of them.
//!
//! Cost says how natural a path is, never whether it is allowed: a stroke costs [`STROKE`], a cut
//! costs by where it falls (free at an ordinary syllable break, a little inside a consonant
//! cluster between syllables, more through a syllable), and a spelled vowel, a dropped vowel or a
//! bank other than the usual one costs a little. Every stroke after the first carries the join key
//! (set later, by `Rules::joined`). See `docs/syllables.md`.

use super::{Rules, Syllable};
use crate::stroke::Stroke;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

/// One stroke, before anything else about it.
const STROKE: u32 = 8;
/// The vowel written as the spelling gives it instead of as it is heard (`PRO`, not `PRU`).
const SPELLED_VOWEL: u32 = 1;
/// A reduced syllable struck as just its consonants, as a suffix is (`-BL`).
const DROPPED_VOWEL: u32 = 1;
/// Consonants alone on the left hand (`K`, `PHR`) instead of the right.
const LEFT_BANK: u32 = 1;
/// A cut between consonants of the cluster between two syllables that is not an ordinary break.
const CUT_CLUSTER: u32 = 2;
/// A cut that leaves a vowel to start a stroke after its consonants stayed behind.
const CUT_BEFORE_VOWEL: u32 = 3;
/// A cut inside a word's first onset or last coda, which is not a break between syllables.
const CUT_WITHIN: u32 = 4;
/// A cut between a syllable's onset and its vowel (`K` / `O`).
const CUT_ONSET_VOWEL: u32 = 5;
/// A sound struck out of steno order in its stroke, each: the keys are the same, the stroke just
/// reads less like the sounds as they were said.
const OUT_OF_ORDER: u32 = 3;
/// A bank that is not where the consonants belong: the right hand before a word's first vowel, the
/// left hand after its last.
const WRONG_BANK: u32 = 3;
/// A chord with the star in it, which belongs to the vowels and is better saved for telling words apart.
const STAR: u32 = 6;
/// A word's first syllable struck as a bare suffix-style continuation (`-BG` for *k-u*), which is no
/// way to start a word.
const DROPPED_FIRST: u32 = 8;
/// The most sounds one stroke may cover.
const MAX_SEGMENT: usize = 6;
/// The most strokes a path may have.
const MAX_STROKES: usize = 8;
/// The most consonants one vowelless stroke may cover.
const MAX_BARE: usize = 3;
/// How many partial paths the search for one stroke count may expand before it stops, a bound on
/// the odd word.
const MAX_EXPANSIONS: usize = 60_000;

/// One way to write a run of sounds as a stroke, and what it costs.
type Writing = (Stroke, u32);

/// A partial path in the search: the least it can still cost in all, what it has cost, where it
/// has got to, how many strokes are left, and its strokes.
type Partial = (u32, u32, usize, usize, Vec<Stroke>);

/// How many paths of each stroke count to list: two strokes, three, four, and so on.
pub type Quotas = [usize];

fn is_vowel(sound: &str) -> bool {
    sound.ends_with(|c: char| c.is_ascii_digit())
}

impl Rules {
    /// The cheapest ways to write `word` as two strokes or more, at most `quotas[k - 2]` of those
    /// with `k` strokes, all cheapest first: the strokes (no join key yet) and the path's cost.
    pub fn lattice_paths(&self, word: &str, sounds: &[String], quotas: &Quotas) -> Vec<(u32, Vec<Stroke>)> {
        let spelled = self.label(word, sounds);
        let heard: Vec<String> = spelled
            .iter()
            .map(|s| if matches!(super::unlabelled(s).as_str(), "AH0" | "IH0") { super::unlabelled(s) } else { s.clone() })
            .collect();
        let plain: Vec<String> = self
            .label_plain(word, sounds)
            .into_iter()
            .zip(&spelled)
            .zip(&heard)
            .map(|((plain, spelled), heard)| if plain == *spelled { heard.clone() } else { plain })
            .collect();
        let lattice = Lattice::new(self, &heard, &spelled, &plain);
        lattice.best_paths(quotas)
    }

    /// Every chord that writes the sounds `from..to` of `sounds` as one stroke.
    fn segment_writings(&self, sounds: &[String], from: usize, to: usize, bank: Bank) -> Vec<Writing> {
        let segment = &sounds[from..to];
        let mut out: Vec<Writing> = Vec::new();
        if let Some(v) = segment.iter().position(|s| is_vowel(s)) {
            let syllable = Syllable { onset: segment[..v].to_vec(), vowel: segment[v].clone(), coda: segment[v + 1..].to_vec() };
            if let Some((stroke, out_of_order)) = self.stroke_with(&syllable, false, true) {
                out.push((stroke, out_of_order * OUT_OF_ORDER));
            }
            if self.is_reduced(&syllable)
                && let Some(stroke) = self.continuation(&syllable)
            {
                out.push((stroke, if from == 0 { DROPPED_FIRST } else { DROPPED_VOWEL }));
            }
        } else {
            let (right, left) = match bank {
                Bank::Leading => (WRONG_BANK, 0),
                Bank::Trailing => (0, WRONG_BANK),
                Bank::Between => (0, LEFT_BANK),
            };
            if let Some((stroke, out_of_order)) = Self::consonants_shared(segment, &self.coda, &self.coda_cluster) {
                out.push((stroke, right + out_of_order * OUT_OF_ORDER));
            }
            if let Some((stroke, out_of_order)) = Self::consonants_shared(segment, &self.onset, &self.onset_cluster) {
                out.push((stroke, left + out_of_order * OUT_OF_ORDER));
            }
        }
        out.retain(|(stroke, _)| !stroke.keys().is_empty());
        for (stroke, cost) in &mut out {
            if stroke.0 & Stroke::STAR != 0 {
                *cost += STAR;
            }
        }
        out
    }
}

/// Which hand the consonants of a vowelless stroke belong on.
#[derive(Clone, Copy)]
enum Bank {
    /// Before the word's first vowel: the left hand
    Leading,
    /// After its last vowel: the right hand
    Trailing,
    /// Between vowels: either, the right (the syllable before) first
    Between,
}

/// A word's sounds as a graph of strokes.
struct Lattice {
    sounds: usize,
    /// Every stroke from each position: where it ends, the chord, and the cost of taking it,
    /// including the cut it makes at its end.
    edges: Vec<Vec<(usize, Stroke, u32)>>,
    /// `to_go[r][p]`: the least a path from position `p` to the end can still cost in exactly `r`
    /// more strokes (`u32::MAX` when there is none).
    to_go: Vec<Vec<u32>>,
}

impl Lattice {
    fn new(rules: &Rules, heard: &[String], spelled: &[String], plain: &[String]) -> Self {
        let n = heard.len();
        let cuts = cut_costs(rules, heard);
        let vowels: Vec<usize> = (0..n).filter(|&i| is_vowel(&heard[i])).collect();
        let (first, last) = (vowels.first().copied().unwrap_or(0), vowels.last().copied().unwrap_or(0));
        let mut edges: Vec<Vec<(usize, Stroke, u32)>> = vec![Vec::new(); n + 1];
        for from in 0..n {
            for to in from + 1..=n.min(from + MAX_SEGMENT) {
                let vowels = heard[from..to].iter().filter(|s| is_vowel(s)).count();
                if vowels > 1 || (vowels == 0 && to - from > MAX_BARE) {
                    continue;
                }
                let bank = if to <= first {
                    Bank::Leading
                } else if from > last {
                    Bank::Trailing
                } else {
                    Bank::Between
                };
                let mut writings: HashMap<Stroke, u32> = HashMap::new();
                for (list, extra) in [(heard, 0), (spelled, SPELLED_VOWEL), (plain, SPELLED_VOWEL)] {
                    for (stroke, cost) in rules.segment_writings(list, from, to, bank) {
                        let slot = writings.entry(stroke).or_insert(u32::MAX);
                        *slot = (*slot).min(cost + extra);
                    }
                }
                let cut = if to < n { cuts[to] } else { 0 };
                let mut writings: Vec<_> = writings.into_iter().collect();
                writings.sort_unstable();
                edges[from].extend(writings.into_iter().map(|(stroke, cost)| (to, stroke, STROKE + cost + cut)));
            }
        }
        let longest = n.min(MAX_STROKES);
        let mut to_go = vec![vec![u32::MAX; n + 1]; longest + 1];
        to_go[0][n] = 0;
        for r in 1..=longest {
            for from in 0..n {
                to_go[r][from] = edges[from]
                    .iter()
                    .filter(|(to, ..)| to_go[r - 1][*to] != u32::MAX)
                    .map(|&(to, _, cost)| cost + to_go[r - 1][to])
                    .min()
                    .unwrap_or(u32::MAX);
            }
        }
        Self { sounds: n, edges, to_go }
    }

    /// For each stroke count `k` (from 2), the `quotas[k - 2]` cheapest paths of exactly `k`
    /// strokes, all together cheapest first. Each count is its own best-first search with an exact
    /// bound, so the longer, dearer paths are found as cheaply as the short ones.
    fn best_paths(&self, quotas: &Quotas) -> Vec<(u32, Vec<Stroke>)> {
        let mut found: Vec<(u32, Vec<Stroke>)> = Vec::new();
        for (k, &quota) in (2..).zip(quotas) {
            if k >= self.to_go.len() || quota == 0 || self.to_go[k][0] == u32::MAX {
                continue;
            }
            // (cost so far + the least still to come, cost so far, position, strokes left, strokes so far)
            let mut open: BinaryHeap<Reverse<Partial>> = BinaryHeap::new();
            open.push(Reverse((self.to_go[k][0], 0, 0, k, Vec::new())));
            let (mut expanded, mut taken) = (0, 0);
            while let Some(Reverse((_, so_far, at, left, path))) = open.pop() {
                if left == 0 {
                    if at == self.sounds && !found.iter().any(|(_, p)| *p == path) {
                        found.push((so_far, path));
                        taken += 1;
                        if taken == quota {
                            break;
                        }
                    }
                    continue;
                }
                expanded += 1;
                if expanded > MAX_EXPANSIONS {
                    break;
                }
                for &(to, stroke, cost) in &self.edges[at] {
                    if self.to_go[left - 1][to] == u32::MAX {
                        continue;
                    }
                    let mut next = path.clone();
                    next.push(stroke);
                    open.push(Reverse((so_far + cost + self.to_go[left - 1][to], so_far + cost, to, left - 1, next)));
                }
            }
        }
        found.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.len().cmp(&b.1.len())));
        found
    }
}

/// What a cut before each sound costs, indexed by that sound's position (a cut at `p` leaves
/// sounds `..p` before it and `p..` after).
fn cut_costs(rules: &Rules, sounds: &[String]) -> Vec<u32> {
    let n = sounds.len();
    let vowels: Vec<usize> = (0..n).filter(|&i| is_vowel(&sounds[i])).collect();
    let mut ordinary = vec![false; n + 1];
    for keep_lax in [true, false] {
        if let Some(syllables) = rules.syllables_split(sounds, keep_lax, None) {
            let mut at = 0;
            for syllable in &syllables[..syllables.len() - 1] {
                at += syllable.onset.len() + 1 + syllable.coda.len();
                ordinary[at] = true;
            }
        }
    }
    let (Some(&first), Some(&last)) = (vowels.first(), vowels.last()) else { return vec![CUT_WITHIN; n + 1] };
    (0..=n)
        .map(|p| {
            if ordinary[p] {
                0
            } else if p == first {
                CUT_ONSET_VOWEL
            } else if p < first || p > last {
                CUT_WITHIN
            } else if vowels.contains(&p) {
                CUT_BEFORE_VOWEL
            } else {
                CUT_CLUSTER
            }
        })
        .collect()
}

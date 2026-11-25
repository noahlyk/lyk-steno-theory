//! The recorded split of a generated outline: which keys wrote which of its word's sounds, as the
//! rules wrote them ([`crate::rules::Group`]), carried through every step that changes an outline
//! after it is written (the join key, the star ladder, the `-S` respelling, a pinky swap, an
//! untypable stroke struck as two) and composed for the outlines made of other outlines (a family
//! shape, a backup join, a merged phrase). The short notation is written from it
//! (`crate::notation`); see `site/reference/notation.json`.

use crate::rules::{Group, Split};
use crate::stroke::{Stroke, render_outline};
use std::collections::HashMap;

/// The keys a split never names: the star (a ladder step or a marker), the join key, the
/// spelling key and the number bar.
const IGNORED: u32 = Stroke::STAR | Stroke::JOIN | Stroke::SPELL | 1;

/// A stroke's keys a split must name.
const fn named(stroke: Stroke) -> u32 {
    stroke.0 & !IGNORED
}

/// The pinky swaps a stroke may be written with (`Stroke::typable_swaps`, the `-S` respelling):
/// the key, and the key it moves to.
const SWAPS: [(u32, u32); 4] = [(1 << 19, 1 << 21), (1 << 21, 1 << 19), (1 << 20, 1 << 22), (1 << 22, 1 << 20)];

/// Whether `split` names every key of `outline` (but the [`IGNORED`] ones), each key once, stroke
/// for stroke.
#[must_use]
pub fn valid(outline: &[Stroke], split: &Split) -> bool {
    outline.len() == split.len()
        && outline.iter().zip(split).all(|(stroke, groups)| {
            let mut seen = 0u32;
            for (keys, _) in groups {
                if *keys & seen != 0 {
                    // A key two groups share (the `R` a vowel and a coda chord both hold, a prefix
                    // key the onset has too) is named once, by the first
                    if *keys & !seen == 0 {
                        continue;
                    }
                }
                seen |= keys;
            }
            seen == named(*stroke)
        })
}

/// `groups` with each key that moved from `from` to `to` by a pinky swap renamed; `None` if the
/// strokes differ in any other way.
fn swapped(from: Stroke, groups: &[Group], to: Stroke) -> Option<Vec<Group>> {
    let (a, b) = (named(from), named(to));
    if a == b {
        return Some(groups.to_vec());
    }
    let mut map: Vec<(u32, u32)> = Vec::new();
    for (key, partner) in SWAPS {
        if a & key != 0 && b & key == 0 && b & partner != 0 && a & partner == 0 {
            map.push((key, partner));
        }
    }
    let renamed = |keys: u32| map.iter().fold(keys, |k, (key, partner)| if keys & key != 0 { (k & !key) | partner } else { k });
    let out: Vec<Group> = groups.iter().map(|(keys, sounds)| (renamed(*keys), sounds.clone())).collect();
    (out.iter().fold(0, |all, (keys, _)| all | keys) == b).then_some(out)
}

/// The split of `to`, an outline made from `from` (whose split is `split`) by any of the steps
/// that keep its sounds: the join key, the star, a pinky swap or the `-S` respelling, an
/// untypable stroke struck as two, marker strokes after it. `None` when `to` is not such a step
/// of `from`.
#[must_use]
pub fn carry(from: &[Stroke], split: &Split, to: &[Stroke]) -> Option<Split> {
    if from.len() != split.len() {
        return None;
    }
    let mut out: Split = Vec::with_capacity(to.len());
    let mut j = 0;
    for (stroke, groups) in from.iter().zip(split) {
        let next = *to.get(j)?;
        if let Some(groups) = swapped(*stroke, groups, next) {
            out.push(groups);
            j += 1;
            continue;
        }
        // Struck as two: every group on one side or the other
        let after = *to.get(j + 1)?;
        let (a, b) = (named(next), named(after));
        if a & b != 0 || a | b != named(*stroke) {
            return None;
        }
        let first: Vec<Group> = groups.iter().filter(|(keys, _)| keys & !a == 0).cloned().collect();
        let second: Vec<Group> = groups.iter().filter(|(keys, _)| keys & !b == 0).cloned().collect();
        if first.len() + second.len() != groups.len() {
            return None;
        }
        out.push(first);
        out.push(second);
        j += 2;
    }
    // Marker strokes after the word (`/+*`) name no keys
    for stroke in &to[j..] {
        if named(*stroke) != 0 {
            return None;
        }
        out.push(Vec::new());
    }
    valid(to, &out).then_some(out)
}

/// Every entry's recorded split but the backups' (composed later, see `crate::notation`), by the
/// entry's text: the candidate it was settled from ([`super::candidates::Candidate::split`]),
/// `recorded` (what a pass wrote as it placed an entry: another pronunciation), a standalone suffix's
/// chord as one group, a family shape as its root's split with the ending's keys, a merged phrase
/// as its two words' splits side by side. An entry with none (a gold brief, a grammar phrase) is
/// left out.
#[must_use]
pub fn record(
    setup: &crate::analysis::Setup,
    pools: &super::settle::Pools,
    links: &HashMap<String, crate::family::Link>,
    entries: &[super::Entry],
    recorded: &HashMap<String, Split>,
) -> HashMap<String, Split> {
    use super::Origin;
    let mut out: HashMap<String, Split> = recorded.clone();
    let mut from_pools: HashMap<(&str, &str), &Split> = HashMap::new();
    for (i, (word, candidates)) in pools.iter().enumerate() {
        for (candidate, text) in candidates.iter().zip(&pools.texts[i]) {
            if let Some(split) = &candidate.split {
                from_pools.entry((word, text.as_str())).or_insert(split);
            }
        }
    }
    let mut by_word_pools: HashMap<&str, Vec<(&[Stroke], &Split)>> = HashMap::new();
    for (word, candidates) in pools.iter() {
        for candidate in candidates {
            if let Some(split) = &candidate.split {
                by_word_pools.entry(word).or_default().push((&candidate.outline, split));
            }
        }
    }
    let texts: Vec<String> = entries.iter().map(|e| render_outline(&e.outline)).collect();
    for (entry, text) in entries.iter().zip(&texts) {
        if entry.origin == Origin::Backup || out.contains_key(text) {
            continue;
        }
        if entry.origin == Origin::Suffix {
            // A suffix chord stands for the ending as a whole
            out.insert(text.clone(), entry.outline.iter().map(|s| vec![(named(*s), Vec::new())]).collect());
            continue;
        }
        if let Some(split) = from_pools.get(&(entry.word.as_str(), text.as_str())) {
            out.insert(text.clone(), (*split).clone());
            continue;
        }
        // An outline the rules did not write (a gold brief) whose keys are one the rules wrote
        // for the word by a step that keeps its sounds: Plover's `SAOEPLS` is the rules'
        // `SAOEPLZ` with the `-Z` respelled `-S`
        let derived = by_word_pools.get(entry.word.as_str()).into_iter().flatten().find_map(|(from, split)| carry(from, split, &entry.outline));
        if let Some(split) = derived {
            out.insert(text.clone(), split);
        }
    }
    // The compositions, from the splits found so far: each word's entries with one
    let mut by_word: HashMap<&str, Vec<(&[Stroke], Split)>> = HashMap::new();
    for (entry, text) in entries.iter().zip(&texts) {
        if let Some(split) = out.get(text) {
            by_word.entry(entry.word.as_str()).or_default().push((&entry.outline, split.clone()));
        }
    }
    for (entry, text) in entries.iter().zip(&texts) {
        if entry.origin == Origin::Backup || out.contains_key(text) {
            continue;
        }
        let found = match entry.origin {
            Origin::Phrase => phrase(&entry.word, &entry.outline, &by_word),
            _ => family(setup, links, &entry.word, &entry.outline, &by_word),
        };
        if let Some(split) = found {
            out.insert(text.clone(), split);
        }
    }
    out
}

/// A family member's shape (`builds` `PWHR-Z`): its root's split, the last stroke's groups kept
/// to the keys still down, and the ending's keys as one group of the sounds the member adds.
fn family(
    setup: &crate::analysis::Setup,
    links: &HashMap<String, crate::family::Link>,
    word: &str,
    outline: &[Stroke],
    by_word: &HashMap<&str, Vec<(&[Stroke], Split)>>,
) -> Option<Split> {
    let (root, keys) = crate::family::chain(links, word)?;
    let sounds = ending_sounds(setup, root, word);
    by_word.get(root)?.iter().find_map(|(root_outline, split)| {
        crate::family::shapes(root_outline, keys).iter().any(|s| s.as_slice() == outline).then_some(())?;
        let (last, stroke) = (split.len().checked_sub(1)?, *outline.last()?);
        let mut out = split.clone();
        let down = named(stroke);
        let mut groups: Vec<Group> = out[last].iter().filter(|(k, _)| k & down != 0).map(|(k, s)| (k & down, s.clone())).collect();
        let covered = groups.iter().fold(0, |a, (k, _)| a | k);
        if down & !covered != 0 {
            groups.push((down & !covered, sounds.clone()));
        }
        out[last] = groups;
        valid(outline, &out).then_some(out)
    })
}

/// The sounds `word` says after its root's (stress aside): `builds` after `build` is *z*.
fn ending_sounds(setup: &crate::analysis::Setup, root: &str, word: &str) -> Vec<String> {
    let bare = |s: &String| s.trim_end_matches(|c: char| c.is_ascii_digit()).to_owned();
    let (Some(r), Some(w)) = (setup.pron.get(root).and_then(|p| p.first()), setup.pron.get(word).and_then(|p| p.first())) else {
        return Vec::new();
    };
    if w.len() > r.len() && r.iter().zip(w).all(|(a, b)| bare(a) == bare(b)) { w[r.len()..].to_vec() } else { Vec::new() }
}

/// A merged phrase (`of the`): its two words' entries struck together, each stroke the two
/// strokes' keys (none shared), so its groups are both words' groups.
fn phrase(text: &str, outline: &[Stroke], by_word: &HashMap<&str, Vec<(&[Stroke], Split)>>) -> Option<Split> {
    let (w1, w2) = text.split_once(' ')?;
    let (first, second) = (by_word.get(super::entry_word(w1))?, by_word.get(super::entry_word(w2))?);
    for (a, sa) in first {
        for (b, sb) in second {
            if a.len() != outline.len() || b.len() != outline.len() {
                continue;
            }
            let fits = a.iter().zip(*b).zip(outline).all(|((x, y), z)| named(*x) & named(*y) == 0 && named(*x) | named(*y) == named(*z));
            if fits {
                let split: Split = sa.iter().zip(sb).map(|(ga, gb)| ga.iter().chain(gb).cloned().collect()).collect();
                if valid(outline, &split) {
                    return Some(split);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stroke::parse_outline;

    fn g(keys: &str, sounds: &[&str]) -> Group {
        let stroke = Stroke::parse(keys).expect("keys");
        (stroke.0, sounds.iter().map(|s| (*s).to_owned()).collect())
    }

    #[test]
    fn a_respelled_s_keeps_its_sound_and_the_star_and_markers_name_nothing() {
        let from = parse_outline("SAOEPLZ").unwrap();
        let split = vec![vec![g("S", &["S"]), g("AOE", &["IY1"]), g("-PL", &["M"]), g("-Z", &["Z"])]];
        let to = parse_outline("SAOEPLS").unwrap();
        let carried = carry(&from, &split, &to).expect("the -S respelling carries");
        assert_eq!(carried[0][3], g("-S", &["Z"]));
        let starred = parse_outline("SAO*EPLZ/+*").unwrap();
        let carried = carry(&from, &split, &starred).expect("the ladder carries");
        assert_eq!(carried[1], Vec::<Group>::new());
        assert!(carry(&from, &split, &parse_outline("SAOEPL").unwrap()).is_none());
    }
}

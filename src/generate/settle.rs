//! Settling every word's primary outline: each word gets exactly one of its candidates, no two
//! words the same text, at the least total cost weighted by how often each word is said.
//!
//! A pass settles in two steps. **Peeling** settles every word whose cheapest candidate's text no
//! other unsettled word has anywhere in its list: it can't cost anyone anything. That never takes
//! a text another unsettled word holds, so the lists left behind never change and one worklist
//! pass over how many words hold each text reaches the fixed point. What is left, the contested
//! words, splits into connected components (words linked by a shared text), each solved exactly
//! by min-cost matching ([`super::solver`]); components can't affect each other.
//!
//! Passes repeat over **family offers** (see `docs/syllables.md`): after the first, every family
//! member is offered its root's settled outline with its endings' keys (`Origin::Family`), every
//! root is held to its first-pass outline (moving only to something strictly cheaper), and the
//! whole dictionary is settled again from the same start, until the offers stop changing. Then
//! the `LOCK_TOP` commonest words are locked exactly and one final pass runs with every rarer
//! word's skeleton short form in (see `docs/brevity.md`). Last, any word on a worse outline than
//! one of its own candidates nobody holds moves to it.

use super::candidates::{Candidate, candidate_cost};
use super::solver::{no_match_cost, solve_contested};
use super::{Book, Entry, FAMILY_DISCOUNT, Origin, typable};
use crate::analysis::Setup;
use crate::family::{Link, PrefixLink};
use crate::rules::Step;
use crate::stroke::{Stroke, render_outline};
use std::collections::{HashMap, HashSet};

/// Settling passes at most. Family roots are held after the first, so the offers settle within a
/// pass or two after that.
const MAX_FAMILY_PASSES: usize = 4;
/// The commonest words are locked to their settled outlines before the final settling pass, the
/// one in which every word past `SKELETON_LATE_RANK` gets its one-stroke skeleton.
const LOCK_TOP: usize = 5_000;
/// A word ranked past this gets its `skeleton` short form only in the final pass, once the
/// `LOCK_TOP` commonest words are locked, so the earlier passes settle as they did before every
/// word had one.
const SKELETON_LATE_RANK: usize = 8_000;

/// Every word that competes for a primary, with its candidates (enumerated once) and each
/// candidate's text (rendered once).
pub struct Pools<'a> {
    pub words: Vec<&'a str>,
    pub ranks: Vec<usize>,
    pub candidates: Vec<Vec<Candidate>>,
    pub texts: Vec<Vec<String>>,
}

impl<'a> Pools<'a> {
    pub fn new(words: Vec<(&'a str, usize)>, candidates: Vec<Vec<Candidate>>) -> Self {
        let texts = candidates.iter().map(|list| list.iter().map(|c| render_outline(&c.outline)).collect()).collect();
        let (words, ranks) = words.into_iter().unzip();
        Self { words, ranks, candidates, texts }
    }

    /// Each word with its candidates.
    pub fn iter(&self) -> impl Iterator<Item = (&'a str, &[Candidate])> {
        self.words.iter().copied().zip(self.candidates.iter().map(Vec::as_slice))
    }
}

/// The family links that shape the offers: see `crate::family`.
pub struct Families<'a> {
    pub links: &'a HashMap<String, Link>,
    pub prefixes: &'a HashMap<String, PrefixLink>,
}

/// What settling leaves besides the entries: the last pass's family offers (the extras pass offers
/// them again) and how many words the solver had to arbitrate in it.
pub struct Settled {
    pub offers: HashMap<String, Vec<Vec<Stroke>>>,
    pub contested: usize,
}

/// A text and the cost it was settled at: a family root's or a locked word's hold on its outline.
type Hold = (String, u32);

/// The holds a pass settles under: see the module doc.
#[derive(Default)]
struct Holds {
    roots: HashMap<String, Hold>,
    locked: HashMap<String, Hold>,
    /// Every held text: nobody but its holder may take one.
    texts: HashSet<String>,
    /// The last pass: the commonest words are locked and every skeleton competes.
    last: bool,
}

pub fn settle(setup: &Setup, pools: &Pools, families: &Families, book: &mut Book) -> Settled {
    let base = book.mark();
    let rank_of: HashMap<&str, usize> = pools.words.iter().copied().zip(pools.ranks.iter().copied()).collect();
    let mut holds = Holds::default();
    let mut offers: HashMap<String, Vec<Vec<Stroke>>> = HashMap::new();
    let mut contested = 0;
    for pass in 0..MAX_FAMILY_PASSES {
        if pass > 0 {
            let settled: HashMap<&str, &[Stroke]> =
                book.entries[base.entries..].iter().map(|e| (e.word.as_str(), e.outline.as_slice())).collect();
            let hold = |o: &[Stroke]| (render_outline(o), candidate_cost(o));
            let roots: HashMap<String, Hold> = families
                .links
                .keys()
                .filter_map(|w| crate::family::chain(families.links, w))
                .filter_map(|(root, _)| settled.get(root).map(|o| (root.to_owned(), hold(o))))
                .collect();
            let next = family_offers(families, &book.entries);
            if holds.last {
                break;
            }
            if (next == offers && roots == holds.roots) || pass + 1 == MAX_FAMILY_PASSES {
                holds.last = true;
                holds.locked = settled
                    .iter()
                    .filter(|(w, _)| rank_of.get(*w).is_some_and(|&r| r < LOCK_TOP))
                    .map(|(w, o)| ((*w).to_owned(), hold(o)))
                    .collect();
            }
            holds.roots = roots;
            holds.texts = holds.roots.values().chain(holds.locked.values()).map(|(text, _)| text.clone()).collect();
            offers = next;
            book.reset(&base);
        }
        let open = open_words(pools, &offers, &holds, book);
        contested = place(setup, open, book);
    }
    unstrand(pools, base.entries, book);
    Settled { offers, contested }
}

/// For every linked family member, every [`crate::family::shapes`] of its root's settled outline
/// with its endings' keys (`build` `PWHR`: `builds` `PWHR-Z`, `building` `PWHR-G`), and for every
/// prefixed word its prefix's stroke then its stem's outline (`rebuild` `RE/+PWHR`). `entries`
/// must hold every settled primary.
fn family_offers(families: &Families, entries: &[Entry]) -> HashMap<String, Vec<Vec<Stroke>>> {
    let primary: HashMap<&str, &[Stroke]> =
        entries.iter().filter(|e| e.origin != Origin::Suffix).map(|e| (e.word.as_str(), e.outline.as_slice())).collect();
    let mut out: HashMap<String, Vec<Vec<Stroke>>> = HashMap::new();
    for word in families.links.keys() {
        let Some((root, keys)) = crate::family::chain(families.links, word) else { continue };
        let Some(root_outline) = primary.get(root) else { continue };
        let mut shapes = crate::family::shapes(root_outline, keys);
        shapes.retain(|shape| typable(shape));
        if !shapes.is_empty() {
            out.insert(word.clone(), shapes);
        }
    }
    for (word, link) in families.prefixes {
        let Some(stem) = primary.get(link.stem.as_str()) else { continue };
        if let Some(outline) = crate::family::prefixed(link, stem, &primary).filter(|o| typable(o)) {
            out.entry(word.clone()).or_default().push(outline);
        }
    }
    out
}

/// A word still to settle: its candidates that are still possible, cheapest first (ties by
/// text), each with its text.
struct Open<'a> {
    word: &'a str,
    options: Vec<(String, Candidate)>,
}

/// Every word's live candidates for this pass: its own and its family offers (counted
/// `FAMILY_DISCOUNT` cheaper), minus every text already taken or held for someone else, one per
/// text. A word with none left is unresolved.
fn open_words<'a>(pools: &Pools<'a>, offers: &HashMap<String, Vec<Vec<Stroke>>>, holds: &Holds, book: &mut Book) -> Vec<Open<'a>> {
    let mut open = Vec::with_capacity(pools.words.len());
    for (i, (word, candidates)) in pools.iter().enumerate() {
        let family = offers.get(word).into_iter().flatten().map(|outline| {
            let candidate =
                Candidate { outline: outline.clone(), origin: Origin::Family, cost: candidate_cost(outline).saturating_sub(FAMILY_DISCOUNT) };
            (render_outline(outline), candidate)
        });
        let own = pools.texts[i].iter().cloned().zip(candidates.iter().cloned());
        // A locked word keeps exactly its settled outline; a root keeps its first-pass outline but
        // may still move to something strictly cheaper. Neither may take another's held text.
        let exact = holds.locked.contains_key(word);
        let pinned = holds.locked.get(word).or_else(|| holds.roots.get(word));
        let late = !holds.last && pools.ranks[i] >= SKELETON_LATE_RANK;
        let mut options: Vec<(String, Candidate)> = Vec::new();
        let mut at: HashMap<String, usize> = HashMap::new();
        for (text, candidate) in own.chain(family) {
            if late && candidate.origin == Origin::Short(Step::Skeleton) {
                continue;
            }
            let barred = match pinned {
                Some((held, cost)) => *held != text && (exact || candidate.cost >= *cost || holds.texts.contains(&text)),
                None => holds.texts.contains(&text),
            };
            if barred || book.taken.contains_key(&text) {
                continue;
            }
            match at.get(&text) {
                Some(&j) if options[j].1.cost <= candidate.cost => {}
                Some(&j) => options[j].1 = candidate,
                None => {
                    at.insert(text.clone(), options.len());
                    options.push((text, candidate));
                }
            }
        }
        if options.is_empty() {
            book.unresolved.push(word.to_owned());
            continue;
        }
        options.sort_by(|(ta, a), (tb, b)| a.cost.cmp(&b.cost).then_with(|| ta.cmp(tb)));
        open.push(Open { word, options });
    }
    open
}

/// Settles every open word into `book`: peeling, then the solver per component. Returns how many
/// words the solver had to arbitrate.
fn place(setup: &Setup, open: Vec<Open>, book: &mut Book) -> usize {
    // Every text as a dense id, every word's options as ids.
    let mut ids: HashMap<&str, u32> = HashMap::new();
    let lists: Vec<Vec<u32>> = open
        .iter()
        .map(|o| {
            o.options
                .iter()
                .map(|(text, _)| {
                    let next = u32::try_from(ids.len()).expect("fewer than 2^32 texts");
                    *ids.entry(text.as_str()).or_insert(next)
                })
                .collect()
        })
        .collect();
    let texts = ids.len();
    drop(ids);

    // Who holds each text, and how many unsettled words still do.
    let mut holders: Vec<Vec<u32>> = vec![Vec::new(); texts];
    for (w, list) in lists.iter().enumerate() {
        for &t in list {
            holders[t as usize].push(u32::try_from(w).expect("fewer than 2^32 words"));
        }
    }
    let mut count: Vec<u32> = holders.iter().map(|h| u32::try_from(h.len()).expect("fewer than 2^32 words")).collect();
    let mut settled = vec![false; open.len()];
    let mut queue: Vec<usize> = (0..open.len()).filter(|&w| count[lists[w][0] as usize] == 1).collect();
    while let Some(w) = queue.pop() {
        if settled[w] {
            continue;
        }
        settled[w] = true;
        for &t in &lists[w] {
            count[t as usize] -= 1;
            if count[t as usize] == 1
                && let Some(&other) = holders[t as usize].iter().find(|&&o| !settled[o as usize])
                && lists[other as usize][0] == t
            {
                queue.push(other as usize);
            }
        }
    }

    // Components of what is left, linked by shared texts.
    let mut parent: Vec<usize> = (0..open.len()).collect();
    fn find(parent: &mut [usize], mut x: usize) -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    }
    for holders in &holders {
        let mut live = holders.iter().map(|&w| w as usize).filter(|&w| !settled[w]);
        if let Some(first) = live.next() {
            for other in live {
                let (a, b) = (find(&mut parent, first), find(&mut parent, other));
                parent[a] = b;
            }
        }
    }
    let mut components: HashMap<usize, Vec<usize>> = HashMap::new();
    for w in (0..open.len()).filter(|&w| !settled[w]) {
        components.entry(find(&mut parent, w)).or_default().push(w);
    }
    let contested = components.values().map(Vec::len).sum();

    let mut open: Vec<Option<Open>> = open.into_iter().map(Some).collect();
    for (w, slot) in open.iter_mut().enumerate() {
        if settled[w] {
            let Open { word, mut options } = slot.take().expect("each word is settled once");
            let (text, candidate) = options.swap_remove(0);
            book.place(word, candidate.outline, candidate.origin, text);
        }
    }
    let mut components: Vec<Vec<usize>> = components.into_values().collect();
    for component in &mut components {
        component.sort_unstable_by_key(|&w| open[w].as_ref().map(|o| o.word));
    }
    components.sort_unstable_by_key(|c| open[c[0]].as_ref().map(|o| o.word));
    for component in components {
        let mut no_match = Vec::with_capacity(component.len());
        let contested: Vec<(&str, Vec<(Candidate, u64)>)> = component
            .iter()
            .map(|&w| {
                let o = open[w].take().expect("each word is in one component");
                let weight = setup.count.get(o.word).copied().unwrap_or(0);
                let candidates: Vec<Candidate> = o.options.into_iter().map(|(_, c)| c).collect();
                no_match.push(no_match_cost(o.word, weight, &candidates));
                (o.word, candidates.into_iter().map(|c| { let edge = weight * u64::from(c.cost); (c, edge) }).collect())
            })
            .collect();
        let assignment = solve_contested(&contested, &no_match);
        for (word, candidates) in contested {
            // Missing only when one component has more mutually exclusive words than texts, even
            // after the solver's own "leave out whoever loses least" fallback: unresolved.
            let Some(&index) = assignment.get(word) else {
                book.unresolved.push(word.to_owned());
                continue;
            };
            let candidate = candidates.into_iter().nth(index).expect("a valid candidate index").0;
            let text = render_outline(&candidate.outline);
            book.place(word, candidate.outline, candidate.origin, text);
        }
    }
    contested
}

/// A word whose primary costs more than one of its own candidates nobody holds moves to the
/// cheapest such candidate. The last pass reserves every locked word's and root's text for its
/// owner even when the owner then moves to something cheaper, which can leave that text free but
/// unreachable (*spectre* on `SPEBGT/+*R` with `SPEBGT/+-R` free). Moving to an unclaimed text
/// costs nobody anything; in frequency order, so a text one move frees is there for the next.
fn unstrand(pools: &Pools, base: usize, book: &mut Book) {
    let index: HashMap<&str, usize> =
        book.entries.iter().enumerate().skip(base).map(|(i, entry)| (entry.word.as_str(), i)).collect();
    let moves: Vec<(usize, usize)> =
        pools.iter().enumerate().filter_map(|(p, (word, _))| index.get(word).map(|&i| (p, i))).collect();
    for (p, i) in moves {
        let current = candidate_cost(&book.entries[i].outline);
        let better = pools.candidates[p]
            .iter()
            .zip(&pools.texts[p])
            .filter(|(c, text)| c.cost < current && !book.taken.contains_key(*text))
            .min_by(|(a, ta), (b, tb)| a.cost.cmp(&b.cost).then_with(|| ta.cmp(tb)));
        if let Some((better, text)) = better {
            let word = book.entries[i].word.clone();
            book.taken.remove(&render_outline(&book.entries[i].outline));
            book.taken.insert(text.clone(), word);
            book.entries[i].outline.clone_from(&better.outline);
            book.entries[i].origin = better.origin;
        }
    }
}

//! Min-cost bipartite matching for the words peeling could not settle: see `settle`.

use super::STROKE_WEIGHT;
use super::candidates::Candidate;
use crate::stroke::render_outline;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

/// What it costs a word, in `edge_cost` units, to end up with no primary outline at all: it is
/// then typed by fingerspelling, one stroke per letter, so that is what this charges - never less
/// than one stroke worse than the word's own worst real candidate, so the solver only ever leaves
/// a word unplaced when every real option is already gone. Comparable to every other edge cost on
/// purpose: a rare word losing its last slot is weighed against a common word's extra stroke
/// honestly, rather than treated as a near-hard constraint (see `site/reference/collisions.json`).
pub(super) fn no_match_cost(word: &str, weight: u64, candidates: &[Candidate]) -> u64 {
    let worst = candidates.iter().map(|c| u64::from(c.cost)).max().unwrap_or(0);
    let letters = u64::try_from(word.chars().count()).unwrap_or(u64::MAX);
    let spelled = letters.saturating_mul(u64::from(STROKE_WEIGHT));
    weight.saturating_mul(spelled.max(worst + u64::from(STROKE_WEIGHT)))
}

/// A min-cost bipartite assignment over the contested subgraph only: words with edges to a shared
/// pool of outline-text nodes, each usable by at most one word. Solved by successive shortest
/// augmenting paths (hand-rolled, not dense Hungarian - O(n^3) is infeasible and wasteful for a
/// sparse graph here - and not a crate: every edge cost is non-negative by construction, so
/// repeated Dijkstra with Johnson-style potentials, initialized to zero, needs no Bellman-Ford
/// seeding pass; `Cargo.toml` has five dependencies today, none a graph library, and this project's
/// whole ethos - exhaustive justification, exact and explainable - argues against an opaque
/// third-party solver for the single piece of logic now most load-bearing for every word's primary
/// outline).
///
/// `contested[i] = (word, candidates)`, each candidate paired with its own *edge* cost (frequency
/// weight already folded in - see `generate`'s own `edge_cost`), widened to `u64` here rather than
/// the `u32` a bare `candidate_cost` fits in: a weight up to ~100,000 times a multi-stroke,
/// multi-star candidate's cost can exceed `u32::MAX`, and there is no reason to risk that wrapping
/// silently in the one place a wrong number would be hardest to notice. Returns, for every
/// contested word, the index into its own candidate list it was assigned - asserted to cover every
/// word passed in: given every word here has at least one real edge (peeling already routed a
/// zero-candidate word to `unresolved` before this ever runs), a perfect assignment must exist, so
/// a flow that saturates anything less than every word is a bug in this function, not a legitimate
/// shortfall to paper over silently.
pub(super) fn solve_contested<'a>(contested: &[(&'a str, Vec<(Candidate, u64)>)], no_match: &[u64]) -> HashMap<&'a str, usize> {
    if contested.is_empty() {
        return HashMap::new();
    }
    // Node numbering: 0 = source, 1..=W = one per word, W+1..=W+T = one per distinct outline text,
    // W+T+1 = sink. Each word's edges are in its own candidate order, so an edge's position is the
    // candidate's index.
    let words = contested.len();
    let mut text_index: HashMap<String, usize> = HashMap::new();
    let word_edges: Vec<Vec<(usize, u64)>> = contested
        .iter()
        .map(|(_, candidates)| {
            candidates
                .iter()
                .map(|(candidate, cost)| {
                    let next = text_index.len();
                    (*text_index.entry(render_outline(&candidate.outline)).or_insert(next), *cost)
                })
                .collect()
        })
        .collect();
    let (source, sink) = (0, 1 + words + text_index.len());
    let word_node = |w: usize| 1 + w;
    let text_node = |t: usize| 1 + words + t;
    let mut network = Network::new(sink + 1);
    for (w, edges) in word_edges.iter().enumerate() {
        network.link(source, word_node(w), 0);
        for &(t, cost) in edges {
            network.link(word_node(w), text_node(t), i64::try_from(cost).unwrap_or(i64::MAX));
        }
        // A private escape straight to the sink: one component can still hold more mutually
        // exclusive words than texts (a homophone cluster deeper than its star ladder). At
        // `no_match_cost`, the solver leaves out whichever word loses least by it.
        network.link(word_node(w), sink, i64::try_from(no_match[w]).unwrap_or(i64::MAX));
    }
    for t in 0..text_index.len() {
        network.link(text_node(t), sink, 0);
    }

    let mut flow = 0;
    while flow < words
        && let Some(path) = network.shortest_path(source, sink)
    {
        network.augment(&path);
        flow += 1;
    }
    assert_eq!(
        flow, words,
        "every word has a path to the sink (a candidate, or its own escape), so the solver must \
         saturate every word: this is a bug in solve_contested"
    );

    // Each word node's first edge is from the source; exactly one edge after it carries flow: a
    // real candidate (its position is the candidate's index) or the escape (the last edge), which
    // leaves the word out of the map, unresolved.
    let mut result = HashMap::new();
    for (w, (word, _)) in contested.iter().enumerate() {
        let matched = network.edges[word_node(w)].iter().skip(1).position(|edge| edge.cap == 0);
        match matched {
            Some(index) if index < word_edges[w].len() => {
                result.insert(*word, index);
            }
            Some(_) => {}
            None => unreachable!("the saturation assert above guarantees every word has a match"),
        }
    }
    result
}

/// A residual flow network of unit capacities: each edge knows the position of its reverse edge
/// in `to`'s list. Successive shortest paths keep Johnson potentials, so Dijkstra's reduced costs
/// stay non-negative.
struct Network {
    edges: Vec<Vec<Edge>>,
    potential: Vec<i64>,
}

#[derive(Clone)]
struct Edge {
    to: usize,
    rev: usize,
    cap: u32,
    cost: i64,
}

impl Network {
    fn new(nodes: usize) -> Self {
        Self { edges: vec![Vec::new(); nodes], potential: vec![0; nodes] }
    }

    /// An edge of capacity one from `from` to `to`, and its empty reverse.
    fn link(&mut self, from: usize, to: usize, cost: i64) {
        let (fwd, rev) = (self.edges[from].len(), self.edges[to].len());
        self.edges[from].push(Edge { to, rev, cap: 1, cost });
        self.edges[to].push(Edge { to: from, rev: fwd, cap: 0, cost: -cost });
    }

    /// The cheapest path from `source` to `sink` with room left, as (node, edge position) steps
    /// from the sink back, updating the potentials; `None` when there is none. Stops as soon as the
    /// sink is settled: every node still unsettled is at least as far, which capping the
    /// potential update at the sink's distance accounts for.
    fn shortest_path(&mut self, source: usize, sink: usize) -> Option<Vec<(usize, usize)>> {
        let nodes = self.edges.len();
        let mut dist = vec![i64::MAX; nodes];
        let mut prev: Vec<Option<(usize, usize)>> = vec![None; nodes];
        dist[source] = 0;
        let mut heap = BinaryHeap::from([Reverse((0, source))]);
        while let Some(Reverse((d, u))) = heap.pop() {
            if d > dist[u] {
                continue;
            }
            if u == sink {
                break;
            }
            for (ei, edge) in self.edges[u].iter().enumerate().filter(|(_, e)| e.cap > 0) {
                let next = d + edge.cost + self.potential[u] - self.potential[edge.to];
                if next < dist[edge.to] {
                    dist[edge.to] = next;
                    prev[edge.to] = Some((u, ei));
                    heap.push(Reverse((next, edge.to)));
                }
            }
        }
        if dist[sink] == i64::MAX {
            return None;
        }
        let reach = dist[sink];
        for (slot, d) in self.potential.iter_mut().zip(&dist) {
            *slot += (*d).min(reach);
        }
        let mut path = Vec::new();
        let mut v = sink;
        while let Some((u, ei)) = prev[v] {
            path.push((u, ei));
            v = u;
        }
        Some(path)
    }

    /// One unit of flow along `path`: every capacity is one, so there is no bottleneck to find.
    fn augment(&mut self, path: &[(usize, usize)]) {
        for &(u, ei) in path {
            let (to, rev) = (self.edges[u][ei].to, self.edges[u][ei].rev);
            self.edges[u][ei].cap -= 1;
            self.edges[to][rev].cap += 1;
        }
    }
}

//! Min-cost bipartite matching for the words peeling could not settle: see `settle`.

use super::STROKE_WEIGHT;
use super::candidates::Candidate;
use crate::stroke::render_outline;
use std::collections::{BinaryHeap, HashMap};

/// What it costs a word, in `edge_cost` units, to end up with no primary outline at all: it is
/// then typed by fingerspelling, one stroke per letter, so that is what this charges - never less
/// than one stroke worse than the word's own worst real candidate, so the solver only ever leaves
/// a word unplaced when every real option is already gone. Comparable to every other edge cost on
/// purpose: a rare word losing its last slot is weighed against a common word's extra stroke
/// honestly, rather than treated as a near-hard constraint (see `docs/collisions.md`).
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
    // W+T+1 = sink.
    let word_count = contested.len();
    let mut text_index: HashMap<String, usize> = HashMap::new();
    // For each word: (text node offset, edge cost) per candidate, in the word's own candidate order
    // - the candidate's own index in this word's list is implicit in its position here.
    let mut word_edges: Vec<Vec<(usize, u64)>> = vec![Vec::new(); word_count];

    for (w, (_, candidates)) in contested.iter().enumerate() {
        for (candidate, cost) in candidates {
            let text = render_outline(&candidate.outline);
            let next = text_index.len();
            let t = *text_index.entry(text).or_insert(next);
            word_edges[w].push((t, *cost));
        }
    }

    let text_count = text_index.len();
    let source = 0usize;
    let word_node = |w: usize| 1 + w;
    let text_node = |t: usize| 1 + word_count + t;
    let sink = 1 + word_count + text_count;
    let node_count = sink + 1;

    // A standard residual graph: each edge stores the index of its own reverse edge in `to`'s
    // adjacency list.
    #[derive(Clone)]
    struct Edge {
        to: usize,
        rev: usize,
        cap: u32,
        cost: i64,
    }
    let mut graph: Vec<Vec<Edge>> = vec![Vec::new(); node_count];
    let link = |graph: &mut Vec<Vec<Edge>>, from: usize, to: usize, cap: u32, cost: i64| {
        let fwd = graph[from].len();
        let rev = graph[to].len();
        graph[from].push(Edge { to, rev, cap, cost });
        graph[to].push(Edge { to: from, rev: fwd, cap: 0, cost: -cost });
    };

    for w in 0..word_count {
        link(&mut graph, source, word_node(w), 1, 0);
        for &(t, cost) in &word_edges[w] {
            link(&mut graph, word_node(w), text_node(t), 1, i64::try_from(cost).unwrap_or(i64::MAX));
        }
        // A direct, private escape valve straight to the sink, bypassing every text: a genuine
        // Hall-violation can still happen inside one connected component (more mutually-exclusive
        // words than distinct texts - a multi-way homophone cluster deeper than its own star
        // ladder). Its cost is `no_match_cost`'s, so the solver leaves unplaced whichever word
        // loses least by it, measured on the same scale as every real candidate.
        let no_match_cost = i64::try_from(no_match[w]).unwrap_or(i64::MAX);
        link(&mut graph, word_node(w), sink, 1, no_match_cost);
    }
    for t in 0..text_count {
        link(&mut graph, text_node(t), sink, 1, 0);
    }

    let mut potential = vec![0i64; node_count];
    let mut flow = 0usize;

    while flow < word_count {
        // Dijkstra over reduced costs `cost(u,v) + potential[u] - potential[v]`, which stay
        // non-negative as long as `potential` was last updated by a real shortest-path run - true
        // on the first iteration (all zero, all real costs non-negative) and maintained below.
        let mut dist = vec![i64::MAX; node_count];
        let mut prev_edge: Vec<Option<(usize, usize)>> = vec![None; node_count];
        dist[source] = 0;
        let mut heap: BinaryHeap<std::cmp::Reverse<(i64, usize)>> = BinaryHeap::new();
        heap.push(std::cmp::Reverse((0, source)));
        while let Some(std::cmp::Reverse((d, u))) = heap.pop() {
            if d > dist[u] {
                continue;
            }
            // Only the path to the sink is needed: every node still unsettled has a distance of
            // at least `dist[sink]`, which the potential update below accounts for.
            if u == sink {
                break;
            }
            for (ei, edge) in graph[u].iter().enumerate() {
                if edge.cap == 0 {
                    continue;
                }
                let reduced = edge.cost + potential[u] - potential[edge.to];
                let nd = d + reduced;
                if nd < dist[edge.to] {
                    dist[edge.to] = nd;
                    prev_edge[edge.to] = Some((u, ei));
                    heap.push(std::cmp::Reverse((nd, edge.to)));
                }
            }
        }
        if dist[sink] == i64::MAX {
            break;
        }
        // Capping every node at `dist[sink]` (settled nodes are already below it; unsettled and
        // unreached ones are not) keeps every residual edge's reduced cost non-negative for the
        // next round - the standard early-exit form of successive shortest paths.
        let dist_sink = dist[sink];
        for (v, slot) in potential.iter_mut().enumerate() {
            *slot += dist[v].min(dist_sink);
        }
        // Walk the augmenting path once more, pushing one unit of flow along it (every capacity
        // here is 1, so there is never a smaller bottleneck to find first) - this only ever moves
        // flow between a word's own candidate edges (see this function's own doc: the source-word
        // edge, once saturated, is never the one that gets displaced), so the final matching is
        // read off directly from edge capacities once flow is complete, not tracked here.
        let mut v = sink;
        let mut path: Vec<(usize, usize)> = Vec::new();
        while let Some((u, ei)) = prev_edge[v] {
            path.push((u, ei));
            v = u;
        }
        for (u, ei) in path {
            let (to, rev) = (graph[u][ei].to, graph[u][ei].rev);
            graph[u][ei].cap -= 1;
            graph[to][rev].cap += 1;
        }
        flow += 1;
    }

    assert_eq!(
        flow, word_count,
        "the contested-subgraph solver failed to saturate every word's own source edge - every \
         word has a guaranteed path to the sink (a real candidate, or its own no-match escape \
         valve above), so this is a bug in solve_contested, not a legitimate shortfall"
    );

    // The source-word edge (index 0 in each word node's own adjacency list) is always saturated
    // and never displaced once flow is complete (see the loop above); exactly one of the edges
    // after it is the one actually carrying flow - either a real candidate (its index in the
    // word's own list is what `generate` asked for) or this word's own no-match escape valve
    // (the last edge added, right after its real candidates) - a genuine Hall-violation inside
    // this component that the escape valve's own cost-scaling chose to sacrifice. `generate`
    // treats a word missing from the returned map as unresolved, exactly as it would have been
    // before this solver existed.
    let mut result = HashMap::new();
    for (w, (word, _)) in contested.iter().enumerate() {
        let real_count = word_edges[w].len();
        let matched = graph[word_node(w)].iter().skip(1).position(|edge| edge.cap == 0);
        match matched {
            Some(candidate_index) if candidate_index < real_count => {
                result.insert(*word, candidate_index);
            }
            Some(_) => {} // the no-match escape valve won: a genuine, cost-minimal sacrifice
            None => unreachable!("the saturation assert above guarantees every word has a match"),
        }
    }
    result
}

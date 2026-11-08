// The solver's core: given a run of strokes that missed the dictionary, find the word (or the
// words and suffixes, joined) it most likely meant.
//
// This file is self-contained on purpose. lyk compiles it natively as `src/rules/solve/core.rs`
// (for `resolve`, `lookup`, the eval and the site data) and the plugin generator splices it,
// unchanged, into the keymux plugin's source: one decoder, never two copies that can drift. So:
// no `crate::`/`super::` paths, no dependencies outside `std`, no inner attributes.
//
// How it works (see `site/reference/fallback.json` for the rules a typist needs):
//
// - Every stroke is read back into *every* run of sounds its keys can write: each way to cover
//   the left bank with onset chords, the vowel keys as any vowel they write (heard, or as the
//   word spells it), each way to cover the right bank with coda chords, the star as a sound or
//   as a marker. Those readings form a small tree of sound tokens per stroke.
// - The lexicon is a trie over every word's pronunciation (and over a reading of its spelling),
//   with each vowel edge knowing which vowel letters spell it in that word.
// - A beam of trie positions is pushed through each stroke's tokens: a token that matches an edge
//   costs nothing, a sound the word has that the strokes skipped costs by what it is (an
//   unstressed vowel almost nothing, a stressed one a lot), a sound the strokes have that the
//   word does not costs more. At each stroke boundary a position that ends a word may emit that
//   word (costing more the rarer it is) and start the next piece. A suffix chord and a run of
//   strokes that is exactly a known word's outline are exact pieces.
// - The cheapest way through the whole run wins. Every loop is bounded by the run's length and
//   the beam's width: there is no input that makes it slow.

use std::collections::HashMap;

pub const JOIN: u32 = 1 << 23;
pub const SPELL: u32 = 1 << 24;
pub const NUMBER: u32 = 1;
pub const LEFT: u32 = 0b1111_1110;
pub const VOWELS: u32 = (0b11 << 8) | (0b11 << 11);
pub const STAR: u32 = 1 << 10;
pub const RIGHT: u32 = 0b11_1111_1111 << 13;
const KEYS: &[u8; 23] = b"#STKPWHRAO*EUFRPBLGTSDZ";
const RIGHT_START: usize = 13;

/// A sound in the run that the word does not have.
pub const INSERT: u32 = 6;
/// A consonant read as its voiced or unvoiced twin (`-F` for V, `-S` for Z), beyond what the
/// chord tables already allow.
pub const SUB_CONSONANT: u32 = 4;
/// A vowel struck as a different vowel than the word has, heard or spelled.
pub const SWAP_VOWEL: u32 = 4;
/// A vowel struck as a letter the word does not spell that vowel with.
pub const SPELL_MISS: u32 = 3;
/// A vowel struck as the word spells it rather than as it is heard (`O` for the OY of *toilet*),
/// as the generator's own lattice prices it.
pub const SPELLED_VOWEL: u32 = 1;
/// Every piece (word, suffix, known outline) after the first.
pub const PIECE: u32 = 10;
/// A suffix chord read as its suffix.
pub const SUFFIX: u32 = 1;
/// A known word's exact outline as a piece after the first.
pub const STEM_LATER: u32 = 2;
/// The star down with nothing to write: the generator's homophone marker.
pub const MARKER: u32 = 1;
/// Two consonants of one stroke matched in the other order than the keys have them, per swap:
/// cheap for `R` and `L`, which the rules let land out of order, dearer for the rest.
pub const ORDER_LOOSE: u32 = 1;
pub const ORDER: u32 = 3;
/// A word's rarity: `RANK_WEIGHT * log2(rank + 2)`, in tenths.
pub const RANK_WEIGHT_TENTHS: u32 = 12;
/// Positions kept per step, and how far behind the best a position may fall before it is dropped.
const BEAM: usize = 384;
const REL: u32 = 26;
/// Skipped word sounds in a row, at most.
const MAX_DELETES: usize = 3;
/// Readings of the run's end kept (distinct texts), and words emitted per trie position.
const KEEP_ENDS: usize = 8;
const EMIT_PER_NODE: usize = 3;
/// The longest run the solver reads: a longer one is the dictionary's business.
pub const MAX_RUN: usize = 12;
/// Above this (plus the per-stroke allowance) the best reading is no reading: the plain
/// read-back is shown instead.
pub const THRESHOLD: u32 = 40;
pub const THRESHOLD_PER_STROKE: u32 = 6;

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
enum Tok {
    /// A consonant sound
    C(u8),
    /// The vowel keys: any vowel they write
    V(u32),
    /// One reading of a vowel: its sound and the letters it is spelled with (0: as heard)
    S(u8, u8),
}

#[derive(Clone, Copy)]
struct Edge {
    /// The sounds this edge accepts, one bit each
    mask: u64,
    child: u32,
    /// The vowel letters the word spells this sound with (`a e i o u y`, one bit each)
    letters: u8,
    /// What skipping it costs
    drop: u8,
    vowel: bool,
}

#[derive(Clone, Copy)]
struct Node {
    edges: (u32, u32),
    ends: (u32, u32),
}

#[derive(Clone, Copy)]
struct St {
    node: u32,
    cost: u32,
    back: u32,
}

#[derive(Clone, Copy)]
enum Piece {
    Word(u32),
    Suffix(u32),
    TwoSuffix(u32),
    Stem(u32),
}

const NO_BACK: u32 = u32::MAX;

/// One reading of a run: its text, its cost and its pieces.
#[derive(Clone, Debug)]
pub struct Reading {
    pub text: String,
    pub cost: u32,
    pub pieces: Vec<String>,
}

pub struct Model {
    sound_names: Vec<String>,
    sound_spelling: Vec<String>,
    vowel_sound: u64,
    loose: u64,
    confusable: Vec<u64>,
    onset: Vec<(u32, Vec<u8>)>,
    coda: Vec<(u32, Vec<u8>)>,
    vowel_readings: Vec<(u32, u8, u8)>,
    vowel_coda: Vec<(u32, u8, u8, u8)>,
    suffixes: Vec<(u32, String)>,
    two_suffixes: Vec<(u32, u32, String)>,
    spelling: Vec<(u32, String)>,
    stems: HashMap<Vec<u32>, u32>,
    stem_text: Vec<String>,
    max_stem: usize,
    words: Vec<String>,
    word_cost: Vec<u32>,
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    ends: Vec<(u32, u8)>,
    /// Per trie node, the cheapest word at or below it: charged as soon as a path enters the node,
    /// so a half-matched word competes in the beam on what finishing it will cost at least
    lookahead: Vec<u32>,
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn u8(&mut self) -> u8 {
        self.at += 1;
        self.b[self.at - 1]
    }
    fn u16(&mut self) -> u16 {
        u16::from(self.u8()) | (u16::from(self.u8()) << 8)
    }
    fn u32(&mut self) -> u32 {
        u32::from(self.u16()) | (u32::from(self.u16()) << 16)
    }
    fn u64(&mut self) -> u64 {
        u64::from(self.u32()) | (u64::from(self.u32()) << 32)
    }
    fn str(&mut self) -> String {
        let n = self.u16() as usize;
        self.at += n;
        String::from_utf8_lossy(&self.b[self.at - n..self.at]).into_owned()
    }
    fn bytes(&mut self) -> Vec<u8> {
        let n = self.u8() as usize;
        self.at += n;
        self.b[self.at - n..self.at].to_vec()
    }
}

/// The blob's first bytes and format version: `pack.rs` writes what `Model::load` reads.
pub const MAGIC: &[u8; 4] = b"LYKS";
pub const VERSION: u32 = 1;

fn rank_cost(rank: u32) -> u32 {
    let log2 = f64::from(rank + 2).log2();
    (log2 * f64::from(RANK_WEIGHT_TENTHS) / 10.0).round() as u32
}

impl Model {
    /// Read the packed tables `pack.rs` wrote. `None` for a blob of another version.
    pub fn load(blob: &[u8]) -> Option<Self> {
        if blob.len() < 8 || &blob[..4] != MAGIC {
            return None;
        }
        let mut r = Reader { b: blob, at: 4 };
        if r.u32() != VERSION {
            return None;
        }
        let n = r.u16() as usize;
        let mut sound_names = Vec::with_capacity(n);
        let mut sound_spelling = Vec::with_capacity(n);
        let mut vowel_sound = 0u64;
        for i in 0..n {
            sound_names.push(r.str());
            sound_spelling.push(r.str());
            if r.u8() != 0 {
                vowel_sound |= 1 << i;
            }
        }
        let loose = r.u64();
        let confusable = (0..n).map(|_| r.u64()).collect();
        let mut chords = |r: &mut Reader| -> Vec<(u32, Vec<u8>)> {
            let n = r.u32() as usize;
            (0..n).map(|_| (r.u32(), r.bytes())).collect()
        };
        let onset = chords(&mut r);
        let coda = chords(&mut r);
        let n = r.u32() as usize;
        let vowel_readings = (0..n).map(|_| (r.u32(), r.u8(), r.u8())).collect();
        let n = r.u32() as usize;
        let vowel_coda = (0..n).map(|_| (r.u32(), r.u8(), r.u8(), r.u8())).collect();
        let n = r.u32() as usize;
        let suffixes = (0..n).map(|_| (r.u32(), r.str())).collect();
        let n = r.u32() as usize;
        let two_suffixes = (0..n).map(|_| (r.u32(), r.u32(), r.str())).collect();
        let n = r.u32() as usize;
        let spelling = (0..n).map(|_| (r.u32(), r.str())).collect();
        let n = r.u32() as usize;
        let mut stems = HashMap::with_capacity(n);
        let mut stem_text = Vec::with_capacity(n);
        let mut max_stem = 0;
        for i in 0..n {
            let len = r.u8() as usize;
            let outline: Vec<u32> = (0..len).map(|_| r.u32()).collect();
            max_stem = max_stem.max(len);
            stem_text.push(r.str());
            stems.entry(outline).or_insert(i as u32);
        }
        let n = r.u32() as usize;
        let mut words = Vec::with_capacity(n);
        let mut word_cost = Vec::with_capacity(n);
        for _ in 0..n {
            words.push(r.str());
            word_cost.push(rank_cost(r.u32()));
        }
        let n = r.u32() as usize;
        let mut nodes = Vec::with_capacity(n);
        for _ in 0..n {
            let edges = (r.u32(), r.u32());
            let ends = (r.u32(), r.u32());
            nodes.push(Node { edges, ends });
        }
        let n = r.u32() as usize;
        let mut edges = Vec::with_capacity(n);
        for _ in 0..n {
            let mask = r.u64();
            let child = r.u32();
            let letters = r.u8();
            let drop = r.u8();
            let vowel = r.u8() != 0;
            edges.push(Edge { mask, child, letters, drop, vowel });
        }
        let n = r.u32() as usize;
        let ends: Vec<(u32, u8)> = (0..n).map(|_| (r.u32(), r.u8())).collect();
        // Children always come after their parent, so one pass from the back fills every node
        let mut lookahead = vec![u32::MAX / 4; nodes.len()];
        for at in (0..nodes.len()).rev() {
            let node: Node = nodes[at];
            let mut best = lookahead[at];
            for &(w, extra) in &ends[node.ends.0 as usize..node.ends.1 as usize] {
                best = best.min(word_cost[w as usize] + u32::from(extra));
            }
            for e in &edges[node.edges.0 as usize..node.edges.1 as usize] {
                best = best.min(lookahead[e.child as usize]);
            }
            lookahead[at] = best;
        }
        if let Some(root) = lookahead.first_mut() {
            *root = 0;
        }
        Some(Self {
            sound_names,
            sound_spelling,
            vowel_sound,
            loose,
            confusable,
            onset,
            coda,
            vowel_readings,
            vowel_coda,
            suffixes,
            two_suffixes,
            spelling,
            stems,
            stem_text,
            max_stem,
            words,
            word_cost,
            nodes,
            edges,
            ends,
            lookahead,
        })
    }

    pub fn word_count(&self) -> usize {
        self.words.len()
    }
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
    pub fn sound_name(&self, id: u8) -> &str {
        &self.sound_names[id as usize]
    }

    // --- Reading strokes ---

    /// Every way to cover `bits` exactly with chords from `table`, each as the chords used in key
    /// order (a chord's place is its lowest key that is not the star).
    fn covers(bits: u32, table: &[(u32, Vec<u8>)], cap: usize) -> Vec<Vec<usize>> {
        fn go(bits: u32, table: &[(u32, Vec<u8>)], cur: &mut Vec<usize>, out: &mut Vec<Vec<usize>>, cap: usize) {
            if out.len() >= cap {
                return;
            }
            if bits == 0 {
                out.push(cur.clone());
                return;
            }
            let low = 1u32 << bits.trailing_zeros();
            for (i, (keys, _)) in table.iter().enumerate() {
                if *keys != 0 && keys & low != 0 && keys & !bits == 0 {
                    cur.push(i);
                    go(bits & !keys, table, cur, out, cap);
                    cur.pop();
                }
            }
        }
        let mut out = Vec::new();
        go(bits, table, &mut Vec::new(), &mut out, cap);
        for cover in &mut out {
            cover.sort_by_key(|&i| {
                let k = table[i].0 & !STAR;
                if k == 0 { 0 } else { k.trailing_zeros() }
            });
        }
        out
    }

    fn vowel_letters_of(&self, vowel_bits: u32) -> bool {
        self.vowel_readings.iter().any(|&(k, _, _)| k == vowel_bits)
    }

    /// The right bank's readings (vowel and coda), each a token list and its cost.
    fn right_readings(&self, vowel_bits: u32, right_all: u32) -> Vec<(Vec<Tok>, u32)> {
        let mut out = Vec::new();
        let star = right_all & STAR != 0;
        for (right, extra) in [(right_all, 0), (right_all & !STAR, MARKER)] {
            if extra > 0 && !star {
                continue;
            }
            // A vowel and the consonant after it on the keys together (`AR`, `OPBG`)
            for &(combo, v, letters, c) in &self.vowel_coda {
                let combo_right = combo & (RIGHT | STAR);
                if combo & VOWELS == vowel_bits && vowel_bits != 0 && right & combo_right == combo_right {
                    for cover in Self::covers(right & !combo_right, &self.coda, 12) {
                        let mut placed: Vec<(u32, Vec<u8>)> =
                            cover.iter().map(|&i| (place(self.coda[i].0), self.coda[i].1.clone())).collect();
                        placed.push((place(combo_right), vec![c]));
                        placed.sort_by_key(|p| p.0);
                        let coda: Vec<u8> = placed.into_iter().flat_map(|p| p.1).collect();
                        let mut toks = vec![Tok::S(v, letters)];
                        toks.extend(coda.iter().map(|&s| Tok::C(s)));
                        out.push((toks, extra));
                    }
                }
            }
            if vowel_bits != 0 && !self.vowel_letters_of(vowel_bits) {
                continue;
            }
            for cover in Self::covers(right, &self.coda, 12) {
                let coda: Vec<u8> = cover.iter().flat_map(|&i| self.coda[i].1.iter().copied()).collect();
                let mut toks = Vec::new();
                if vowel_bits != 0 {
                    toks.push(Tok::V(vowel_bits));
                }
                toks.extend(coda.iter().map(|&s| Tok::C(s)));
                out.push((toks, extra));
            }
        }
        out
    }

    /// Every reading of one stroke as sound tokens, with what each costs, cheapest first.
    fn readings(&self, stroke: u32) -> Vec<(Vec<Tok>, u32)> {
        let bits = stroke & !(JOIN | SPELL | NUMBER);
        let left = bits & LEFT;
        let vowel_bits = bits & VOWELS;
        let right_all = bits & (RIGHT | STAR);
        let onsets: Vec<Vec<u8>> = Self::covers(left, &self.onset, 12)
            .into_iter()
            .map(|cover| cover.iter().flat_map(|&i| self.onset[i].1.iter().copied()).collect())
            .collect();
        let rights = self.right_readings(vowel_bits, right_all);
        let mut out: Vec<(Vec<Tok>, u32)> = Vec::new();
        for onset in &onsets {
            for (right, cost) in &rights {
                // The consonants after the vowel in every order, a swap costing what the rules
                // say it does: the keys are in steno order, the sounds need not be
                let at = right.iter().position(|t| !matches!(t, Tok::C(_))).map_or(0, |p| p + 1);
                for (coda, order_cost) in self.orders(&right[at..]) {
                    let mut toks: Vec<Tok> = onset.iter().map(|&s| Tok::C(s)).collect();
                    toks.extend_from_slice(&right[..at]);
                    toks.extend(coda);
                    if !toks.is_empty() {
                        out.push((toks, cost + order_cost));
                    }
                }
            }
        }
        out.sort();
        out.dedup_by(|a, b| a.0 == b.0);
        out.sort_by_key(|a| a.1);
        out
    }

    /// `coda` as struck, and (for three consonants or fewer) in every other order with what the
    /// swaps cost.
    fn orders(&self, coda: &[Tok]) -> Vec<(Vec<Tok>, u32)> {
        let mut out = vec![(coda.to_vec(), 0)];
        if coda.len() < 2 || coda.len() > 3 {
            return out;
        }
        let n = coda.len();
        let perms: &[&[usize]] = if n == 2 { &[&[1, 0]] } else { &[&[0, 2, 1], &[1, 0, 2], &[1, 2, 0], &[2, 0, 1], &[2, 1, 0]] };
        for p in perms {
            let mut cost = 0;
            for i in 0..n {
                for j in i + 1..n {
                    if p[i] > p[j] {
                        let loose = |t: Tok| matches!(t, Tok::C(s) if self.loose & (1 << s) != 0);
                        cost += if loose(coda[p[i]]) || loose(coda[p[j]]) { ORDER_LOOSE } else { ORDER };
                    }
                }
            }
            out.push((p.iter().map(|&i| coda[i]).collect(), cost));
        }
        out
    }

    // --- Matching ---

    fn match_cost(&self, tok: Tok, e: &Edge) -> Option<u32> {
        match tok {
            Tok::C(s) => {
                if e.vowel {
                    None
                } else if e.mask & (1 << s) != 0 {
                    Some(0)
                } else if e.mask & self.confusable[s as usize] != 0 {
                    Some(SUB_CONSONANT)
                } else {
                    None
                }
            }
            Tok::S(s, letters) => e.vowel.then(|| self.vowel_cost(s, letters, e)),
            Tok::V(keys) => {
                if !e.vowel {
                    return None;
                }
                let mut best = SWAP_VOWEL;
                for &(k, s, letters) in &self.vowel_readings {
                    if k == keys {
                        best = best.min(self.vowel_cost(s, letters, e));
                        if best == 0 {
                            break;
                        }
                    }
                }
                Some(best)
            }
        }
    }

    fn vowel_cost(&self, sound: u8, letters: u8, e: &Edge) -> u32 {
        if e.mask & (1 << sound) == 0 {
            SWAP_VOWEL
        } else if letters == 0 {
            0
        } else if letters & !e.letters == 0 {
            SPELLED_VOWEL
        } else {
            SPELL_MISS
        }
    }

    // --- The beam ---

    fn edges_of(&self, node: u32) -> &[Edge] {
        let (a, b) = self.nodes[node as usize].edges;
        &self.edges[a as usize..b as usize]
    }

    /// Keep each trie position's cheapest state (the root keeps several: different texts), then
    /// the best of those within reach of the leader.
    fn prune(states: &mut Vec<St>) {
        states.sort_unstable_by_key(|s| (s.node, s.cost));
        let mut kept: Vec<St> = Vec::with_capacity(states.len());
        let mut roots = 0;
        for &s in states.iter() {
            if s.node == 0 {
                if roots < KEEP_ENDS {
                    kept.push(s);
                    roots += 1;
                }
            } else if kept.last().is_none_or(|k| k.node != s.node) {
                kept.push(s);
            }
        }
        kept.sort_unstable_by_key(|s| s.cost);
        if let Some(best) = kept.first().map(|s| s.cost) {
            kept.retain(|s| s.cost <= best + REL);
        }
        // The beam bounds the half-matched words; the finished ones (few) are always kept
        let mut seen = 0;
        kept.retain(|s| {
            seen += usize::from(s.node != 0);
            s.node == 0 || seen <= BEAM
        });
        *states = kept;
    }

    /// Skipping sounds of the word the strokes did not write: up to [`MAX_DELETES`] in a row.
    fn closure(&self, states: &mut Vec<St>) {
        let mut fresh: Vec<St> = states.clone();
        for _ in 0..MAX_DELETES {
            let best = states.iter().map(|s| s.cost).min().unwrap_or(0);
            let mut next = Vec::new();
            for s in &fresh {
                for e in self.edges_of(s.node) {
                    let cost = s.cost + u32::from(e.drop) + self.lookahead[e.child as usize] - self.lookahead[s.node as usize];
                    if cost <= best + REL {
                        next.push(St { node: e.child, cost, back: s.back });
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            states.extend_from_slice(&next);
            fresh = next;
        }
        Self::prune(states);
    }

    fn step(&self, states: &[St], tok: Tok) -> Vec<St> {
        let mut out = Vec::with_capacity(states.len() * 3);
        for s in states {
            out.push(St { node: s.node, cost: s.cost + INSERT, back: s.back });
            for e in self.edges_of(s.node) {
                if let Some(c) = self.match_cost(tok, e) {
                    let cost = s.cost + c + self.lookahead[e.child as usize] - self.lookahead[s.node as usize];
                    out.push(St { node: e.child, cost, back: s.back });
                }
            }
        }
        Self::prune(&mut out);
        self.closure(&mut out);
        out
    }

    /// Push `states` through every reading of a stroke at once: readings that share a first few
    /// tokens share the work of matching them.
    fn walk(&self, alts: &[(Vec<Tok>, u32)], depth: usize, states: &[St], out: &mut Vec<St>) {
        let mut i = 0;
        while i < alts.len() {
            if alts[i].0.len() == depth {
                out.extend(states.iter().map(|s| St { cost: s.cost + alts[i].1, ..*s }));
                i += 1;
                continue;
            }
            let tok = alts[i].0[depth];
            let mut j = i;
            while j < alts.len() && alts[j].0.len() > depth && alts[j].0[depth] == tok {
                j += 1;
            }
            let next = self.step(states, tok);
            if !next.is_empty() {
                self.walk(&alts[i..j], depth + 1, &next, out);
            }
            i = j;
        }
    }

    fn emit(&self, states: &[St], arena: &mut Vec<(u32, Piece)>, out: &mut Vec<St>) {
        for s in states {
            let (a, b) = self.nodes[s.node as usize].ends;
            for &(word, extra) in self.ends[a as usize..b as usize].iter().take(EMIT_PER_NODE) {
                let piece = if s.back == NO_BACK { 0 } else { PIECE };
                arena.push((s.back, Piece::Word(word)));
                out.push(St {
                    node: 0,
                    cost: s.cost + piece + self.word_cost[word as usize] + u32::from(extra) - self.lookahead[s.node as usize],
                    back: (arena.len() - 1) as u32,
                });
            }
        }
    }

    /// The cheapest readings of `run` (stroke bits, as keymux's own tape writes them), best
    /// first, at most `keep`.
    pub fn solve(&self, run: &[u32], keep: usize) -> Vec<Reading> {
        let k = run.len().min(MAX_RUN);
        let run = &run[..k];
        if k == 0 {
            return Vec::new();
        }
        let mut arena: Vec<(u32, Piece)> = Vec::new();
        let mut pending: Vec<Vec<St>> = vec![Vec::new(); k + 1];
        let mut states = vec![St { node: 0, cost: 0, back: NO_BACK }];
        self.closure(&mut states);
        for i in 0..k {
            // Exact pieces that start here, from the positions that just finished a piece
            let roots: Vec<St> = states.iter().filter(|s| s.node == 0).copied().collect();
            for root in &roots {
                let first = root.back == NO_BACK;
                let piece = if first { 0 } else { PIECE };
                let stroke = run[i];
                if !first && stroke & JOIN != 0 {
                    if let Some(at) = self.suffixes.iter().position(|(s, _)| *s == stroke) {
                        arena.push((root.back, Piece::Suffix(at as u32)));
                        pending[i + 1].push(St { node: 0, cost: root.cost + SUFFIX, back: (arena.len() - 1) as u32 });
                    }
                    if i + 1 < k {
                        if let Some(at) = self.two_suffixes.iter().position(|(a, b, _)| *a == stroke && *b == run[i + 1]) {
                            arena.push((root.back, Piece::TwoSuffix(at as u32)));
                            pending[i + 2].push(St { node: 0, cost: root.cost + SUFFIX, back: (arena.len() - 1) as u32 });
                        }
                    }
                }
                let mut key: Vec<u32> = Vec::new();
                for j in i..k.min(i + self.max_stem) {
                    key.push(run[j] & !JOIN);
                    if j > i && run[j] & JOIN == 0 {
                        break;
                    }
                    if let Some(&stem) = self.stems.get(&key) {
                        arena.push((root.back, Piece::Stem(stem)));
                        let cost = root.cost + piece + if first { 0 } else { STEM_LATER };
                        pending[j + 1].push(St { node: 0, cost, back: (arena.len() - 1) as u32 });
                    }
                }
            }
            let mut alts = self.readings(run[i]);
            alts.sort();
            let mut next = Vec::new();
            self.walk(&alts, 0, &states, &mut next);
            Self::prune(&mut next);
            let mut emitted = Vec::new();
            self.emit(&next, &mut arena, &mut emitted);
            next.extend(emitted);
            next.append(&mut pending[i + 1]);
            Self::prune(&mut next);
            self.closure(&mut next);
            states = next;
        }
        let mut ends: Vec<St> = states.into_iter().filter(|s| s.node == 0 && s.back != NO_BACK).collect();
        ends.sort_unstable_by_key(|s| s.cost);
        let mut out: Vec<Reading> = Vec::new();
        for s in ends {
            let reading = self.reading(&arena, s);
            if !out.iter().any(|r| r.text == reading.text) {
                out.push(reading);
                if out.len() >= keep {
                    break;
                }
            }
        }
        out
    }

    fn reading(&self, arena: &[(u32, Piece)], end: St) -> Reading {
        let mut pieces: Vec<(Piece, String)> = Vec::new();
        let mut at = end.back;
        while at != NO_BACK {
            let (back, piece) = arena[at as usize];
            let text = match piece {
                Piece::Word(w) => self.words[w as usize].clone(),
                Piece::Suffix(s) => self.suffixes[s as usize].1.clone(),
                Piece::TwoSuffix(s) => self.two_suffixes[s as usize].2.clone(),
                Piece::Stem(s) => self.stem_text[s as usize].clone(),
            };
            pieces.push((piece, text));
            at = back;
        }
        pieces.reverse();
        let mut text = String::new();
        for (i, (_, piece)) in pieces.iter().enumerate() {
            let suffix_next = matches!(pieces.get(i + 1), Some((Piece::Suffix(_) | Piece::TwoSuffix(_), _)));
            let next_vowel = pieces.get(i + 1).is_some_and(|(_, t)| t.starts_with(['a', 'e', 'i', 'o', 'u', 'y']));
            text.push_str(piece);
            if suffix_next {
                join_suffix(&mut text, next_vowel, pieces[i + 1].1.starts_with('i'));
            }
        }
        if text == "i" {
            text = "I".into();
        }
        Reading { text, cost: end.cost, pieces: pieces.into_iter().map(|(_, t)| t).collect() }
    }

    // --- What the plugin answers ---

    /// The best reading of `outline` (Plover notation, `KAT/+-G`), or the plain read-back of its
    /// keys when no reading is close enough.
    pub fn answer(&self, outline: &str) -> Option<String> {
        let run: Vec<u32> = outline.split('/').map(parse_stroke).collect::<Option<_>>()?;
        if run.first().is_some_and(|s| s & SPELL != 0) {
            return self.spelled(&run);
        }
        let limit = THRESHOLD + THRESHOLD_PER_STROKE * run.len() as u32;
        match self.solve(&run, 1).into_iter().next() {
            Some(best) if best.cost <= limit => Some(best.text),
            _ => self.read_back(&run),
        }
    }

    /// A spelling-mode run, one letter a stroke.
    pub fn spelled(&self, run: &[u32]) -> Option<String> {
        let mut text = String::new();
        for &stroke in run {
            let bits = stroke & !(JOIN | SPELL);
            text.push_str(&self.spelling.iter().find(|(b, _)| *b == bits)?.1);
        }
        Some(text)
    }

    /// The run's keys read back as plain sounds, the floor when no word is close.
    pub fn read_back(&self, run: &[u32]) -> Option<String> {
        let mut text = String::new();
        for (i, &stroke) in run.iter().enumerate() {
            if i > 0 && stroke & JOIN != 0 {
                if let Some((_, suffix)) = self.suffixes.iter().find(|(s, _)| *s == stroke) {
                    text.push_str(suffix);
                    continue;
                }
            }
            let (toks, _) = self.readings(stroke).into_iter().next()?;
            for tok in toks {
                let sound = match tok {
                    Tok::C(s) | Tok::S(s, _) => s,
                    Tok::V(keys) => self.vowel_readings.iter().find(|r| r.0 == keys && r.2 == 0).or_else(|| self.vowel_readings.iter().find(|r| r.0 == keys))?.1,
                };
                text.push_str(&self.sound_spelling[sound as usize]);
            }
        }
        (!text.is_empty()).then_some(text)
    }

    /// A stroke's readings, written out: what the solver thinks its keys can say.
    pub fn describe(&self, stroke: u32) -> Vec<String> {
        self.readings(stroke)
            .into_iter()
            .map(|(toks, cost)| {
                let names: Vec<String> = toks
                    .iter()
                    .map(|t| match *t {
                        Tok::C(s) => self.sound_names[s as usize].clone(),
                        Tok::V(k) => format!("V{k:x}"),
                        Tok::S(s, l) => format!("{}.{l}", self.sound_names[s as usize]),
                    })
                    .collect();
                format!("{} ({cost})", names.join(" "))
            })
            .collect()
    }

    pub fn is_vowel(&self, id: u8) -> bool {
        self.vowel_sound & (1 << id) != 0
    }
}

/// A chord's place in key order: its lowest key that is not the star.
fn place(keys: u32) -> u32 {
    let k = keys & !STAR;
    if k == 0 { 0 } else { k.trailing_zeros() }
}

/// English's spelling at a suffix: a silent `e` drops before a vowel (`make` + `ing`), a `y`
/// after a consonant turns to `i` before anything but `i` (`happy` + `ness`).
fn join_suffix(text: &mut String, next_vowel: bool, next_i: bool) {
    let b = text.as_bytes();
    let n = b.len();
    let vowel = |c: u8| matches!(c, b'a' | b'e' | b'i' | b'o' | b'u');
    if n > 1 && next_vowel && b[n - 1] == b'e' && !vowel(b[n - 2]) {
        text.pop();
    } else if n > 1 && !next_i && b[n - 1] == b'y' && !vowel(b[n - 2]) {
        text.pop();
        text.push('i');
    }
}

/// One stroke's notation (`KAT`, `-T`, `+AO`, `~S`, `#KOPL`) as its bits.
pub fn parse_stroke(text: &str) -> Option<u32> {
    let (mut bits, text) = match text.strip_prefix('+') {
        Some(rest) => (JOIN, rest),
        None => match text.strip_prefix('~') {
            Some(rest) => (SPELL, rest),
            None => (0, text),
        },
    };
    let mut cursor = 0usize;
    for c in text.bytes() {
        if c == b'-' {
            cursor = cursor.max(RIGHT_START);
            continue;
        }
        let at = KEYS[cursor..].iter().position(|&k| k == c)? + cursor;
        bits |= 1 << at;
        cursor = at + 1;
    }
    (bits & !(JOIN | SPELL) != 0).then_some(bits)
}

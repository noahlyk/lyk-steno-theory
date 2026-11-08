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
pub const SWAP_VOWEL: u32 = 6;
/// A vowel struck as a letter the word does not spell that vowel with.
pub const SPELL_MISS: u32 = 5;
/// A vowel struck as the word spells it rather than as it is heard (`O` for the OY of *toilet*),
/// as the generator's own lattice prices it.
pub const SPELLED_VOWEL: u32 = 1;
/// The `-R` key standing for the vowel ER (`PHART`, *mat-er-*), as the rules fold it.
pub const R_FOR_ER: u32 = 1;
/// Every piece (word, suffix, known outline) after the first.
pub const PIECE: u32 = 10;
/// A suffix chord read as its suffix.
pub const SUFFIX: u32 = 1;
/// A known word's exact outline as a piece after the first.
pub const STEM_LATER: u32 = 2;
/// A stroke read as a known single-stroke outline one key away (a key too many or one missed): a
/// misstroke. 0 turns the reading off.
pub const SLIP: u32 = 5;
/// 1: every stroke of a run must match at least one sound of the word it is read into (or be a
/// suffix chord or a known outline), never be explained away as sounds the word does not have.
pub const MUST_USE: u32 = 1;
/// 1: each `+*` ending a run steps one reading down; 0: it is only a marker, read as nothing.
pub const STAR_NEXT: u32 = 1;
/// A known outline carried on into a longer word by the strokes after it (`SKEL` *cancel*, then
/// `+EUPBG`: *canceling*).
pub const STEM_ENTER: u32 = 1;
/// A prefix chord (`KP` for *ex-*) read as the prefix's sounds, on a run's first stroke.
pub const PREFIX: u32 = 1;
/// A reading of several pieces whose text is not itself a word.
pub const NONWORD: u32 = 6;
/// The star down with nothing to write: the generator's homophone marker.
pub const MARKER: u32 = 1;
/// Two consonants of one stroke matched in the other order than the keys have them, per swap:
/// cheap for `R` and `L`, which the rules let land out of order, dearer for the rest.
pub const ORDER_LOOSE: u32 = 1;
pub const ORDER: u32 = 3;
/// A word's rarity: `RANK_WEIGHT * log2(rank + 2)`, in tenths: 0.5 a doubling, tuned on the eval.
pub const RANK_WEIGHT_TENTHS: u32 = 5;
/// A stroke's readings tried: those within this of its cheapest, at most this many.
pub const READING_SLACK: u32 = 4;
pub const MAX_READINGS: u32 = 40;
/// Ways to cover one bank's keys with chords, at most (the tables list bigger chords first).
const COVERS: usize = 48;
/// Positions kept per step, and how far behind the best a position may fall before it is dropped.
/// 512 over 256: +0.6 points for +1 ms a run (3 ms mean, 8 ms p99); 1024 adds 0.1 for 13 ms more.
const BEAM: usize = 512;
const REL: u32 = 10;
/// (tuning) the beam threshold, natively overridable while tuning
fn rel() -> u32 {
    std::env::var("LYK_SOLVE_REL").ok().and_then(|v| v.parse().ok()).unwrap_or(REL)
}
/// The most a word's skipped first sound may cost (an unstressed vowel, a weak `HH` or `Y`).
const ROOT_DROP: u32 = 2;
/// Skipped word sounds in a row, at most.
const MAX_DELETES: usize = 3;
/// Readings of the run's end kept (distinct texts), and words emitted per trie position.
const KEEP_ENDS: usize = 16;
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

/// One edge of the trie, in eight bytes: the node it leads to, which sounds it accepts (an index
/// into [`Model::masks`], since a handful of masks serve every edge), the vowel letters the word
/// spells it with (`a e i o u y`, one bit each), and what skipping it costs with, in the top bit,
/// whether it is a vowel.
#[derive(Clone, Copy)]
struct Edge {
    child: u32,
    class: u16,
    letters: u8,
    flags: u8,
}

impl Edge {
    const fn drop(self) -> u32 {
        (self.flags & 0x7f) as u32
    }
    const fn vowel(self) -> bool {
        self.flags & 0x80 != 0
    }
}

#[derive(Clone, Copy)]
struct St {
    node: u32,
    cost: u32,
    back: u32,
    /// Whether the stroke being read has matched a sound of this word yet: a stroke that is
    /// struck is used, never explained away as sounds the word does not have
    used: bool,
}

#[derive(Clone, Copy)]
enum Piece {
    Word(u32),
    Suffix(u32),
    TwoSuffix(u32),
    Stem(u32),
    /// A known outline one key off the stroke struck: a guess at a misstroke, never a word
    /// struck on purpose, so it does not take a known outline's precedence
    Slip(u32),
}

const NO_BACK: u32 = u32::MAX;

/// A vowel chord's keys, the sound it writes, and the letters of the spelling it stands for (0:
/// as heard).
type VowelReading = (u32, u8, u8);

/// The costs above, as one decoder uses them: the constants, or (natively, while tuning) an
/// environment override of any of them, `LYK_SOLVE_<NAME>`. A plugin has no environment, so it
/// always runs on the constants.
#[derive(Clone, Copy, Debug)]
pub struct Costs {
    pub insert: u32,
    pub sub_consonant: u32,
    pub swap_vowel: u32,
    pub spell_miss: u32,
    pub spelled_vowel: u32,
    pub piece: u32,
    pub suffix: u32,
    pub stem_later: u32,
    pub nonword: u32,
    pub marker: u32,
    pub r_for_er: u32,
    pub order_loose: u32,
    pub order: u32,
    pub reading_slack: u32,
    pub max_readings: u32,
    pub slip: u32,
    pub prefix: u32,
    pub stem_enter: u32,
    pub must_use: u32,
    pub star_next: u32,
}

fn tuned(name: &str, default: u32) -> u32 {
    std::env::var(format!("LYK_SOLVE_{name}")).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

impl Costs {
    fn new() -> Self {
        Self {
            insert: tuned("INSERT", INSERT),
            sub_consonant: tuned("SUB_CONSONANT", SUB_CONSONANT),
            swap_vowel: tuned("SWAP_VOWEL", SWAP_VOWEL),
            spell_miss: tuned("SPELL_MISS", SPELL_MISS),
            spelled_vowel: tuned("SPELLED_VOWEL", SPELLED_VOWEL),
            piece: tuned("PIECE", PIECE),
            suffix: tuned("SUFFIX", SUFFIX),
            stem_later: tuned("STEM_LATER", STEM_LATER),
            nonword: tuned("NONWORD", NONWORD),
            marker: tuned("MARKER", MARKER),
            r_for_er: tuned("R_FOR_ER", R_FOR_ER),
            order_loose: tuned("ORDER_LOOSE", ORDER_LOOSE),
            order: tuned("ORDER", ORDER),
            reading_slack: tuned("READING_SLACK", READING_SLACK),
            max_readings: tuned("MAX_READINGS", MAX_READINGS),
            slip: tuned("SLIP", SLIP),
            prefix: tuned("PREFIX", PREFIX),
            stem_enter: tuned("STEM_ENTER", STEM_ENTER),
            must_use: tuned("MUST_USE", MUST_USE),
            star_next: tuned("STAR_NEXT", STAR_NEXT),
        }
    }
}

enum Offer {
    New,
    Better,
    Worse,
}

/// The decoder's working memory, one slot per trie node, reused from run to run: which beam a
/// node was last put in (`stamp`), and its cost and back-pointer there.
pub struct Scratch {
    stamp: Vec<u32>,
    cost: Vec<u32>,
    back: Vec<u32>,
    used: Vec<bool>,
    generation: u32,
}

impl Scratch {
    fn new(nodes: usize) -> Self {
        Self { stamp: vec![0; nodes], cost: vec![0; nodes], back: vec![0; nodes], used: vec![false; nodes], generation: 0 }
    }
    fn begin(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.stamp.fill(0);
            self.generation = 1;
        }
    }
    /// Put `node` in the beam being built at `cost`; whether it was not there before.
    fn offer(&mut self, s: St) -> bool {
        matches!(self.offer_new(s), Offer::New)
    }
    /// A state that has used the stroke beats one that has not at the same cost.
    fn offer_new(&mut self, s: St) -> Offer {
        let n = s.node as usize;
        if self.stamp[n] != self.generation {
            self.stamp[n] = self.generation;
            self.cost[n] = s.cost;
            self.back[n] = s.back;
            self.used[n] = s.used;
            Offer::New
        } else if s.cost < self.cost[n] || (s.cost == self.cost[n] && s.used && !self.used[n]) {
            self.cost[n] = s.cost;
            self.back[n] = s.back;
            self.used[n] = s.used;
            Offer::Better
        } else {
            Offer::Worse
        }
    }
    fn state(&self, node: u32) -> St {
        let n = node as usize;
        St { node, cost: self.cost[n], back: self.back[n], used: self.used[n] }
    }
}

/// One reading of a run: its text, its cost and its pieces.
#[derive(Clone, Debug)]
pub struct Reading {
    pub text: String,
    pub cost: u32,
    pub pieces: Vec<String>,
    /// What the pieces' rarity added to the cost
    pub rarity: u32,
    /// A known outline followed by nothing but suffix chords: the word you know, with endings
    pub composed: bool,
}

pub struct Model {
    sound_names: Vec<String>,
    sound_spelling: Vec<String>,
    loose: u64,
    confusable: Vec<u64>,
    onset: Vec<(u32, Vec<u8>)>,
    coda: Vec<(u32, Vec<u8>)>,
    /// What reading each chord costs: a chord the rules give a sound only for a spelling (`T.ed`
    /// on `-D`, `NG.ing` on `-G`) costs a little, since the trie does not know the spelling
    onset_cost: Vec<u32>,
    coda_cost: Vec<u32>,
    vowel_readings: Vec<(u32, u8, u8)>,
    /// Vowel entries whose keys reach into the right hand (`ER` is `UR`): keys, sound, letters
    vowel_right: Vec<(u32, u8, u8)>,
    /// `[[prefix]]` chords (`ex-` is `KP`), read on a run's first stroke: keys, and each way of
    /// saying the prefix as tokens
    prefixes: Vec<(u32, Vec<Vec<Tok>>)>,
    /// By the vowel keys ([`vowel_index`]): the sounds they write as heard, the sounds they
    /// write only as some spelling, and (by keys and sound) those spellings' letters
    heard: [u64; 16],
    spelled_any: [u64; 16],
    spelled: Vec<Vec<u8>>,
    vowel_coda: Vec<(u32, u8, u8, u8)>,
    /// Suffix chords and their text, and whether the chord is the hard one (the star on its
    /// first stroke, `+*-D`, the suffix sheet's own entry): always the suffix, never read as
    /// sounds. The plain chord (`+-D`) is one reading among the stroke's others.
    suffixes: Vec<(u32, String, bool)>,
    two_suffixes: Vec<(u32, u32, String, bool)>,
    spelling: Vec<(u32, String)>,
    stems: HashMap<Vec<u32>, u32>,
    stem_text: Vec<String>,
    /// A known outline's word's rarity, as a word's
    stem_cost: Vec<u32>,
    /// Where a known outline's word's sounds end in the trie, to carry on into a longer word
    stem_nodes: Vec<Vec<u32>>,
    max_stem: usize,
    /// `R` and `ER`: an R-coloured vowel is often struck as the `-R` key (`PHART`, *mat-er-*)
    r_sound: u8,
    er_sound: u8,
    words: Vec<String>,
    word_cost: Vec<u32>,
    /// Every word's text, to tell a reading of several pieces that spells a real word from one that does not
    known: HashMap<String, u32>,
    /// Each trie node's edges are `edges[edge_start[n]..edge_start[n + 1]]`, its words
    /// `ends[end_start[n]..end_start[n + 1]]`
    edge_start: Vec<u32>,
    end_start: Vec<u32>,
    masks: Vec<u64>,
    edges: Vec<Edge>,
    ends: Vec<(u32, u8)>,
    scratch: std::sync::Mutex<Option<Scratch>>,
    rel: u32,
    c: Costs,
    /// Per trie node, the cheapest word at or below it: charged as soon as a path enters the node,
    /// so a half-matched word competes in the beam on what finishing it will cost at least
    lookahead: Vec<u32>,
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    const fn u8(&mut self) -> u8 {
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
pub const VERSION: u32 = 7;

fn rank_cost(rank: u32) -> u32 {
    let log2 = f64::from(rank + 2).log2();
    (log2 * f64::from(rank_weight()) / 10.0).round() as u32
}

/// [`RANK_WEIGHT_TENTHS`], or (natively, while tuning) `LYK_SOLVE_RANK` in tenths.
fn rank_weight() -> u32 {
    std::env::var("LYK_SOLVE_RANK").ok().and_then(|v| v.parse().ok()).unwrap_or(RANK_WEIGHT_TENTHS)
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
        for _ in 0..n {
            sound_names.push(r.str());
            sound_spelling.push(r.str());
            // Whether it is a vowel: the trie's edges say so already
            r.u8();
        }
        let loose = r.u64();
        let r_sound = r.u8();
        let er_sound = r.u8();
        let confusable = (0..n).map(|_| r.u64()).collect();
        let chords = |r: &mut Reader| -> (Vec<(u32, Vec<u8>)>, Vec<u32>) {
            let n = r.u32() as usize;
            (0..n).map(|_| ((r.u32(), r.bytes()), u32::from(r.u8()))).unzip()
        };
        let (onset, onset_cost) = chords(&mut r);
        let (coda, coda_cost) = chords(&mut r);
        let n = r.u32() as usize;
        // A vowel entry with right-hand keys in it (`ER` is `UR`) is read like a vowel and coda
        // chord, never by its vowel keys alone
        let (vowel_right, vowel_readings): (Vec<VowelReading>, Vec<VowelReading>) =
            (0..n).map(|_| (r.u32(), r.u8(), r.u8())).partition(|v| v.0 & (RIGHT | STAR) != 0);
        let mut heard = [0u64; 16];
        let mut spelled_any = [0u64; 16];
        let mut spelled = vec![Vec::new(); 16 * 64];
        for &(keys, sound, letters) in &vowel_readings {
            let k = vowel_index(keys);
            if letters == 0 {
                heard[k] |= 1 << sound;
            } else {
                spelled_any[k] |= 1 << sound;
                spelled[k * 64 + sound as usize].push(letters);
            }
        }
        let n = r.u32() as usize;
        let vowel_coda = (0..n).map(|_| (r.u32(), r.u8(), r.u8(), r.u8())).collect();
        let n = r.u32() as usize;
        let suffixes = (0..n).map(|_| (r.u32(), r.str(), r.u8() != 0)).collect();
        let n = r.u32() as usize;
        let two_suffixes = (0..n).map(|_| (r.u32(), r.u32(), r.str(), r.u8() != 0)).collect();
        let n = r.u32() as usize;
        let spelling = (0..n).map(|_| (r.u32(), r.str())).collect();
        let n = r.u32() as usize;
        let prefixes = (0..n)
            .map(|_| {
                let keys = r.u32();
                let ways = (0..r.u8())
                    .map(|_| (0..r.u8()).map(|_| (r.u8(), r.u8())).map(|(sound, vowel)| if vowel != 0 { Tok::S(sound, 0) } else { Tok::C(sound) }).collect())
                    .collect();
                (keys, ways)
            })
            .collect();
        let n = r.u32() as usize;
        let mut stems = HashMap::with_capacity(n);
        let mut stem_text = Vec::with_capacity(n);
        let mut stem_cost = Vec::with_capacity(n);
        let mut stem_nodes: Vec<Vec<u32>> = Vec::with_capacity(n);
        let mut max_stem = 0;
        for i in 0..n {
            let len = r.u8() as usize;
            let outline: Vec<u32> = (0..len).map(|_| r.u32()).collect();
            max_stem = max_stem.max(len);
            stem_text.push(r.str());
            stem_cost.push(rank_cost(r.u32()));
            let ends = r.u8();
            stem_nodes.push((0..ends).map(|_| r.u32()).collect());
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
        let edge_start: Vec<u32> = (0..=n).map(|_| r.u32()).collect();
        let end_start: Vec<u32> = (0..=n).map(|_| r.u32()).collect();
        let m = r.u32() as usize;
        let masks: Vec<u64> = (0..m).map(|_| r.u64()).collect();
        let m = r.u32() as usize;
        let edges: Vec<Edge> = (0..m).map(|_| Edge { child: r.u32(), class: r.u16(), letters: r.u8(), flags: r.u8() }).collect();
        let m = r.u32() as usize;
        let ends: Vec<(u32, u8)> = (0..m).map(|_| (r.u32(), r.u8())).collect();
        // Children always come after their parent, so one pass from the back fills every node
        let mut lookahead = vec![u32::MAX / 4; n];
        for at in (0..n).rev() {
            let mut best = lookahead[at];
            for &(w, extra) in &ends[end_start[at] as usize..end_start[at + 1] as usize] {
                best = best.min(word_cost[w as usize] + u32::from(extra));
            }
            for e in &edges[edge_start[at] as usize..edge_start[at + 1] as usize] {
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
            loose,
            confusable,
            onset,
            coda,
            onset_cost,
            coda_cost,
            vowel_readings,
            vowel_right,
            prefixes,
            heard,
            spelled_any,
            spelled,
            vowel_coda,
            suffixes,
            two_suffixes,
            spelling,
            stems,
            stem_text,
            stem_cost,
            stem_nodes,
            max_stem,
            r_sound,
            er_sound,
            known: words.iter().enumerate().map(|(i, w)| (w.clone(), i as u32)).collect(),
            words,
            word_cost,
            edge_start,
            end_start,
            masks,
            edges,
            ends,
            lookahead,
            scratch: std::sync::Mutex::new(None),
            rel: rel(),
            c: Costs::new(),
        })
    }

    pub const fn word_count(&self) -> usize {
        self.words.len()
    }
    pub const fn node_count(&self) -> usize {
        self.lookahead.len()
    }
    pub const fn edge_count(&self) -> usize {
        self.edges.len()
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
        for (right, extra) in [(right_all, 0), (right_all & !STAR, self.c.marker)] {
            if extra > 0 && !star {
                continue;
            }
            // A vowel and the consonant after it on the keys together (`AR`, `OPBG`)
            for &(combo, v, letters, c) in &self.vowel_coda {
                let combo_right = combo & (RIGHT | STAR);
                if combo & VOWELS == vowel_bits && vowel_bits != 0 && right & combo_right == combo_right {
                    for cover in Self::covers(right & !combo_right, &self.coda, COVERS) {
                        let cost: u32 = cover.iter().map(|&i| self.coda_cost[i]).sum();
                        let mut placed: Vec<(u32, Vec<u8>)> =
                            cover.iter().map(|&i| (place(self.coda[i].0), self.coda[i].1.clone())).collect();
                        placed.push((place(combo_right), vec![c]));
                        placed.sort_by_key(|p| p.0);
                        let coda: Vec<u8> = placed.into_iter().flat_map(|p| p.1).collect();
                        let mut toks = vec![Tok::S(v, letters)];
                        toks.extend(coda.iter().map(|&s| Tok::C(s)));
                        out.push((toks, extra + cost));
                    }
                }
            }
            // A vowel whose own keys reach into the right hand (`UR` for ER)
            for &(combo, v, letters) in &self.vowel_right {
                let combo_right = combo & (RIGHT | STAR);
                if combo & VOWELS == vowel_bits && right & combo_right == combo_right {
                    for cover in Self::covers(right & !combo_right, &self.coda, COVERS) {
                        let cost: u32 = cover.iter().map(|&i| self.coda_cost[i]).sum();
                        let mut toks = vec![Tok::S(v, letters)];
                        toks.extend(cover.iter().flat_map(|&i| self.coda[i].1.iter().map(|&s| Tok::C(s))));
                        out.push((toks, extra + cost));
                    }
                }
            }
            if vowel_bits != 0 && !self.vowel_letters_of(vowel_bits) {
                continue;
            }
            for cover in Self::covers(right, &self.coda, COVERS) {
                let cost: u32 = cover.iter().map(|&i| self.coda_cost[i]).sum();
                let coda: Vec<u8> = cover.iter().flat_map(|&i| self.coda[i].1.iter().copied()).collect();
                let mut toks = Vec::new();
                if vowel_bits != 0 {
                    toks.push(Tok::V(vowel_bits));
                }
                toks.extend(coda.iter().map(|&s| Tok::C(s)));
                out.push((toks, extra + cost));
            }
        }
        out
    }

    /// Every reading of one stroke as sound tokens, with what each costs, cheapest first.
    fn readings(&self, stroke: u32, first: bool) -> Vec<(Vec<Tok>, u32)> {
        let bits = stroke & !(JOIN | SPELL | NUMBER);
        let left = bits & LEFT;
        let vowel_bits = bits & VOWELS;
        let right_all = bits & (RIGHT | STAR);
        let onset_of = |left: u32| -> Vec<(Vec<Tok>, u32)> {
            Self::covers(left, &self.onset, COVERS)
                .into_iter()
                .map(|cover| {
                    let cost = cover.iter().map(|&i| self.onset_cost[i]).sum();
                    (cover.iter().flat_map(|&i| self.onset[i].1.iter().map(|&s| Tok::C(s))).collect(), cost)
                })
                .collect()
        };
        let mut onsets = onset_of(left);
        // A prefix chord in the first stroke of a run, its sounds before the rest of the onset
        if first {
            for (keys, ways) in &self.prefixes {
                if left & keys == *keys {
                    for (rest, cost) in onset_of(left & !keys) {
                        for way in ways {
                            let mut toks = way.clone();
                            toks.extend_from_slice(&rest);
                            onsets.push((toks, cost + self.c.prefix));
                        }
                    }
                }
            }
        }
        let rights = self.right_readings(vowel_bits, right_all);
        let mut out: Vec<(Vec<Tok>, u32)> = Vec::new();
        for (onset, onset_cost) in &onsets {
            for (right, cost) in &rights {
                // The consonants after the vowel in every order, a swap costing what the rules
                // say it does: the keys are in steno order, the sounds need not be
                let at = right.iter().position(|t| !matches!(t, Tok::C(_))).map_or(0, |p| p + 1);
                for (coda, order_cost) in self.orders(&right[at..]) {
                    let mut toks: Vec<Tok> = onset.clone();
                    toks.extend_from_slice(&right[..at]);
                    toks.extend(coda);
                    if !toks.is_empty() {
                        out.push((toks, cost + order_cost + onset_cost));
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
                        cost += if loose(coda[p[i]]) || loose(coda[p[j]]) { self.c.order_loose } else { self.c.order };
                    }
                }
            }
            out.push((p.iter().map(|&i| coda[i]).collect(), cost));
        }
        out
    }

    // --- Matching ---

    fn match_cost(&self, tok: Tok, e: Edge) -> Option<u32> {
        match tok {
            Tok::C(s) => {
                let mask = self.masks[e.class as usize];
                if e.vowel() {
                    (s == self.r_sound && mask & (1 << self.er_sound) != 0).then_some(self.c.r_for_er)
                } else if mask & (1 << s) != 0 {
                    Some(0)
                } else if mask & self.confusable[s as usize] != 0 {
                    Some(self.c.sub_consonant)
                } else {
                    None
                }
            }
            Tok::S(s, letters) => e.vowel().then(|| self.vowel_cost(s, letters, e)),
            Tok::V(keys) => {
                if !e.vowel() {
                    return None;
                }
                let mask = self.masks[e.class as usize];
                let k = vowel_index(keys);
                if mask & self.heard[k] != 0 {
                    return Some(0);
                }
                let mut best = self.c.swap_vowel;
                let mut sounds = mask & self.spelled_any[k];
                while sounds != 0 {
                    let s = sounds.trailing_zeros() as usize;
                    for &letters in &self.spelled[k * 64 + s] {
                        best = best.min(self.vowel_cost(s as u8, letters, e));
                    }
                    sounds &= sounds - 1;
                }
                Some(best)
            }
        }
    }

    fn vowel_cost(&self, sound: u8, letters: u8, e: Edge) -> u32 {
        if self.masks[e.class as usize] & (1 << sound) == 0 {
            self.c.swap_vowel
        } else if letters == 0 {
            0
        } else if if letters.is_power_of_two() { letters == e.letters } else { letters & !e.letters == 0 } {
            // A single vowel letter only reads a vowel the word spells with that letter alone, as
            // the generator's own letter reading does (`said`'s `ai` is not an `i`)
            self.c.spelled_vowel
        } else {
            self.c.spell_miss
        }
    }

    // --- The beam ---

    fn edges_of(&self, node: u32) -> &[Edge] {
        let n = node as usize;
        &self.edges[self.edge_start[n] as usize..self.edge_start[n + 1] as usize]
    }

    /// The candidates as a beam: each trie position once, at its cheapest (the root keeps several,
    /// since each is a different text so far), every sound of the word then skippable up to
    /// [`MAX_DELETES`] in a row (when `skips`),
    /// and only the [`BEAM`] best within [`REL`] of the leader. No sorting: a stamp per trie node
    /// says whether it is in the beam being built.
    fn settle(&self, sc: &mut Scratch, cands: &[St], skips: bool) -> Vec<St> {
        sc.begin();
        let mut nodes: Vec<u32> = Vec::with_capacity(cands.len());
        let mut roots: Vec<St> = Vec::new();
        let mut best = u32::MAX;
        for c in cands {
            best = best.min(c.cost);
            if c.node == 0 {
                roots.push(*c);
            } else if sc.offer(*c) {
                nodes.push(c.node);
            }
        }
        if best == u32::MAX {
            return Vec::new();
        }
        roots.sort_unstable_by_key(|r| r.cost);
        roots.truncate(KEEP_ENDS);
        let limit = best + self.rel;
        if skips {
            let mut frontier: Vec<St> = roots.clone();
            frontier.extend(nodes.iter().map(|&n| sc.state(n)));
            for round in 0..MAX_DELETES {
                let mut next = Vec::new();
                for s in &frontier {
                    if s.cost > limit {
                        continue;
                    }
                    // A word starts with at most one weak sound skipped (the `a-` of *about*):
                    // anything more from the root would be every word's every beginning at once
                    if s.node == 0 && round > 0 {
                        continue;
                    }
                    let here = self.lookahead[s.node as usize];
                    for e in self.edges_of(s.node) {
                        if s.node == 0 && e.drop() > ROOT_DROP {
                            continue;
                        }
                        let cost = s.cost + e.drop() + self.lookahead[e.child as usize] - here;
                        if cost <= limit {
                            let skipped = St { node: e.child, cost, ..*s };
                            match sc.offer_new(skipped) {
                                Offer::New => {
                                    nodes.push(e.child);
                                    next.push(skipped);
                                }
                                Offer::Better => next.push(skipped),
                                Offer::Worse => {}
                            }
                        }
                    }
                }
                if next.is_empty() {
                    break;
                }
                frontier = next;
            }
        }
        let mut out: Vec<St> = nodes.iter().map(|&n| sc.state(n)).filter(|s| s.cost <= limit).collect();
        if out.len() > BEAM {
            out.select_nth_unstable_by_key(BEAM - 1, |s| s.cost);
            out.truncate(BEAM);
        }
        // Finished readings are measured against each other, not against half-matched words that
        // may never finish
        let best_root = roots.first().map_or(0, |r| r.cost);
        out.extend(roots.into_iter().filter(|r| r.cost <= best_root + self.rel));
        out
    }

    fn step(&self, sc: &mut Scratch, states: &[St], tok: Tok) -> Vec<St> {
        let mut out = Vec::with_capacity(states.len() * 3);
        for s in states {
            // A sound the word does not have; never before any word has started, or a stroke
            // could be skipped outright
            if s.node != 0 {
                out.push(St { cost: s.cost + self.c.insert, ..*s });
            }
            let here = self.lookahead[s.node as usize];
            for e in self.edges_of(s.node) {
                if let Some(c) = self.match_cost(tok, *e) {
                    let cost = s.cost + c + self.lookahead[e.child as usize] - here;
                    out.push(St { node: e.child, cost, back: s.back, used: true });
                }
            }
        }
        self.settle(sc, &out, true)
    }

    /// Push `states` through every reading of a stroke at once: readings that share a first few
    /// tokens share the work of matching them.
    fn walk(&self, sc: &mut Scratch, alts: &[(Vec<Tok>, u32)], depth: usize, states: &[St], out: &mut Vec<St>) {
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
            let next = self.step(sc, states, tok);
            if !next.is_empty() {
                self.walk(sc, &alts[i..j], depth + 1, &next, out);
            }
            i = j;
        }
    }

    fn emit(&self, states: &[St], arena: &mut Vec<(u32, Piece)>, out: &mut Vec<St>) {
        for s in states {
            let n = s.node as usize;
            let ends = &self.ends[self.end_start[n] as usize..self.end_start[n + 1] as usize];
            for &(word, extra) in ends.iter().take(EMIT_PER_NODE) {
                let piece = if s.back == NO_BACK { 0 } else { self.c.piece };
                arena.push((s.back, Piece::Word(word)));
                out.push(St {
                    node: 0,
                    cost: s.cost + piece + self.word_cost[word as usize] + u32::from(extra) - self.lookahead[s.node as usize],
                    back: (arena.len() - 1) as u32,
                    used: true,
                });
            }
        }
    }

    /// The cheapest readings of `run` (stroke bits, as keymux's own tape writes them), best
    /// first, at most `keep`.
    pub fn solve(&self, run: &[u32], keep: usize) -> Vec<Reading> {
        let mut guard = self.scratch.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let readings = self.solve_with(guard.get_or_insert_with(|| Scratch::new(self.lookahead.len())), run, keep);
        drop(guard);
        readings
    }

    /// Working memory for [`Self::solve_with`], for a caller solving on several threads at once.
    pub fn scratch(&self) -> Scratch {
        Scratch::new(self.lookahead.len())
    }

    /// [`Self::solve`] with the caller's own working memory.
    pub fn solve_with(&self, sc: &mut Scratch, run: &[u32], keep: usize) -> Vec<Reading> {
        let k = run.len().min(MAX_RUN);
        let run = &run[..k];
        if k == 0 {
            return Vec::new();
        }
        // Each `+*` that ends the run steps one reading down: the next word that reads the same
        let stars = run.iter().skip(1).rev().take_while(|&&s| s & !JOIN == STAR).count();
        if stars > 0 && self.c.star_next > 0 {
            let mut out = self.solve_with(sc, &run[..k - stars], keep + stars);
            out.drain(..stars.min(out.len().saturating_sub(1)));
            out.truncate(keep);
            return out;
        }
        let mut arena: Vec<(u32, Piece)> = Vec::new();
        let mut pending: Vec<Vec<St>> = vec![Vec::new(); k + 1];
        let mut states = self.settle(sc, &[St { node: 0, cost: 0, back: NO_BACK, used: true }], true);
        // A hard suffix chord is only ever its suffix: the strokes it covers are not read as sounds
        let mut hard_until = 0;
        for i in 0..k {
            if i > 0 && run[i] & JOIN != 0 && i >= hard_until {
                if self.suffixes.iter().any(|&(s, _, hard)| hard && s == run[i]) {
                    hard_until = i + 1;
                } else if run.get(i + 1).is_some_and(|&next| self.two_suffixes.iter().any(|&(a, b, _, hard)| hard && a == run[i] && b == next)) {
                    hard_until = i + 2;
                }
            }
            let hard = i < hard_until;
            // A stroke of the star alone is the star ladder's marker (`+*`): it only tells a word
            // from one that sounds the same, so it is used, at a marker's cost, and reads as nothing
            if i > 0 && run[i] & !JOIN == STAR {
                let mut next: Vec<St> = states.iter().map(|s| St { cost: s.cost + self.c.marker, ..*s }).collect();
                next.append(&mut pending[i + 1]);
                self.keep_ends(&arena, &mut next);
                states = self.settle(sc, &next, true);
                continue;
            }
            // Exact pieces that start here, from the positions that just finished a piece
            let roots: Vec<St> = states.iter().filter(|s| s.node == 0).copied().collect();
            for root in &roots {
                let first = root.back == NO_BACK;
                let piece = if first { 0 } else { self.c.piece };
                let stroke = run[i];
                if !first && stroke & JOIN != 0 {
                    if let Some(at) = self.suffixes.iter().position(|(s, _, _)| *s == stroke) {
                        let cost = if self.suffixes[at].2 { 0 } else { self.c.suffix };
                        arena.push((root.back, Piece::Suffix(at as u32)));
                        pending[i + 1].push(St { node: 0, cost: root.cost + cost, back: (arena.len() - 1) as u32, used: true });
                    }
                    if let Some(at) = run
                        .get(i + 1)
                        .and_then(|&next| self.two_suffixes.iter().position(|(a, b, _, _)| *a == stroke && *b == next))
                    {
                        let cost = if self.two_suffixes[at].3 { 0 } else { self.c.suffix };
                        arena.push((root.back, Piece::TwoSuffix(at as u32)));
                        pending[i + 2].push(St { node: 0, cost: root.cost + cost, back: (arena.len() - 1) as u32, used: true });
                    }
                }
                if hard {
                    continue;
                }
                let mut key: Vec<u32> = Vec::new();
                for j in i..k.min(i + self.max_stem) {
                    key.push(run[j] & !JOIN);
                    if j > i && run[j] & JOIN == 0 {
                        break;
                    }
                    if let Some(&stem) = self.stems.get(&key) {
                        arena.push((root.back, Piece::Stem(stem)));
                        let later = if first { 0 } else { self.c.stem_later };
                        let cost = root.cost + piece + self.stem_cost[stem as usize] + later;
                        pending[j + 1].push(St { node: 0, cost, back: (arena.len() - 1) as u32, used: true });
                        // The same word's sounds, for the strokes after to carry on into a longer
                        // word: its rarity and its piece are charged when that word ends
                        for &node in &self.stem_nodes[stem as usize] {
                            let cost = root.cost + later + self.c.stem_enter + self.lookahead[node as usize];
                            pending[j + 1].push(St { node, cost, back: root.back, used: true });
                        }
                    }
                }
                // A misstroke: one key more or fewer than a known single-stroke outline
                if self.c.slip > 0 {
                    let bits = run[i] & !JOIN;
                    for key in 0..23 {
                        let near = bits ^ (1 << key);
                        if near & !(SPELL | NUMBER) == 0 {
                            continue;
                        }
                        if let Some(&stem) = self.stems.get(&[near][..]) {
                            arena.push((root.back, Piece::Slip(stem)));
                            let cost = root.cost + piece + self.c.slip + self.stem_cost[stem as usize] + if first { 0 } else { self.c.stem_later };
                            pending[i + 1].push(St { node: 0, cost, back: (arena.len() - 1) as u32, used: true });
                        }
                    }
                }
            }
            let mut alts = self.readings(run[i], i == 0);
            // Only the readings near the stroke's plainest: the rest never win
            let cheapest = alts.first().map_or(0, |a| a.1);
            alts.retain(|a| a.1 <= cheapest + self.c.reading_slack);
            alts.truncate(self.c.max_readings as usize);
            alts.sort();
            if hard {
                alts.clear();
            }
            // Each stroke must match at least one sound of the word it is read into
            let starting: Vec<St> = states.iter().map(|s| St { used: false, ..*s }).collect();
            let mut next = Vec::new();
            self.walk(sc, &alts, 0, &starting, &mut next);
            if self.c.must_use > 0 {
                next.retain(|s| s.used);
            }
            let mut next = self.settle(sc, &next, false);
            let mut emitted = Vec::new();
            self.emit(&next, &mut arena, &mut emitted);
            next.extend(emitted);
            next.append(&mut pending[i + 1]);
            self.keep_ends(&arena, &mut next);
            states = self.settle(sc, &next, true);
        }
        let ends: Vec<St> = states.into_iter().filter(|s| s.node == 0 && s.back != NO_BACK).collect();
        let mut out: Vec<Reading> = Vec::new();
        for s in ends {
            let mut reading = self.reading(&arena, s);
            // Pieces that do not add up to a word are a guess at one the lexicon lacks: allowed,
            // but only when nothing that is a word reads as well
            // Pieces that do add up to a word are that word, and as rare as it is: `part` + `s` is
            // *parts*, no commoner than the word *parts* itself
            if reading.pieces.len() > 1 {
                match self.known.get(&reading.text.to_lowercase()) {
                    Some(&word) => reading.cost = reading.cost.saturating_sub(reading.rarity) + self.word_cost[word as usize].max(reading.rarity),
                    None => reading.cost += self.c.nonword,
                }
            }
            match out.iter_mut().find(|r| r.text == reading.text) {
                Some(seen) => seen.cost = seen.cost.min(reading.cost),
                None => out.push(reading),
            }
        }
        // Precedence, not only cost: a word you know followed by ending chords, when they spell a
        // real word, is that word (`PROB/+-BL` *probable*); only then the cheapest reading
        out.sort_by_key(|r| (!(r.composed && self.known.contains_key(&r.text.to_lowercase())), r.cost));
        out.truncate(keep);
        out
    }

    /// The finished readings worth carrying on, at most [`KEEP_ENDS`], ranked as the end will rank
    /// them: pieces that spell no word cost [`NONWORD`] more there, so a heap of cheap nonsense
    /// (*pro* + *able*, *from* + *able*) must not crowd a real word (*probable*) out before then.
    fn keep_ends(&self, arena: &[(u32, Piece)], states: &mut Vec<St>) {
        let mut ends: Vec<(u32, St)> = states
            .iter()
            .filter(|s| s.node == 0 && s.back != NO_BACK)
            .map(|&s| {
                let r = self.reading(arena, s);
                let nonsense = r.pieces.len() > 1 && !self.known.contains_key(&r.text.to_lowercase());
                (s.cost + if nonsense { self.c.nonword } else { 0 }, s)
            })
            .collect();
        if ends.len() <= KEEP_ENDS {
            return;
        }
        ends.sort_unstable_by_key(|e| e.0);
        ends.truncate(KEEP_ENDS);
        states.retain(|s| s.node != 0 || s.back == NO_BACK);
        states.extend(ends.into_iter().map(|e| e.1));
    }

    fn reading(&self, arena: &[(u32, Piece)], end: St) -> Reading {
        let mut pieces: Vec<(Piece, String)> = Vec::new();
        let mut rarity = 0;
        let mut at = end.back;
        while at != NO_BACK {
            let (back, piece) = arena[at as usize];
            rarity += match piece {
                Piece::Word(w) => self.word_cost[w as usize],
                Piece::Stem(s) | Piece::Slip(s) => self.stem_cost[s as usize],
                Piece::Suffix(_) | Piece::TwoSuffix(_) => 0,
            };
            let text = match piece {
                Piece::Word(w) => self.words[w as usize].clone(),
                Piece::Suffix(s) => self.suffixes[s as usize].1.clone(),
                Piece::TwoSuffix(s) => self.two_suffixes[s as usize].2.clone(),
                Piece::Stem(s) | Piece::Slip(s) => self.stem_text[s as usize].clone(),
            };
            pieces.push((piece, text));
            at = back;
        }
        pieces.reverse();
        let mut text = String::new();
        for (i, (_, piece)) in pieces.iter().enumerate() {
            text.push_str(piece);
            if let Some((Piece::Suffix(_) | Piece::TwoSuffix(_), next)) = pieces.get(i + 1) {
                join_suffix(&mut text, next);
            }
        }
        if text == "i" {
            text = "I".into();
        }
        let composed = pieces.len() > 1
            && matches!(pieces[0].0, Piece::Stem(_))
            && pieces[1..].iter().all(|(p, _)| matches!(p, Piece::Suffix(_) | Piece::TwoSuffix(_)));
        Reading { text, cost: end.cost, pieces: pieces.into_iter().map(|(_, t)| t).collect(), rarity, composed }
    }

    // --- What the plugin answers ---

    /// The best reading of `outline` (Plover notation, `KAT/+-G`), or the plain read-back of its
    /// keys when no reading is close enough.
    pub fn answer(&self, outline: &str) -> Option<String> {
        let mut guard = self.scratch.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let text = self.answer_with(guard.get_or_insert_with(|| Scratch::new(self.lookahead.len())), outline);
        drop(guard);
        text
    }

    /// [`Self::answer`] with the caller's own working memory.
    pub fn answer_with(&self, sc: &mut Scratch, outline: &str) -> Option<String> {
        let run: Vec<u32> = outline.split('/').map(parse_stroke).collect::<Option<_>>()?;
        if run.first().is_some_and(|s| s & SPELL != 0) {
            return self.spelled(&run);
        }
        let limit = THRESHOLD + THRESHOLD_PER_STROKE * run.len() as u32;
        match self.solve_with(sc, &run, 1).into_iter().next() {
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
            if let Some((_, suffix, _)) = self.suffixes.iter().find(|(s, _, _)| i > 0 && stroke & JOIN != 0 && *s == stroke) {
                text.push_str(suffix);
                continue;
            }
            let (toks, _) = self.readings(stroke, false).into_iter().next()?;
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
        self.readings(stroke, true)
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
}

/// The four vowel keys (`A O E U`) as a number 0..16.
const fn vowel_index(keys: u32) -> usize {
    (((keys >> 8) & 3) | (((keys >> 11) & 3) << 2)) as usize
}

/// A chord's place in key order: its lowest key that is not the star.
const fn place(keys: u32) -> u32 {
    let k = keys & !STAR;
    if k == 0 { 0 } else { k.trailing_zeros() }
}

/// English's spelling at a suffix, applied to `text` before `next` is added: a consonant and `le`
/// become `ly` with *-ly* (`probable` + `ly`, *probably*; `simple`, *simply*) and `-ble` becomes
/// `-bil` before *-ity* (*probability*); a silent `e` drops before a vowel (`make` + `ing`); a
/// `y` after a consonant turns to `i` before anything but `i` (`happy` + `ness`).
fn join_suffix(text: &mut String, next: &str) {
    let b = text.as_bytes();
    let n = b.len();
    let vowel = |c: u8| matches!(c, b'a' | b'e' | b'i' | b'o' | b'u');
    let next_vowel = next.starts_with(['a', 'e', 'i', 'o', 'u', 'y']);
    let next_i = next.starts_with('i');
    if n > 2 && text.ends_with("le") && !vowel(b[n - 3]) && next == "ly" {
        text.truncate(n - 2);
    } else if text.ends_with("ble") && next.starts_with("ity") {
        text.truncate(n - 2);
        text.push_str("il");
    } else if n > 1 && next_vowel && b[n - 1] == b'e' && !vowel(b[n - 2]) {
        text.pop();
    } else if n > 1 && !next_i && b[n - 1] == b'y' && !vowel(b[n - 2]) {
        // `fly` + `s` is *flies*, `happy` + `ness` is *happiness*
        text.pop();
        text.push_str(if next == "s" { "ie" } else { "i" });
    }
}

/// One stroke's notation (`KAT`, `-T`, `+AO`, `~S`, `#KOPL`) as its bits.
pub fn parse_stroke(text: &str) -> Option<u32> {
    let (mut bits, text) = text
        .strip_prefix('+')
        .map(|rest| (JOIN, rest))
        .or_else(|| text.strip_prefix('~').map(|rest| (SPELL, rest)))
        .unwrap_or((0, text));
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

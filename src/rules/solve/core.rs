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
/// Joining two pieces that together spell a real word past the lexicon (*look* + *up*, *lookup*), in
/// place of [`PIECE`].
pub const COMPOUND_PIECE: u32 = 2;
/// A morpheme of `rules/morphemes.toml` (*anti-*, *-ization*) read as a piece, as a word's rarity.
pub const MORPHEME: u32 = 4;
/// Joining a morpheme on (a prefix before what follows it, a suffix after what it follows), in place
/// of [`PIECE`].
pub const MORPHEME_PIECE: u32 = 1;
/// What a morpheme counts as, at least, in the bound a half-matched path is charged ahead (the
/// cheapest word below it): cheaper, and a prefix's paths crowd a word out of the search (at 6,
/// *thinking* falls to *the* + *knocking*).
pub const MORPHEME_LOOKAHEAD: u32 = 8;
/// A reading built with morphemes that is no word the lexicon has, in place of [`NONWORD`]: a word
/// you know, read as well, still wins (*goodness* over *goodnesses*).
/// 4 over 0: the backups' misses as without morphemes (3,470 to 3,497), giant words 80.8% to 84.2%.
pub const BUILT: u32 = 4;
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
/// The readings of a first stroke with a prefix chord in it kept past the cap on readings.
const PREFIX_READINGS: usize = 3;
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
/// The runs read last that a working memory keeps, to go on from.
const RUNS_KEPT: usize = 4;
const EMIT_PER_NODE: usize = 3;
/// The longest run the solver reads as one word (a giant word struck syllable by syllable is 12
/// at most): the strokes past it are read back as keys, never dropped.
pub const MAX_RUN: usize = 24;
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

/// One trie node, in sixteen bytes, with all a step reads about it: its first child (a node's
/// children are numbered together, breadth first, so they run to the next node's first child),
/// the cheapest word at or below it ([`Model::load`]), what ends at or below it ([`BELOW_WORD`] and
/// the rest), and the edge into it: which sounds that accepts (an index into [`Model::masks`],
/// since a handful of masks serve every edge), the vowel letters the word spells it with
/// (`a e i o u y`, one bit each), and what skipping it costs with, in the top bit, whether it is a
/// vowel. Children side by side, so stepping a position through them reads one stretch of memory.
#[derive(Clone, Copy, Default)]
struct Node {
    first: u32,
    lookahead: u32,
    class: u16,
    letters: u8,
    flags: u8,
    below: u8,
}

impl Node {
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
    /// The text a lone `+` locked in: the run's first piece, fixed, that the rest adds to
    Locked,
    /// A prefix or suffix of `rules/morphemes.toml`, by its id among the words (after them all)
    Morpheme(u32),
}

/// What a piece may follow: nothing yet, a prefix morpheme, a suffix morpheme, a suffix chord, a
/// word too short for a morpheme ([`MORPHEME_WORD_LETTERS`]), or any other word.
#[derive(Clone, Copy, PartialEq, Eq)]
enum After {
    Start,
    Prefix,
    Suffix,
    Chord,
    Short,
    Other,
}

/// The word a morpheme joins, at least this many letters: shorter, and every short word with a
/// cheap morpheme on it reads as something (*a* + *less*, *are* + *ity*).
const MORPHEME_WORD_LETTERS: usize = 4;

/// What ends at or below a trie node: a word, a prefix, a suffix (one bit each).
const BELOW_WORD: u8 = 1;
const BELOW_PREFIX: u8 = 2;
const BELOW_SUFFIX: u8 = 4;

const NO_BACK: u32 = u32::MAX;

/// One reading of a stroke: its sounds, in order, and what reading them so costs.
type StrokeReading = (Vec<Tok>, u32);

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
    pub compound_piece: u32,
    pub morpheme: u32,
    pub morpheme_piece: u32,
    pub morpheme_lookahead: u32,
    pub built: u32,
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
            compound_piece: tuned("COMPOUND_PIECE", COMPOUND_PIECE),
            morpheme: tuned("MORPHEME", MORPHEME),
            morpheme_piece: tuned("MORPHEME_PIECE", MORPHEME_PIECE),
            morpheme_lookahead: tuned("MORPHEME_LOOKAHEAD", MORPHEME_LOOKAHEAD),
            built: tuned("BUILT", BUILT),
        }
    }
}

enum Offer {
    New,
    Better,
    Worse,
}

/// One trie node's slot in the working memory: which beam it was last put in (`stamp`), and its
/// state there. One slot together, so putting a node in the beam touches one place in memory.
#[derive(Clone, Copy, Default)]
struct Slot {
    stamp: u32,
    cost: u32,
    back: u32,
    used: bool,
}

/// The decoder's working memory, one [`Slot`] per trie node, reused from run to run.
pub struct Scratch {
    slots: Vec<Slot>,
    generation: u32,
    /// The runs read last, step by step, most recent first: keymux asks again on every stroke, and
    /// the next run usually is one of these and one stroke more (it also asks for the newest
    /// stroke alone, so one run kept is not enough)
    runs: Vec<Progress>,
    steps: usize,
}

/// Where reading a run stands after each of its strokes, to go on from.
#[derive(Default, Clone)]
struct Progress {
    run: Vec<u32>,
    /// The text locked in ahead of the run (a lone `+`), which the run adds to
    locked: Option<String>,
    /// `before[i]`: the beam before stroke `i` (so `before[run.len()]` is the end)
    before: Vec<Vec<St>>,
    /// The pieces found so far, and how many there were before each stroke
    arena: Vec<(u32, Piece)>,
    arena_at: Vec<usize>,
    /// Whether each stroke is a hard suffix chord's, never read as sounds
    hard: Vec<bool>,
    hard_until_at: Vec<usize>,
}

impl Scratch {
    fn new(nodes: usize) -> Self {
        Self {
            slots: vec![Slot::default(); nodes],
            generation: 0,
            runs: Vec::new(),
            steps: 0,
        }
    }

    /// How many strokes this working memory has read in all (each stroke of each run once,
    /// except the ones read again because the run changed under them).
    #[cfg(test)]
    #[must_use]
    pub const fn steps_taken(&self) -> usize {
        self.steps
    }
    fn begin(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.slots.fill(Slot::default());
            self.generation = 1;
        }
    }
    /// Put `node` in the beam being built at `cost`; whether it was not there before.
    fn offer(&mut self, s: St) -> bool {
        matches!(self.offer_new(s), Offer::New)
    }
    /// A state that has used the stroke beats one that has not at the same cost.
    fn offer_new(&mut self, s: St) -> Offer {
        let generation = self.generation;
        let slot = &mut self.slots[s.node as usize];
        let fresh = slot.stamp != generation;
        if fresh || s.cost < slot.cost || (s.cost == slot.cost && s.used && !slot.used) {
            *slot = Slot { stamp: generation, cost: s.cost, back: s.back, used: s.used };
            if fresh { Offer::New } else { Offer::Better }
        } else {
            Offer::Worse
        }
    }
    fn state(&self, node: u32) -> St {
        let slot = self.slots[node as usize];
        St { node, cost: slot.cost, back: slot.back, used: slot.used }
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
    /// One known outline that is the whole run: the word it is, struck as taught
    pub exact: bool,
    /// How many word pieces after the first were joined on, each at [`PIECE`]
    pub joins: u32,
    /// Built with morphemes (prefixes before one word, suffixes after it)
    pub morphemic: bool,
    /// Ends on a prefix: never the run's reading, only a way on to one
    pub open: bool,
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
    /// Real words past the lexicon, as `(spelling_hash, rank)` sorted: what pieces may spell
    spellings: Vec<(u64, u32)>,
    /// The morphemes are `words[first_morpheme..]`, each a prefix or a suffix
    first_morpheme: u32,
    morpheme_prefix: Vec<bool>,
    /// The trie's nodes and one past the last, whose `first` ends the last node's children ([`Node`]);
    /// node `n`'s words are `ends[end_start[n]..end_start[n + 1]]`
    nodes: Vec<Node>,
    end_start: Vec<u32>,
    masks: Vec<u64>,
    ends: Vec<(u32, u8)>,
    scratch: std::sync::Mutex<Option<Scratch>>,
    rel: u32,
    c: Costs,
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
pub const VERSION: u32 = 10;

/// `run` with each `+*` (the join key and the star alone) put on the stroke before it as its star:
/// the star struck late, `+-T` then `+*` is `+*T`.
fn late_stars(run: &[u32]) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::with_capacity(run.len());
    for &stroke in run {
        match out.last_mut() {
            Some(before) if stroke == JOIN | STAR => *before |= STAR,
            _ => out.push(stroke),
        }
    }
    out
}

/// Where the last lone `+` (the join key alone) is in `run`, after its first stroke: it locks in
/// what the strokes before it type, and the strokes after it add to that.
fn last_lock(run: &[u32]) -> Option<usize> {
    run.iter().skip(1).rposition(|&s| s == JOIN).map(|at| at + 1)
}

/// A spelling's key in the table of real words past the lexicon (FNV-1a, 64 bits): the packer and
/// the decoder hash alike, natively and in wasm.
#[must_use]
pub fn spelling_hash(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

impl Model {
    /// Whether word id `w` is a morpheme, and if so whether a prefix.
    fn morpheme(&self, w: u32) -> Option<bool> {
        w.checked_sub(self.first_morpheme).map(|m| self.morpheme_prefix[m as usize])
    }

    /// What the piece at `back` (in `arena`) lets follow it.
    fn after(&self, arena: &[(u32, Piece)], back: u32) -> After {
        let short = |text: &str| if text.len() < MORPHEME_WORD_LETTERS { After::Short } else { After::Other };
        match arena.get(back as usize) {
            None => After::Start,
            Some(&(_, Piece::Morpheme(w))) => if self.morpheme(w) == Some(true) { After::Prefix } else { After::Suffix },
            Some((_, Piece::Suffix(_) | Piece::TwoSuffix(_))) => After::Chord,
            Some(&(_, Piece::Word(w))) => short(&self.words[w as usize]),
            Some(&(_, Piece::Stem(s) | Piece::Slip(s))) => short(&self.stem_text[s as usize]),
            Some((_, Piece::Locked)) => After::Other,
        }
    }

    /// The kinds of piece ([`BELOW_WORD`] and the rest) that may come after the piece at `back`: a
    /// path into the trie only goes where one of them ends.
    fn may_follow(&self, arena: &[(u32, Piece)], back: u32) -> u8 {
        match self.after(arena, back) {
            After::Start | After::Prefix => BELOW_WORD | BELOW_PREFIX,
            After::Suffix | After::Other => BELOW_WORD | BELOW_SUFFIX,
            After::Chord | After::Short => BELOW_WORD,
        }
    }

    /// What joining `text` (a word or a known outline's) on costs: nothing first, a morpheme's join
    /// after a prefix, a piece's after anything else. `None`: after a prefix, too short a word.
    fn join_cost(&self, after: After, text: &str) -> Option<u32> {
        match after {
            After::Start => Some(0),
            After::Prefix => (text.len() >= MORPHEME_WORD_LETTERS).then_some(self.c.morpheme_piece),
            _ => Some(self.c.piece),
        }
    }

    /// What joining morpheme `w` on costs, `None` where it may not go: a prefix starts the run or
    /// follows a prefix, a suffix follows a word long enough or another suffix morpheme.
    const fn morpheme_cost(&self, after: After, prefix: bool) -> Option<u32> {
        match (prefix, after) {
            (true, After::Start) => Some(0),
            (true, After::Prefix) | (false, After::Other | After::Suffix) => Some(self.c.morpheme_piece),
            _ => None,
        }
    }

    /// A real word past the lexicon's rank, when `text` is one (the table of spellings).
    fn spelled_rank(&self, text: &str) -> Option<u32> {
        let hash = spelling_hash(text);
        self.spellings.binary_search_by_key(&hash, |e| e.0).ok().map(|at| self.spellings[at].1)
    }
}

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
        let spellings: Vec<(u64, u32)> = (0..n).map(|_| (r.u64(), r.u32())).collect();
        let c = Costs::new();
        let first_morpheme = words.len() as u32;
        let known = words.iter().enumerate().map(|(i, w)| (w.clone(), i as u32)).collect();
        let n = r.u32() as usize;
        let mut morpheme_prefix = Vec::with_capacity(n);
        for _ in 0..n {
            words.push(r.str());
            word_cost.push(c.morpheme);
            morpheme_prefix.push(r.u8() != 0);
        }
        let n = r.u32() as usize;
        let edge_start: Vec<u32> = (0..=n).map(|_| r.u32()).collect();
        let end_start: Vec<u32> = (0..=n).map(|_| r.u32()).collect();
        let m = r.u32() as usize;
        let masks: Vec<u64> = (0..m).map(|_| r.u64()).collect();
        // Numbered breadth first, edge `k` leads to node `k + 1`: the edge's sounds go with the node
        let m = r.u32() as usize;
        if n == 0 || m + 1 != n || edge_start[n] as usize != m {
            return None;
        }
        let mut nodes = vec![Node { lookahead: u32::MAX / 4, ..Node::default() }; n + 1];
        for (at, node) in nodes.iter_mut().enumerate() {
            node.first = edge_start[at] + 1;
        }
        for k in 0..m {
            if r.u32() as usize != k + 1 {
                return None;
            }
            let node = &mut nodes[k + 1];
            (node.class, node.letters, node.flags) = (r.u16(), r.u8(), r.u8());
        }
        let m = r.u32() as usize;
        let ends: Vec<(u32, u8)> = (0..m).map(|_| (r.u32(), r.u8())).collect();
        // Children always come after their parent, so one pass from the back fills every node
        for at in (0..n).rev() {
            let kids = nodes[at].first as usize..nodes[at + 1].first as usize;
            let mut below = 0;
            let mut best = nodes[at].lookahead;
            for &(w, extra) in &ends[end_start[at] as usize..end_start[at + 1] as usize] {
                below |= match w.checked_sub(first_morpheme) {
                    None => BELOW_WORD,
                    Some(m) if morpheme_prefix[m as usize] => BELOW_PREFIX,
                    Some(_) => BELOW_SUFFIX,
                };
                let cost = if w < first_morpheme { word_cost[w as usize] } else { word_cost[w as usize].max(c.morpheme_lookahead) };
                best = best.min(cost + u32::from(extra));
            }
            for kid in &nodes[kids] {
                below |= kid.below;
                best = best.min(kid.lookahead);
            }
            nodes[at].below = below;
            nodes[at].lookahead = best;
        }
        nodes[0].lookahead = 0;
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
            known,
            spellings,
            first_morpheme,
            morpheme_prefix,
            words,
            word_cost,
            nodes,
            end_start,
            masks,
            ends,
            scratch: std::sync::Mutex::new(None),
            rel: rel(),
            c,
        })
    }

    pub const fn word_count(&self) -> usize {
        self.first_morpheme as usize
    }
    pub const fn node_count(&self) -> usize {
        self.nodes.len() - 1
    }
    /// A trie's edges, one into each node but the root.
    pub const fn edge_count(&self) -> usize {
        self.nodes.len() - 2
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
        self.readings_kept(stroke, first).0
    }

    /// [`Self::readings`], and the few with a prefix chord that are kept past the cap on them.
    fn readings_kept(&self, stroke: u32, first: bool) -> (Vec<StrokeReading>, Vec<StrokeReading>) {
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
        let plain_onsets = onsets.len();
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
        // A left-hand stroke alone after the first goes on with the word's consonants, which the
        // left bank can strike in steno order only (`STKPWHR`, Z then L, for *curls*' L Z): any
        // order, free, since the keys' order says nothing there
        if !first && vowel_bits == 0 && right_all == 0 {
            onsets = onsets.iter().flat_map(|(toks, cost)| self.orders(toks).into_iter().map(move |(t, _)| (t, *cost))).collect();
        }
        let plain_onsets = if first { plain_onsets } else { onsets.len() };
        let rights = self.right_readings(vowel_bits, right_all);
        let mut out: Vec<(Vec<Tok>, u32)> = Vec::new();
        let mut with_prefix: Vec<(Vec<Tok>, u32)> = Vec::new();
        for (i, (onset, onset_cost)) in onsets.iter().enumerate() {
            for (right, cost) in &rights {
                // The consonants after the vowel in every order, a swap costing what the rules
                // say it does: the keys are in steno order, the sounds need not be
                let at = right.iter().position(|t| !matches!(t, Tok::C(_))).map_or(0, |p| p + 1);
                for (coda, order_cost) in self.orders(&right[at..]) {
                    let mut toks: Vec<Tok> = onset.clone();
                    toks.extend_from_slice(&right[..at]);
                    toks.extend(coda);
                    if !toks.is_empty() {
                        let into = if i < plain_onsets { &mut out } else { &mut with_prefix };
                        into.push((toks, cost + order_cost + onset_cost));
                    }
                }
            }
        }
        // Cheapest first, and of equal cost the fewest sounds: a big chord read as its plainest
        // (`STKPWHR` Z L before J DH P), so the cap on readings keeps those
        let order = |v: &mut Vec<(Vec<Tok>, u32)>| {
            v.sort();
            v.dedup_by(|a, b| a.0 == b.0);
            v.sort_by_key(|a| (a.1, a.0.len()));
        };
        order(&mut with_prefix);
        // The prefix chord's best readings are kept past the cap on readings: a stroke with dozens
        // of free readings (`STKPAOER`) would otherwise cut *dis-* altogether
        let kept: Vec<(Vec<Tok>, u32)> = with_prefix.iter().take(PREFIX_READINGS).cloned().collect();
        out.extend(with_prefix);
        order(&mut out);
        (out, kept)
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

    fn match_cost(&self, tok: Tok, e: &Node) -> Option<u32> {
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

    fn vowel_cost(&self, sound: u8, letters: u8, e: &Node) -> u32 {
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

    /// A node's children, each with its id.
    fn children(&self, node: u32) -> impl Iterator<Item = (u32, &Node)> {
        let (first, end) = (self.nodes[node as usize].first, self.nodes[node as usize + 1].first);
        (first..end).zip(&self.nodes[first as usize..end as usize])
    }

    /// The candidates as a beam: each trie position once, at its cheapest (the root keeps several,
    /// since each is a different text so far), every sound of the word then skippable up to
    /// [`MAX_DELETES`] in a row (when `skips`),
    /// and only the [`BEAM`] best within [`REL`] of the leader. No sorting: a stamp per trie node
    /// says whether it is in the beam being built.
    fn settle(&self, sc: &mut Scratch, arena: &[(u32, Piece)], cands: &[St], skips: bool) -> Vec<St> {
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
                    let here = self.nodes[s.node as usize].lookahead;
                    let may = self.may_follow(arena, s.back);
                    for (child, e) in self.children(s.node) {
                        if (s.node == 0 && e.drop() > ROOT_DROP) || e.below & may == 0 {
                            continue;
                        }
                        let cost = s.cost + e.drop() + e.lookahead - here;
                        if cost <= limit {
                            let skipped = St { node: child, cost, ..*s };
                            match sc.offer_new(skipped) {
                                Offer::New => {
                                    nodes.push(child);
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

    fn step(&self, sc: &mut Scratch, arena: &[(u32, Piece)], states: &[St], tok: Tok) -> Vec<St> {
        let mut out = Vec::with_capacity(states.len() * 3);
        for s in states {
            // A sound the word does not have; never before any word has started, or a stroke
            // could be skipped outright
            if s.node != 0 {
                out.push(St { cost: s.cost + self.c.insert, ..*s });
            }
            let here = self.nodes[s.node as usize].lookahead;
            let may = self.may_follow(arena, s.back);
            for (child, e) in self.children(s.node) {
                if e.below & may == 0 {
                    continue;
                }
                if let Some(c) = self.match_cost(tok, e) {
                    let cost = s.cost + c + e.lookahead - here;
                    out.push(St { node: child, cost, back: s.back, used: true });
                }
            }
        }
        self.settle(sc, arena, &out, true)
    }

    /// Push `states` through every reading of a stroke at once: readings that share a first few
    /// tokens share the work of matching them.
    fn walk(&self, sc: &mut Scratch, arena: &[(u32, Piece)], alts: &[(Vec<Tok>, u32)], depth: usize, states: &[St], out: &mut Vec<St>) {
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
            let next = self.step(sc, arena, states, tok);
            if !next.is_empty() {
                self.walk(sc, arena, &alts[i..j], depth + 1, &next, out);
            }
            i = j;
        }
    }

    fn emit(&self, states: &[St], arena: &mut Vec<(u32, Piece)>, out: &mut Vec<St>) {
        for s in states {
            let n = s.node as usize;
            let ends = &self.ends[self.end_start[n] as usize..self.end_start[n + 1] as usize];
            let after = self.after(arena, s.back);
            // The commonest words that end here, and every morpheme (they sort after the words)
            let words = ends.iter().filter(|e| e.0 < self.first_morpheme).take(EMIT_PER_NODE);
            for &(word, extra) in words.chain(ends.iter().filter(|e| e.0 >= self.first_morpheme)) {
                let piece = self.morpheme(word).map_or_else(|| self.join_cost(after, &self.words[word as usize]), |prefix| self.morpheme_cost(after, prefix));
                let Some(piece) = piece else { continue };
                arena.push((s.back, self.morpheme(word).map_or(Piece::Word(word), |_| Piece::Morpheme(word))));
                out.push(St {
                    node: 0,
                    cost: s.cost + piece + self.word_cost[word as usize] + u32::from(extra) - self.nodes[s.node as usize].lookahead,
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
        let readings = self.solve_with(guard.get_or_insert_with(|| Scratch::new(self.node_count())), run, keep);
        drop(guard);
        readings
    }

    /// Working memory for [`Self::solve_with`], for a caller solving on several threads at once.
    pub fn scratch(&self) -> Scratch {
        Scratch::new(self.node_count())
    }

    /// [`Self::solve`] with the caller's own working memory.
    pub fn solve_with(&self, sc: &mut Scratch, run: &[u32], keep: usize) -> Vec<Reading> {
        let run = late_stars(run);
        let Some(at) = last_lock(&run) else { return self.solve_locked(sc, &run, keep, None) };
        let Some(locked) = self.answer_run(sc, &run[..at]) else { return Vec::new() };
        let tail = &run[at + 1..];
        if tail.is_empty() {
            let pieces = vec![locked.clone()];
            return vec![Reading { text: locked, cost: 0, pieces, rarity: 0, composed: false, exact: false, joins: 0, morphemic: false, open: false }];
        }
        self.solve_locked(sc, tail, keep, Some(&locked))
    }

    /// [`Self::solve_with`] for a run with no lock or late star in it, after `locked` (the text a
    /// lone `+` kept), if any.
    fn solve_locked(&self, sc: &mut Scratch, run: &[u32], keep: usize, locked: Option<&str>) -> Vec<Reading> {
        let k = run.len().min(MAX_RUN);
        let run = &run[..k];
        if k == 0 {
            return Vec::new();
        }
        self.advance(sc, run, locked);
        let arena = &sc.runs[0].arena;
        let states = sc.runs[0].before[k].clone();
        let ends: Vec<St> = states.into_iter().filter(|s| s.node == 0 && s.back != NO_BACK).collect();
        let mut out: Vec<Reading> = Vec::new();
        for s in ends {
            let mut reading = self.reading(arena, s, locked);
            if reading.open {
                continue;
            }
            // Pieces that do not add up to a word are a guess at one the lexicon lacks: allowed,
            // but only when nothing that is a word reads as well
            // Pieces that do add up to a word are that word, and as rare as it is: `part` + `s` is
            // *parts*, no commoner than the word *parts* itself
            if reading.pieces.len() > 1 {
                match self.known.get(&reading.text.to_lowercase()) {
                    Some(&word) => reading.cost = reading.cost.saturating_sub(reading.rarity) + self.word_cost[word as usize].max(reading.rarity),
                    // Words joined that spell a real word the lexicon lacks: that word, its joins
                    // cheap. Only words joined to words (*look* + *up*): a stem and suffix chords
                    // must spell a lexicon word, since the list's long tail holds misspellings
                    // (*regreting*)
                    None => match self.spelled_rank(&reading.text.to_lowercase()).filter(|_| reading.joins > 0) {
                        Some(rank) => {
                            let refund = reading.joins * self.c.piece.saturating_sub(self.c.compound_piece);
                            reading.cost = (reading.cost.saturating_sub(reading.rarity) + rank_cost(rank).max(reading.rarity)).saturating_sub(refund);
                        }
                        // Prefixes, one word, suffixes: a word built as English builds them
                        None if reading.morphemic && reading.joins == 0 => reading.cost += self.c.built,
                        None => reading.cost += self.c.nonword,
                    },
                }
            }
            match out.iter_mut().find(|r| r.text == reading.text) {
                Some(seen) => {
                    seen.cost = seen.cost.min(reading.cost);
                    seen.exact |= reading.exact;
                }
                None => out.push(reading),
            }
        }
        // Precedence, not only cost: an outline a word is known or taught by, the whole run, is that
        // word (`WAUT/+-R` *water*); then a word you know followed by ending chords, when they
        // spell a real word, is that word (`PROB/+-BL` *probable*); only then the cheapest reading.
        // A tie goes to the commoner word (ids run most common first): rarity is rounded to whole
        // points, so *eight* and *ate* cost the same
        out.sort_by_key(|r| {
            let id = self.known.get(&r.text.to_lowercase()).copied();
            (!r.exact, !(r.composed && id.is_some()), r.cost, id.unwrap_or(u32::MAX))
        });
        out.truncate(keep);
        out
    }

    /// Read `run` into `sc`'s progress stroke by stroke, going on from the run read before as far
    /// as the two agree: keymux asks again on every stroke, so most calls read one stroke. After
    /// it, `sc.progress.before[run.len()]` is the beam at the run's end.
    fn advance(&self, sc: &mut Scratch, run: &[u32], locked: Option<&str>) {
        let shared = |p: &Progress| if p.locked.as_deref() == locked { p.run.iter().zip(run).take_while(|(a, b)| a == b).count() } else { 0 };
        // The kept run that goes furthest along this one, the most recent of those that tie
        let best = (0..sc.runs.len()).rev().max_by_key(|&at| shared(&sc.runs[at])).filter(|&at| shared(&sc.runs[at]) > 0);
        // Taken when this run goes on from all of it; copied when it parts from it partway, so the
        // run kept is still there to go back to
        let mut p = match best {
            Some(at) if shared(&sc.runs[at]) == sc.runs[at].run.len() => sc.runs.remove(at),
            Some(at) => sc.runs[at].clone(),
            None => Progress::default(),
        };
        let same = shared(&p);
        // The last stroke both share is read again only when it may start a hard two-stroke
        // suffix: that depends on the stroke after it, which has changed
        let from = match same.checked_sub(1) {
            Some(last) if self.two_suffixes.iter().any(|&(a, _, _, hard)| hard && a == run[last]) => last,
            _ => same,
        };
        if from == 0 || p.before.is_empty() {
            p = Progress { locked: locked.map(str::to_owned), ..Progress::default() };
            // A locked text is the run's first piece already: what follows goes on from it
            let back = if locked.is_some() {
                p.arena.push((NO_BACK, Piece::Locked));
                0
            } else {
                NO_BACK
            };
            p.before.push(self.settle(sc, &p.arena, &[St { node: 0, cost: 0, back, used: true }], true));
            p.arena_at.push(p.arena.len());
            p.hard_until_at.push(0);
        } else {
            p.before.truncate(from + 1);
            p.arena.truncate(p.arena_at[from]);
            p.arena_at.truncate(from + 1);
            p.hard.truncate(from);
            p.hard_until_at.truncate(from + 1);
        }
        for i in p.hard.len()..run.len() {
            self.read_stroke(sc, &mut p, run, i);
            sc.steps += 1;
        }
        p.run = run.to_vec();
        sc.runs.insert(0, p);
        sc.runs.truncate(RUNS_KEPT);
    }

    /// Read stroke `i` of `run`: the beam before it is `p.before[i]`, the one after it is pushed.
    fn read_stroke(&self, sc: &mut Scratch, p: &mut Progress, run: &[u32], i: usize) {
        // A hard suffix chord is only ever its suffix: the strokes it covers are not read as sounds
        let mut hard_until = p.hard_until_at[i];
        if i > 0 && run[i] & JOIN != 0 && i >= hard_until {
            if self.suffixes.iter().any(|&(s, _, hard)| hard && s == run[i]) {
                hard_until = i + 1;
            } else if run.get(i + 1).is_some_and(|&next| self.two_suffixes.iter().any(|&(a, b, _, hard)| hard && a == run[i] && b == next)) {
                hard_until = i + 2;
            }
        }
        let hard = i < hard_until;
        p.hard.push(hard);
        p.hard_until_at.push(hard_until);
        let states = p.before[i].clone();
        // Exact pieces that end with this stroke, each from the positions that had just finished
        // a piece where it starts
        let mut arrived = Vec::new();
        self.pieces_ending(run, i, p, &mut arrived);
        let next = if Self::is_marker(run, i) {
            // A stroke of the star alone is the star ladder's marker (`+*`): it only tells a word
            // from one that sounds the same, so it is used, at a marker's cost, and reads as nothing
            let mut next: Vec<St> = states.iter().map(|s| St { cost: s.cost + self.c.marker, ..*s }).collect();
            next.append(&mut arrived);
            next
        } else {
            // A word's first stroke, unless a locked text came first
            let (mut alts, kept) = self.readings_kept(run[i], i == 0 && p.locked.is_none());
            // Only the readings near the stroke's plainest: the rest never win
            let cheapest = alts.first().map_or(0, |a| a.1);
            alts.retain(|a| a.1 <= cheapest + self.c.reading_slack);
            alts.truncate(self.c.max_readings as usize);
            for k in kept {
                if !alts.contains(&k) {
                    alts.push(k);
                }
            }
            alts.sort();
            if hard {
                alts.clear();
            }
            // Each stroke must match at least one sound of the word it is read into
            let starting: Vec<St> = states.iter().map(|s| St { used: false, ..*s }).collect();
            let mut next = Vec::new();
            self.walk(sc, &p.arena, &alts, 0, &starting, &mut next);
            if self.c.must_use > 0 {
                next.retain(|s| s.used);
            }
            let mut next = self.settle(sc, &p.arena, &next, false);
            let mut emitted = Vec::new();
            self.emit(&next, &mut p.arena, &mut emitted);
            next.extend(emitted);
            next.append(&mut arrived);
            next
        };
        let mut next = next;
        self.keep_ends(&p.arena, &mut next, p.locked.as_deref());
        p.before.push(self.settle(sc, &p.arena, &next, true));
        p.arena_at.push(p.arena.len());
    }

    /// Whether stroke `i` is the star ladder's marker, a stroke of the star alone after the first.
    const fn is_marker(run: &[u32], i: usize) -> bool {
        i > 0 && run[i] & !JOIN == STAR
    }

    /// The exact pieces that end with stroke `j`: a suffix chord, a two-stroke suffix, a known
    /// outline of one stroke or several joined, a misstroke of a one-stroke one. Each starts where
    /// a piece had just finished, never on a marker, and a known outline never on a hard suffix's
    /// stroke.
    fn pieces_ending(&self, run: &[u32], j: usize, p: &mut Progress, out: &mut Vec<St>) {
        let roots = |before: &[St]| -> Vec<St> { before.iter().filter(|s| s.node == 0).copied().collect() };
        // A suffix chord, and a two-stroke suffix ending here
        for (start, two) in [(Some(j), false), (j.checked_sub(1), true)] {
            let Some(start) = start else { continue };
            if Self::is_marker(run, start) || run[start] & JOIN == 0 {
                continue;
            }
            for root in roots(&p.before[start]) {
                // Nor after a morpheme: a suffix morpheme has its plural its own (*-nesses*), and a
                // prefix wants a word
                if root.back == NO_BACK || matches!(self.after(&p.arena, root.back), After::Prefix | After::Suffix) {
                    continue;
                }
                let found = if two {
                    self.two_suffixes
                        .iter()
                        .position(|(a, b, _, _)| *a == run[start] && *b == run[j])
                        .map(|at| (Piece::TwoSuffix(at as u32), self.two_suffixes[at].3))
                } else {
                    self.suffixes.iter().position(|(s, _, _)| *s == run[start]).map(|at| (Piece::Suffix(at as u32), self.suffixes[at].2))
                };
                if let Some((piece, hard)) = found {
                    p.arena.push((root.back, piece));
                    let cost = if hard { 0 } else { self.c.suffix };
                    out.push(St { node: 0, cost: root.cost + cost, back: (p.arena.len() - 1) as u32, used: true });
                }
            }
        }
        // A known outline: one stroke, or several joined, ending here
        for start in (j + 1).saturating_sub(self.max_stem)..=j {
            if p.hard[start] || Self::is_marker(run, start) || run[start + 1..=j].iter().any(|s| s & JOIN == 0) {
                continue;
            }
            let key: Vec<u32> = run[start..=j].iter().map(|s| s & !JOIN).collect();
            let Some(&stem) = self.stems.get(&key) else { continue };
            for root in roots(&p.before[start]) {
                let first = root.back == NO_BACK;
                let Some(piece) = self.join_cost(self.after(&p.arena, root.back), &self.stem_text[stem as usize]) else { continue };
                let later = if first { 0 } else { self.c.stem_later };
                p.arena.push((root.back, Piece::Stem(stem)));
                out.push(St { node: 0, cost: root.cost + piece + self.stem_cost[stem as usize] + later, back: (p.arena.len() - 1) as u32, used: true });
                // The same word's sounds, for the strokes after to carry on into a longer word:
                // its rarity and its piece are charged when that word ends
                for &node in &self.stem_nodes[stem as usize] {
                    out.push(St { node, cost: root.cost + later + self.c.stem_enter + self.nodes[node as usize].lookahead, back: root.back, used: true });
                }
            }
        }
        // A misstroke: one key more or fewer than a known single-stroke outline
        if self.c.slip > 0 && !p.hard[j] && !Self::is_marker(run, j) {
            let bits = run[j] & !JOIN;
            for root in roots(&p.before[j]) {
                let first = root.back == NO_BACK;
                let piece = if first { 0 } else { self.c.piece };
                for key in 0..23 {
                    let near = bits ^ (1 << key);
                    if near & !(SPELL | NUMBER) == 0 {
                        continue;
                    }
                    if let Some(&stem) = self.stems.get(&[near][..]) {
                        p.arena.push((root.back, Piece::Slip(stem)));
                        let cost = root.cost + piece + self.c.slip + self.stem_cost[stem as usize] + if first { 0 } else { self.c.stem_later };
                        out.push(St { node: 0, cost, back: (p.arena.len() - 1) as u32, used: true });
                    }
                }
            }
        }
    }

    /// The finished readings worth carrying on, at most [`KEEP_ENDS`], ranked as the end will rank
    /// them: pieces that spell no word cost [`NONWORD`] more there, so a heap of cheap nonsense
    /// (*pro* + *able*, *from* + *able*) must not crowd a real word (*probable*) out before then.
    fn keep_ends(&self, arena: &[(u32, Piece)], states: &mut Vec<St>, locked: Option<&str>) {
        let mut ends: Vec<(u32, St)> = states
            .iter()
            .filter(|s| s.node == 0 && s.back != NO_BACK)
            .map(|&s| {
                let r = self.reading(arena, s, locked);
                let text = r.text.to_lowercase();
                let built = r.morphemic && r.joins == 0;
                let nonsense = r.pieces.len() > 1 && !built && !self.known.contains_key(&text) && (r.joins == 0 || self.spelled_rank(&text).is_none());
                let unknown_built = built && !self.known.contains_key(&text);
                (s.cost + if nonsense { self.c.nonword } else if unknown_built { self.c.built } else { 0 }, s)
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

    fn reading(&self, arena: &[(u32, Piece)], end: St, locked: Option<&str>) -> Reading {
        let mut pieces: Vec<(Piece, String)> = Vec::new();
        let mut rarity = 0;
        let mut at = end.back;
        while at != NO_BACK {
            let (back, piece) = arena[at as usize];
            rarity += match piece {
                Piece::Word(w) | Piece::Morpheme(w) => self.word_cost[w as usize],
                Piece::Stem(s) | Piece::Slip(s) => self.stem_cost[s as usize],
                Piece::Suffix(_) | Piece::TwoSuffix(_) | Piece::Locked => 0,
            };
            let text = match piece {
                Piece::Word(w) | Piece::Morpheme(w) => self.words[w as usize].clone(),
                Piece::Suffix(s) => self.suffixes[s as usize].1.clone(),
                Piece::TwoSuffix(s) => self.two_suffixes[s as usize].2.clone(),
                Piece::Stem(s) | Piece::Slip(s) => self.stem_text[s as usize].clone(),
                Piece::Locked => locked.unwrap_or_default().to_owned(),
            };
            pieces.push((piece, text));
            at = back;
        }
        pieces.reverse();
        let mut text = String::new();
        let suffix_morpheme = |p: &Piece| matches!(p, &Piece::Morpheme(w) if self.morpheme(w) == Some(false));
        for (i, (_, piece)) in pieces.iter().enumerate() {
            text.push_str(piece);
            match pieces.get(i + 1) {
                Some((Piece::Suffix(_) | Piece::TwoSuffix(_), next)) => join_suffix(&mut text, next),
                Some((p, next)) if suffix_morpheme(p) => join_suffix(&mut text, next),
                _ => {}
            }
        }
        if text == "i" {
            text = "I".into();
        }
        let composed = pieces.len() > 1
            && matches!(pieces[0].0, Piece::Stem(_) | Piece::Locked)
            && pieces[1..].iter().all(|(p, _)| matches!(p, Piece::Suffix(_) | Piece::TwoSuffix(_)));
        let exact = pieces.len() == 1 && matches!(pieces[0].0, Piece::Stem(_));
        // Each word piece after the first was charged a piece's join, but after a prefix
        let prefix = |p: &Piece| matches!(p, &Piece::Morpheme(w) if self.morpheme(w) == Some(true));
        let joins = pieces
            .windows(2)
            .filter(|w| !prefix(&w[0].0) && !matches!(w[1].0, Piece::Suffix(_) | Piece::TwoSuffix(_) | Piece::Morpheme(_)))
            .count() as u32;
        let morphemic = pieces.iter().any(|(p, _)| matches!(p, Piece::Morpheme(_)));
        // A prefix with nothing after it is no word yet
        let open = pieces.last().is_some_and(|(p, _)| prefix(p));
        Reading { text, cost: end.cost, pieces: pieces.into_iter().map(|(_, t)| t).collect(), rarity, composed, exact, joins, morphemic, open }
    }

    // --- What the plugin answers ---

    /// The best reading of `outline` (Plover notation, `KAT/+-G`), or the plain read-back of its
    /// keys when no reading is close enough.
    pub fn answer(&self, outline: &str) -> Option<String> {
        let mut guard = self.scratch.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let text = self.answer_with(guard.get_or_insert_with(|| Scratch::new(self.node_count())), outline);
        drop(guard);
        text
    }

    /// [`Self::answer`] with the caller's own working memory.
    pub fn answer_with(&self, sc: &mut Scratch, outline: &str) -> Option<String> {
        let run: Vec<u32> = outline.split('/').map(parse_stroke).collect::<Option<_>>()?;
        if run.first().is_some_and(|s| s & SPELL != 0) {
            return self.spelled(&run);
        }
        self.answer_run(sc, &run)
    }

    /// What a run types: its late stars put on the strokes before them, each lone `+` locking in
    /// what came before it.
    fn answer_run(&self, sc: &mut Scratch, run: &[u32]) -> Option<String> {
        let run = late_stars(run);
        let Some(at) = last_lock(&run) else { return self.answer_locked(sc, &run, None) };
        let locked = self.answer_run(sc, &run[..at])?;
        let tail = &run[at + 1..];
        if tail.is_empty() {
            return Some(locked);
        }
        self.answer_locked(sc, tail, Some(&locked))
    }

    /// What a run with no lock or late star in it types after `locked`.
    fn answer_locked(&self, sc: &mut Scratch, run: &[u32], locked: Option<&str>) -> Option<String> {
        // A run past the solver's reach: its first [`MAX_RUN`] strokes read, the rest read back,
        // never dropped
        let (head, tail) = run.split_at(run.len().min(MAX_RUN));
        let limit = THRESHOLD + THRESHOLD_PER_STROKE * head.len() as u32;
        let mut text = match self.solve_locked(sc, head, 1, locked).into_iter().next() {
            Some(best) if best.cost <= limit => best.text,
            _ => format!("{}{}", locked.unwrap_or(""), self.read_back(head)?),
        };
        if !tail.is_empty() {
            text.push_str(&self.read_back(tail)?);
        }
        Some(text)
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
    // No keys is no stroke, but for the join key alone: the lone `+` that locks a run's text in
    (bits & !(JOIN | SPELL) != 0 || bits == JOIN).then_some(bits)
}

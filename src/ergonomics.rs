//! Physical finger ergonomics, separate from key count. Two strokes can cost the same number of
//! keys and feel completely different to type - `TK` (one finger, both its own rows, used
//! everywhere for the "D" sound) is comfortable, while `STWH` (alternating top/bottom across four
//! different fingers) is not. `cheapest` and the whole collision/brief system only ever counted
//! keys before this module existed.
//!
//! **Every number in this module traces to one of two places**: the real keyboard geometry
//! `keymux` actually presses (confirmed against `~/.config/keymux/config.ron`'s `steno` layer and
//! its default `qwerty.ron`), or a real finger-biomechanics measurement (Häger-Ross & Schieber,
//! *J Neurosci* 2000, "Quantifying the independence of human finger movements"). Nothing here is a
//! hand-picked constant chosen to make a particular word come out right.
//!
//! ## The keyboard geometry
//!
//! `keymux`'s steno layer presses ordinary QWERTY keys, two per column for most fingers (a home-row
//! key and the top-row key directly above it, the two keys a touch-typist already assigns to that
//! finger) - **not** a dedicated stenotype board with physical top/bottom banks. The one override
//! in the user's own config (`"+": Key(KC_Q)`) confirms the left pinky's column is `Q` (join, top)
//! over `A` (`S-`, home) exactly like every other column; `S-` itself has no top position of its
//! own and is fixed at the column's bottom. The right pinky's column is doubled: `T-`/`S-` on the
//! normal home/top pair (`;`/`P`), plus `D`/`Z` one key further right on the *same* two rows (`'`/
//! `[`) - a real further lateral reach, not a modeling choice. The right index gets the same kind
//! of second position, confirmed directly by the user: `*` is always struck by the right index via
//! `H` (home row, one key left of its own `J`) or `Y` (top row, one key left of its own `U`) - never
//! the left hand's `T`/`G` alternates the default layout also allows.
//!
//! So every key a finger can press reduces to a **reach vector** `(vertical, lateral)`, each either
//! 0 (the finger's own row) or 1 (one key away from it):
//!
//! | | no lateral stretch | with lateral stretch (right pinky's `D`/`Z`, right index's `*`) |
//! |---|---|---|
//! | home row | `(0, 0)` | `(0, 1)` |
//! | top row | `(1, 0)` | `(1, 1)` |
//!
//! A same-finger double press (`TK`, `PW`, or `*` landing on the same finger as `F`/`R2`) takes
//! whichever of its keys' vectors has the larger reach, not the sum of both - the finger is already
//! extending that far, so pressing the key it passes on the way there costs nothing extra.
//!
//! ## The two coupling tables
//!
//! Häger-Ross & Schieber measured two different things per digit, and this module uses each for
//! the physical situation it actually describes, instead of the one number pretending to cover
//! both (the module's own earlier revision's mistake):
//!
//! - **Individuation Index (II)**: how independently a finger can move *to its own target* while
//!   its neighbors are also moving to theirs. `1 - II` is the right quantity for two
//!   simultaneously-active fingers each demanding their own precise position - this module's
//!   **transition** cost.
//! - **Stationarity Index (SI)**: how still a finger can stay *while its neighbors move*. `1 - SI`
//!   is the right quantity for a finger that must be actively held up and out of the way between
//!   two active neighbors - this module's **gap** cost.
//!
//! Real values (self-paced, averaged across the paper's left/right hand columns, `1 − index`,
//! rounded to the nearest 0.001 and scaled ×1000 for clean integer constants):
//!
//! | Digit | 1 − II (`TRANS`) | 1 − SI (`GAP`) |
//! |---|---|---|
//! | Index | 22 | 30 |
//! | Middle | 64 | 67 |
//! | Ring | 98 | 94 |
//! | Little | 56 | 57 |
//!
//! The deficit (`1 - index`), not the raw index, is the physically meaningful quantity - it is
//! literally "the fraction of independent control this finger does not have" - and its spread
//! across fingers is far wider than the raw indices' own ~8% range suggests: ring's deficit is
//! ~4.5x index's, not the ~1.8x an earlier revision of this module capped itself to by rescaling
//! the raw index linearly instead of using the deficit directly.
//!
//! ## The cost itself
//!
//! For each hand independently, with `active` meaning "at least one of this finger's keys is
//! held," in physical order (pinky -> ring -> middle -> index, or its mirror):
//!
//! - **Transition cost**: for every pair of *consecutively active* fingers (skipping over a gap,
//!   which is costed separately below), `(Δvertical² + Δlateral²) * (TRANS[i] + TRANS[j])` between
//!   their reach vectors. This is the real fix for the model's founding bug: two fingers reading as
//!   the same *row* under the old binary model (a stretched reach and a plain top-row reach, both
//!   "top") now correctly cost something, because their vectors actually differ; two fingers at the
//!   literal same vector (a plain top-row reach next to another plain top-row reach) correctly cost
//!   nothing, because reaching to the same relative place *together* is a coordinated motion, not a
//!   conflicting one. Squaring the per-axis difference makes the cost convex: a chord asking two
//!   already-poorly-coupled fingers to each reach *further* costs much more than twice a chord
//!   asking them to reach a little.
//! - **Engagement cost**: once two or more fingers on a hand are active at once (a flat exemption
//!   that keeps any single-finger stroke, including a same-finger double, essentially free), each
//!   active finger also costs `(vertical + lateral) * TRANS[finger]` on its own - coordinating
//!   multiple simultaneously-moving fingers has a real cost even when they happen to agree on
//!   where to go, which the transition term alone (zero for agreeing fingers) can't capture.
//! - **Gap cost**: a finger with no key held, strictly between two active neighbors on the same
//!   hand, costs its own `GAP[finger]` flat - the motor cost of actively holding a finger out of
//!   the way, not a reach distance.
//! - **Pinky tax**: a small flat `PINKY_TAX` per press of a pinky key (`S-` on the left; `T-`, `S-`,
//!   `D`, `Z` on the right), excluding the join key (see below). This is **not** about
//!   independence - the data above already says the pinky isn't the least independent digit - it's
//!   a separate, smaller leverage/grip-strength tax, and the right pinky's extra reach is already
//!   priced in by its vector, so this tax no longer needs to double for `D`/`Z` the way an earlier
//!   revision did.
//!
//! The join key is excluded from every term above: it appears in essentially every multi-stroke
//! continuation regardless of which words are involved, so costing it would penalize "more than one
//! stroke," not any real word choice.
//!
//! This module is wired into `generate::candidate_cost` - see `docs/decisions.md` for the real
//! dictionary-wide numbers this scale produces and `cargo run -- ergonomics` to inspect any single
//! stroke's own breakdown term by term.

use crate::stroke::Stroke;

// Bit indices, matching stroke.rs's KEYS = "#STKPWHRAO*EUFRPBLGTSDZ"
const S: u32 = 1 << 1;
const T: u32 = 1 << 2;
const K: u32 = 1 << 3;
const P: u32 = 1 << 4;
const W: u32 = 1 << 5;
const H: u32 = 1 << 6;
const R: u32 = 1 << 7;
const F: u32 = 1 << 13;
const R2: u32 = 1 << 14;
const P2: u32 = 1 << 15;
const B: u32 = 1 << 16;
const L: u32 = 1 << 17;
const G: u32 = 1 << 18;
const T2: u32 = 1 << 19;
const S2: u32 = 1 << 20;
const D: u32 = 1 << 21;
const Z: u32 = 1 << 22;

/// A key's physical distance from its finger's own row, as `(vertical, lateral)`, each 0 or 1 -
/// see the module doc's geometry table.
type Reach = (u32, u32);
const HOME: Reach = (0, 0);
const TOP: Reach = (1, 0);
const LATERAL: Reach = (0, 1);
const STRETCH: Reach = (1, 1);

const fn norm(r: Reach) -> u32 {
    r.0 + r.1
}

const fn max_norm(a: Reach, b: Reach) -> Reach {
    if norm(a) >= norm(b) {
        a
    } else {
        b
    }
}

/// `1 - Individuation Index`, Häger-Ross & Schieber 2000, self-paced, averaged across the paper's
/// left/right hand columns, ×1000. Order: index, middle, ring, little. Used for transition and
/// engagement cost - see the module doc.
const TRANS: [u32; 4] = [22, 64, 98, 56];
/// `1 - Stationarity Index`, same source, same order. Used for gap cost only.
const GAP: [u32; 4] = [30, 67, 94, 57];

const LEFT_TRANS: [u32; 4] = [TRANS[3], TRANS[2], TRANS[1], TRANS[0]]; // little, ring, middle, index
const LEFT_GAP: [u32; 4] = [GAP[3], GAP[2], GAP[1], GAP[0]];
const RIGHT_TRANS: [u32; 4] = TRANS; // index, middle, ring, little
const RIGHT_GAP: [u32; 4] = GAP;

/// A small, flat leverage/grip-strength tax, not independence-based - see the module doc.
const PINKY_TAX: u32 = 5;

/// One finger's reach this stroke, and whether more than one of its own keys fired at once (a
/// same-finger double press, which still costs only its highest reach - see the module doc).
type FingerState = Option<(Reach, bool)>;

/// A plain two-key column (a home key and the top key directly above it): `T-`/`K-`, `P-`/`W-`,
/// `H-`/`R-` on the left; `F`/`R2`, `P2`/`B`, `L`/`G` on the right. Neither key has a lateral
/// stretch counterpart.
const fn pair_reach(bits: u32, top_bit: u32, home_bit: u32) -> FingerState {
    match (bits & top_bit != 0, bits & home_bit != 0) {
        (true, is_double) => Some((TOP, is_double)),
        (false, true) => Some((HOME, false)),
        (false, false) => None,
    }
}

/// The right pinky's four keys: the normal pair (`T-`/`S-`) plus the further-out stretch pair
/// (`D`/`Z`), on the same two rows.
fn right_pinky_reach(bits: u32) -> FingerState {
    let keys = [(T2, TOP), (S2, HOME), (D, STRETCH), (Z, LATERAL)];
    let mut best: Option<Reach> = None;
    let mut count = 0u32;
    for (bit, reach) in keys {
        if bits & bit != 0 {
            count += 1;
            best = Some(best.map_or(reach, |b| max_norm(b, reach)));
        }
    }
    best.map(|r| (r, count > 1))
}

/// The right index's normal pair (`F`/`R2`) plus `*`, confirmed (by the user, directly) to always
/// land on the right index via `H` (home-row lateral) or `Y` (top-row lateral) - never the left
/// hand's alternates. `*` fills whichever of the index's two rows `F`/`R2` isn't already using: if
/// `F` (top) is also held, `*` is `Y` (top + lateral); otherwise it's the cheaper `H` (lateral
/// only), which is also what it costs used alone, since a free choice always takes the cheaper key.
fn right_index_reach(bits: u32) -> FingerState {
    let f = bits & F != 0;
    let r2 = bits & R2 != 0;
    let star = bits & Stroke::STAR != 0;
    let count = u32::from(f) + u32::from(r2) + u32::from(star);
    if count == 0 {
        return None;
    }
    let mut reach = if f { TOP } else { HOME };
    if star {
        reach = max_norm(reach, if f { STRETCH } else { LATERAL });
    }
    Some((reach, count > 1))
}

/// A single stroke's ergonomic components. See the module doc for what each one means.
pub struct Score {
    /// Transition + engagement cost (see the module doc), both hands combined.
    pub reach_cost: u32,
    /// Gap cost, both hands combined.
    pub gap_cost: u32,
    /// `PINKY_TAX` times the number of pinky-key presses (excluding the join key).
    pub pinky_tax: u32,
    /// Raw count of adjacent active-finger pairs whose reach vectors actually differ (diagnostic -
    /// not separately weighted; folded into `reach_cost`).
    pub transitions: u32,
    /// Raw count of skipped-finger gaps (diagnostic; see `gap_cost`).
    pub gaps: u32,
    /// Raw count of same-finger double presses, both hands (diagnostic; a nominal `+1` each is
    /// still added in `weighted` so a double isn't literally free, just cheap).
    pub double_presses: u32,
    /// Right-pinky stretch-key (`D`/`Z`) presses specifically (diagnostic only now - their extra
    /// distance is already priced into `reach_cost` via their reach vector).
    pub stretch_keys: u32,
}

/// Both hands' finger states for a stroke's keys (join/spelling bits already stripped), pinky to
/// index each. Shared by `score` and `explain`.
fn hands(bits: u32) -> ([FingerState; 4], [FingerState; 4]) {
    let left: [FingerState; 4] = [
        (bits & S != 0).then_some((HOME, false)),
        pair_reach(bits, T, K),
        pair_reach(bits, P, W),
        pair_reach(bits, H, R),
    ];
    let right: [FingerState; 4] =
        [right_index_reach(bits), pair_reach(bits, P2, B), pair_reach(bits, L, G), right_pinky_reach(bits)];
    (left, right)
}

/// Scores one stroke's physical comfort. Independent of word, frequency, or key count - just the
/// shape of the keys actually held down.
#[must_use]
pub fn score(stroke: Stroke) -> Score {
    let bits = stroke.0 & !(Stroke::JOIN | Stroke::SPELL);
    let (left, right) = hands(bits);

    let walk = |fingers: &[FingerState; 4], trans: &[u32; 4], gap: &[u32; 4]| -> (u32, u32, u32, u32, u32) {
        let active: Vec<(usize, Reach)> =
            fingers.iter().enumerate().filter_map(|(i, f)| f.map(|(r, _)| (i, r))).collect();
        let doubles = u32::try_from(fingers.iter().filter(|f| f.is_some_and(|(_, d)| d)).count()).unwrap_or(u32::MAX);

        let mut transitions = 0;
        let mut reach_cost = 0;
        for pair in active.windows(2) {
            let (i, ri) = pair[0];
            let (j, rj) = pair[1];
            let dv = ri.0.abs_diff(rj.0);
            let dl = ri.1.abs_diff(rj.1);
            let dist2 = dv * dv + dl * dl;
            if dist2 > 0 {
                transitions += 1;
            }
            reach_cost += dist2 * (trans[i] + trans[j]);
        }
        if active.len() >= 2 {
            for &(i, r) in &active {
                reach_cost += norm(r) * trans[i];
            }
        }

        let mut gaps = 0;
        let mut gap_cost = 0;
        for i in 1..fingers.len() - 1 {
            if fingers[i].is_none() && fingers[i - 1].is_some() && fingers[i + 1].is_some() {
                gaps += 1;
                gap_cost += gap[i];
            }
        }
        (transitions, reach_cost, doubles, gaps, gap_cost)
    };

    let (lt, lrc, ld, lg, lgc) = walk(&left, &LEFT_TRANS, &LEFT_GAP);
    let (rt, rrc, rd, rg, rgc) = walk(&right, &RIGHT_TRANS, &RIGHT_GAP);
    let pinky_keys = u32::from(bits & S != 0) + (bits & (T2 | S2 | D | Z)).count_ones();
    let stretch_keys = (bits & (D | Z)).count_ones();

    Score {
        reach_cost: lrc + rrc,
        gap_cost: lgc + rgc,
        pinky_tax: PINKY_TAX * pinky_keys,
        transitions: lt + rt,
        gaps: lg + rg,
        double_presses: ld + rd,
        stretch_keys,
    }
}

/// A single weighted number, low = comfortable. See the module doc for the research and geometry
/// behind each term and `docs/decisions.md` for the real distribution this produces.
#[must_use]
pub fn weighted(stroke: Stroke) -> u32 {
    let s = score(stroke);
    s.reach_cost + s.gap_cost + s.pinky_tax + s.double_presses
}

/// A whole outline's ergonomic cost: the sum of each of its strokes' own.
#[must_use]
pub fn weighted_outline(outline: &[Stroke]) -> u32 {
    outline.iter().map(|&s| weighted(s)).sum()
}

const LEFT_NAMES: [&str; 4] = ["pinky (S-)", "ring (T-/K-)", "middle (P-/W-)", "index (H-/R-)"];
const RIGHT_NAMES: [&str; 4] = ["index (F/R/*)", "middle (P/B)", "ring (L/G)", "pinky (T/S/D/Z)"];

/// A line-by-line breakdown of one stroke's score: every term `weighted` actually adds, with
/// which finger and which pair caused it, not just the final number. The evaluation tool
/// (`cargo run -- ergonomics`) is this, printed.
#[must_use]
pub fn explain(stroke: Stroke) -> String {
    use std::fmt::Write as _;

    let bits = stroke.0 & !(Stroke::JOIN | Stroke::SPELL);
    let (left, right) = hands(bits);
    let mut out = String::new();
    let mut total = 0u32;

    let mut describe = |hand: &str, fingers: &[FingerState; 4], names: &[&str; 4], trans: &[u32; 4], gap: &[u32; 4]| {
        let active: Vec<(usize, Reach)> =
            fingers.iter().enumerate().filter_map(|(i, f)| f.map(|(r, _)| (i, r))).collect();
        if active.is_empty() {
            return;
        }
        let _ = writeln!(out, "{hand} hand:");
        for &(i, r) in &active {
            let double = fingers[i].is_some_and(|(_, d)| d);
            let tag = if double { " (same-finger double, free)" } else { "" };
            let _ = writeln!(out, "  {:<22} reach (vertical {}, lateral {}){tag}", names[i], r.0, r.1);
        }
        for pair in active.windows(2) {
            let (i, ri) = pair[0];
            let (j, rj) = pair[1];
            let dv = ri.0.abs_diff(rj.0);
            let dl = ri.1.abs_diff(rj.1);
            let cost = (dv * dv + dl * dl) * (trans[i] + trans[j]);
            if cost > 0 {
                let _ = writeln!(out, "  transition  {} <-> {}: +{cost}", names[i], names[j]);
                total += cost;
            }
        }
        if active.len() >= 2 {
            for &(i, r) in &active {
                let e = (r.0 + r.1) * trans[i];
                if e > 0 {
                    let _ = writeln!(out, "  engagement  {}: +{e}", names[i]);
                    total += e;
                }
            }
        }
        for i in 1..fingers.len() - 1 {
            if fingers[i].is_none() && fingers[i - 1].is_some() && fingers[i + 1].is_some() {
                let _ = writeln!(out, "  gap         {}: +{}", names[i], gap[i]);
                total += gap[i];
            }
        }
    };

    describe("left", &left, &LEFT_NAMES, &LEFT_TRANS, &LEFT_GAP);
    describe("right", &right, &RIGHT_NAMES, &RIGHT_TRANS, &RIGHT_GAP);

    let pinky_keys = u32::from(bits & S != 0) + (bits & (T2 | S2 | D | Z)).count_ones();
    if pinky_keys > 0 {
        let tax = PINKY_TAX * pinky_keys;
        let _ = writeln!(out, "pinky tax   ({pinky_keys} key(s)): +{tax}");
        total += tax;
    }
    let doubles = u32::try_from(left.iter().chain(right.iter()).filter(|f| f.is_some_and(|(_, d)| d)).count())
        .unwrap_or(u32::MAX);
    if doubles > 0 {
        let _ = writeln!(out, "double press(es): {doubles} x 1 = +{doubles}");
        total += doubles;
    }
    let s = score(stroke);
    let _ = writeln!(
        out,
        "summary: {} transition(s), {} gap(s), {} stretch key(s), {} double press(es)",
        s.transitions, s.gaps, s.stretch_keys, s.double_presses
    );
    let _ = write!(out, "total: {total}");
    debug_assert_eq!(total, weighted(stroke), "explain and weighted drifted apart");
    out
}

/// `explain`, for a whole outline: each stroke's own breakdown, then the outline's total.
#[must_use]
pub fn explain_outline(outline: &[Stroke]) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    for (i, &stroke) in outline.iter().enumerate() {
        if outline.len() > 1 {
            let _ = writeln!(out, "stroke {} ({stroke}):", i + 1);
        }
        out.push_str(&explain(stroke));
        out.push('\n');
    }
    if outline.len() > 1 {
        let _ = write!(out, "outline total: {}", weighted_outline(outline));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictionary_fixture;
    use crate::stroke::parse_outline;

    struct Band {
        name: &'static str,
        max_rank: usize,
        avg_ceiling: f64,
        bad_pct_ceiling: f64,
    }

    #[test]
    fn a_lone_same_finger_double_press_is_not_penalized_as_a_transition() {
        // TK ("D") and PW ("B") are foundational, universal steno chords, each one finger's own
        // two rows - a double press with nothing else active costs nothing but the press itself.
        for text in ["TK", "PW"] {
            let stroke = parse_outline(text).unwrap()[0];
            let s = score(stroke);
            assert_eq!(s.transitions, 0, "{text} should have no transitions");
            assert_eq!(s.double_presses, 1, "{text} should register its own double press");
            assert_eq!(s.reach_cost, 0, "{text} alone should have no reach cost either");
        }
    }

    #[test]
    fn alternating_rows_cost_more_than_a_staircase() {
        // SKPH: bottom, bottom, top, top - one staircase step.
        let skph = parse_outline("SKPH").unwrap()[0];
        // STWH: bottom, top, bottom, top - alternates every finger.
        let stwh = parse_outline("STWH").unwrap()[0];
        assert_eq!(score(skph).transitions, 1);
        assert_eq!(score(stwh).transitions, 3);
        assert!(weighted(stwh) > weighted(skph));
    }

    #[test]
    #[allow(clippy::similar_names)] // swr/skwr deliberately echo the SWR/SKWR outlines compared
    fn a_skipped_finger_between_two_active_neighbors_is_a_gap() {
        // SWR skips the ring (K) between the pinky (S) and middle (W).
        let swr = parse_outline("SWR").unwrap()[0];
        assert_eq!(score(swr).gaps, 1);
        // SKWR has no gap - every finger from pinky to index is active.
        let skwr = parse_outline("SKWR").unwrap()[0];
        assert_eq!(score(skwr).gaps, 0);
    }

    #[test]
    fn the_join_key_alone_never_costs_a_pinky_tax() {
        let joined = Stroke(Stroke::JOIN | (1 << 18)); // +-G, a plain continuation
        assert_eq!(score(joined).pinky_tax, 0);
    }

    #[test]
    fn two_fingers_reaching_the_same_relative_place_cost_less_than_reaching_different_ones() {
        // -LT: right ring's top (L) next to right little's plain top (T2) - same reach vector,
        // a coordinated motion, not a conflicting one.
        let lt = parse_outline("-LT").unwrap()[0];
        // -LD: right ring's top (L) next to right little's *stretch* top (D) - different reach
        // vectors even though the old binary model would have called both "top".
        let ld = parse_outline("-LD").unwrap()[0];
        assert!(weighted(ld) > weighted(lt), "stretching past a matching neighbor should cost more");
    }

    #[test]
    fn a_wider_reaching_chord_on_the_same_two_fingers_costs_more() {
        // -RLD: -LD plus the index resting at home - a real neighbor-to-neighbor mismatch next to
        // the ring, not just the ring-little pair alone.
        let ld = parse_outline("-LD").unwrap()[0];
        let rld = parse_outline("-RLD").unwrap()[0];
        assert!(weighted(rld) > weighted(ld));
    }

    #[test]
    fn star_on_the_right_index_costs_more_than_resting_at_home_next_to_a_stretched_neighbor() {
        // Confirmed directly: `*` is always the right index via H/Y, never the left hand's T/G.
        // `-*LD` asks the index to also reach a lateral position (H), which doesn't share an axis
        // with the ring's vertical-only reach (L) the way `-RLD`'s resting index (R2, home) does
        // not conflict with it either - but `*`'s own lateral reach is a genuine axis mismatch
        // against the ring's vertical one, on top of the ring-little mismatch both chords share.
        let rld = parse_outline("-RLD").unwrap()[0];
        let star_ld = parse_outline("*LD").unwrap()[0];
        assert!(
            weighted(star_ld) > weighted(rld),
            "an active star reaching a different axis than its neighbor should cost more than a \
             resting index next to the same neighbor"
        );
    }

    /// A score at or above this is "bad" for the `bad_pct_ceiling` counts below - ~4x this
    /// revision's own real top-1,000 average (136.96), the same ratio the previous revision's cutoff
    /// (40, ~4x its own 9.99 average) used, kept proportionate rather than carried over as a number.
    const BAD_SCORE: u32 = 550;

    /// Every ceiling here is set a little above a real scan of the dictionary, post-geometry
    /// revision (see `docs/decisions.md`'s ergonomics entries for the exact numbers and the
    /// research/geometry behind the new scale) - loose enough that adding a handful of new words
    /// doesn't break it, tight enough that a real regression still fails it.
    #[test]
    fn the_real_dictionary_stays_within_known_ergonomic_ceilings() {
        let setup = dictionary_fixture::setup();
        let lyk = dictionary_fixture::cheapest();

        let bands = [
            Band { name: "top 1,000", max_rank: 999, avg_ceiling: 150.0, bad_pct_ceiling: 5.0 },
            Band { name: "1,000-10,000", max_rank: 9_999, avg_ceiling: 250.0, bad_pct_ceiling: 12.0 },
            Band { name: "10,000+", max_rank: usize::MAX, avg_ceiling: 290.0, bad_pct_ceiling: 18.0 },
        ];

        for band in &bands {
            let min_rank = if band.name == "1,000-10,000" { 1_000 } else if band.name == "10,000+" { 10_000 } else { 0 };
            let scores: Vec<u32> = lyk
                .iter()
                .filter(|(word, _)| {
                    let rank = setup.rank.get(**word).copied().unwrap_or(usize::MAX);
                    rank >= min_rank && rank <= band.max_rank
                })
                .map(|(_, outline)| weighted_outline(outline))
                .collect();
            if scores.is_empty() {
                continue;
            }
            let n = f64::from(u32::try_from(scores.len()).unwrap());
            let avg = scores.iter().map(|&s| f64::from(s)).sum::<f64>() / n;
            let bad = scores.iter().filter(|&&s| s >= BAD_SCORE).count();
            let bad_pct = f64::from(u32::try_from(bad).unwrap()) * 100.0 / n;
            assert!(
                avg <= band.avg_ceiling,
                "{}: average ergonomic score {avg:.2} exceeds ceiling {}",
                band.name,
                band.avg_ceiling
            );
            assert!(
                bad_pct <= band.bad_pct_ceiling,
                "{}: {bad_pct:.2}% of words score >= {BAD_SCORE}, exceeds ceiling {}%",
                band.name,
                band.bad_pct_ceiling
            );
        }
    }
}

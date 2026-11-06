//! The fragment check: every key an outline presses must be explained by one of the word's own
//! sounds on the stroke it is struck on, so a brief or a folded form never carries a stray
//! consonant a learner could not account for. See `site/reference/briefs.json`.

use super::{Rules, Syllable};
use crate::stroke::Stroke;
use std::collections::HashMap;

impl Rules {
    /// Whether `bits` holds only part of `target`'s own keys: some of them down, but not all.
    /// Zero overlap is fine (the candidate isn't trying to be this sound at all, which is just an
    /// unrelated memorized key, not a fragment of anything); full overlap is fine (the sound is
    /// there, whole, however many other keys ride along with it); only a partial hit means a key
    /// combination this one sound needs *together* shows up with a piece of it missing.
    pub(super) const fn fragments(bits: u32, target: Stroke) -> bool {
        target.0.count_ones() > 1 && bits & target.0 != 0 && bits & target.0 != target.0
    }

    /// The keys a sound token is written with, checking the plain table first and the cluster
    /// table (`[onset_cluster]`/`[coda_cluster]`, built from more than one sound together) next.
    pub(super) fn token_bits(token: &str, table: &HashMap<String, Stroke>, cluster: &HashMap<String, Stroke>) -> Option<Stroke> {
        table.get(token).or_else(|| cluster.get(token)).copied()
    }

    /// Bits legitimately spoken for by some *other* onset or coda token than the one at
    /// `exclude` (a `(syllable index, is-coda-list, token index within that list)` triple) — a
    /// one-stroke brief for a multi-syllable word routinely holds more than one syllable's sounds
    /// at once, and a bit that's already some other token's own complete sound isn't a fragment of
    /// anything, no matter which unrelated cluster it also happens to overlap. This covers two
    /// different shapes of "other": every *other syllable's* own onset/coda tokens (used to clear
    /// a stray bit before checking it against the *first* syllable's onset or the *last*
    /// syllable's coda, so a real, complete middle — or, symmetrically, first/last — cluster can
    /// never be mistaken for a partial attempt at some other syllable's own sound; see
    /// [`Self::matches_real_consonants`]'s own doc comment for the `district`/`describe`/
    /// `captain` cases this resolves), and every *sibling* token within the exact same syllable as
    /// `exclude` (every onset/coda token of that syllable except `exclude` itself — two consonants
    /// struck on the same stroke can legitimately share a physical key between their own,
    /// completely different target chords, without either being a fragment of the other).
    ///
    /// `bits` is the actual stroke being checked (the one `onset_bits`/`coda_bits` is computed
    /// from) — only a target chord *fully* present in `bits` counts as explained; one merely
    /// *intended* by some other token's own sound, but not actually struck here at all, must never
    /// be subtracted. Requiring `bits & target.0 == target.0` (not just that the other token
    /// exists) is what fixes the cross-syllable case without disturbing `district`/`describe`/
    /// `captain`'s own real cases, where the other syllable's target genuinely is the stroke's
    /// whole content — see `docs/decisions.md` for the `beautiful` bug (`PW-FL`'s last syllable's
    /// onset `/f/`, target `TP`, sharing its `P` key with the first syllable's own cluster) this
    /// containment check was originally added for.
    ///
    /// The sibling-token half of this (excluding only `exclude`'s own list position, not its
    /// whole syllable) is what fixes `beautiful`'s own remaining gap: its first syllable's two
    /// onset tokens, "b" (target `PW`) and "y" (target `KWR`), are both legitimately struck at
    /// once on `PWAOUFL`, sharing the `W` key between their two separate chords. "b"'s own target
    /// is fully present, so checking "y" now finds `W` already explained by "b" (a sibling, not
    /// "y" itself) and clears before `y`'s own, now-unrelated, partial `KWR` overlap is tested —
    /// see `rules::tests::beautifuls_cross_syllable_and_sibling_token_false_flags_are_both_fixed`,
    /// which this fix turns into a fully-passing case for both tokens.
    ///
    /// `own_target` is `exclude`'s own target chord — needed only to guard the sibling case. A
    /// same-syllable *sibling*'s target only explains a bit away when it is not itself a (possibly
    /// equal) *subset* of `own_target`: Plover sometimes folds one token's entire sound into
    /// another, bigger token's own chord on purpose (`film`'s "l", target `-L`, is exactly the `L`
    /// key inside "m"'s own `-PL`; `frank`'s "k", target `-BG`, is exactly the `B G` inside "ng"'s
    /// own `-PBG`) — the smaller token is never a separate, independently-struck sound competing
    /// for the same key the way `beautiful`'s "b"/"y" genuinely are, so letting it "explain away"
    /// a key its own bigger sibling needs would wrongly turn that sibling's genuinely complete
    /// chord into a reported fragment. A genuinely *other syllable's* target has no such
    /// restriction (unchanged from before this sibling case existed, subset or not): `captain`'s
    /// own real case needs its first syllable's single-key coda "p" (`-P`, a subset of the *last*
    /// syllable's own dropped "n" target `-PB`) to explain away that very `P` - cross-syllable
    /// sounds are independent, never folded into each other the way same-syllable tokens can be.
    pub(super) fn explained_elsewhere(&self, syllables: &[Syllable], exclude: (usize, bool, usize), own_target: u32, bits: u32) -> u32 {
        let (ex_syllable, ex_coda, ex_token) = exclude;
        syllables
            .iter()
            .enumerate()
            .flat_map(|(i, syllable)| {
                let skip_onset = i == ex_syllable && !ex_coda;
                let skip_coda = i == ex_syllable && ex_coda;
                let same_syllable = i == ex_syllable;
                let onset = syllable
                    .onset
                    .iter()
                    .enumerate()
                    .filter(move |&(j, _)| !(skip_onset && j == ex_token))
                    .filter_map(move |(_, t)| Self::token_bits(t, &self.onset, &self.onset_cluster).map(|tb| (same_syllable, tb)));
                let coda = syllable
                    .coda
                    .iter()
                    .enumerate()
                    .filter(move |&(j, _)| !(skip_coda && j == ex_token))
                    .filter_map(move |(_, t)| Self::token_bits(t, &self.coda, &self.coda_cluster).map(|tb| (same_syllable, tb)));
                onset.chain(coda)
            })
            .filter(|&(same_syllable, target)| {
                let folded_into_own = same_syllable && target.0 & own_target == target.0;
                !folded_into_own && bits & target.0 == target.0
            })
            .fold(0, |acc, (_, stroke)| acc | stroke.0)
    }

    /// Whether this outline's first onset and last coda are each a whole key or cluster toward
    /// the word's own real first and last sound, never half of one. Plover and Lapwing's own
    /// shortest brief for a word is sometimes their own theory's memorized one-off trick that
    /// leaves only part of a key combination the rules only ever use together for *that* sound
    /// (`good`'s Plover brief `KPWAOD` keeps the `P W` of `TKPW`'s own `G` but drops the `T`;
    /// `work`'s `WORB` keeps only the `B` of `-BG`'s own `K`). That fragment still decodes as
    /// *some* sound, just never the one that was meant, which reads as arbitrary rather than
    /// learnable even to someone who already knows the brief. A whole key used as a pure
    /// memorized mnemonic completely unrelated to the word's own sound (`it` on the onset `T`,
    /// which "it" doesn't start with) isn't this — it shares no keys with "it" having no onset at
    /// all, so there's nothing to fragment, and it's exactly as learnable as any other memorized
    /// brief, nothing new to work out. Only a sound's *own* keys, partly present, are rejected.
    ///
    /// This only ever checks the *first* syllable's onset and the *last* syllable's coda — a
    /// one-stroke brief for a longer word routinely condenses every syllable in between onto that
    /// same stroke too, and [`Self::explained_elsewhere`] is what keeps those middle (or, the
    /// mirror case, first/last) syllables' own complete sounds from being mistaken for a fragment
    /// of the one actually being checked, in both directions this used to get wrong:
    /// `district`/`describe` reduce their own real first sound (a "d") to nothing, so their
    /// one-stroke brief's left-hand keys are entirely the *second* syllable's own "tr"/"skr"
    /// cluster — correctly whole, but sharing a key with "d"'s own onset cluster, which used to
    /// get this flagged as a fragment of "d" and "repaired" into dropping a real "t"/"k" out of
    /// "tr"/"skr" (`rules/briefs.toml` pinned both to their correct, literal outline to work
    /// around it). `captain` is the mirror case on the coda side: its own first syllable's real,
    /// legitimate coda "p" shares a key with the word's entirely-dropped final syllable's own "n"
    /// cluster (`-PB`), which used to flag the brief `KAPT` as holding only a fragment of "n" and
    /// offer `KAT` — `cat`'s own natural outline — as a "repair", displacing `cat` onto a starred
    /// variant for no reason related to `cat` at all. [`Self::explained_elsewhere`] also clears a
    /// *sibling* token's own fully-struck target within the very same syllable being checked —
    /// `beautiful`'s first syllable has two onset tokens, "b" (`PW`) and "y" (`KWR`), sharing a `W`
    /// key between their two separate, legitimately-struck chords; without this, "y"'s own check
    /// would see a stray partial `KWR` hit that is really just "b"'s own `W`, not a fragment of
    /// "y" at all.
    pub fn matches_real_consonants(&self, outline: &[Stroke], word: &str, sounds: &[String]) -> bool {
        let Some(syllables) = self.syllables(&self.label(word, sounds)) else {
            return true;
        };
        let (Some(first), Some(last), Some(&first_stroke), Some(&last_stroke)) = (
            syllables.first(),
            syllables.last(),
            outline.first(),
            outline.last(),
        ) else {
            return true;
        };
        let (first_window, last_window) = Self::stroke_windows(&syllables, outline.len());
        let last_index = last_window.len() - 1;
        let onset_mask = first_stroke.0 & Stroke::LEFT;
        let coda_mask = last_stroke.0 & (Stroke::RIGHT | Stroke::STAR);
        self.fragment_bits(first_window, 0, false, &first.onset, first_stroke.0, onset_mask, &self.onset, &self.onset_cluster) == 0
            && self.fragment_bits(last_window, last_index, true, &last.coda, last_stroke.0, coda_mask, &self.coda, &self.coda_cluster) == 0
    }

    /// The syllables an outline's first stroke and last stroke can each hold at most: every one
    /// of them when the outline is a single stroke (a one-stroke brief condenses every syllable
    /// onto it), otherwise `syllables - strokes + 1` from the matching end, since every other
    /// stroke holds at least one syllable of its own. [`Self::explained_elsewhere`] only looks
    /// inside this window: a sound that can only be on *another* stroke never explains a key on
    /// this one. Without it, `dictate`'s second stroke's own "t" (`TAEUT`) explained away the `T`
    /// of the first stroke's `TK` ("d"), leaving a lone `K` to be "repaired" into `TEUBG/TAEUT`
    /// (tick-tate); `seldom`'s first-stroke "l" did the same to the `L` of `-PL` ("m") on its
    /// second, repairing `S-L/TKOPL` into `S-L/TKOL`.
    pub(super) fn stroke_windows(syllables: &[Syllable], strokes: usize) -> (&[Syllable], &[Syllable]) {
        let window = syllables.len().saturating_sub(strokes.max(1)) + 1;
        let window = window.min(syllables.len());
        (&syllables[..window], &syllables[syllables.len() - window..])
    }

    /// The bits of `masked_bits` that are a fragment of one of `tokens`' own multi-key sounds —
    /// the part [`Self::matches_real_consonants`] rejects a candidate for holding. Each token is
    /// judged against its own, token-specific explained-bits set (see
    /// [`Self::explained_elsewhere`]): `full_bits` is the whole stroke being checked (what
    /// `explained_elsewhere` tests containment against), `masked_bits` is that same stroke
    /// narrowed to the side actually being checked (left for an onset, right/star for a coda) —
    /// a sibling token elsewhere in `tokens` (same syllable, same list, a different index) that is
    /// itself fully struck explains away the bits it owns before this token's own fragment test
    /// runs, exactly like another syllable's fully-struck target already did.
    #[allow(clippy::too_many_arguments)] // one small, cohesive bit-test; splitting it apart would
    // only scatter the same handful of values this one test needs together
    pub(super) fn fragment_bits(
        &self,
        syllables: &[Syllable],
        syllable_index: usize,
        in_coda: bool,
        tokens: &[String],
        full_bits: u32,
        masked_bits: u32,
        table: &HashMap<String, Stroke>,
        cluster: &HashMap<String, Stroke>,
    ) -> u32 {
        tokens
            .iter()
            .enumerate()
            .filter_map(|(i, t)| Self::token_bits(t, table, cluster).map(|target| (i, target)))
            .fold(0, |acc, (i, target)| {
                let explained = self.explained_elsewhere(syllables, (syllable_index, in_coda, i), target.0, full_bits);
                let bits = masked_bits & !explained;
                if Self::fragments(bits, target) {
                    acc | (bits & target.0)
                } else {
                    acc
                }
            })
    }

    /// A shorter, equally legitimate alternative to completing a fragment: drop it instead. A
    /// brief is always free to leave out a whole sound it isn't attempting to represent (`it` on
    /// the bare onset `T`, nothing of "it"'s own first sound at all, is exactly as fine as ever);
    /// what [`Self::matches_real_consonants`] rejects is holding only *part* of a sound's own key
    /// combination. Completing the fragment (the fallback already in place) and removing it
    /// outright are both ways to stop holding only part of it — removing is usually fewer keys,
    /// so this is offered as a second candidate alongside the word's full, correct outline, and
    /// the normal cheapest-and-correct selection picks whichever one is both. `None` if there is
    /// no fragment to drop, or dropping it would leave a stroke with no keys down at all.
    pub fn drop_fragments(&self, outline: &[Stroke], word: &str, sounds: &[String]) -> Option<Vec<Stroke>> {
        let syllables = self.syllables(&self.label(word, sounds))?;
        let (first, last, &first_stroke, &last_stroke) = (
            syllables.first()?,
            syllables.last()?,
            outline.first()?,
            outline.last()?,
        );
        let (first_window, last_window) = Self::stroke_windows(&syllables, outline.len());
        let last_index = last_window.len() - 1;
        let onset_mask = first_stroke.0 & Stroke::LEFT;
        let coda_mask = last_stroke.0 & (Stroke::RIGHT | Stroke::STAR);
        let onset_fragment = self.fragment_bits(first_window, 0, false, &first.onset, first_stroke.0, onset_mask, &self.onset, &self.onset_cluster);
        let coda_fragment = self.fragment_bits(last_window, last_index, true, &last.coda, last_stroke.0, coda_mask, &self.coda, &self.coda_cluster);
        if onset_fragment == 0 && coda_fragment == 0 {
            return None;
        }
        let mut out = outline.to_vec();
        let last_index = out.len() - 1;
        out[0] = Stroke(out[0].0 & !onset_fragment);
        out[last_index] = Stroke(out[last_index].0 & !coda_fragment);
        (out[0].0 != 0 && out[last_index].0 != 0).then_some(out)
    }
}

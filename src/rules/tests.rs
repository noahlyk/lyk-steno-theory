use super::*;
use crate::num::ToF64;
use crate::stroke::{parse_outline, render_outline};

fn sounds(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_owned).collect()
}

fn write(word: &str, text: &str) -> String {
    Rules::built_in()
        .unwrap()
        .outline(word, &sounds(text))
        .map_or_else(|| "-".to_owned(), |o| render_outline(&o))
}

#[test]
fn a_syllable_is_its_onset_vowel_and_coda() {
    assert_eq!(write("cat", "K AE1 T"), "KAT");
    assert_eq!(write("stop", "S T AA1 P"), "STOP");
    assert_eq!(write("dog", "D AO1 G"), "TKOG");
}

#[test]
fn spelling_picks_between_ways_to_write_a_sound() {
    // The same vowel is two keys written `oo` and three written `u`
    assert_eq!(write("moon", "M UW1 N"), "PHAOPB");
    assert_eq!(write("tune", "T UW1 N"), "TAOUPB");
    assert_eq!(write("when", "W EH1 N"), "WHEPB");
}

#[test]
fn an_unstressed_syllable_joins_the_stroke_before_it() {
    assert_eq!(write("living", "L IH1 V IH0 NG"), "HREUFG");
    assert_eq!(write("special", "S P EH1 SH AH0 L"), "SPERBL");
}

#[test]
fn a_suffix_that_does_not_fit_is_a_stroke_of_its_consonants() {
    // The stem keeps its consonant and the ending is a bare continuation stroke
    assert_eq!(write("ended", "EH1 N D AH0 D"), "EPBD/-D");
    assert_eq!(write("breaking", "B R EY1 K IH0 NG"), "PWRAEUBG/-G");
    assert_eq!(write("hidden", "HH IH1 D AH0 N"), "HEUD/-PB");
    // A reduced `er` that does not fold is -R alone
    assert_eq!(write("harder", "HH AA1 R D ER0"), "HARD/-R");
}

#[test]
fn two_stroke_suffixes_are_found_by_spelling_not_sound() {
    let rules = Rules::built_in().unwrap();
    // "ism" is the longer, correct match over any shorter coincidental suffix spelling.
    let (text, strokes) = rules.two_stroke_suffix("cubism").unwrap();
    assert_eq!(text, "ism");
    assert_eq!(render_outline(strokes), "+EUZ/+-PL");
    // "ify" the same way.
    let (text, strokes) = rules.two_stroke_suffix("quantify").unwrap();
    assert_eq!(text, "ify");
    assert_eq!(render_outline(strokes), "+U/+TPAOEU");
    // A word that IS the suffix, with nothing left for a stem, is not a match.
    assert!(rules.two_stroke_suffix("ism").is_none());
    // A word that merely contains the letters, not as a true suffix match at all, still
    // matches by spelling - this function only ever answers "could this be composed this
    // way," the backup pass's own stem lookup (does a placed word exist with this exact
    // remaining spelling) is what actually gates whether a backup entry gets built.
    assert!(rules.two_stroke_suffix("prism").is_some());
    // No suffix spelling present at all.
    assert!(rules.two_stroke_suffix("hello").is_none());
}

#[test]
fn consonants_only_join_the_stroke_before_in_steno_order() {
    assert_eq!(write("goodness", "G UH1 D N AH0 S"), "TKPWAOD/-PBS");
    // Except a lone L or R, which may land before the stem's last key as in Plover
    assert_eq!(write("lately", "L EY1 T L IY0"), "HRAEULT");
    assert_eq!(write("color", "K AH1 L ER0"), "KORL");
}

#[test]
fn a_prefix_has_keys_of_its_own() {
    assert_eq!(write("expect", "IH0 K S P EH1 K T"), "KPEBGT");
    assert_eq!(write("exist", "IH0 G Z IH1 S T"), "KPEUFT");
    assert_eq!(write("distance", "D IH1 S T AH0 N S"), "STKUPBS");
    // Not a prefix when nothing with a vowel follows
    assert_eq!(write("disk", "D IH1 S K"), "TKEUFBG");
}

#[test]
fn a_reduced_first_syllable_takes_its_spelled_vowel() {
    assert_eq!(write("believe", "B IH0 L IY1 V"), "PWE/HRAOEF");
    assert_eq!(write("about", "AH0 B AW1 T"), "A/PWOUT");
    // Not when the vowel is stressed
    assert_eq!(write("bee", "B IY1"), "PWAOE");
}

#[test]
fn a_sound_after_an_r_vowel_shares_the_r_key() {
    assert_eq!(write("serve", "S ER1 V"), "SEFRB");
    assert_eq!(write("march", "M AA1 R CH"), "PHAFRPB");
    assert_eq!(write("harsh", "HH AA1 R SH"), "HARB");
    // Only after the R
    assert_eq!(write("survive", "S ER0 V AY1 V"), "SUR/SRAOEUF");
}

#[test]
fn a_stressed_short_vowel_keeps_a_consonant() {
    assert_eq!(write("every", "EH1 V R IY0"), "EF/REU");
    assert_eq!(write("better", "B EH1 T ER0"), "PWERT");
}

#[test]
fn a_miss_is_read_back_into_sounds() {
    let rules = Rules::built_in().unwrap();
    let read = |text: &str| rules.resolve(&parse_outline(text).unwrap(), None);
    // A plain stroke that spells no word the rules know, read back the same way the rules
    // would have written it: onset, vowel, coda
    assert_eq!(read("STKPW"), Some("z".to_owned()));
    // Two labels can share the exact same keys (a plain sound and a spelling variant, or a
    // sound and its silent-glide cluster); the read-back always picks the same one
    for _ in 0..5 {
        assert_eq!(read("A/+A"), Some("aa".to_owned()));
    }
}

#[test]
fn a_suffix_stroke_reads_as_its_own_spelling_after_any_miss_not_just_a_known_one() {
    let rules = Rules::built_in().unwrap();
    let read = |text: &str| rules.resolve(&parse_outline(text).unwrap(), None);
    // `STKPW` ("z") has no dictionary entry at all, let alone one composed with any of
    // these suffixes - the continuation stroke must still read as the suffix's own real
    // spelling, not a generic phonetic guess at its bare consonants (`-D` alone would
    // otherwise read as plain "d", not "ed").
    assert_eq!(read("STKPW/+-D"), Some("zed".to_owned()));
    assert_eq!(read("STKPW/+-G"), Some("zing".to_owned()));
    assert_eq!(read("STKPW/+-S"), Some("zs".to_owned()));
    assert_eq!(read("STKPW/+-L"), Some("zly".to_owned()));
    assert_eq!(read("STKPW/+-PBS"), Some("zness".to_owned()));
    assert_eq!(read("STKPW/+*T"), Some("zth".to_owned()));
    // A suffix struck as the *first* stroke (no join bit set, since nothing precedes it) is
    // not this case at all - it reads as the generic phonetic guess at its bare consonants,
    // same as any other bare consonant-only stroke would (and, in a real dictionary, this
    // exact outline is already a literal entry for the suffix itself - see
    // `rules/suffixes.toml` - so `resolve` is never even reached for it there).
    assert_eq!(read("-D"), Some("d".to_owned()));
}

#[test]
fn a_run_whose_first_stroke_is_a_known_word_reads_as_that_word_plus_the_suffix() {
    let rules = Rules::built_in().unwrap();
    let skp = parse_outline("SKP").unwrap()[0];
    let pwhr = parse_outline("PWHR").unwrap()[0];
    let mut stems = HashMap::new();
    stems.insert(skp, "and".to_owned());
    stems.insert(pwhr, "build".to_owned());
    let read = |text: &str| rules.resolve(&parse_outline(text).unwrap(), Some(&stems));
    // `SKP` alone has no phonetic reading that spells "and" at all (it's a memorized brief,
    // not derived from "and"'s own sounds) - without the stem lookup this used to read back
    // as whatever `SKP` means phonetically, garbling the word entirely.
    assert_eq!(read("SKP/+-R"), Some("ander".to_owned()));
    assert_eq!(read("PWHR/+-D"), Some("builded".to_owned()));
    // With no stems table at all (the plain phonetic fallback, e.g. a stroke nothing has
    // ever placed), the first stroke is read phonetically same as always.
    assert_eq!(rules.resolve(&parse_outline("SKP/+-R").unwrap(), None), Some("sper".to_owned()));
    // A single stroke that happens to be a known word's own outline is never routed through
    // the stem table - a real dictionary would have already answered it directly, so this
    // case should never actually reach `resolve` with `outline.len() == 1`.
    assert_eq!(read("SKP"), Some("sp".to_owned()));
}

#[test]
fn a_stems_own_silent_e_drops_before_a_vowel_initial_continuation() {
    let rules = Rules::built_in().unwrap();
    let sr = parse_outline("SR").unwrap()[0];
    let mut stems = HashMap::new();
    let read = |rules: &Rules, stems: &HashMap<Stroke, String>, text: &str| {
        rules.resolve(&parse_outline(text).unwrap(), Some(stems))
    };
    // "have" + the standalone "-ing" suffix used to concatenate literally ("haveing"); English
    // drops a stem's own trailing silent `e` before a vowel-initial suffix.
    stems.insert(sr, "have".to_owned());
    assert_eq!(read(&rules, &stems, "SR/+-G"), Some("having".to_owned()));
    // A consonant-initial suffix never drops it ("entirely", not "entirly" - not a real stem
    // here, but the same mechanism, exercised through a made-up one).
    stems.insert(sr, "entire".to_owned());
    assert_eq!(read(&rules, &stems, "SR/+-L"), Some("entirely".to_owned()));
    // An `e` that wasn't silent to begin with (preceded by another vowel) never drops either.
    stems.insert(sr, "agree".to_owned());
    assert_eq!(read(&rules, &stems, "SR/+-G"), Some("agreeing".to_owned()));
}

#[test]
fn a_word_with_no_pronunciation_can_be_spelled_letter_by_letter() {
    let rules = Rules::built_in().unwrap();
    let read = |text: &str| rules.resolve(&parse_outline(text).unwrap(), None);
    // "aboveground" has no entry in CMUdict, MFA, or WikiPron (see `crate::ipa` and
    // `crate::data::fallback_pronunciations`), so the phonetic rules have nothing at all to
    // generate from; spelling mode (the `~` marker on the first stroke, `+` continuing every
    // one after it, same as any other multi-stroke word) reads it back letter-exact instead.
    // `spell_word` itself is a pure letter-to-chord table lookup with no pronunciation input,
    // so this and the next test would pass for any a-z word regardless - "aboveground" is
    // used because it is real, still-unrescued example data, not because the mechanism here
    // depends on it specifically.
    assert_eq!(
        read("~A/+PW/+O/+SR/+E/+TKPW/+R/+O/+U/+TPH/+TK"),
        Some("aboveground".to_owned())
    );
    // A chord with no entry in rules/spelling.toml is a true miss, same as any other.
    assert_eq!(read("~*"), None);
}

#[test]
fn spell_word_is_resolve_spellings_exact_inverse() {
    let rules = Rules::built_in().unwrap();
    // The encode direction: given just the word, not a stroke sequence someone already wrote
    // out by hand, produce the same outline `docs/spelling-mode.md` hand-writes for
    // "aboveground" - a word with no pronunciation on record anywhere (CMUdict, MFA, or
    // WikiPron) for the phonetic rules to run from.
    let outline = rules.spell_word("aboveground").unwrap();
    assert_eq!(render_outline(&outline), "~A/+PW/+O/+SR/+E/+TKPW/+R/+O/+U/+TPH/+TK");
    // Round-trips back through the decode direction exactly.
    assert_eq!(rules.resolve(&outline, None), Some("aboveground".to_owned()));
    // A character spelling mode has no chord for (anything but a-z) is a true miss, same as
    // a stroke chord with no entry in the table.
    assert_eq!(rules.spell_word("o'clock"), None);
}

#[test]
fn only_a_truly_empty_stroke_is_a_true_miss() {
    let rules = Rules::built_in().unwrap();
    // Every key this theory uses means something alone or in some combination; what has no
    // reading at all is a stroke with nothing left once the star and join bits are set
    // aside: no onset, no vowel, no coda.
    assert_eq!(rules.resolve_stroke(Stroke(Stroke::STAR)), None);
    assert_eq!(rules.resolve_stroke(Stroke(Stroke::JOIN)), None);
    assert_eq!(
        rules.resolve_stroke(Stroke(Stroke::STAR | Stroke::JOIN)),
        None
    );
}

#[test]
fn an_earlier_syllables_own_sound_is_never_mistaken_for_a_fragment_of_a_later_one() {
    let rules = Rules::built_in().unwrap();
    // "captain"'s own first syllable really does end in a "p" (K AE1 | P T AH0 N); that "p"
    // happens to share a key with the entirely-dropped final syllable's "n" cluster (`-PB`),
    // which used to make `KAPT` look like it held only a fragment of "n" - wrongly, since the
    // brief isn't attempting "n" at all. See `rules/briefs.toml`'s `district`/`describe` note
    // for the mirror case on the onset side.
    let captain = sounds("K AE1 P T AH0 N");
    let kapt = parse_outline("KAPT").unwrap();
    assert!(rules.matches_real_consonants(&kapt, "captain", &captain));
    assert_eq!(rules.drop_fragments(&kapt, "captain", &captain), None);
    // `district`'s real first sound (a "d") reduces to nothing, so its one-stroke brief's
    // left-hand keys are entirely the *second* syllable's own "tr" cluster - correctly whole,
    // but sharing a key with "d"'s own onset cluster (`TK`).
    let district = sounds("D IH1 S T R IH0 K T");
    let tr_t = parse_outline("TR-T").unwrap();
    assert!(rules.matches_real_consonants(&tr_t, "district", &district));
    assert_eq!(rules.drop_fragments(&tr_t, "district", &district), None);
}

#[test]
fn a_sound_on_another_stroke_never_explains_a_key_on_this_one() {
    let rules = Rules::built_in().unwrap();
    // "dictate" as Plover writes it, one syllable per stroke: the second stroke's own "t"
    // (`T` of `TAEUT`) used to explain away the `T` of the first stroke's `TK` ("d"), leaving
    // a lone `K` to be "repaired" out - `TEUBG/TAEUT`, which reads "tick-tate".
    let dictate = sounds("D IH0 K T EY1 T");
    let outline = parse_outline("TKEUBG/TAEUT").unwrap();
    assert!(rules.matches_real_consonants(&outline, "dictate", &dictate));
    assert_eq!(rules.drop_fragments(&outline, "dictate", &dictate), None);
    // The mirror case on the coda side: "seldom"'s first-stroke "l" used to explain away the
    // `L` of its second stroke's `-PL` ("m"), repairing `S-L/TKOPL` into `S-L/TKOL` ("dol").
    let seldom = sounds("S EH1 L D AH0 M");
    let outline = parse_outline("S-L/TKOPL").unwrap();
    assert!(rules.matches_real_consonants(&outline, "seldom", &seldom));
    assert_eq!(rules.drop_fragments(&outline, "seldom", &seldom), None);
    // A one-stroke outline still lets every syllable explain every other: `district`'s case
    // above is unchanged.
    let syllables = rules.syllables(&rules.label("dictate", &dictate)).unwrap();
    let (first, last) = Rules::stroke_windows(&syllables, 2);
    assert_eq!((first.len(), last.len()), (1, 1));
    let (first, last) = Rules::stroke_windows(&syllables, 1);
    assert_eq!((first.len(), last.len()), (2, 2));
}

#[test]
fn a_reduced_er_continuation_keeps_its_r_when_its_onset_goes_on_the_left() {
    let rules = Rules::built_in().unwrap();
    // "-ner" after a stem that can't take the "n": the onset goes on the left and the `R` of
    // the reduced "er" stays on the right - `TPH-R`, never a bare `TPH` that reads as "n".
    let examiner = sounds("IH0 G Z AE1 M AH0 N ER0");
    let outline = rules.outline("examiner", &examiner).unwrap();
    let last = render_outline(&outline[outline.len() - 1..]);
    assert!(last.ends_with('R'), "examiner's continuation is {last}, which has lost its -er");
    let amber = sounds("AE1 M B ER0");
    let outline = rules.outline("amber", &amber).unwrap();
    assert_eq!(render_outline(&outline), "APL/PW-R");
}

#[test]
fn every_word_of_two_syllables_has_syllable_paths_a_learner_can_work_out() {
    let rules = Rules::built_in().unwrap();
    let text = |word: &str, pron: &str| -> Vec<String> {
        rules.syllable_paths(word, &sounds(pron)).iter().map(|o| render_outline(o)).collect()
    };
    // A reduced last syllable struck as a suffix is, with every vowel written
    let table = text("table", "T EY1 B AH0 L");
    assert!(table.contains(&"TAEU/-BL".to_owned()), "{table:?}");
    assert!(table.contains(&"TAEU/PWUL".to_owned()), "{table:?}");
    // Both ways to hear where the syllables split: `lem-on` and `le-mon`
    let lemon = text("lemon", "L EH1 M AH0 N");
    assert!(lemon.iter().any(|o| o.starts_with("HREPL/")), "{lemon:?}");
    assert!(lemon.iter().any(|o| o.starts_with("HRE/")), "{lemon:?}");
    // An unstressed schwa that keeps its vowel is `U` whatever letter spells it, and also the
    // letter that does
    let profitable = text("profitable", "P R AA1 F AH0 T AH0 B AH0 L");
    assert!(profitable.contains(&"PRAF/U/TU/-BL".to_owned()), "{profitable:?}");
    assert!(profitable.contains(&"PRAF/O/TO/-BL".to_owned()), "{profitable:?}");
    let compromise = text("compromise", "K AA1 M P R AH0 M AY2 Z");
    assert!(compromise.contains(&"KOPL/PRO/PHAOEUZ".to_owned()), "{compromise:?}");
    // One syllable: nothing to join
    assert!(text("cat", "K AE1 T").is_empty());
}

#[test]
fn explained_elsewhere_only_counts_a_target_fully_struck_on_the_checked_stroke() {
    // A direct, isolated test of the fix below (not entangled with any real word's own extra
    // complications): a later syllable's own onset token "f" (target `TP`, two keys) used to
    // get OR'd into "explained" bits just because the syllable exists, regardless of whether
    // its own target was actually struck on the stroke being checked at all.
    let rules = Rules::built_in().unwrap();
    let syllables = vec![
        Syllable { onset: vec![], vowel: "AH0".to_owned(), coda: vec![] },
        Syllable { onset: vec!["F".to_owned()], vowel: "AH0".to_owned(), coda: vec![] },
    ];
    let f_target = Rules::token_bits("F", &rules.onset, &rules.onset_cluster).unwrap().0;
    assert_eq!(f_target.count_ones(), 2, "\"f\" (TP) is a genuine two-key target to test with");
    // Full containment: the stroke genuinely has every one of "f"'s own keys down (T and P
    // both) - this is "d"/"tr" (`district`) and "n"/"p" (`captain`)'s own real shape above,
    // and must still be explained away.
    // `exclude` is `(0, false, 0)`: syllable 0's own (empty) onset, token index 0 - a no-op
    // exclusion, since syllable 0 has no tokens at all; what's actually being measured here is
    // whether syllable 1's own "f" is counted as explained. `own_target` (0 here) only matters
    // for a same-syllable sibling, which this test never exercises (the one real candidate,
    // "f", always comes from the *other* syllable).
    assert_eq!(rules.explained_elsewhere(&syllables, (0, false, 0), 0, f_target), f_target);
    // Partial containment: only one of "f"'s two keys is actually down on this stroke - "f"'s
    // sound was never struck here at all, so none of its target should be treated as
    // explained. This is "beautiful"'s own real case: `PWAOUFL` has one of "f"'s `TP` keys
    // down (borrowed from "b"'s own, separate, legitimate `PW`), but not the other at all.
    let one_key_only = f_target.isolate_lowest_one();
    assert_eq!(rules.explained_elsewhere(&syllables, (0, false, 0), 0, one_key_only), 0);
    // Absent entirely: nothing of "f" struck at all - also not explained, trivially.
    assert_eq!(rules.explained_elsewhere(&syllables, (0, false, 0), 0, 0), 0);
}

#[test]
fn beautifuls_cross_syllable_and_sibling_token_false_flags_are_both_fixed() {
    // "beautiful"'s own gold brief, `PWAOUFL` (B Y UW1 T AH0 F AH0 L), used to fail
    // `matches_real_consonants` entirely over a cross-syllable bug: the *last* syllable's own
    // onset "f" (target `TP`) shares its `P` key with "b"'s own, separate, legitimate `PW` -
    // but `PWAOUFL` never has a `T` down anywhere, so "f"'s sound isn't struck here at all, and
    // `explained_elsewhere` used to subtract "f"'s target unconditionally regardless. Fixed by
    // requiring full containment - "b"'s own flag is gone:
    let rules = Rules::built_in().unwrap();
    let beautiful = sounds("B Y UW1 T AH0 F AH0 L");
    let pwaoufl = parse_outline("PWAOUFL").unwrap();
    let b_target = Rules::token_bits("B", &rules.onset, &rules.onset_cluster).unwrap();
    assert!(
        !Rules::fragments(pwaoufl[0].0 & Stroke::LEFT, b_target),
        "\"b\"'s own target (PW) is now recognized as fully present, not a fragment"
    );
    // A second, separate bug, previously left deliberately unfixed: "y"'s own target (`KWR`)
    // shares its `W` key with "b"'s own `PW` - since "b"'s own chord genuinely does strike `W`,
    // and "y"'s target also needs that same physical key, checking "y" against the syllable's
    // shared onset bits used to find a real partial overlap that had nothing to do with
    // cross-syllable explanation at all (both tokens are in the *same* first syllable).
    // `explained_elsewhere` now also excludes a *sibling* onset/coda token's own fully-struck
    // target within the same syllable - "b"'s own full `PW` explains away the shared `W` before
    // "y"'s own check runs, so "y" sees nothing left to overlap with its `KWR` at all, and the
    // word's own gold brief now clears `matches_real_consonants` entirely, both tokens:
    assert!(rules.matches_real_consonants(&pwaoufl, "beautiful", &beautiful));
    let y_target = Rules::token_bits("Y", &rules.onset, &rules.onset_cluster).unwrap();
    let syllables = rules.syllables(&rules.label("beautiful", &beautiful)).unwrap();
    let onset_mask = pwaoufl[0].0 & Stroke::LEFT;
    let explained = rules.explained_elsewhere(&syllables, (0, false, 1), y_target.0, pwaoufl[0].0);
    assert!(
        !Rules::fragments(onset_mask & !explained, y_target),
        "\"y\"'s own shared W key is now explained away by its sibling \"b\", not a fragment"
    );
}

#[test]
fn a_sibling_token_folded_entirely_into_a_bigger_ones_chord_never_explains_it_away() {
    // "film" (F IH1 L M): the coda is two tokens, "l" (target `-L`, one key) and "m" (target
    // `-PL`, two keys) - Plover's own outline, `TPEUPL`, folds "l" entirely into "m"'s own `-PL`
    // chord (there's no separate `-L` key struck for "l" at all, just the `P L` that spells
    // "m"). A naive sibling fix (exclude any *other* token's fully-struck target, with no
    // further check) would see "l"'s own trivial single-key target fully present and subtract
    // its `L` key before checking "m" - wrongly turning "m"'s genuinely complete `-PL` into a
    // reported fragment (`stray=-P`), since "l"'s target is really just a subset of "m"'s own,
    // not a separate, competing sound sharing a key the way `beautiful`'s "b"/"y" do.
    let rules = Rules::built_in().unwrap();
    let film = sounds("F IH1 L M");
    let tpeupl = parse_outline("TPEUPL").unwrap();
    assert!(
        rules.matches_real_consonants(&tpeupl, "film", &film),
        "\"m\"'s complete -PL must not be flagged just because \"l\"'s own target is a subset of it"
    );
    assert_eq!(rules.drop_fragments(&tpeupl, "film", &film), None);
    // "frank" (F R AE1 NG K): same shape, but with a *two*-key subset this time - the coda
    // tokens are "ng" (target `-PBG`, three keys) and "k" (target `-BG`, two keys), and "k"'s
    // own `-BG` is a strict subset of "ng"'s own `-PBG`. A fix that only special-cased
    // single-key siblings (an earlier, narrower version of this fix) would still wrongly flag
    // "ng" here even after fixing "film".
    let frank = sounds("F R AE1 NG K");
    let tprapbg = parse_outline("TPRAPBG").unwrap();
    assert!(
        rules.matches_real_consonants(&tprapbg, "frank", &frank),
        "\"ng\"'s complete -PBG must not be flagged just because \"k\"'s own target is a subset of it"
    );
    assert_eq!(rules.drop_fragments(&tprapbg, "frank", &frank), None);
}

#[test]
fn spelling_tells_words_that_sound_the_same_apart() {
    let rules = Rules::built_in().unwrap();
    let sound = sounds("S EY1 L");
    // The same outline either way; only the silent-e word moves aside when they clash
    assert_eq!(write("sail", "S EY1 L"), write("sale", "S EY1 L"));
    assert_eq!(rules.variant("sail", &sound), None);
    let sale = rules.variant("sale", &sound).unwrap();
    assert_eq!(render_outline(&sale), "SAEL");
}

#[test]
fn a_long_word_can_be_one_skeleton_stroke() {
    let rules = Rules::built_in().unwrap();
    let forms = |word: &str, text: &str| -> Vec<String> {
        rules
            .skeletons(word, &sounds(text))
            .into_iter()
            .map(|(step, vowel, stroke)| {
                format!("{step}{} {stroke}", if vowel { "+v" } else { "" })
            })
            .collect()
    };
    // Every consonant after the first vowel in steno order, bare and with the stressed vowel
    let window = forms("window", "W IH1 N D OW0");
    assert_eq!(window[..2], ["skeleton W-PBD", "skeleton+v WEUPBD"]);
    assert!(window.contains(&"ends W-D".to_owned()), "{window:?}");
    // The L of `-ly` would land before the T, so only the last syllable's form is in order
    assert_eq!(
        forms("lately", "L EY1 T L IY0"),
        ["ends HR-L", "ends+v HRAEUL"]
    );
    // Out of order (the D of `-dish` would land after the SH), neither the full skeleton
    // nor the stressed-and-last form is offered; the last syllable's form is Plover's brief
    assert_eq!(
        forms("condition", "K AH0 N D IH1 SH AH0 N"),
        ["ends K-PB", "ends+v KEUPB"]
    );
    // A dropped `a-` leaves the next onset as the left hand; a form never lacks one
    let america = forms("america", "AH0 M EH1 R IH0 K AH0");
    assert!(america.iter().all(|f| f.contains(" PH")), "{america:?}");
    // An unstressed `in-` is the left hand, with the rest on the right
    let information = forms("information", "IH2 N F ER0 M EY1 SH AH0 N");
    assert!(
        information.iter().all(|f| f.contains(" TPH")),
        "{information:?}"
    );
}

/// Diagnostic: measures how often the *final, settled* dictionary (what `build` actually
/// ships, after briefs/compressions/backups all compete) lands on an outline that keeps a
/// stray, partial trace of one of this theory's required multi-key chords — the thing
/// [`Rules::matches_real_consonants`] flags, now that `gold_candidates` (`src/generate/`,
/// commit b070938) no longer blanket-rejects those candidates before they can win. Two
/// outcomes for a word whose chosen outline shares *any* key with its own first syllable's
/// onset chord or last syllable's coda chord: holding *none* of that chord's keys is a clean,
/// deliberate drop (Category A, already accepted); holding *some* but not all of them is a
/// misleading leftover (Category B, e.g. `form` -> `HORPL`, a stray `H`) — see this module's
/// own `fragments`/`fragment_bits` for the exact bit test this reuses.
///
/// Run with `cargo test --release -- --ignored --nocapture dump_stray_fragment_leftovers`.
#[test]
#[ignore = "diagnostic: measures stray-fragment leftovers in the final, settled dictionary"]
fn dump_stray_fragment_leftovers() {
    let setup = crate::dictionary_fixture::setup();
    let rules = &setup.rules;
    let mut tally = StrayTally::default();
    for (&word, outline) in &crate::dictionary_fixture::cheapest() {
        // Needs a pronunciation and a syllable split to know where the first onset and last coda
        // are; a word the rules can't label (an acronym with no entry) is skipped, not guessed.
        let Some(sounds) = setup.pron.get(word).and_then(|variants| variants.first()) else { continue };
        let Some(syllables) = rules.syllables(&rules.label(word, sounds)) else { continue };
        let (Some(first), Some(last), Some(&first_stroke), Some(&last_stroke)) =
            (syllables.first(), syllables.last(), outline.first(), outline.last())
        else {
            continue;
        };
        let sides = [
            Side {
                name: "onset",
                syllable: 0,
                in_coda: false,
                tokens: &first.onset,
                stroke: first_stroke.0,
                mask: first_stroke.0 & Stroke::LEFT,
                table: &rules.onset,
                cluster: &rules.onset_cluster,
            },
            Side {
                name: "coda",
                syllable: syllables.len() - 1,
                in_coda: true,
                tokens: &last.coda,
                stroke: last_stroke.0,
                mask: last_stroke.0 & (Stroke::RIGHT | Stroke::STAR),
                table: &rules.coda,
                cluster: &rules.coda_cluster,
            },
        ];
        let weight = setup.count.get(word).copied().unwrap_or(0).to_f64();
        let stray = sides.iter().any(|side| {
            rules.fragment_bits(&syllables, side.syllable, side.in_coda, side.tokens, side.stroke, side.mask, side.table, side.cluster)
                != 0
        });
        let found: Vec<(String, String)> =
            if stray { sides.iter().flat_map(|side| strays(rules, &syllables, side)).collect() } else { Vec::new() };
        tally.add(word, &render_outline(outline), &sounds.join(" "), weight, stray, &found);
    }
    tally.print();
}

/// One end of a word's outline the stray-fragment diagnostic checks: the first syllable's onset
/// on the first stroke, or the last syllable's coda on the last.
struct Side<'a> {
    name: &'static str,
    syllable: usize,
    in_coda: bool,
    tokens: &'a [String],
    stroke: u32,
    mask: u32,
    table: &'a HashMap<String, Stroke>,
    cluster: &'a HashMap<String, Stroke>,
}

/// Each sound on `side` whose chord the stroke holds only part of, as (label, the stray keys).
/// Per sound, like `Rules::fragment_bits`: what other sounds explain never includes its own keys.
fn strays(rules: &Rules, syllables: &[Syllable], side: &Side) -> Vec<(String, String)> {
    side.tokens
        .iter()
        .enumerate()
        .filter_map(|(i, token)| {
            let target = Rules::token_bits(token, side.table, side.cluster)?;
            let explained = rules.explained_elsewhere(syllables, (side.syllable, side.in_coda, i), target.0, side.stroke);
            let bits = side.mask & !explained;
            Rules::fragments(bits, target).then(|| {
                let label = format!("{} /{token}/ ({})", side.name, render_outline(&[target]));
                (label, render_outline(&[Stroke(bits & target.0)]))
            })
        })
        .collect()
}

/// What the stray-fragment diagnostic counts: every word, the clean drops (category A), the stray
/// leftovers (category B) with their details, and which chords the strays come from.
#[derive(Default)]
struct StrayTally {
    words: usize,
    weight: f64,
    clean: (usize, f64),
    stray: (usize, f64),
    hits: Vec<(String, String, String, String, f64)>,
    chords: HashMap<String, (usize, f64)>,
}

impl StrayTally {
    fn add(&mut self, word: &str, outline: &str, sounds: &str, weight: f64, stray: bool, found: &[(String, String)]) {
        self.words += 1;
        self.weight += weight;
        let bucket = if stray { &mut self.stray } else { &mut self.clean };
        bucket.0 += 1;
        bucket.1 += weight;
        if !stray {
            return;
        }
        for (label, _) in found {
            let chord = self.chords.entry(label.clone()).or_default();
            chord.0 += 1;
            chord.1 += weight;
        }
        let detail = found.iter().map(|(label, stray)| format!("{label} stray={stray}")).collect::<Vec<_>>().join("; ");
        self.hits.push((word.to_owned(), outline.to_owned(), sounds.to_owned(), detail, weight));
    }

    fn print(mut self) {
        let share = |n: usize, w: f64| (100.0 * n.to_f64() / self.words.to_f64(), 100.0 * w / self.weight);
        let (clean_words, clean_weight) = share(self.clean.0, self.clean.1);
        let (stray_words, stray_weight) = share(self.stray.0, self.stray.1);
        println!(
            "words checked: {} (usage weight {:.0})\n\
             Category A (clean drop):   {:>6} words ({clean_words:.3}% of words), {:.0} usage ({clean_weight:.3}% of weighted typing)\n\
             Category B (stray leftover): {:>6} words ({stray_words:.3}% of words), {:.0} usage ({stray_weight:.3}% of weighted typing)",
            self.words, self.weight, self.clean.0, self.clean.1, self.stray.0, self.stray.1,
        );
        self.hits.sort_by(|a, b| b.4.total_cmp(&a.4));
        println!("\n--- top 40 Category B words by usage weight ---");
        for (word, outline, sounds, detail, weight) in self.hits.iter().take(40) {
            println!("{word:<16} outline={outline:<14} sounds=[{sounds}]  {detail}  weight={weight:.0}");
        }
        let mut chords: Vec<(&String, &(usize, f64))> = self.chords.iter().collect();
        chords.sort_by(|a, b| b.1.1.total_cmp(&a.1.1));
        println!("\n--- stray chord/sound breakdown (by distinct-word count and usage weight) ---");
        for (label, (count, weight)) in chords {
            println!("{label:<28} words={count:<6} weight={weight:.0}");
        }
    }
}

#[test]
fn sounds_may_share_a_key_already_down() {
    // The p of m-p-r is inside the PH of the m: one chord, PHR, not a refusal
    let rules = Rules::built_in().unwrap();
    let paths = rules.lattice_paths("compromise", &sounds("K AA1 M P R AH0 M AY2 Z"), &[400; 5]);
    let texts: Vec<String> = paths.iter().map(|(_, o)| render_outline(o)).collect();
    assert!(texts.contains(&"KO/PHRO/PHAOEUZ".to_owned()), "co-mpro-mise is a path");
    assert!(texts.contains(&"KOPL/PRO/PHAOEUZ".to_owned()), "com-pro-mise is a path");
}

#[test]
fn the_lattice_lists_every_break_and_size_of_stroke_cheapest_first() {
    let rules = Rules::built_in().unwrap();
    let paths = rules.lattice_paths("complete", &sounds("K AH0 M P L IY1 T"), &[2_000; 5]);
    let texts: Vec<String> = paths.iter().map(|(_, o)| render_outline(o)).collect();
    for want in ["KUPL/PHRAOET", "KO/PHRAOET", "K/UPL/PHRAOET", "KO/PH/PHRAOET", "K/O/PH/PHRAOET"] {
        assert!(texts.contains(&want.to_owned()), "{want} is a way to write complete: {texts:?}");
    }
    assert!(paths.windows(2).all(|w| w[0].0 <= w[1].0), "cheapest first");
    assert!(paths.iter().all(|(_, o)| o.len() >= 2), "one stroke is the primary's business");
    let strokes = |n: usize| paths.iter().any(|(_, o)| o.len() == n);
    assert!(strokes(2) && strokes(3) && strokes(4) && strokes(5), "two to five strokes");
}

#[test]
fn a_sound_out_of_steno_order_costs_but_is_not_refused() {
    let rules = Rules::built_in().unwrap();
    // `-MP` would put the m after the p: PL after P. Offered, and dearer than an in-order writing.
    let paths = rules.lattice_paths("temper", &sounds("T EH1 M P ER0"), &[400; 5]);
    assert!(!paths.is_empty());
    assert!(paths.windows(2).all(|w| w[0].0 <= w[1].0));
}

#[test]
fn a_long_vowel_spelled_with_its_plain_letter_is_offered_beside_the_heard_one() {
    let rules = Rules::built_in().unwrap();
    let texts = |word: &str, pron: &str| -> Vec<String> {
        rules
            .lattice_paths(word, &sounds(pron), &[2_000; 5])
            .iter()
            .map(|(_, o)| render_outline(o))
            .collect()
    };
    // `control`: the heard OE and the spelled O, the way Plover writes it
    let control = texts("control", "K AH0 N T R OW1 L");
    for want in ["KOPB/TROEL", "KOPB/TROL"] {
        assert!(control.contains(&want.to_owned()), "{want}: {control:?}");
    }
    // a word spelled with another vowel letter keeps only the heard one
    let boat = texts("reboat", "R IY0 B OW1 T");
    assert!(boat.iter().all(|o| !o.contains("BOT")), "oa is not a plain o: {boat:?}");
}

#[test]
fn every_vowel_can_be_struck_as_the_letter_the_word_spells_it_with() {
    let rules = Rules::built_in().unwrap();
    let pron = sounds("K AA2 N T AH0 M P L EY1 SH AH0 N");
    let texts: Vec<String> =
        rules.lattice_paths("contemplation", &pron, &[2_000; 5]).iter().map(|(_, o)| render_outline(o)).collect();
    // `con` as the letter o, `tem` as the letter e, `-ation` as the heard AEU or the letter A
    for want in ["KOPB/TEPL/PHRAEU/-GS", "KOPB/TEPL/PHRA/-GS"] {
        assert!(texts.contains(&want.to_owned()), "{want}: {texts:?}");
    }
    // the letters only line up when the word has as many vowel letter groups as vowel sounds
    let lined = Rules::vowel_letters("contemplation", &sounds("K AA2 N T AH0 M P L EY1 SH AH0 N"));
    assert_eq!(lined.iter().flatten().collect::<Vec<_>>(), [&'o', &'e', &'a']);
}

use super::*;
use crate::stroke::{parse_outline, render_outline};

fn sounds(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_owned).collect()
}

/// Whether `outline` is faithful to `word`, pronounced `text` (see `Rules::faithful`).
fn faithful(word: &str, text: &str, outline: &str) -> bool {
    Rules::built_in().unwrap().faithful(&parse_outline(outline).unwrap(), word, &sounds(text))
}

#[test]
fn the_rules_own_reading_is_always_faithful() {
    let rules = Rules::built_in().unwrap();
    for (word, text) in [
        ("graph", "G R AE1 F"),
        ("grab", "G R AE1 B"),
        ("green", "G R IY1 N"),
        ("great", "G R EY1 T"),
        ("glass", "G L AE1 S"),
        ("blue", "B L UW1"),
        ("plan", "P L AE1 N"),
        ("stop", "S T AA1 P"),
        ("judge", "JH AH1 JH"),
        ("job", "JH AA1 B"),
        ("string", "S T R IH1 NG"),
        ("thing", "TH IH1 NG"),
        ("going", "G OW1 IH0 NG"),
        ("dogs", "D AO1 G Z"),
        ("worked", "W ER1 K T"),
        ("film", "F IH1 L M"),
        ("got", "G AA1 T"),
        ("find", "F AY1 N D"),
    ] {
        let outline = rules.outline(word, &sounds(text)).unwrap();
        assert!(rules.faithful(&outline, word, &sounds(text)), "{word} {}", render_outline(&outline));
    }
}

#[test]
fn a_g_missing_one_of_its_four_keys_is_unfaithful() {
    // `graph`: G is TKPW, R is R. Plover's own brief weakens the G to TPW (no K).
    assert!(faithful("graph", "G R AE1 F", "TKPWRAF"));
    assert!(!faithful("graph", "G R AE1 F", "TPWRAF"));
    // a stripped onset: the whole of G dropped
    assert!(!faithful("grab", "G R AE1 B", "RAB"));
    assert!(faithful("grab", "G R AE1 B", "TKPWRAB"));
    assert!(!faithful("green", "G R IY1 N", "RAOEPB"));
    assert!(faithful("green", "G R IY1 N", "TKPWRAOEPB"));
    assert!(!faithful("great", "G R EY1 T", "RAET"));
    assert!(faithful("great", "G R EY1 T", "TKPWRAET"));
    assert!(!faithful("glass", "G L AE1 S", "TPWHRAS"));
    assert!(faithful("glass", "G L AE1 S", "TKPWHRAS"));
    // `got`: the G weakened to TKP is the kind of drop the rule exists for
    assert!(!faithful("got", "G AA1 T", "TKPOT"));
    assert!(faithful("got", "G AA1 T", "TKPWOT"));
}

#[test]
fn a_cluster_needs_the_keys_of_every_one_of_its_sounds() {
    assert!(faithful("blue", "B L UW1", "PWHRAO"));
    assert!(!faithful("blue", "B L UW1", "PWAO"));
    assert!(!faithful("blue", "B L UW1", "HRAO"));
    assert!(faithful("plan", "P L AE1 N", "PHRAPB"));
    assert!(!faithful("plan", "P L AE1 N", "HRAPB"));
    assert!(faithful("stop", "S T AA1 P", "STOP"));
    assert!(!faithful("stop", "S T AA1 P", "TOP"));
    assert!(!faithful("stop", "S T AA1 P", "SOP"));
    // three consonants: S, T and R
    assert!(faithful("string", "S T R IH1 NG", "STREUPBG"));
    assert!(!faithful("string", "S T R IH1 NG", "TREUPBG"));
    assert!(!faithful("string", "S T R IH1 NG", "SREUPBG"));
}

#[test]
fn j_is_all_four_of_its_keys() {
    assert!(faithful("judge", "JH AH1 JH", "SKWRUPBLG"));
    assert!(!faithful("judge", "JH AH1 JH", "KWRUPBLG"));
    assert!(faithful("job", "JH AA1 B", "SKWROB"));
    assert!(!faithful("job", "JH AA1 B", "KWROB"));
    assert!(!faithful("job", "JH AA1 B", "SOB"));
}

#[test]
fn the_coda_may_drop_a_sound() {
    // `find`: the N of the word is not keyed, and that is fine
    assert!(faithful("find", "F AY1 N D", "TPAOEUD"));
    assert!(faithful("kind", "K AY1 N D", "KAOEUD"));
    assert!(faithful("thing", "TH IH1 NG", "THEUPB"));
    assert!(faithful("going", "G OW1 IH0 NG", "TKPWO"));
    assert!(faithful("dogs", "D AO1 G Z", "TKO"));
    assert!(faithful("worked", "W ER1 K T", "WOR"));
    // the onset still has to be there
    assert!(!faithful("find", "F AY1 N D", "AOEUPBD"));
    assert!(!faithful("going", "G OW1 IH0 NG", "TKWOG"));
}

#[test]
fn a_y_glide_before_uw_is_part_of_the_vowel() {
    assert!(faithful("use", "Y UW1 Z", "AOUS"));
    assert!(faithful("unit", "Y UW1 N AH0 T", "AOUBT"));
    assert!(faithful("user", "Y UW1 Z ER0", "AOUZ"));
    assert!(faithful("use", "Y UW1 Z", "KWRAOUZ"));
    // a Y before any other vowel is a sound like another
    assert!(!faithful("yes", "Y EH1 S", "ES"));
    assert!(faithful("yes", "Y EH1 S", "KWRES"));
    assert!(!faithful("young", "Y AH1 NG", "UPBG"));
}

#[test]
fn vowels_and_extra_keys_are_not_the_checks_business() {
    assert!(faithful("graph", "G R AE1 F", "TKPWRAOF"));
    assert!(faithful("graph", "G R AE1 F", "TKPWRA*F"));
    assert!(faithful("cat", "K AE1 T", "SKAT"));
    // a vowel-first word asks nothing of the left hand
    assert!(faithful("up", "AH1 P", "UP"));
    assert!(faithful("go", "G OW1", "TKPWO"));
    assert!(!faithful("go", "G OW1", "O"));
}

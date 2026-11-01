//! IPA to ARPAbet: translating `data/pronunciation/english_us_mfa.dict` and
//! `wikipron_us_broad.tsv` (both IPA) into the exact 39-symbol ARPAbet alphabet
//! `data/pronunciation/cmudict.phones` lists and `rules/plover.toml`'s onset/coda/vowel tables
//! are keyed on - so a word CMUdict has no entry for at all can still be looked up there, as a
//! fallback (see `data::merge_fallback_pronunciations`). CMUdict-covered words never pass through
//! here.
//!
//! The base consonant/vowel correspondence is the standard one: Wikipedia's ARPABET article
//! (<https://en.wikipedia.org/wiki/ARPABET>, itself citing the usual speech-processing
//! references), cross-checked against `cmudict.phones`' own 39 symbols - CMUdict never uses
//! ARPAbet's extended symbols (`DX`, `NX`, `Q`, `WH`, `AX`, `IX`, `UX`, `EL`, `EM`, `EN`), so
//! neither does this table; every one of those collapses onto the plain symbol CMUdict itself
//! would have used for the same sound (e.g. the flap `DX`/ɾ is always spelled as its underlying
//! `T` or `D`, exactly as CMUdict spells "water"/"ladder" without ever writing a flap).
//!
//! MFA's own symbol set (documented at
//! <https://mfa-models.readthedocs.io/en/latest/dictionary/English/English%20(US)%20MFA%20dictionary.html>)
//! and WikiPron's (scraped from Wiktionary per Lee, Ashby, Prud'hommeaux & Hulden, *WikiPron: A
//! Multilingual Pronunciation Dictionary from Wiktionary*, LREC 2020) are themselves two
//! different IPA conventions, not one - confirmed directly against the real files in this repo:
//! MFA writes each diphthong as one token (`aw`, `ej`) and marks aspiration/palatalization with
//! modifier letters (`pʰ`, `tʲ`); WikiPron instead splits a diphthong across two adjacent vowel
//! tokens (`b a ʊ t` for "bout", not one `aʊ` token) and is considerably noisier - scraped from
//! many Wiktionary contributors, it carries dialectal and narrow-transcription variants even for
//! common words (see `docs/decisions.md`'s entry on this fallback for the "bird" example: eleven
//! wildly different transcriptions in the real file, several not General American at all).
//!
//! Neither file marks lexical stress at all (confirmed: zero `ˈ`/`ˌ` characters in either real
//! downloaded file) - ARPAbet vowels are not optional about this (ever digit-suffixed: `AH0`,
//! `AH1`, `AH2`; `Rules::syllables` only recognises a token as a vowel at all if it ends in a
//! digit). With no stress signal to translate, [`translate`] assigns the crudest defensible
//! default instead of guessing per-word: the first vowel is marked primary stress (`1`), every
//! other vowel is unstressed (`0`). This matches the majority trochaic pattern of English
//! disyllabic words often enough to be useful, but is frequently wrong for words stressed later
//! (`began`, `suppose`) - a known, documented limitation of translating stress-blind source data,
//! not a bug.

use std::collections::HashMap;
use std::sync::LazyLock;

/// Consonants: IPA symbol (after [`clean`] strips non-distinctive diacritics) to its ARPAbet
/// letter. Several IPA symbols collapse onto the same ARPAbet consonant because ARPAbet (and
/// CMUdict) does not distinguish allophones the acoustic-model-driven sources do:
/// aspirated/unaspirated stops, palatalized consonants, the flap, or the wine/whine distinction.
const CONSONANTS: &[(&str, &str)] = &[
    ("p", "P"),
    ("b", "B"),
    ("t", "T"),
    ("d", "D"),
    ("k", "K"),
    ("ɡ", "G"),
    ("g", "G"), // plain ASCII 'g', in case a source ever writes it instead of IPA ɡ (U+0261)
    ("c", "K"),  // voiceless palatal stop: MFA's allophone of /k/ before a front vowel ("creaks")
    ("ɟ", "G"),  // its voiced counterpart, the /g/ allophone ("again" -> "ə ɟ ej n" in MFA)
    ("tʃ", "CH"),
    ("t͡ʃ", "CH"), // WikiPron's tie-barred spelling of the same affricate
    ("dʒ", "JH"),
    ("d͡ʒ", "JH"),
    ("d͜ʒ", "JH"), // an alternate tie-bar character WikiPron also uses
    ("f", "F"),
    ("v", "V"),
    ("θ", "TH"),
    ("ð", "DH"),
    ("s", "S"),
    ("z", "Z"),
    ("ʃ", "SH"),
    ("ʒ", "ZH"),
    ("h", "HH"),
    ("ç", "HH"), // voiceless palatal fricative: the /h/ allophone before /j/ or a high front vowel
    ("m", "M"),
    ("ɱ", "M"),  // labiodental nasal: the /m/ allophone before /f/ or /v/
    ("n", "N"),
    ("ɲ", "N"),  // palatal nasal: a narrow /n/ allophone before a palatal consonant
    ("ŋ", "NG"),
    ("l", "L"),
    ("ɫ", "L"),  // dark/velarized l
    ("ʎ", "L"),  // palatal lateral: a narrow /l/ allophone, seen in MFA ("feelin'", "hobgoblin")
    ("ɹ", "R"),
    ("ɾ", "T"),  // flap: usually realizes /t/ between vowels ("butter") - CMUdict spells it that
                 // way too (never writes a separate flap symbol); a real /d/-flap ("ladder") is
                 // spelled the same, an accepted, minor imprecision (see module doc)
    ("ɾ̃", "N"),  // nasalized flap ("winter" -> "winner"-like, "mountain"): the /nt/ realization
    ("ʔ", "T"),  // glottal stop: almost always the realization of an underlying /t/ ("uh-oh",
                 // "button" before its own syllabic n)
    ("w", "W"),
    ("ʍ", "W"),  // voiceless w: the wine/whine distinction CMUdict itself does not keep either
    ("j", "Y"),
];

/// Vowels, bare (no stress digit - [`translate`] appends that separately). A handful of IPA
/// symbols that only ever show up as one half of a diphthong pair in WikiPron's split-diphthong
/// style (`e`, `o`, a bare `a`) are deliberately *not* listed here: they must be consumed by
/// [`DIPHTHONGS`] first, and a leftover, unpaired one should make the whole pronunciation fail
/// to translate rather than silently map to some monophthong it was never meant to be.
const VOWELS: &[(&str, &str)] = &[
    ("ɑ", "AA"),
    ("ɑː", "AA"),
    ("æ", "AE"),
    ("ʌ", "AH"),
    ("ɐ", "AH"), // near-open central vowel: a narrow /ʌ/ allophone
    ("ə", "AH"), // CMUdict always spells unstressed schwa as AH0, never a separate AX
    ("ɔ", "AO"),
    ("ɒ", "AO"), // British/RP "lot" vowel - WikiPron is not purely General American; treated as
                 // the closest General American vowel rather than rejected outright
    ("ɒː", "AO"),
    ("ɚ", "ER"), // unstressed r-colored vowel ("letter") - CMUdict has no separate AXR
    ("ɝ", "ER"), // stressed r-colored vowel ("bird")
    ("ɪ", "IH"),
    ("i", "IY"),
    ("iː", "IY"),
    ("ʊ", "UH"),
    ("u", "UW"),
    ("uː", "UW"),
    ("ʉ", "UW"), // MFA's fronted /u/
    ("ʉː", "UW"),
    ("ɛ", "EH"),
];

/// Two-token diphthongs: WikiPron's own style splits each one across two adjacent vowel tokens
/// (`b a ʊ t` for "bout", not one `aʊ` token) - confirmed directly against the real file, and
/// inconsistently: the offglide sometimes carries a non-syllabic diacritic (`ʊ̯`) and sometimes
/// not, and "mouth" uses `ʊ` for its offglide while "house" uses the glide consonant `w` for the
/// same diphthong. [`clean`] already strips the non-syllabic diacritic before this table is
/// consulted, so only the plain pairs need listing. MFA never needs this table - it already
/// writes each diphthong as its own single token (`aw`, `ej`, `ow`, `aj`, `ɔj`), handled directly
/// by [`DIPHTHONG_TOKENS`].
const DIPHTHONGS: &[(&str, &str, &str)] = &[
    ("a", "ɪ", "AY"), // price
    ("a", "ʊ", "AW"), // mouth
    ("a", "w", "AW"), // house (WikiPron's glide-consonant spelling of the same diphthong)
    ("ɔ", "ɪ", "OY"), // choice
    ("e", "ɪ", "EY"), // face
    ("o", "ʊ", "OW"), // goat
];

/// MFA's own single-token diphthong spelling.
const DIPHTHONG_TOKENS: &[(&str, &str)] =
    &[("aj", "AY"), ("aw", "AW"), ("ej", "EY"), ("ow", "OW"), ("ɔj", "OY")];

/// A phone that is itself not a real phone at all: MFA's placeholder for non-speech noise
/// (`[laughter]`, `<unk>`) - a pronunciation containing it is never translatable.
const NOT_A_PHONE: &str = "spn";

struct Tables {
    consonants: HashMap<&'static str, &'static str>,
    vowels: HashMap<&'static str, &'static str>,
    diphthong_tokens: HashMap<&'static str, &'static str>,
}

static TABLES: LazyLock<Tables> = LazyLock::new(|| Tables {
    consonants: CONSONANTS.iter().copied().collect(),
    vowels: VOWELS.iter().copied().collect(),
    diphthong_tokens: DIPHTHONG_TOKENS.iter().copied().collect(),
});

/// Strips the modifier letters and combining marks this module treats as non-distinctive for
/// ARPAbet's purposes (aspiration, palatalization, labialization, length, the dental and
/// affricate-tie marks), and separately reports whether the syllabic-consonant mark (`◌̩`) was
/// present, since that changes the token into two output phones rather than one (see
/// [`translate`]). Every character stripped here is a deliberate, named exception - this does
/// *not* strip combining marks in general (a nasalized vowel like `ã`, a stray tie bar over an
/// un-recognized cluster, or anything else not on this list is left alone, so the token fails to
/// match [`CONSONANTS`]/[`VOWELS`] and the whole pronunciation is rejected rather than guessed).
fn clean(token: &str) -> (String, bool) {
    let mut syllabic = false;
    let cleaned: String = token
        .chars()
        .filter(|&c| match c {
            '\u{02B0}' // ʰ aspirated
            | '\u{02B2}' // ʲ palatalized
            | '\u{02B7}' // ʷ labialized
            | '\u{02D0}' // ː long
            | '\u{032A}' // ̪ dental
            | '\u{0361}' // ͡ tie bar above
            | '\u{035C}' // ͜ tie bar below
            => false,
            '\u{0329}' => { // ̩ syllabic
                syllabic = true;
                false
            }
            '\u{032F}' => false, // ̯ non-syllabic (diphthong offglide marker)
            _ => true,
        })
        .collect();
    (cleaned, syllabic)
}

/// One word's IPA pronunciation (as `load_mfa`/`load_wikipron` return it, one phone per token) to
/// ARPAbet, or `None` if any phone in it is not one this module recognises - a deliberate refusal
/// rather than a guess (see module doc). Each output vowel carries a stress digit: `1` for the
/// first vowel, `0` for every other one (there is no stress information in the source to do
/// better with - see module doc).
pub fn translate(phones: &[String]) -> Option<Vec<String>> {
    // Clean every token first, and expand a syllabic consonant into an inserted schwa plus the
    // plain consonant (`m̩` -> `AH`, `M`) - CMUdict always spells a syllabic consonant with an
    // explicit vowel ("button" -> B AH1 T AH0 N), never a syllabic-consonant symbol of its own.
    let mut cleaned: Vec<String> = Vec::new();
    for raw in phones {
        if raw == NOT_A_PHONE {
            return None;
        }
        let (token, syllabic) = clean(raw);
        if syllabic {
            cleaned.push("ə".to_owned()); // stands for an inserted schwa; mapped like any vowel
        }
        cleaned.push(token);
    }

    // Merge WikiPron-style two-token diphthongs before anything else is looked up, so a lone
    // offglide vowel (`e`, `o`, the diphthong-only `a`) is never mistaken for some other sound.
    let mut merged: Vec<String> = Vec::new();
    let mut i = 0;
    while i < cleaned.len() {
        if let Some(next) = cleaned.get(i + 1)
            && let Some(&(_, _, arpabet)) = DIPHTHONGS
                .iter()
                .find(|&&(a, b, _)| a == cleaned[i] && b == next.as_str())
        {
            merged.push(format!("\u{0}{arpabet}")); // sentinel prefix: already an ARPAbet vowel
            i += 2;
        } else {
            merged.push(cleaned[i].clone());
            i += 1;
        }
    }

    // Now map every remaining token: an already-merged diphthong (sentinel-prefixed), one of
    // MFA's single-token diphthongs, a plain consonant, or a plain vowel. Track which output
    // phones are vowels (for the stress pass below) without assuming anything about order.
    let mut out: Vec<String> = Vec::new();
    let mut is_vowel: Vec<bool> = Vec::new();
    for token in &merged {
        if let Some(arpabet) = token.strip_prefix('\u{0}') {
            out.push(arpabet.to_owned());
            is_vowel.push(true);
        } else if let Some(&arpabet) = TABLES.diphthong_tokens.get(token.as_str()) {
            out.push(arpabet.to_owned());
            is_vowel.push(true);
        } else if let Some(&arpabet) = TABLES.vowels.get(token.as_str()) {
            out.push(arpabet.to_owned());
            is_vowel.push(true);
        } else if let Some(&arpabet) = TABLES.consonants.get(token.as_str()) {
            out.push(arpabet.to_owned());
            is_vowel.push(false);
        } else {
            return None; // an IPA symbol this module does not recognise: refuse, don't guess
        }
    }
    if !is_vowel.iter().any(|&v| v) {
        return None; // no vowel at all: not a word `Rules::syllables` could ever place
    }

    // Stress: the first vowel is primary, every other vowel unstressed (see module doc).
    let mut seen_vowel = false;
    for (phone, &vowel) in out.iter_mut().zip(is_vowel.iter()) {
        if vowel {
            phone.push(if seen_vowel { '0' } else { '1' });
            seen_vowel = true;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phones(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn mfa_style_single_token_diphthong() {
        // "sofa": MFA spells it "s ow f ə"
        assert_eq!(
            translate(&phones("s ow f ə")),
            Some(vec!["S".into(), "OW1".into(), "F".into(), "AH0".into()])
        );
    }

    #[test]
    fn wikipron_style_split_diphthong() {
        // "about": WikiPron spells it "ə b a ʊ̯ t"
        assert_eq!(
            translate(&phones("ə b a ʊ̯ t")),
            Some(vec!["AH1".into(), "B".into(), "AW0".into(), "T".into()])
        );
    }

    #[test]
    fn glide_consonant_offglide_diphthong() {
        // "house": WikiPron spells it "h a w s" - one vowel (the diphthong), so it is primary
        assert_eq!(
            translate(&phones("h a w s")),
            Some(vec!["HH".into(), "AW1".into(), "S".into()])
        );
    }

    #[test]
    fn aspiration_and_palatalization_are_stripped() {
        // "cat": MFA spells it "cʰ æ t" - one vowel, so it is primary
        assert_eq!(
            translate(&phones("cʰ æ t")),
            Some(vec!["K".into(), "AE1".into(), "T".into()])
        );
    }

    #[test]
    fn syllabic_consonant_gets_an_inserted_schwa() {
        // a syllabic /n/ as in "button"'s second syllable, spelled bare here for the test
        assert_eq!(
            translate(&phones("b ʌ ʔ n̩")),
            Some(vec!["B".into(), "AH1".into(), "T".into(), "AH0".into(), "N".into()])
        );
    }

    #[test]
    fn an_unrecognized_phone_refuses_rather_than_guesses() {
        assert_eq!(translate(&phones("b ɻ a t")), None); // ɻ: not in the table
    }

    #[test]
    fn the_noise_placeholder_is_never_translatable() {
        assert_eq!(translate(&phones("spn")), None);
    }

    #[test]
    fn a_consonant_only_phone_list_has_no_vowel_and_fails() {
        assert_eq!(translate(&phones("s t")), None);
    }
}

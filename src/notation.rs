//! The short notation: how a stroke is written for people, key groups as the one letter they
//! stand for (`TPHAEPL` is `NAEM`, `TPHROUR` is `FLOUR`), vowels and the star as they are, `#`
//! and letters for a number-key stroke. Dictionary files stay in raw Plover notation; this is
//! only how a stroke is shown. See `site/reference/notation.json`.
//!
//! The aliases come from the rules' own sound tables (`[onset]` and `[coda]` of
//! `rules/plover.toml`): every chord of two keys or more that writes one sound is written as that
//! sound's letter (`TPH` N, `-PB` N, `SKWR` J). A run of keys is read from its lowest key: the
//! longest alias starting there whose keys are all down, else the key's own letter. Where that
//! reading is not how the run's sounds usually split (`TPHR` is F + L far more often than N + R),
//! a compound alias (`TPHR` FL) is added, chosen by [`measure`] from the outlines the rules wrote
//! for the built dictionary. An outline whose own sounds split another way than the table reads
//! it is listed on its own, in [`Notation::outlines`].
//!
//! `build` writes the table and the per-outline list as keymux's notation file ([`FILE`]), which
//! `deploy` ships into the steno folder, so keymux writes strokes the same way.

use crate::rules::{Rules, Syllable};
use crate::stroke::Stroke;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

/// keymux's notation file, written by `build` and shipped by `deploy`: hidden, so keymux never
/// reads it as a dictionary.
pub const FILE: &str = ".keymux-notation.json";

/// The number key's bit.
const NUMBER: u32 = 1;

/// One alias: the keys of a chord and the letters it is written as.
pub type Alias = (u32, String);

/// The short notation: the aliases of each hand and the outlines written their own way.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Notation {
    pub left: Vec<Alias>,
    pub right: Vec<Alias>,
    /// Raw outline -> short outline, for outlines whose sounds split another way than the table
    pub outlines: BTreeMap<String, String>,
}

/// The letters a sound is written as: `N`, `CH`, `J` for `JH`, `TH` for `DH`, `H` for `HH`. A
/// spelling variant (`W.wh`) is the sound's own letters too. `None` for a sound with no letter.
fn letters(sound: &str) -> Option<&'static str> {
    let base = sound.split('.').next().unwrap_or(sound);
    Some(match base {
        "P" => "P",
        "B" => "B",
        "T" => "T",
        "D" => "D",
        "K" => "K",
        "G" => "G",
        "M" => "M",
        "N" => "N",
        "NG" => "NG",
        "F" => "F",
        "V" => "V",
        "TH" | "DH" => "TH",
        "S" => "S",
        "Z" => "Z",
        "SH" => "SH",
        "ZH" | "JH" => "J",
        "CH" => "CH",
        "HH" => "H",
        "L" => "L",
        "R" => "R",
        "W" => "W",
        "Y" => "Y",
        _ => return None,
    })
}

/// The raw letters of a chord's keys, without the `-` of the right hand.
fn raw(bits: u32) -> String {
    Stroke(bits).to_string().replace('-', "")
}

/// Each hand's aliases from the rules' sound tables: a chord of two keys or more (the star
/// aside) written as its sound's letters, where those differ from the keys' own. Sounds are taken
/// in a fixed order, so a chord two sounds share (`SKWR`, *j* and *zh*) is the first one's.
#[must_use]
pub fn base(rules: &Rules) -> (Vec<Alias>, Vec<Alias>) {
    let order = [
        "P", "B", "T", "D", "K", "G", "M", "N", "NG", "F", "V", "TH", "DH", "S", "Z", "SH", "CH", "JH", "ZH", "HH",
        "L", "R", "W", "Y",
    ];
    let hand = |table: &HashMap<String, Stroke>, mask: u32| -> Vec<Alias> {
        let mut out: Vec<Alias> = Vec::new();
        for sound in order {
            let (Some(stroke), Some(text)) = (table.get(sound), letters(sound)) else { continue };
            if stroke.0 & Stroke::STAR != 0 || stroke.0 & !mask != 0 || stroke.0.count_ones() < 2 {
                continue;
            }
            if raw(stroke.0) == text || out.iter().any(|(bits, _)| *bits == stroke.0) {
                continue;
            }
            // A letter already taken by another chord would read back as two chords
            if out.iter().any(|(_, t)| t == text) {
                continue;
            }
            out.push((stroke.0, text.to_owned()));
        }
        out
    };
    (hand(rules.onset_table(), Stroke::LEFT), hand(rules.coda_table(), Stroke::RIGHT))
}

/// A run of one hand's keys written with `aliases`, as tokens with their lowest key: from the
/// lowest key left, the alias with the most keys that starts there, whose keys are all still to
/// write and with no other key still to write between its lowest and highest (`SR` V is not read
/// inside `STR`), else the key's own letter.
fn tokens(aliases: &[Alias], bits: u32) -> Vec<(u32, String)> {
    let mut left = bits;
    let mut out = Vec::new();
    while left != 0 {
        let lowest = left.trailing_zeros();
        let best = aliases
            .iter()
            .filter(|(keys, _)| {
                let span = (1u32 << (32 - keys.leading_zeros())) - (1u32 << keys.trailing_zeros());
                keys.trailing_zeros() == lowest && keys & !left == 0 && left & span & !keys == 0
            })
            .max_by_key(|(keys, _)| keys.count_ones());
        if let Some((keys, text)) = best {
            out.push((lowest, text.clone()));
            left &= !keys;
        } else {
            out.push((lowest, raw(1 << lowest)));
            left &= !(1 << lowest);
        }
    }
    out
}

/// A run of one hand's keys written with `aliases` (see [`tokens`]).
#[must_use]
pub fn cluster(aliases: &[Alias], bits: u32) -> String {
    tokens(aliases, bits).into_iter().map(|(_, text)| text).collect()
}

/// A stroke written from its parts: the join key, `#`, the left run as `left`, the vowels and
/// star, a `-` when nothing else parts the hands, the right run as `right`.
fn assemble(stroke: Stroke, left: &str, right: &str) -> String {
    let mut out = String::new();
    if stroke.0 & Stroke::JOIN != 0 {
        out.push('+');
    }
    if stroke.0 & Stroke::SPELL != 0 {
        out.push('~');
    }
    if stroke.0 & NUMBER != 0 {
        out.push('#');
    }
    out.push_str(left);
    let middle = stroke.0 & (Stroke::VOWELS | Stroke::STAR);
    if middle != 0 {
        out.push_str(&Stroke(middle).to_string());
    } else if !right.is_empty() {
        out.push('-');
    }
    out.push_str(right);
    out
}

impl Notation {
    /// A stroke in the short notation, by the table alone.
    #[must_use]
    pub fn stroke(&self, stroke: Stroke) -> String {
        assemble(
            stroke,
            &cluster(&self.left, stroke.0 & Stroke::LEFT),
            &cluster(&self.right, stroke.0 & Stroke::RIGHT),
        )
    }

    /// An outline in the short notation: its own entry in [`Self::outlines`], else stroke by
    /// stroke.
    #[must_use]
    pub fn outline(&self, strokes: &[Stroke]) -> String {
        let raw = crate::stroke::render_outline(strokes);
        if let Some(short) = self.outlines.get(&raw) {
            return short.clone();
        }
        strokes.iter().map(|&s| self.stroke(s)).collect::<Vec<_>>().join("/")
    }

    /// keymux's notation file: `#` with letters, the aliases of each hand, the outlines written
    /// their own way.
    #[must_use]
    pub fn to_json(&self) -> String {
        let hand = |aliases: &[Alias]| -> serde_json::Map<String, serde_json::Value> {
            aliases.iter().map(|(bits, text)| (raw(*bits), serde_json::Value::String(text.clone()))).collect()
        };
        let value = serde_json::json!({
            "number": "letters",
            "left": hand(&self.left),
            "right": hand(&self.right),
            "outlines": self.outlines,
        });
        let mut text = serde_json::to_string_pretty(&value).unwrap_or_default();
        text.push('\n');
        text
    }
}

/// How the outlines of one run of keys split by their sounds: each way it was written, and how
/// often.
#[derive(Debug, Clone, Default)]
pub struct Split {
    /// The hand: `true` for the left
    pub left: bool,
    /// The run's keys
    pub keys: u32,
    /// Each way, written in short form, and how many strokes split so
    pub ways: BTreeMap<String, usize>,
    /// How the base aliases alone read it
    pub table: String,
}

/// The short notation measured on the built dictionary, and what the measuring found.
#[derive(Debug, Clone, Default)]
pub struct Measured {
    pub notation: Notation,
    /// Every run of keys more than one way was split, or split another way than the base
    /// aliases read it, by hand and keys
    pub splits: Vec<Split>,
    /// Outlines whose every stroke was read back to its sounds
    pub known: usize,
}

/// A stroke's left and right run in short form.
type Runs = (String, String);

/// One stroke's runs as its sounds split them: the short form of the left and the right run, or
/// `None` where the pieces do not add up to the stroke's keys.
fn sounded(rules: &Rules, aliases: &(Vec<Alias>, Vec<Alias>), syllable: &Syllable, stroke: Stroke) -> Option<(String, String)> {
    // Each piece's tokens: a sound with an alias of its own is that alias; a sound written with
    // keys that are no alias (`-FRB` for the *v* of `serve`) is its keys as they are, so no alias
    // claims a sound that is not there; a run of sounds a cluster rule writes is read by the
    // table. All tokens in order of their lowest key.
    let piece_text = |table: &[Alias], pieces: &[(Stroke, Vec<String>)]| -> String {
        let mut all: Vec<(u32, String)> = Vec::new();
        // A key two pieces share (the `R` of `march`, in the vowel's `R` and in `-FRPB`) is
        // written once
        let mut written = 0u32;
        for (stroke, sounds) in pieces {
            let bits = stroke.0 & !Stroke::STAR & !written;
            written |= bits;
            if bits == 0 {
                continue;
            }
            if let Some((_, text)) = table.iter().find(|(keys, _)| *keys == bits) {
                all.push((bits.trailing_zeros(), text.clone()));
            } else if sounds.len() == 1 {
                all.extend((0..32).filter(|i| bits & (1 << i) != 0).map(|i| (i, raw(1 << i))));
            } else {
                all.extend(tokens(table, bits));
            }
        }
        all.sort_by_key(|(at, _)| *at);
        all.into_iter().map(|(_, text)| text).collect()
    };
    let onset = rules.consonant_pieces(&syllable.onset, true)?;
    let left_keys: u32 = onset.iter().fold(0, |a, (s, _)| a | s.0);
    if left_keys & !Stroke::STAR != stroke.0 & Stroke::LEFT {
        return None;
    }
    let right_of = |pieces: &[(Stroke, Vec<String>)]| pieces.iter().fold(0, |a, (s, _)| a | (s.0 & Stroke::RIGHT));
    let want = stroke.0 & Stroke::RIGHT;
    // The whole coda, or the coda after a first sound the vowel took (`AA R`)
    let coda = rules
        .consonant_pieces(&syllable.coda, false)
        .filter(|p| right_of(p) == want)
        .or_else(|| {
            syllable.coda.get(1..).and_then(|rest| rules.consonant_pieces(rest, false)).filter(|p| right_of(p) == want)
        })?;
    let right: Vec<(Stroke, Vec<String>)> =
        coda.into_iter().map(|(s, sounds)| (Stroke(s.0 & Stroke::RIGHT), sounds)).collect();
    Some((piece_text(&aliases.0, &onset), piece_text(&aliases.1, &right)))
}

/// The short notation for these entries: the base aliases, a compound alias for each run whose
/// sounds usually split another way than they read it, and the outlines whose own sounds split
/// another way than the finished table reads them. Only strokes the rules write syllable by
/// syllable from a word's sounds count: a brief's keys say nothing about its sounds.
#[must_use]
pub fn measure(rules: &Rules, pron: &crate::data::Pronunciations, entries: &[(String, Vec<Stroke>)]) -> Measured {
    let aliases = base(rules);
    let mut ways: HashMap<(bool, u32), BTreeMap<String, usize>> = HashMap::new();
    // Each outline read back to its sounds: the outline, and each stroke's two runs
    let mut known: Vec<(&[Stroke], Vec<Runs>)> = Vec::new();
    for (word, outline) in entries {
        if outline.iter().any(|s| s.0 & Stroke::SPELL != 0) {
            continue;
        }
        let Some(variants) = pron.get(word) else { continue };
        let read = variants.iter().find_map(|sounds| {
            let syllables = rules.syllables(&rules.label(word, sounds))?;
            if syllables.len() != outline.len() {
                return None;
            }
            syllables
                .iter()
                .zip(outline)
                .map(|(syllable, &stroke)| {
                    let keys = Stroke(stroke.0 & !Stroke::JOIN);
                    (rules.syllable_stroke(syllable)? == keys).then_some(())?;
                    sounded(rules, &aliases, syllable, keys)
                })
                .collect::<Option<Vec<_>>>()
        });
        let Some(runs) = read else { continue };
        for (stroke, (left, right)) in outline.iter().zip(&runs) {
            for (is_left, keys, text) in [(true, stroke.0 & Stroke::LEFT, left), (false, stroke.0 & Stroke::RIGHT, right)] {
                if keys.count_ones() >= 2 {
                    *ways.entry((is_left, keys)).or_default().entry(text.clone()).or_default() += 1;
                }
            }
        }
        known.push((outline, runs));
    }
    // A compound where the commonest way is not how the base aliases read the run
    let (mut left, mut right) = aliases.clone();
    let mut splits = Vec::new();
    let mut keys: Vec<&(bool, u32)> = ways.keys().collect();
    keys.sort();
    for key in keys {
        let (is_left, bits) = *key;
        let counted = &ways[key];
        let table = cluster(if is_left { &aliases.0 } else { &aliases.1 }, bits);
        let Some((best, _)) = counted.iter().max_by_key(|(text, n)| (**n, std::cmp::Reverse((*text).clone()))) else { continue };
        if counted.len() > 1 || *best != table {
            splits.push(Split { left: is_left, keys: bits, ways: counted.clone(), table: table.clone() });
        }
        if *best != table {
            let hand = if is_left { &mut left } else { &mut right };
            hand.push((bits, best.clone()));
        }
    }
    let mut notation = Notation { left, right, outlines: BTreeMap::new() };
    // The outlines the finished table reads another way than their sounds split
    for (outline, runs) in &known {
        let own: Vec<String> = outline.iter().zip(runs).map(|(&s, (l, r))| assemble(s, l, r)).collect();
        let own = own.join("/");
        if own != notation.outline(outline) {
            notation.outlines.insert(crate::stroke::render_outline(outline), own);
        }
    }
    Measured { notation, splits, known: known.len() }
}

/// What [`measure`] found, as `build --report` prints it: each run split more than one way, how
/// often each way, and the way the table now reads it.
#[must_use]
pub fn report(measured: &Measured) -> String {
    let mut out = format!(
        "short notation: {} outlines read back to their sounds, {} written their own way\n",
        measured.known,
        measured.notation.outlines.len()
    );
    for split in &measured.splits {
        let keys = if split.left { raw(split.keys) } else { format!("-{}", raw(split.keys)) };
        let total: usize = split.ways.values().sum();
        let mut ways: Vec<(&String, &usize)> = split.ways.iter().collect();
        ways.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        let shown: Vec<String> = ways.iter().map(|(text, n)| format!("{text} {n}")).collect();
        let _ = writeln!(out, "  {keys:<8} {total:>6} strokes: {}  (base reads {})", shown.join(", "), split.table);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> Stroke {
        Stroke::parse(text).expect("a stroke")
    }

    fn table() -> Notation {
        let rules = &crate::dictionary_fixture::setup().rules;
        let (left, right) = base(rules);
        Notation { left, right, outlines: BTreeMap::new() }
    }

    #[test]
    fn the_base_aliases_are_the_sound_tables_chords() {
        let t = table();
        let find = |aliases: &[Alias], keys: &str| aliases.iter().find(|(b, _)| raw(*b) == keys).map(|(_, t)| t.clone());
        assert_eq!(find(&t.left, "TPH").as_deref(), Some("N"));
        assert_eq!(find(&t.left, "SKWR").as_deref(), Some("J"));
        assert_eq!(find(&t.left, "HR").as_deref(), Some("L"));
        assert_eq!(find(&t.right, "PB").as_deref(), Some("N"));
        assert_eq!(find(&t.right, "PBLG").as_deref(), Some("J"));
        assert_eq!(find(&t.left, "TH"), None, "a chord written as its own keys needs none");
    }

    #[test]
    fn strokes_are_written_short() {
        let t = table();
        assert_eq!(t.stroke(s("TPHAEPL")), "NAEM");
        assert_eq!(t.stroke(s("-PB")), "-N");
        assert_eq!(t.stroke(s("KAT")), "KAT");
        assert_eq!(t.stroke(s("+STKPWAEUGS")), "+ZAEUGS");
        assert_eq!(t.stroke(s("#TR-RL")), "#TR-RL");
        assert_eq!(t.stroke(s("SKWR-PBLG")), "J-J");
        assert_eq!(t.stroke(s("TPHROUR")), "NROUR", "without the measured compound");
    }

    #[test]
    fn the_measured_table_reads_flower_as_its_sounds() {
        let setup = crate::dictionary_fixture::setup();
        let generated = crate::dictionary_fixture::generated();
        let entries: Vec<(String, Vec<Stroke>)> =
            generated.entries.iter().map(|e| (e.word.clone(), e.outline.clone())).collect();
        let measured = measure(&setup.rules, &setup.pron, &entries);
        let n = &measured.notation;
        assert_eq!(cluster(&n.left, s("TPHR").0), "FL", "{}", report(&measured));
        assert_eq!(n.stroke(s("TPHROUR")), "FLOUR");
        assert_eq!(n.stroke(s("TPHAEPL")), "NAEM");
        assert!(measured.known > 1000, "{}", measured.known);
        // An outline written its own way: `orb` is O, R and B, not the SH of `-RB`
        assert_eq!(n.outlines.get("ORB").map(String::as_str), Some("ORB"));
        assert_eq!(n.outline(&[s("ORB")]), "ORB");
        assert_eq!(n.stroke(s("ORB")), "OSH", "the table alone");
        let json: serde_json::Value = serde_json::from_str(&n.to_json()).expect("json");
        assert_eq!(json["number"], "letters");
        assert_eq!(json["left"]["TPHR"], "FL");
    }
}

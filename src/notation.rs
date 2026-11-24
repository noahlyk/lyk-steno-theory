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
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;

/// keymux's notation file, written by `build` and shipped by `deploy`: hidden, so keymux never
/// reads it as a dictionary.
pub const FILE: &str = ".keymux-notation.json";

/// The number key's bit.
const NUMBER: u32 = 1;

/// One alias: the keys of a chord and the letters it is written as.
pub type Alias = (u32, String);

/// The short notation: the aliases of each hand and of the vowel keys, the outlines written their
/// own way, and the outlines whose sounds split as the table reads them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Notation {
    pub left: Vec<Alias>,
    pub right: Vec<Alias>,
    /// The vowel chords written as one mark (`AOEU` is `Ī`): the vowel keys alone, never the star
    pub middle: Vec<Alias>,
    /// Raw outline -> short outline, for outlines whose sounds split another way than the table
    pub outlines: BTreeMap<String, String>,
    /// Raw outlines whose every stroke's sounds split exactly as the table writes it: with
    /// [`Self::outlines`], the outlines whose split is known (keymux fuses a known split's groups
    /// on its keyboard diagram, and only a known one)
    pub agree: BTreeSet<String>,
}

/// The vowel marks: each vowel chord lyk writes as one sound, as the standard single character
/// for that sound where one exists (the dictionary macron for the long vowels, the American
/// Heritage `ô` for *aw*, IPA `ʊ` for the *oo* of *book*), else the dictionary respelling (`OI`).
/// `OU` (*ow*) is already its respelling and `EU` the short *i* is `I`; the single vowels and `AE`
/// stay as their keys (see `site/reference/notation.json`).
pub const VOWEL_MARKS: [(&str, &str); 9] = [
    ("EU", "I"),
    ("AEU", "Ā"),
    ("AOE", "Ē"),
    ("AOEU", "Ī"),
    ("OE", "Ō"),
    ("AOU", "Ū"),
    ("AU", "Ô"),
    ("AO", "Ʊ"),
    ("OEU", "OI"),
];

/// Chords of two or more sounds written as one standard letter or their sounds' letters: `-GS` is
/// the *shun* of `-tion` (`ŠN`), `-BGS` the *ks* of `tax` (`X`: every stroke lyk writes from a
/// word's sounds with `-BGS` is K and S, none is *kshun*), `-GTS` a `T` struck inside the *shun*
/// chord (`notation` `TPHAEUGTS`, `NĀTŠN`). Raw keys, the letters, the sounds they stand for.
/// `-FRB` is the *rv* of `serve` (the `R` the vowel's, the `V` the chord's: `RV`), and `KP` the
/// *x* of the `ex-` prefix (`X`, the letter English writes *ks* with).
pub const CLUSTERS: [(&str, &str, &str); 5] = [
    ("-GS", "ŠN", "SH N"),
    ("-BGS", "X", "K S"),
    ("-GTS", "TŠN", "T SH N"),
    ("-FRB", "RV", "R V"),
    ("KP", "X", "K S"),
];

/// Plain-ASCII spellings read as input for a symbol, where they read no other keys: `SH` for `Š`,
/// `OO` for `Ʊ`. Never written.
pub const ASCII: [(&str, &str); 6] = [("Š", "SH"), ("Č", "CH"), ("Ž", "ZH"), ("Ŋ", "NG"), ("Þ", "TH"), ("Ʊ", "OO")];

/// The letter a sound is written as: one capital each, a mark on a familiar letter where no plain
/// letter fits (`Š` *sh*, `Č` *ch*, `Ž` *zh*, `Ŋ` *ng*, `Þ` *th* and *dh*), `J` for *j*, `H` for
/// `HH`. A spelling variant (`W.wh`) is the sound's own letter too. `None` for a sound with none.
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
        "NG" => "Ŋ",
        "F" => "F",
        "V" => "V",
        "TH" | "DH" => "Þ",
        "S" => "S",
        "Z" => "Z",
        "SH" => "Š",
        "ZH" => "Ž",
        "JH" => "J",
        "CH" => "Č",
        "HH" => "H",
        "L" => "L",
        "R" => "R",
        "W" => "W",
        "Y" => "Y",
        _ => return None,
    })
}

/// A sound whose chord takes the `R` of its spelling (`V.rv`, `CH.rch`) written as `R` and its
/// letter (`RV`, `RČ`).
fn respelled(sound: &str) -> Option<String> {
    let (base, spelling) = sound.split_once('.')?;
    let letter = letters(base)?;
    spelling.starts_with('r').then(|| format!("R{letter}"))
}

/// The raw letters of a chord's keys, without the `-` of the right hand.
fn raw(bits: u32) -> String {
    Stroke(bits).to_string().replace('-', "")
}

/// Each hand's aliases from the rules' sound tables: a chord of two keys or more (the star
/// aside) written as its sound's letter, where that differs from the keys' own; then the
/// [`CLUSTERS`]. Sounds are taken in a fixed order, so a chord two sounds share (`SKWR`, *j* and
/// *zh*) is the first one's.
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
    let (mut left, mut right) = (hand(rules.onset_table(), Stroke::LEFT), hand(rules.coda_table(), Stroke::RIGHT));
    for (keys, text, _) in CLUSTERS {
        let Some(stroke) = Stroke::parse(keys) else { continue };
        let hand = if stroke.0 & Stroke::LEFT != 0 { &mut left } else { &mut right };
        if !hand.iter().any(|(b, t)| *b == stroke.0 || t == text) {
            hand.push((stroke.0, text.to_owned()));
        }
    }
    (left, right)
}

/// The vowel marks as aliases.
#[must_use]
pub fn vowel_marks() -> Vec<Alias> {
    VOWEL_MARKS.iter().filter_map(|(keys, mark)| Some((Stroke::parse(keys)?.0, (*mark).to_owned()))).collect()
}

/// The vowel keys and star written with `middle`: a chord with a mark is that mark, then the star
/// if it is down (`Ī*`); otherwise the keys as they are, in steno order (`A*`, `*EU`).
fn middle_pieces(middle: &[Alias], bits: u32) -> Vec<(u32, String)> {
    let vowels = bits & Stroke::VOWELS;
    if let Some((keys, mark)) = middle.iter().find(|(keys, _)| *keys == vowels && vowels != 0) {
        let mut out = vec![(*keys, mark.clone())];
        if bits & Stroke::STAR != 0 {
            out.push((Stroke::STAR, "*".to_owned()));
        }
        return out;
    }
    (0..32).filter(|i| bits & MIDDLE & (1 << i) != 0).map(|i| (1 << i, raw(1 << i))).collect()
}

/// A run of one hand's keys written with `aliases`, as tokens with their keys: from the lowest
/// key left, the alias with the most keys that starts there, whose keys are all still to write
/// and with no other key still to write between its lowest and highest (`SR` V is not read inside
/// `STR`), else the key's own letter. Of two such aliases with as many keys, the first listed
/// (keymux's rule, so both write a stroke the same way).
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
            .fold(None::<&Alias>, |best, alias| match best {
                Some(b) if b.0.count_ones() >= alias.0.count_ones() => Some(b),
                _ => Some(alias),
            });
        if let Some((keys, text)) = best {
            out.push((*keys, text.clone()));
            left &= !keys;
        } else {
            out.push((1 << lowest, raw(1 << lowest)));
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
/// star with `middle`, a `-` when nothing else parts the hands, the right run as `right`.
fn assemble(stroke: Stroke, left: &str, middle: &[Alias], right: &str) -> String {
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
    let vowels = stroke.0 & MIDDLE;
    if vowels != 0 {
        out.extend(middle_pieces(middle, vowels).into_iter().map(|(_, text)| text));
    } else if !right.is_empty() {
        out.push('-');
    }
    out.push_str(right);
    out
}

/// One piece of a written stroke: its text and the keys it stands for (`0` for the `-` that only
/// parts the hands). Joined, the pieces are the written stroke. keymux's `Piece`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    pub text: String,
    pub keys: u32,
}

/// One stroke in raw notation as pieces, a key each (the `-` a piece of no keys).
#[must_use]
pub fn raw_pieces(stroke: Stroke) -> Vec<Piece> {
    let text = stroke.to_string();
    let mut keys = (0..23u32).filter(|bit| stroke.0 & (1 << bit) != 0).map(|bit| 1u32 << bit);
    text.chars()
        .map(|c| {
            let keys = match c {
                '+' => Stroke::JOIN,
                '~' => Stroke::SPELL,
                '-' => 0,
                _ => keys.next().unwrap_or(0),
            };
            Piece { text: c.to_string(), keys }
        })
        .collect()
}

/// The vowel keys and the star, which part the hands.
const MIDDLE: u32 = Stroke::VOWELS | Stroke::STAR;

/// One hand's keys as pieces, by [`tokens`].
fn hand_pieces(aliases: &[Alias], bits: u32, out: &mut Vec<Piece>) {
    out.extend(tokens(aliases, bits).into_iter().map(|(keys, text)| Piece { text, keys }));
}

/// What one hand's text is read with: the aliases' letters and the hand's own key letters, longest
/// first (an alias before a key letter of the same length), as keymux reads it.
fn hand_tokens(aliases: &[Alias], mask: u32) -> Vec<(String, u32)> {
    let mut out: Vec<(String, u32)> = aliases
        .iter()
        .map(|(keys, text)| (text.clone(), *keys))
        .chain((0..32).filter(|i| mask & (1 << i) != 0).map(|i| (raw(1 << i), 1 << i)))
        .collect();
    // The ASCII spellings of a symbol, behind every real token of their length, and only where no
    // alias is already spelled so
    for (symbol, ascii) in ASCII {
        if let Some((keys, _)) = aliases.iter().find(|(_, text)| text == symbol)
            && !aliases.iter().any(|(_, text)| text == ascii)
        {
            out.push((ascii.to_owned(), *keys));
        }
    }
    out.sort_by_key(|(text, _)| std::cmp::Reverse(text.chars().count()));
    out
}

/// One hand's text read greedily, longest token first, as pieces; `None` if some text is left
/// that no token reads.
fn read_hand(mut text: &str, tokens: &[(String, u32)], out: &mut Vec<Piece>) -> Option<()> {
    while !text.is_empty() {
        let (token, keys) = tokens.iter().find(|(token, _)| text.starts_with(token.as_str()))?;
        out.push(Piece { text: token.clone(), keys: *keys });
        text = &text[token.len()..];
    }
    Some(())
}

impl Notation {
    /// A stroke in the short notation, by the table alone.
    #[must_use]
    pub fn stroke(&self, stroke: Stroke) -> String {
        self.pieces(stroke).into_iter().map(|p| p.text).collect()
    }

    /// A stroke as the pieces it is written in, by the table alone: the join key, `~`, `#`, the
    /// left hand's tokens, each vowel and the star, a `-` when nothing else parts the hands, the
    /// right hand's tokens.
    #[must_use]
    pub fn pieces(&self, stroke: Stroke) -> Vec<Piece> {
        let bits = stroke.0;
        let mut out = Vec::new();
        for (key, text) in [(Stroke::JOIN, "+"), (Stroke::SPELL, "~"), (NUMBER, "#")] {
            if bits & key != 0 {
                out.push(Piece { text: text.to_owned(), keys: key });
            }
        }
        hand_pieces(&self.left, bits & Stroke::LEFT, &mut out);
        if bits & MIDDLE != 0 {
            out.extend(middle_pieces(&self.middle, bits).into_iter().map(|(keys, text)| Piece { text, keys }));
        } else if bits & Stroke::RIGHT != 0 {
            out.push(Piece { text: "-".to_owned(), keys: 0 });
        }
        hand_pieces(&self.right, bits & Stroke::RIGHT, &mut out);
        out
    }

    /// An outline in the short notation: its own entry in [`Self::outlines`], else stroke by
    /// stroke.
    #[must_use]
    pub fn outline(&self, strokes: &[Stroke]) -> String {
        let raw = crate::stroke::render_outline(strokes);
        if let Some(short) = self.outlines.get(&raw) {
            return short.clone();
        }
        if !self.agree.contains(&raw) {
            return raw;
        }
        self.table_outline(strokes)
    }

    /// An outline written by the table alone, known split or not: what the table names each
    /// group of keys (the measuring, the tests).
    #[must_use]
    pub fn table_outline(&self, strokes: &[Stroke]) -> String {
        strokes.iter().map(|&s| self.stroke(s)).collect::<Vec<_>>().join("/")
    }

    /// Whether an outline's split is known: written its own way, or split by its sounds as the
    /// table writes it. Only a known split is written short; any other outline (a brief, a
    /// misstroke, a stroke no entry has) is written raw, a key each.
    #[must_use]
    pub fn known(&self, raw: &str) -> bool {
        self.outlines.contains_key(raw) || self.agree.contains(raw)
    }

    /// An outline as each stroke's pieces: an outline written its own way ([`Self::outlines`])
    /// split as its own text reads, one whose split agrees with the table by the table, any other
    /// a key a piece, raw.
    #[must_use]
    pub fn outline_pieces(&self, strokes: &[Stroke]) -> Vec<Vec<Piece>> {
        let raw = crate::stroke::render_outline(strokes);
        if !self.known(&raw) {
            return strokes.iter().map(|&s| raw_pieces(s)).collect();
        }
        let own = self.outlines.get(&raw).and_then(|short| {
            let parts: Vec<&str> = short.split('/').collect();
            (parts.len() == strokes.len()).then_some(())?;
            parts
                .iter()
                .zip(strokes)
                .map(|(part, stroke)| {
                    let pieces = self.read_short(part)?;
                    (pieces.iter().fold(0, |a, p| a | p.keys) == stroke.0).then_some(pieces)
                })
                .collect::<Option<Vec<_>>>()
        });
        own.unwrap_or_else(|| strokes.iter().map(|&s| self.pieces(s)).collect())
    }

    /// The short form read back as pieces: `+`, `~` and `#` anywhere (each once), the left hand up
    /// to the first vowel, star or `-`, those as they are and in steno order, then the right hand,
    /// each hand read greedily, longest token first. keymux's `parse_short`.
    fn read_short(&self, text: &str) -> Option<Vec<Piece>> {
        let mut out = Vec::new();
        let mut rest = String::with_capacity(text.len());
        for ch in text.chars() {
            let key = match ch {
                '+' => Stroke::JOIN,
                '~' => Stroke::SPELL,
                '#' => NUMBER,
                _ => {
                    rest.push(ch);
                    continue;
                }
            };
            if out.iter().any(|p: &Piece| p.keys == key) {
                return None;
            }
            out.push(Piece { text: ch.to_string(), keys: key });
        }
        out.sort_by_key(|p| match p.keys {
            Stroke::JOIN => 0,
            Stroke::SPELL => 1,
            _ => 2,
        });
        // The vowel keys' tokens: the marks, their ASCII spellings (`OO` for `Ʊ`), each vowel key,
        // the star and the `-`, longest first; the vowel part is every character of a token
        let mut middle: Vec<(String, u32)> = self.middle.iter().map(|(keys, text)| (text.clone(), *keys)).collect();
        for (symbol, ascii) in ASCII {
            if let Some((keys, _)) = self.middle.iter().find(|(_, text)| text == symbol)
                && !self.middle.iter().any(|(_, text)| text == ascii)
            {
                middle.push((ascii.to_owned(), *keys));
            }
        }
        middle.extend((0..32).filter(|i| MIDDLE & (1 << i) != 0).map(|i| (raw(1 << i), 1 << i)));
        middle.push(("-".to_owned(), 0));
        middle.sort_by_key(|(text, _)| std::cmp::Reverse(text.chars().count()));
        let is_middle = |ch: char| middle.iter().any(|(text, _)| text.contains(ch));
        let start = rest.find(is_middle).unwrap_or(rest.len());
        let end = rest[start..].find(|ch: char| !is_middle(ch)).map_or(rest.len(), |i| start + i);
        read_hand(&rest[..start], &hand_tokens(&self.left, Stroke::LEFT), &mut out)?;
        // In any order, each key once: a mark and the star (`Ī*`) or the keys as they are; one `-`
        let at = out.len();
        read_hand(&rest[start..end], &middle, &mut out)?;
        let mut taken = 0u32;
        let mut dashed = false;
        for piece in &out[at..] {
            if piece.keys == 0 {
                if dashed {
                    return None;
                }
                dashed = true;
            } else if piece.keys & taken != 0 {
                return None;
            }
            taken |= piece.keys;
        }
        read_hand(&rest[end..], &hand_tokens(&self.right, Stroke::RIGHT), &mut out)?;
        Some(out)
    }

    /// One stroke, raw or short: text that is exactly the raw notation of what it parses to is
    /// raw; otherwise the short form; otherwise the raw notation read leniently. So `TPHAEPL` and
    /// `NAEM` are the same stroke. keymux's `parse_stroke`.
    #[must_use]
    pub fn parse_stroke(&self, text: &str) -> Option<Stroke> {
        let raw = Stroke::parse(text);
        if let Some(stroke) = raw
            && stroke.to_string() == text
        {
            return Some(stroke);
        }
        let short = self.read_short(text).map(|pieces| Stroke(pieces.iter().fold(0, |a, p| a | p.keys)));
        short.filter(|s| s.0 != 0).or(raw)
    }

    /// An outline, raw or short, strokes split by `/` (spaces around a `/` ignored): an outline
    /// written its own way ([`Self::outlines`]) is read back whole first, else stroke by stroke
    /// ([`Self::parse_stroke`]).
    #[must_use]
    pub fn parse_outline(&self, text: &str) -> Option<Vec<Stroke>> {
        let text = text.split('/').map(str::trim).collect::<Vec<_>>().join("/");
        if let Some((raw, _)) = self.outlines.iter().find(|(_, short)| **short == text) {
            return crate::stroke::parse_outline(raw);
        }
        text.split('/').map(|part| self.parse_stroke(part)).collect()
    }

    /// The table keymux's notation file gives ([`Self::to_json`]), read as keymux reads it: an
    /// alias whose keys are not of its hand, whose letters are not capitals or hold a vowel key's
    /// letter, or are already taken, is left out; so is an outline that does not read back.
    #[must_use]
    pub fn from_json(text: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(text).ok()?;
        let hand = |side: &str, mask: u32, dash: &str| -> Vec<Alias> {
            let mut out: Vec<Alias> = Vec::new();
            for (keys, letters) in value.get(side).and_then(serde_json::Value::as_object).into_iter().flatten() {
                let Some(stroke) = Stroke::parse(&format!("{dash}{keys}")).filter(|s| s.0 & !mask == 0) else { continue };
                let Some(letters) = letters.as_str().map(str::to_ascii_uppercase) else { continue };
                // Capitals (a symbol with a mark is one), none a vowel key's letter for a hand
                let usable = !letters.is_empty()
                    && letters.chars().all(|c| c.is_alphabetic() && !c.is_lowercase())
                    && (mask == Stroke::VOWELS || !letters.chars().any(|c| "AOEU".contains(c)))
                    && !out.iter().any(|(b, t)| *t == letters || *b == stroke.0);
                if usable {
                    out.push((stroke.0, letters));
                }
            }
            out
        };
        let mut notation = Self {
            left: hand("left", Stroke::LEFT, ""),
            right: hand("right", Stroke::RIGHT, "-"),
            middle: hand("middle", Stroke::VOWELS, ""),
            ..Self::default()
        };
        let mut kept = BTreeMap::new();
        for (raw, short) in value.get("outlines").and_then(serde_json::Value::as_object).into_iter().flatten() {
            let (Some(short), Some(strokes)) = (short.as_str(), crate::stroke::parse_outline(raw)) else { continue };
            if notation.parse_outline(short).as_ref() == Some(&strokes) {
                kept.insert(raw.clone(), short.to_owned());
            }
        }
        notation.outlines = kept;
        notation.agree = value
            .get("agree")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().filter(|raw| crate::stroke::parse_outline(raw).is_some()).map(str::to_owned))
            .collect();
        Some(notation)
    }

    /// keymux's notation file: `#` with letters, the aliases of each hand, the outlines written
    /// their own way.
    #[must_use]
    pub fn to_json(&self) -> String {
        let hand = |aliases: &[Alias]| -> serde_json::Map<String, serde_json::Value> {
            aliases.iter().map(|(bits, text)| (raw(*bits), serde_json::Value::String(text.clone()))).collect()
        };
        let ascii: serde_json::Map<String, serde_json::Value> =
            ASCII.iter().map(|(symbol, text)| ((*symbol).to_owned(), serde_json::Value::String((*text).to_owned()))).collect();
        let value = serde_json::json!({
            "number": "letters",
            "left": hand(&self.left),
            "right": hand(&self.right),
            "middle": hand(&self.middle),
            "ascii": ascii,
            "outlines": self.outlines,
            "agree": self.agree,
        });
        let mut text = serde_json::to_string_pretty(&value).unwrap_or_default();
        text.push('\n');
        text
    }
}

/// The notation every outline a person reads is written in: keymux's notation file as `build`
/// wrote it beside the built dictionary (so lyk and keymux write a stroke the same way), else,
/// before any build, the base aliases of the built-in rules. Tests use the table measured on the
/// shared test dictionary, read back through its file as keymux reads it.
#[must_use]
pub fn shown() -> &'static Notation {
    static SHOWN: std::sync::OnceLock<Notation> = std::sync::OnceLock::new();
    SHOWN.get_or_init(load)
}

#[cfg(not(test))]
fn load() -> Notation {
    let file = crate::paths::built_dir().join(FILE);
    std::fs::read_to_string(file).ok().and_then(|text| Notation::from_json(&text)).unwrap_or_else(|| {
        let (left, right) = Rules::built_in().map_or_else(|_| Default::default(), |rules| base(&rules));
        Notation { left, right, middle: vowel_marks(), ..Notation::default() }
    })
}

#[cfg(test)]
fn load() -> Notation {
    let setup = crate::dictionary_fixture::setup();
    let entries: Vec<(String, Vec<Stroke>)> =
        crate::dictionary_fixture::generated().entries.iter().map(|e| (e.word.clone(), e.outline.clone())).collect();
    let measured = measure(&setup.rules, &setup.pron, &entries, Some((test_model(), &aligned_entries()))).notation;
    Notation::from_json(&measured.to_json()).unwrap_or(measured)
}

/// The solver's model of the shared test dictionary, as `build` packs it.
#[cfg(test)]
pub fn test_model() -> &'static crate::rules::solve::Model {
    static MODEL: std::sync::LazyLock<crate::rules::solve::Model> = std::sync::LazyLock::new(|| {
        let setup = crate::dictionary_fixture::setup();
        let lexicon = crate::rules::solve::Lexicon { pron: &setup.pron, rank: &setup.rank, spellings: &setup.spellings, bigrams: &setup.web_bigrams, common: 100_000 };
        let entries = &crate::dictionary_fixture::generated().entries;
        let blob = crate::rules::solve::pack(&setup.rules, &lexicon, &crate::rules::solve::stems(entries), None);
        crate::rules::solve::Model::load(&blob).expect("the packed blob loads")
    });
    &MODEL
}

/// The entries `build` attributes with the solver's tables: every one but the backups.
#[cfg(test)]
fn aligned_entries() -> Vec<(String, Vec<Stroke>)> {
    crate::dictionary_fixture::generated()
        .entries
        .iter()
        .filter(|e| e.origin != crate::generate::Origin::Backup)
        .map(|e| (e.word.clone(), e.outline.clone()))
        .collect()
}

/// An outline as a person reads it ([`shown`]): short where its split is known (an outline lyk
/// wrote from its sounds, or one written its own way), else raw. Every outline lyk prints for
/// people goes through here; files other programs read keep [`crate::stroke::render_outline`].
#[must_use]
pub fn outline(strokes: &[Stroke]) -> String {
    shown().outline(strokes)
}

/// One stroke as a person reads it: short when it is a one-stroke outline whose split is known,
/// else raw (see [`outline`]).
#[must_use]
pub fn stroke(stroke: Stroke) -> String {
    shown().outline(&[stroke])
}

/// An outline held as raw text (a dictionary key, a line of a file) as a person reads it; text that
/// is no outline is returned as it is.
#[must_use]
pub fn shorten(raw: &str) -> String {
    crate::stroke::parse_outline(raw).map_or_else(|| raw.to_owned(), |strokes| outline(&strokes))
}

/// An outline a person typed, raw or short ([`Notation::parse_outline`] of [`shown`]).
#[must_use]
pub fn parse_outline(text: &str) -> Option<Vec<Stroke>> {
    shown().parse_outline(text)
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
    /// Of those, the ones the solver's chord tables attributed (not written syllable by syllable)
    pub aligned: usize,
    /// Of the outlines given for attribution (every deployed one), how many have a known split
    pub others_known: usize,
    /// How many were given
    pub others: usize,
    /// Each vowel chord (the vowel keys of a stroke written from its sounds) and the sounds it
    /// wrote, how often
    pub vowels: BTreeMap<u32, BTreeMap<String, usize>>,
    /// Each right-hand chord a coda piece was written with (two keys or more) and the sounds it
    /// wrote, how often
    pub chords: BTreeMap<u32, BTreeMap<String, usize>>,
}

/// A stroke's left and right run in short form.
type Runs = (String, String);

/// One stroke's runs as its sounds split them: the short form of the left and the right run, or
/// `None` where the pieces do not add up to the stroke's keys.
fn sounded(rules: &Rules, aliases: &(Vec<Alias>, Vec<Alias>), syllable: &Syllable, stroke: Stroke) -> Option<(String, String)> {
    // Each piece's tokens: a sound with an alias of its own is that alias; a sound written with
    // keys that are no alias (`-FRB` for the *v* of `serve`) is its keys as they are, so no alias
    // claims a sound that is not there; a run of sounds a cluster rule writes is its sounds'
    // letters (`-FT` is S T), else read by the table. All tokens in order of their lowest key, and
    // two tokens side by side whose keys are a [`CLUSTERS`] chord are its symbol (K and S, `X`).
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
                all.push((bits, text.clone()));
            } else if let [sound] = sounds.as_slice()
                && let Some(text) = respelled(sound)
            {
                // A sound written with the `R` its spelling has (`CH.rch`, `-FRPB`, `RČ`)
                all.push((bits, text));
            } else if sounds.len() == 1 {
                all.extend((0..32).filter(|i| bits & (1 << i) != 0).map(|i| (1 << i, raw(1 << i))));
            } else if let Some(text) = sounds.iter().map(|s| letters(s)).collect::<Option<String>>() {
                all.push((bits, text));
            } else {
                all.extend(tokens(table, bits));
            }
        }
        all.sort_by_key(|(keys, _)| keys.trailing_zeros());
        merge_clusters(&mut all);
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

/// Two tokens side by side whose keys are a [`CLUSTERS`] chord, written as its symbol when they
/// are the chord's own sounds: the same letters in another order (`ŠN` and `T` are `TŠN`), or the
/// letters of the sounds it stands for (`K` and `S` are `X`).
fn merge_clusters(all: &mut Vec<(u32, String)>) {
    let clusters: Vec<(u32, &str, &str)> =
        CLUSTERS.iter().filter_map(|(keys, text, sounds)| Some((Stroke::parse(keys)?.0, *text, *sounds))).collect();
    let mut i = 0;
    while i + 1 < all.len() {
        let both = all[i].0 | all[i + 1].0;
        let mut have: Vec<char> = format!("{}{}", all[i].1, all[i + 1].1).chars().collect();
        have.sort_unstable();
        let same = |text: &str| {
            let mut want: Vec<char> = text.chars().collect();
            want.sort_unstable();
            want == have
        };
        let found = clusters.iter().find(|(keys, text, sounds)| {
            *keys == both && (same(text) || sounds.split(' ').map(letters).collect::<Option<String>>().is_some_and(|l| same(&l)))
        });
        if let Some((_, text, _)) = found {
            all[i] = (both, (*text).to_owned());
            all.remove(i + 1);
        } else {
            i += 1;
        }
    }
}

/// A word's (or a phrase's, word by word) pronunciations, each sound a `CMUdict` name.
fn sounds_of(pron: &crate::data::Pronunciations, text: &str) -> Option<Vec<Vec<String>>> {
    let words: Vec<String> = text
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'').to_lowercase())
        .collect();
    if words.is_empty() || words.iter().any(String::is_empty) {
        return None;
    }
    if let [word] = words.as_slice() {
        return pron.get(word).cloned();
    }
    // A phrase: each word's first pronunciation, in order
    let mut all = Vec::new();
    for word in &words {
        all.extend(pron.get(word)?.first()?.iter().cloned());
    }
    Some(vec![all])
}

/// One hand's keys written as `groups` split them: from the lowest key, the group holding it (all
/// down) as the alias for exactly those keys or their own letters, else the key's own letter.
fn split_text(aliases: &[Alias], bits: u32, groups: &[u32]) -> String {
    let mut left = bits;
    let mut all: Vec<(u32, String)> = Vec::new();
    while left != 0 {
        let low = left.isolate_lowest_one();
        let group = groups.iter().copied().find(|&g| g & low != 0 && g & !left == 0 && g & !bits == 0).unwrap_or(low);
        all.push((group, aliases.iter().find(|(keys, _)| *keys == group).map_or_else(|| raw(group), |(_, text)| text.clone())));
        left &= !group;
    }
    merge_clusters(&mut all);
    all.into_iter().map(|(_, text)| text).collect()
}

/// The solver's model and the outlines to attribute with its chord tables ([`measure`]).
pub type Attribute<'a> = (&'a crate::rules::solve::Model, &'a [(String, Vec<Stroke>)]);

/// The fewest strokes a measured compound must write before it is one.
const MIN_COMPOUND: usize = 10;

/// The short notation for these entries: the base aliases, a compound alias for each run whose
/// sounds usually split another way than they read it, and the outlines whose own sounds split
/// another way than the finished table reads them. Only strokes the rules write syllable by
/// syllable from a word's sounds count: a brief's keys say nothing about its sounds.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn measure(rules: &Rules, pron: &crate::data::Pronunciations, entries: &[(String, Vec<Stroke>)], align: Option<Attribute>) -> Measured {
    let aliases = base(rules);
    let middle = vowel_marks();
    let curated: Vec<u32> = CLUSTERS.iter().filter_map(|(keys, _, _)| Some(Stroke::parse(keys)?.0)).collect();
    let mut ways: HashMap<(bool, u32), BTreeMap<String, usize>> = HashMap::new();
    let mut vowels: BTreeMap<u32, BTreeMap<String, usize>> = BTreeMap::new();
    let mut chords: BTreeMap<u32, BTreeMap<String, usize>> = BTreeMap::new();
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
        // What the vowel chords and the right hand's chords wrote (the first pronunciation that
        // reads the outline back)
        if let Some(syllables) = variants.iter().find_map(|sounds| rules.syllables(&rules.label(word, sounds)).filter(|s| s.len() == outline.len())) {
            for (syllable, stroke) in syllables.iter().zip(outline.iter()) {
                let v = stroke.0 & Stroke::VOWELS;
                if v.count_ones() >= 2 {
                    let sound: String = syllable.vowel.split('.').next().unwrap_or_default().chars().filter(char::is_ascii_alphabetic).collect();
                    *vowels.entry(v).or_default().entry(sound).or_default() += 1;
                }
                for (keys, sounds) in rules.consonant_pieces(&syllable.coda, false).into_iter().flatten() {
                    let keys = keys.0 & Stroke::RIGHT;
                    if keys.count_ones() >= 2 {
                        let sounds: Vec<&str> = sounds.iter().map(|s| s.split('.').next().unwrap_or_default()).collect();
                        *chords.entry(keys).or_default().entry(sounds.join(" ")).or_default() += 1;
                    }
                }
            }
        }
        for (stroke, (left, right)) in outline.iter().zip(&runs) {
            for (is_left, keys, text) in [(true, stroke.0 & Stroke::LEFT, left), (false, stroke.0 & Stroke::RIGHT, right)] {
                if keys.count_ones() >= 2 {
                    *ways.entry((is_left, keys)).or_default().entry(text.clone()).or_default() += 1;
                }
            }
        }
        known.push((outline, runs));
    }
    // Every other outline (short forms, folded strokes, briefs, phrases, extras...) whose keys the
    // solver's chord tables attribute to its word's sounds one way only
    let mut aligned = 0;
    let (mut others_known, mut others_total) = (0, 0);
    if let Some((model, others)) = align {
        let seen: std::collections::HashSet<&[Stroke]> = known.iter().map(|(o, _)| *o).collect();
        let read = |(word, outline): &(String, Vec<Stroke>)| -> Option<Vec<Runs>> {
                if seen.contains(outline.as_slice()) || outline.iter().any(|s| s.0 & (Stroke::SPELL | NUMBER) != 0) {
                    return None;
                }
                let run: Vec<u32> = outline.iter().map(|s| s.0).collect();
                sounds_of(pron, word)?.iter().find_map(|sounds| model.split_for(&run, sounds)).map(|split| {
                    outline
                        .iter()
                        .zip(&split)
                        .map(|(s, groups)| (split_text(&aliases.0, s.0 & Stroke::LEFT, groups), split_text(&aliases.1, s.0 & Stroke::RIGHT, groups)))
                        .collect()
                })
        };
        // Four threads, each a share of the outlines, in order
        let share = others.len().div_ceil(4).max(1);
        let found: Vec<Option<Vec<Runs>>> = std::thread::scope(|scope| {
            // Every thread is started before any is joined
            #[allow(clippy::needless_collect)]
            let handles: Vec<_> = others.chunks(share).map(|chunk| scope.spawn(move || chunk.iter().map(read).collect::<Vec<_>>())).collect();
            handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
        });
        others_known = others.iter().filter(|(_, o)| seen.contains(o.as_slice())).count();
        others_total = others.len();
        for ((_, outline), runs) in others.iter().zip(found) {
            let Some(runs) = runs else { continue };
            aligned += 1;
            others_known += 1;
            for (stroke, (left, right)) in outline.iter().zip(&runs) {
                for (is_left, keys, text) in [(true, stroke.0 & Stroke::LEFT, left), (false, stroke.0 & Stroke::RIGHT, right)] {
                    if keys.count_ones() >= 2 {
                        *ways.entry((is_left, keys)).or_default().entry(text.clone()).or_default() += 1;
                    }
                }
            }
            known.push((outline, runs));
        }
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
        // Not over a cluster symbol, and never a spelling another chord of the hand already has:
        // it would read back as that chord (those outlines are written their own way instead)
        // Nor one that is only the keys' own letters (it would only keep the table from reading
        // them), nor one a handful of strokes use: those outlines are written their own way
        let hand = if is_left { &mut left } else { &mut right };
        let used = counted.get(best).copied().unwrap_or(0);
        // Its keys' own letters in another order say only the order of the sounds, not a group
        let sorted = |t: &str| {
            let mut c: Vec<char> = t.chars().collect();
            c.sort_unstable();
            c
        };
        if *best != table
            && !curated.contains(&bits)
            && !hand.iter().any(|(_, t)| t == best)
            && sorted(best) != sorted(&raw(bits))
            && used >= MIN_COMPOUND
        {
            hand.push((bits, best.clone()));
        }
    }
    let mut notation = Notation { left, right, middle, ..Notation::default() };
    // The outlines the finished table reads another way than their sounds split, and those it
    // reads as they split: both are known splits
    let mut agree = BTreeSet::new();
    for (outline, runs) in &known {
        let own: Vec<String> = outline.iter().zip(runs).map(|(&s, (l, r))| assemble(s, l, &notation.middle, r)).collect();
        let own = own.join("/");
        let raw = crate::stroke::render_outline(outline);
        // Known with no group at all: written raw either way, so not listed
        if own == raw {
            continue;
        }
        if own == notation.table_outline(outline) {
            agree.insert(raw);
        } else if notation.parse_outline(&own).as_deref() == Some(*outline) {
            notation.outlines.insert(raw, own);
        }
        // Else its own split does not read back as its keys (two sounds on one key, `K` and the
        // `Y` of `KWR`): not known, written raw
    }
    notation.agree = agree;
    Measured { notation, splits, known: known.len(), vowels, chords, aligned, others_known, others: others_total }
}

/// What [`measure`] found, as `build --report` prints it: each run split more than one way, how
/// often each way, and the way the table now reads it.
#[must_use]
pub fn report(measured: &Measured) -> String {
    let mut out = format!(
        "short notation: {} outlines read back to their sounds ({} of them attributed by the solver's chord tables, not syllable by syllable), {} written their own way; {} of {} deployed outlines have a known split\n",
        measured.known,
        measured.aligned,
        measured.notation.outlines.len(),
        measured.others_known,
        measured.others
    );
    for split in &measured.splits {
        let keys = if split.left { raw(split.keys) } else { format!("-{}", raw(split.keys)) };
        let total: usize = split.ways.values().sum();
        let mut ways: Vec<(&String, &usize)> = split.ways.iter().collect();
        ways.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        let shown: Vec<String> = ways.iter().map(|(text, n)| format!("{text} {n}")).collect();
        let _ = writeln!(out, "  {keys:<8} {total:>6} strokes: {}  (base reads {})", shown.join(", "), split.table);
    }
    let tally = |counts: &BTreeMap<String, usize>| {
        let mut all: Vec<(&String, &usize)> = counts.iter().collect();
        all.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        all.iter().take(6).map(|(sound, n)| format!("{sound} {n}")).collect::<Vec<_>>().join(", ")
    };
    let _ = writeln!(out, "vowel chords and the sounds they write (strokes written from a word's sounds):");
    for (keys, counts) in &measured.vowels {
        let mark = measured.notation.middle.iter().find(|(k, _)| k == keys).map_or("", |(_, m)| m.as_str());
        let _ = writeln!(out, "  {:<5} {mark:<2} {:>6}: {}", raw(*keys), counts.values().sum::<usize>(), tally(counts));
    }
    let _ = writeln!(out, "right-hand chords and the sounds they write:");
    for (keys, counts) in &measured.chords {
        let text = cluster(&measured.notation.right, *keys);
        let _ = writeln!(out, "  -{:<7} {text:<4} {:>6}: {}", raw(*keys), counts.values().sum::<usize>(), tally(counts));
    }
    let _ = writeln!(
        out,
        "known splits: {} outlines agree with the table, {} written their own way",
        measured.notation.agree.len(),
        measured.notation.outlines.len()
    );
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
        Notation { left, right, middle: vowel_marks(), ..Notation::default() }
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
        assert_eq!(find(&t.left, "TH").as_deref(), Some("Þ"));
        assert_eq!(find(&t.left, "KH").as_deref(), Some("Č"));
        assert_eq!(find(&t.right, "RB").as_deref(), Some("Š"));
        assert_eq!(find(&t.right, "FP").as_deref(), Some("Č"));
        assert_eq!(find(&t.right, "PBG").as_deref(), Some("Ŋ"));
        assert_eq!(find(&t.right, "GS").as_deref(), Some("ŠN"));
        assert_eq!(find(&t.right, "BGS").as_deref(), Some("X"));
        assert_eq!(find(&t.left, "KP").as_deref(), Some("X"));
        assert_eq!(find(&t.right, "FRB").as_deref(), Some("RV"));
        assert_eq!(find(&t.right, "GTS").as_deref(), Some("TŠN"));
        assert_eq!(find(&t.left, "SR").as_deref(), Some("V"));
        assert_eq!(find(&t.left, "S"), None, "a key written as its own letter needs none");
    }

    #[test]
    fn strokes_are_written_short() {
        let t = table();
        assert_eq!(t.stroke(s("TPHAEPL")), "NAEM");
        assert_eq!(t.stroke(s("-PB")), "-N");
        assert_eq!(t.stroke(s("KAT")), "KAT");
        assert_eq!(t.stroke(s("+STKPWAEUGS")), "+ZĀŠN");
        assert_eq!(t.stroke(s("#TR-RL")), "#TR-RL");
        assert_eq!(t.stroke(s("SKWR-PBLG")), "J-J");
        assert_eq!(t.stroke(s("TPHROUR")), "NROUR", "without the measured compound");
        // The vowel marks, the star after the mark, and the keys as they are without one
        assert_eq!(t.stroke(s("TPHAOEUT")), "NĪT");
        assert_eq!(t.stroke(s("TKAEU")), "DĀ");
        assert_eq!(t.stroke(s("HROE")), "LŌ");
        assert_eq!(t.stroke(s("TPHAO*EUT")), "NĪ*T");
        assert_eq!(t.stroke(s("TPHA*T")), "NA*T");
        assert_eq!(t.stroke(s("TPH*EUT")), "NI*T");
        // The right hand's symbols, and a T struck inside the shun chord
        assert_eq!(t.stroke(s("TPHAEUGTS")), "NĀTŠN");
        assert_eq!(t.stroke(s("TPHAEUGS")), "NĀŠN");
        assert_eq!(t.stroke(s("AEBGS")), "AEX");
        assert_eq!(t.stroke(s("PWAUL")), "BÔL");
        assert_eq!(t.stroke(s("PWAOBG")), "BƱK");
        assert_eq!(t.stroke(s("PWOEUL")), "BOIL");
        assert_eq!(t.stroke(s("HOUS")), "HOUS");
        assert_eq!(t.stroke(s("WEURB")), "WIŠ");
        assert_eq!(t.stroke(s("PHUFP")), "MUČ");
        assert_eq!(t.stroke(s("TABGS")), "TAX");
        assert_eq!(t.stroke(s("KPEBGT")), "XEKT");
        for (raw, short) in [("TPHAOEUT", "NĪT"), ("TPHAO*EUT", "NĪ*T"), ("TPHAEUGTS", "NĀTŠN"), ("WEURB", "WIŠ"), ("PWOEUL", "BOIL"), ("PWAOBG", "BƱK")] {
            assert_eq!(t.parse_stroke(short), Some(s(raw)), "{short}");
            assert_eq!(t.parse_stroke(raw), Some(s(raw)), "{raw}");
        }
        // ASCII input where it reads no other keys, and the star on either side of a mark
        assert_eq!(t.parse_stroke("NIT"), Some(s("TPHEUT")), "I is EU, the short i");
        assert_eq!(t.stroke(s("SEUT")), "SIT");
        assert_eq!(t.parse_stroke("SEUT"), Some(s("SEUT")));
        assert_eq!(t.parse_stroke("WISH"), Some(s("WEURB")));
        assert_eq!(t.parse_stroke("WEUSH"), Some(s("WEURB")));
        assert_eq!(t.parse_stroke("MUCH"), Some(s("PHUFP")));
        assert_eq!(t.parse_stroke("CHEUP"), Some(s("KHEUP")));
        assert_eq!(t.parse_stroke("SING"), Some(s("SEUPBG")));
        assert_eq!(t.parse_stroke("N*ĪT"), Some(s("TPHAO*EUT")));
        assert_eq!(t.parse_stroke("NĪAT"), None, "a key twice");
        assert_eq!(t.parse_stroke("BOOK"), Some(s("PWAOBG")), "OO for Ʊ");
        assert_eq!(t.parse_stroke("BOI*L"), Some(s("PWO*EUL")));
    }

    #[test]
    fn the_measured_table_reads_flower_as_its_sounds() {
        let setup = crate::dictionary_fixture::setup();
        let generated = crate::dictionary_fixture::generated();
        let entries: Vec<(String, Vec<Stroke>)> =
            generated.entries.iter().map(|e| (e.word.clone(), e.outline.clone())).collect();
        let measured = measure(&setup.rules, &setup.pron, &entries, None);
        let n = &measured.notation;
        assert_eq!(cluster(&n.left, s("TPHR").0), "FL", "{}", report(&measured));
        assert_eq!(n.stroke(s("TPHROUR")), "FLOUR");
        assert_eq!(n.stroke(s("TPHAEPL")), "NAEM");
        assert!(measured.known > 1000, "{}", measured.known);
        // `orb` is O, R and B, not the SH of `-RB`: known with no group, so written raw and not
        // listed
        assert!(!n.outlines.contains_key("ORB"));
        assert_eq!(n.outline(&[s("ORB")]), "ORB");
        assert_eq!(n.stroke(s("ORB")), "OŠ", "the table alone");
        // No alias is only its keys' own letters, and `tax` reads as the table writes it
        assert!(n.left.iter().chain(&n.right).all(|(b, t)| raw(*b) != *t), "{:?} {:?}", n.left, n.right);
        assert_eq!(n.stroke(s("TABGS")), "TAX");
        assert!(!n.outlines.contains_key("TABGS"));
        // Each stroke has one vowel sound, so no vowel chord is ever two sounds: no outline is an
        // exception for its vowel keys alone
        assert!(measured.vowels.values().flat_map(BTreeMap::keys).all(|sound| !sound.contains(' ')));
        let json: serde_json::Value = serde_json::from_str(&n.to_json()).expect("json");
        assert_eq!(json["number"], "letters");
        assert_eq!(json["middle"]["AOEU"], "Ī");
        assert_eq!(json["middle"]["EU"], "I");
        assert_eq!(json["left"]["TPHR"], "FL");
    }

    #[test]
    fn the_shown_notation_writes_and_reads_both_forms() {
        let n = shown();
        let outline = |text: &str| crate::stroke::parse_outline(text).expect("raw");
        // An outline lyk wrote from its sounds is known: short. FL is a measured compound
        assert!(n.known("TPHRAOEUT"));
        assert_eq!(outline_of(n, "TPHRAOEUT"), "FLĪT");
        // Folded, short and brief outlines whose keys the chord tables attribute to their word's
        // sounds one way only are known too: each as its own sounds split it
        for (raw, short) in [
            ("TPHROUR", "FLOUR"),      // flower, two syllables folded on one stroke
            ("TPHRA*EUTS", "NRĀ*TS"),  // narrates: TPHR is N and R here, never FL
            ("TPHAEUFP", "NĀČ"),       // nature
            ("UFP", "UČ"),             // much
            ("-PBG", "-Ŋ"),            // think
            ("SKWRU", "JU"),           // judge
            ("TPHAOEUT", "NĪT"),       // night
        ] {
            assert!(n.known(raw), "{raw}");
            assert_eq!(outline_of(n, raw), short, "{raw}");
        }
        // Keys the tables cannot attribute one way stay raw: `notation`'s T before its vowel in
        // the keys, after it in the word; `nature`'s TPHRAEUFP (no R sound before the vowel)
        assert_eq!(outline_of(n, "TPHAEUGTS"), "TPHAEUGTS");
        assert_eq!(outline_of(n, "TPHRAEUFP"), "TPHRAEUFP");
        // A brief's split is not known: raw, though the table would name it
        assert!(!n.known("TPHAEPL"));
        assert_eq!(outline_of(n, "TPHAEPL"), "TPHAEPL");
        assert_eq!(n.table_outline(&outline("TPHAEPL")), "NAEM");
        assert_eq!(outline_of(n, "-S"), "-S", "`yes`, a brief");
        // A stroke no entry has stays raw
        assert_eq!(outline_of(n, "TPHAOEUTS/TKPWAOEUT"), "TPHAOEUTS/TKPWAOEUT");
        // An outline written its own way: `orb` is O R B, not the SH the table reads `-RB` as
        assert_eq!(outline_of(n, "ORB"), "ORB");
        assert_eq!(n.stroke(s("ORB")), "OŠ");
        // A number-key stroke by the table: `#` and letters
        assert_eq!(n.stroke(s("#TPH-PB")), "#N-N");
        // A join: `/+` stays as it is, each stroke by the table
        assert_eq!(n.table_outline(&outline("EPB/+PHEU")), "EN/+MI");
        for (raw, short) in [("TPHAEPL", "NAEM"), ("TPHROUR", "FLOUR"), ("ORB", "ORB"), ("#TPH-PB", "#N-N"), ("EPB/+PHEU", "EN/+MI")] {
            assert_eq!(n.parse_outline(raw), Some(outline(raw)), "{raw}");
            assert_eq!(n.parse_outline(short), Some(outline(raw)), "{short}");
        }
        // The table's own reading of `orb` reads back too, and spaces around a slash are ignored
        assert_eq!(n.parse_outline("OSH"), Some(outline("ORB")), "ASCII for Š");
        assert_eq!(n.parse_outline("OŠ"), Some(outline("ORB")));
        assert_eq!(n.parse_outline("EN / +MI"), Some(outline("EPB/+PHEU")));
        // `PL` is not how the raw notation writes `P-L`, so it is short: P and L (`HR`)
        assert_eq!(n.parse_stroke("PL"), Some(s("PHR")));
        assert_eq!(n.parse_stroke("P-L"), Some(s("P-L")));
        assert_eq!(n.parse_stroke("NAEQ"), None);
        // Most outlines lyk wrote from sounds are known
        assert!(n.agree.len() > 10_000, "{}", n.agree.len());
    }

    fn outline_of(n: &Notation, raw: &str) -> String {
        n.outline(&crate::stroke::parse_outline(raw).expect("raw"))
    }

    #[test]
    fn every_stroke_written_reads_back_as_itself() {
        let n = shown();
        let mut state = 0x2545_f491_u32;
        for _ in 0..20_000 {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let bits = state & ((1 << 25) - 1);
            // The join key and the spelling marker never share a stroke
            if bits & !(Stroke::JOIN | Stroke::SPELL) == 0 || bits & Stroke::JOIN != 0 && bits & Stroke::SPELL != 0 {
                continue;
            }
            let text = n.stroke(Stroke(bits));
            assert_eq!(n.parse_stroke(&text), Some(Stroke(bits)), "{text}");
            let pieces: String = n.pieces(Stroke(bits)).into_iter().map(|p| p.text).collect();
            assert_eq!(pieces, text);
        }
        // Every outline written its own way reads back, and its pieces spell it
        for (raw, short) in &n.outlines {
            let strokes = crate::stroke::parse_outline(raw).expect("raw");
            assert_eq!(n.parse_outline(short).as_ref(), Some(&strokes), "{raw} {short}");
            let pieces = n.outline_pieces(&strokes);
            let text = pieces.iter().map(|p| p.iter().map(|p| p.text.as_str()).collect::<String>()).collect::<Vec<_>>().join("/");
            assert_eq!(&text, short);
        }
    }

    #[test]
    fn the_file_reads_back_as_the_table_it_was_written_from() {
        let n = shown();
        let back = Notation::from_json(&n.to_json()).expect("json");
        assert_eq!(&back, n);
        // An alias keymux would drop is dropped here too
        let odd = Notation::from_json(r#"{"left": {"TPH": "N", "PW": "N", "TK": "DA", "HR": "l"}, "right": {"PB": "N"}}"#).expect("json");
        assert_eq!(odd.left.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>(), ["L", "N"]);
        assert_eq!(odd.right.len(), 1);
    }
}

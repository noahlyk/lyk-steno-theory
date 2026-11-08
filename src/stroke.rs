//! Steno strokes as bits, in Plover's notation (`KAT`, `-T`, `STKPW`, `TKPWHR*EUFPLT`).

use std::fmt;

/// The keys in steno order. Index is the bit.
const KEYS: &[u8; 23] = b"#STKPWHRAO*EUFRPBLGTSDZ";
/// Index of the first key on the right hand: `F`.
const RIGHT: u32 = 13;
/// Index of the first key of the vowel block: `A`.
const VOWEL: u32 = 8;

/// Which keys of a stroke are down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Stroke(pub u32);

const fn mask(from: u32, to: u32) -> u32 {
    ((1u32 << (to - from + 1)) - 1) << from
}

impl Stroke {
    pub const LEFT: u32 = mask(1, 7);
    /// `A`, `O`, `E`, `U`, without the star.
    pub const VOWELS: u32 = mask(8, 9) | mask(11, 12);
    pub const STAR: u32 = 1 << 10;
    pub const RIGHT: u32 = mask(RIGHT, 22);
    /// The join key: the stroke continues the word before it. Written `+` before the keys.
    pub const JOIN: u32 = 1 << 23;
    /// The spelling-mode marker: this stroke starts a run read letter-by-letter from
    /// `rules/spelling.toml` instead of sounded out. Written `~` before the keys, only ever on
    /// the first stroke of such a run — every stroke after it carries the ordinary join key
    /// instead, the same way any other multi-stroke word continues. See `site/reference/spelling-mode.json`.
    pub const SPELL: u32 = 1 << 24;

    /// Parse notation such as `KAT`, `-T` or `STPH*`, or `+` alone (the join key, which locks in a
    /// run's text so far). Numbers and a bare `#` are not supported.
    pub fn parse(text: &str) -> Option<Self> {
        let (mut bits, text) = match (text.strip_prefix('+'), text.strip_prefix('~')) {
            (Some(rest), _) => (Self::JOIN, rest),
            (None, Some(rest)) => (Self::SPELL, rest),
            (None, None) => (0, text),
        };
        let mut cursor = 0usize;
        for c in text.bytes() {
            if c == b'-' {
                cursor = cursor.max(RIGHT as usize);
                continue;
            }
            let at = KEYS[cursor..].iter().position(|&k| k == c)? + cursor;
            bits |= 1 << at;
            cursor = at + 1;
        }
        (bits & !(Self::JOIN | Self::SPELL) != 0 || bits == Self::JOIN).then_some(Self(bits))
    }

    /// The right pinky's four keys are a 2x2 block (`-T`/`-S` in one column, `-D`/`-Z` in the
    /// next). A pinky can press one of them, two in a row or a column (`-TS`, `-DZ`, `-TD`,
    /// `-SZ`), or all four flat; it cannot press a diagonal pair (`-TZ`, `-SD`) or exactly three
    /// (`-TDZ`). A stroke that needs that is not a stroke that can be typed.
    pub const fn typable(self) -> bool {
        const T: u32 = 1 << 19;
        const S: u32 = 1 << 20;
        const D: u32 = 1 << 21;
        const Z: u32 = 1 << 22;
        let pinky = self.0 & (T | S | D | Z);
        !(pinky == T | Z || pinky == S | D || (pinky.count_ones() == 3))
    }

    /// The typable strokes one or two pinky keys away from this untypable one, each key swapped
    /// for its voiced or unvoiced partner (`-T`/`-D`, `-S`/`-Z`): `-TZ` is `-TS` or `-DZ`, `-SD`
    /// is `-ZD` or `-ST`. Empty when the stroke is already typable. These keep it one stroke.
    pub fn typable_swaps(self) -> Vec<Self> {
        const T: u32 = 1 << 19;
        const S: u32 = 1 << 20;
        const D: u32 = 1 << 21;
        const Z: u32 = 1 << 22;
        if self.typable() {
            return Vec::new();
        }
        let mut found = Vec::new();
        for mask in 1u32..16 {
            let mut bits = self.0;
            let mut ok = true;
            for (n, (from, to)) in [(T, D), (D, T), (S, Z), (Z, S)].into_iter().enumerate() {
                if mask & (1 << n) == 0 {
                    continue;
                }
                if bits & from == 0 || bits & to != 0 {
                    ok = false;
                    break;
                }
                bits = bits & !from | to;
            }
            let stroke = Self(bits);
            if ok && stroke.typable() && !found.contains(&stroke) {
                found.push(stroke);
            }
        }
        // Fewest changes first, so the nearest repair is offered first
        found.sort_by_key(|s| (s.0 ^ self.0).count_ones());
        found
    }

    /// An untypable stroke written as two that can be typed: the first keeps the pinky keys in
    /// order (`-T`, `-S`, `-D`, `-Z`) for as long as they are typable together, the second is a
    /// continuation (join key) with the rest. `-SD` becomes `-S` then `+-D`; `-TDZ` becomes `-TD`
    /// then `+-Z`. `None` when the stroke is already typable.
    pub fn split_untypable(self) -> Option<(Self, Self)> {
        if self.typable() {
            return None;
        }
        let mut first = self.0 & !(0b1111 << 19);
        let mut rest = 0;
        for bit in 19..=22 {
            let key = self.0 & (1 << bit);
            if key == 0 {
                continue;
            }
            if rest == 0 && Self(first | key).typable() {
                first |= key;
            } else {
                rest |= key;
            }
        }
        Some((Self(first), Self(rest | Self::JOIN)))
    }

    /// The lowest key down, as an index in steno order.
    pub const fn lowest(self) -> Option<u32> {
        if self.0 == 0 {
            None
        } else {
            Some(self.0.trailing_zeros())
        }
    }

    /// The highest key down.
    pub const fn highest(self) -> Option<u32> {
        if self.0 == 0 {
            None
        } else {
            Some(self.0.ilog2())
        }
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The keys alone, without the join or spelling-mode marker.
    pub const fn keys(self) -> Self {
        Self(self.0 & !(Self::JOIN | Self::SPELL))
    }

    /// Whether this is the star-ladder marker: a stroke of only the star (`*`, `+*`), struck after
    /// a word to tell it from another that sounds the same.
    pub const fn is_marker(self) -> bool {
        self.keys().0 == Self::STAR
    }

    /// `next` folded onto this stroke, when `next` is right-hand keys only, none of them already
    /// down, all of them after this stroke's own keys in steno order: `SU` + `-BL` is `SUBL`, the
    /// way a reduced syllable joins the stroke before it. The result keeps this stroke's markers.
    pub const fn fold(self, next: Self) -> Option<Self> {
        let (own, added) = (self.keys(), next.keys());
        if added.0 == 0 || added.0 & !Self::RIGHT != 0 || own.0 & added.0 != 0 {
            return None;
        }
        match (own.highest(), added.lowest()) {
            (Some(high), Some(low)) if high >= low => None,
            _ => Some(Self(self.0 | added.0)),
        }
    }
}

impl fmt::Display for Stroke {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let has_vowel = self.0 & (mask(VOWEL, 12)) != 0;
        let mut hyphen = !has_vowel;
        if self.0 & Self::JOIN != 0 {
            write!(f, "+")?;
        } else if self.0 & Self::SPELL != 0 {
            write!(f, "~")?;
        }
        for (index, &key) in (0u32..).zip(KEYS.iter()) {
            if self.0 & (1 << index) == 0 {
                continue;
            }
            if hyphen && index >= RIGHT {
                write!(f, "-")?;
                hyphen = false;
            }
            write!(f, "{}", key as char)?;
        }
        Ok(())
    }
}

/// Parse `KAT/TKOG` into its strokes.
pub fn parse_outline(text: &str) -> Option<Vec<Stroke>> {
    text.split('/').map(Stroke::parse).collect()
}

pub fn render_outline(strokes: &[Stroke]) -> String {
    strokes
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notation_round_trips() {
        for text in [
            "KAT",
            "-T",
            "STPH",
            "TKPWHR*EUFPLT",
            "AOEU",
            "*S",
            "SKWR-PBLG",
            "U",
            "+-G",
            "+AOE",
            "~S",
            "~TKPW",
        ] {
            assert_eq!(Stroke::parse(text).unwrap().to_string(), text, "{text}");
        }
    }

    #[test]
    fn a_hyphen_puts_keys_on_the_right() {
        assert_eq!(Stroke::parse("T"), Stroke::parse("T"));
        assert_ne!(Stroke::parse("T"), Stroke::parse("-T"));
        assert_eq!(Stroke::parse("-T").unwrap().to_string(), "-T");
    }

    #[test]
    fn keys_out_of_order_do_not_parse() {
        assert_eq!(Stroke::parse("TS-K"), None);
    }

    #[test]
    fn the_spelling_marker_is_not_the_join_key() {
        let spell = Stroke::parse("~S").unwrap();
        let join = Stroke::parse("+S").unwrap();
        assert_ne!(spell, join);
        assert_eq!(spell.0 & Stroke::SPELL, Stroke::SPELL);
        assert_eq!(spell.0 & Stroke::JOIN, 0);
        // A bare marker with no keys down means nothing; the join key alone is lock, a stroke
        assert_eq!(Stroke::parse("~"), None);
        assert_eq!(Stroke::parse("+"), Some(Stroke(Stroke::JOIN)));
    }
}

#[cfg(test)]
mod typable_tests {
    use super::Stroke;

    fn typable(text: &str) -> bool {
        Stroke::parse(text).unwrap().typable()
    }

    #[test]
    fn the_right_pinky_presses_one_two_in_a_line_or_all_four() {
        for ok in ["-T", "-S", "-D", "-Z", "-TS", "-DZ", "-TD", "-SZ", "-TSDZ", "KAT", "KAEUPBS", "-PBGS", "-FRPBLGTSDZ"] {
            assert!(typable(ok), "{ok}");
        }
    }

    #[test]
    fn a_diagonal_pair_or_three_pinky_keys_cannot_be_typed() {
        for bad in ["-TZ", "-SD", "-TDZ", "-TSD", "-TSZ", "-SDZ", "KAEUPBSD", "*EPTZ", "-PBGSD"] {
            assert!(!typable(bad), "{bad}");
        }
    }

    #[test]
    fn the_join_key_and_other_hands_do_not_matter() {
        assert!(typable("+-TS"));
        assert!(!typable("+-TZ"));
        assert!(typable("STKPWHRAO*EUFRPBLG"));
    }
}

#[cfg(test)]
mod split_tests {
    use super::Stroke;

    fn split(text: &str) -> (String, String) {
        let (a, b) = Stroke::parse(text).unwrap().split_untypable().unwrap();
        assert!(a.typable() && b.typable());
        (a.to_string(), b.to_string())
    }

    #[test]
    fn an_untypable_pinky_chord_is_split_in_order_onto_a_continuation() {
        assert_eq!(split("KAEUPBSD"), ("KAEUPBS".to_owned(), "+-D".to_owned()));
        assert_eq!(split("-TZ"), ("-T".to_owned(), "+-Z".to_owned()));
        assert_eq!(split("-TDZ"), ("-TD".to_owned(), "+-Z".to_owned()));
        assert_eq!(split("-SDZ"), ("-S".to_owned(), "+-DZ".to_owned()));
        assert!(Stroke::parse("-TS").unwrap().split_untypable().is_none());
    }
}

#[cfg(test)]
mod swap_tests {
    use super::Stroke;

    fn swaps(text: &str) -> Vec<String> {
        Stroke::parse(text).unwrap().typable_swaps().iter().map(ToString::to_string).collect()
    }

    #[test]
    fn a_pinky_key_swaps_for_its_voiced_partner_to_stay_one_stroke() {
        assert!(swaps("KAEUPBTZ").contains(&"KAEUPBTS".to_owned()));
        assert!(swaps("KAEUPBTZ").contains(&"KAEUPBDZ".to_owned()));
        assert!(swaps("PULSD").contains(&"PULDZ".to_owned()));
        assert!(swaps("PULSD").contains(&"PULTS".to_owned()));
        assert!(swaps("KAT").is_empty());
        assert!(swaps("KAEUPBTZ").iter().all(|s| Stroke::parse(s).unwrap().typable()));
    }
}

//! Word families: which words are another word plus a regular ending (`build` -> `building`,
//! `call` -> `called`, `happy` -> `happiness`), found by spelling and confirmed by sound.
//!
//! A [`Link`] ties one word to its stem and to the standalone suffix stroke from
//! `rules/suffixes.toml` that writes its ending. The generator uses links to give every family
//! member a backup outline built from the stem's own outline plus that suffix stroke (see the
//! backup pass in `generate`), and `family`/`build` use them to report how consistently each
//! family's members are stroked. See `docs/syllables.md`.

use crate::data::Pronunciations;
use crate::stroke::{Stroke, parse_outline};
use std::collections::{BTreeMap, HashMap};

const SUFFIXES: &str = include_str!("../rules/suffixes.toml");
const FAMILIES: &str = include_str!("../rules/families.toml");

#[derive(serde::Deserialize)]
struct Families {
    not_family: Vec<String>,
    /// Irregular past tenses and participles, each to its stem (`built` -> `build`).
    #[serde(default)]
    past: BTreeMap<String, String>,
    /// Irregular plurals, each to its stem (`men` -> `man`).
    #[serde(default)]
    plural: BTreeMap<String, String>,
}

fn families() -> Families {
    toml::from_str(FAMILIES).expect("rules/families.toml parses")
}

/// Words that pass the spelling and sound checks by coincidence (`better` is not `bet` + "-er"),
/// from `rules/families.toml`.
fn not_family() -> std::collections::HashSet<String> {
    families().not_family.into_iter().collect()
}

/// The endings a family link may use, as written in `rules/suffixes.toml`. Inflections plus the
/// common derivational endings whose spelling attaches to a whole word; endings that replace part
/// of the stem (`-tion`, `-ic`, `-ity`) are left to the sound-based backup directions.
const FAMILY_ENDINGS: &[&str] = &[
    "ness", "ment", "able", "less", "ship", "hood", "ing", "est", "ful", "ed", "er", "ly", "s",
];

/// One word's tie to the word it is built from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub stem: String,
    /// The ending as written in `rules/suffixes.toml` ("s", "ed", "ing", ...).
    pub suffix: &'static str,
    /// The standalone continuation stroke(s) that write the ending, join key included.
    pub strokes: Vec<Stroke>,
}

fn suffix_strokes() -> HashMap<&'static str, Vec<Vec<Stroke>>> {
    let table: BTreeMap<String, String> = toml::from_str(SUFFIXES).expect("rules/suffixes.toml parses");
    let mut by_text: HashMap<&'static str, Vec<Vec<Stroke>>> = HashMap::new();
    for (outline, text) in table {
        if let Some(&ending) = FAMILY_ENDINGS.iter().find(|e| **e == text)
            && let Some(strokes) = parse_outline(&outline)
        {
            by_text.entry(ending).or_default().push(strokes);
        }
    }
    by_text
}

fn bare(sounds: &[String]) -> Vec<&str> {
    sounds.iter().map(|s| s.trim_end_matches(|c: char| c.is_ascii_digit())).collect()
}

/// Spellings `word` could be `stem + ending` from, most literal first: the plain stem, a dropped
/// final "e" (`making` from `make`), an undoubled final consonant (`running` from `run`), "i" back
/// to "y" (`happiness` from `happy`), and the "-es"/"-ies" plurals.
fn stem_spellings(word: &str, ending: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(base) = word.strip_suffix(ending) {
        out.push(base.to_owned());
        out.push(format!("{base}e"));
        let b = base.as_bytes();
        if b.len() >= 3 && b[b.len() - 1] == b[b.len() - 2] && !b"aeiou".contains(&b[b.len() - 1]) {
            out.push(base[..base.len() - 1].to_owned());
        }
        if let Some(head) = base.strip_suffix('i') {
            out.push(format!("{head}y"));
        }
    }
    if ending == "s" {
        if let Some(head) = word.strip_suffix("ies") {
            out.push(format!("{head}y"));
        }
        if let Some(head) = word.strip_suffix("es") {
            out.push(head.to_owned());
        }
    }
    out.retain(|s| s.len() >= 2 && s != word);
    out
}

/// The sounds each ending may add after its stem, stress ignored.
fn ending_sounds(ending: &str) -> &'static [&'static [&'static str]] {
    match ending {
        "s" => &[&["S"], &["Z"], &["IH", "Z"], &["AH", "Z"]],
        "ed" => &[&["D"], &["T"], &["IH", "D"], &["AH", "D"]],
        "ing" => &[&["IH", "NG"]],
        "er" => &[&["ER"]],
        "est" => &[&["AH", "S", "T"], &["IH", "S", "T"]],
        "ly" => &[&["L", "IY"]],
        "ness" => &[&["N", "AH", "S"], &["N", "IH", "S"]],
        "ment" => &[&["M", "AH", "N", "T"], &["M", "EH", "N", "T"], &["M", "IH", "N", "T"]],
        "able" => &[&["AH", "B", "AH", "L"]],
        "ful" => &[&["F", "AH", "L"], &["F", "UH", "L"]],
        "less" => &[&["L", "AH", "S"], &["L", "IH", "S"], &["L", "EH", "S"]],
        "ship" => &[&["SH", "IH", "P"]],
        "hood" => &[&["HH", "UH", "D"]],
        _ => &[],
    }
}

/// Whether some pronunciation of `word` is some pronunciation of `stem` followed by exactly the
/// sounds `ending` adds, stress ignored - the check that keeps spelling coincidences out (`used`
/// is not `us` + "ed", `need` is not `ne` + "ed"). Returns the word's final sound.
fn sound_split<'a>(pron: &'a Pronunciations, stem: &str, word: &str, ending: &str) -> Option<&'a str> {
    let stems = pron.get(stem)?;
    let words = pron.get(word)?;
    for w in words {
        let w = bare(w);
        for s in stems {
            let s = bare(s);
            if w.len() > s.len() && w.starts_with(&s) && ending_sounds(ending).iter().any(|t| w[s.len()..] == **t) {
                return w.last().copied();
            }
        }
    }
    None
}

/// A stem said this many times less often than its word is a spelling coincidence, not its stem:
/// `need` is not `ne` + "-ed" (1,040,131 uses against 4,128).
const STEM_RARITY_LIMIT: u64 = 50;

/// Every word in `vocabulary` that is another vocabulary word plus one of [`FAMILY_ENDINGS`],
/// confirmed by sound, mapped to its [`Link`]. Longer endings are tried first, so `darkness`
/// links to `dark` + "ness", never to a non-word `darknes` + "s". `count` is how often each word
/// is said (`Setup::count`); a missing word counts as never said.
pub fn links<'a>(
    pron: &Pronunciations,
    count: &HashMap<String, u64>,
    vocabulary: impl IntoIterator<Item = &'a str>,
) -> HashMap<String, Link> {
    let vocabulary: Vec<&str> = vocabulary.into_iter().collect();
    let known: std::collections::HashSet<&str> = vocabulary.iter().copied().collect();
    let strokes = suffix_strokes();
    let not_family = not_family();
    let mut out = HashMap::new();
    for &word in &vocabulary {
        'endings: for &ending in FAMILY_ENDINGS {
            let Some(options) = strokes.get(ending) else { continue };
            if not_family.contains(word) {
                break;
            }
            let said = |w: &str| count.get(w).copied().unwrap_or(0);
            // Every spelling that passes, the most frequent kept: `robber` is `rob` + "-er", not
            // the name `robb`; `lived` is `live` + "-d", not `liv`.
            let best = stem_spellings(word, ending)
                .into_iter()
                .filter(|stem| known.contains(stem.as_str()))
                .filter(|stem| said(stem).saturating_mul(STEM_RARITY_LIMIT) >= said(word))
                .filter_map(|stem| sound_split(pron, &stem, word, ending).map(|last| (stem, last)))
                .max_by(|(a, _), (b, _)| said(a).cmp(&said(b)).then_with(|| b.cmp(a)));
            let Some((stem, last_sound)) = best else { continue };
            // "s" has two strokes: `+-S` for a word that really ends in "s", `+-Z` otherwise.
            let chosen = if ending == "s" {
                let want = if last_sound == "S" { "S" } else { "Z" };
                options.iter().find(|o| crate::stroke::render_outline(o).ends_with(want))
            } else {
                options.first()
            };
            let Some(chosen) = chosen else { continue };
            out.insert(word.to_owned(), Link { stem, suffix: ending, strokes: chosen.clone() });
            break 'endings;
        }
    }
    // Irregular forms from `rules/families.toml`, by rule rather than by spelling or sound: a past
    // form is its stem plus -ed's stroke, a plural its stem plus -s's (`+-S` after an "s" sound).
    let families = families();
    for (table, ending) in [(&families.past, "ed"), (&families.plural, "s")] {
        for (word, stem) in table {
            if !known.contains(word.as_str()) || !known.contains(stem.as_str()) {
                continue;
            }
            let Some(options) = strokes.get(ending) else { continue };
            let ends_in_s = pron.get(word.as_str()).and_then(|p| p.first()).and_then(|p| p.last()).is_some_and(|s| s == "S");
            let want = if ending == "s" && !ends_in_s { "Z" } else if ending == "s" { "S" } else { "D" };
            let Some(chosen) = options.iter().find(|o| crate::stroke::render_outline(o).ends_with(want)) else { continue };
            out.insert(word.clone(), Link { stem: stem.clone(), suffix: ending, strokes: chosen.clone() });
        }
    }
    out
}

/// The prefixes a word can be a family member through, longest first, each with how it is
/// written in front of the stem: a fixed stroke (`re-` is `RE`, `pre-` is `PRE`, as the rules
/// already write those syllables), or the prefix word's own settled outline (`un`, `dis`, ...).
pub const PREFIXES: &[(&str, PrefixStroke)] = &[
    ("under", PrefixStroke::Word("under")),
    ("over", PrefixStroke::Word("over")),
    ("pre", PrefixStroke::Fixed("PRE")),
    ("dis", PrefixStroke::Word("dis")),
    ("mis", PrefixStroke::Word("mis")),
    ("re", PrefixStroke::Fixed("RE")),
    ("un", PrefixStroke::Word("un")),
];

/// How a prefix is written in front of its stem: see [`PREFIXES`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefixStroke {
    Fixed(&'static str),
    Word(&'static str),
}

/// One word's tie to the word it prefixes (`rebuild` -> `re` + `build`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefixLink {
    pub prefix: &'static str,
    pub stroke: PrefixStroke,
    pub stem: String,
}

/// Every word in `vocabulary` that is one of [`PREFIXES`] plus another vocabulary word, where the
/// word's pronunciation ends in the stem's (stress ignored), the stem is at least three letters,
/// and the stem is said at least 1/50th as often as the word: `rebuild` is `re` + `build`, while
/// `reason` (no `ason`) and `under` itself are not prefixed. Words in `not_family` are skipped.
pub fn prefix_links<'a>(
    pron: &Pronunciations,
    count: &HashMap<String, u64>,
    vocabulary: impl IntoIterator<Item = &'a str>,
) -> HashMap<String, PrefixLink> {
    let vocabulary: Vec<&str> = vocabulary.into_iter().collect();
    let known: std::collections::HashSet<&str> = vocabulary.iter().copied().collect();
    let not_family = not_family();
    let said = |w: &str| count.get(w).copied().unwrap_or(0);
    let mut out = HashMap::new();
    for &word in &vocabulary {
        if not_family.contains(word) {
            continue;
        }
        for &(prefix, stroke) in PREFIXES {
            let Some(stem) = word.strip_prefix(prefix) else { continue };
            if stem.len() < 3 || !known.contains(stem) || said(stem).saturating_mul(STEM_RARITY_LIMIT) < said(word) {
                continue;
            }
            let (Some(words), Some(stems)) = (pron.get(word), pron.get(stem)) else { continue };
            let ends_with_stem = words.iter().any(|w| {
                let w = bare(w);
                stems.iter().any(|s| {
                    let s = bare(s);
                    w.len() > s.len() && w.ends_with(&s)
                })
            });
            if ends_with_stem {
                out.insert(word.to_owned(), PrefixLink { prefix, stroke, stem: stem.to_owned() });
                break;
            }
        }
    }
    out
}

/// A prefixed word's family shape: the prefix's stroke(s), then the stem's own outline, every
/// stem stroke continuing the word (`re` + `build` `PWHR` -> `RE/+PWHR`). `None` when a word
/// prefix has no outline of its own in `outlines`.
pub fn prefixed(link: &PrefixLink, stem: &[Stroke], outlines: &HashMap<&str, &[Stroke]>) -> Option<Vec<Stroke>> {
    let mut out = match link.stroke {
        PrefixStroke::Fixed(text) => parse_outline(text)?,
        PrefixStroke::Word(word) => outlines.get(word)?.to_vec(),
    };
    out.extend(stem.iter().map(|s| Stroke(s.0 | Stroke::JOIN)));
    Some(out)
}

/// The word a family is named after: follow links from `word` back to a word with no stem.
pub fn root<'a>(links: &'a HashMap<String, Link>, word: &'a str) -> &'a str {
    let mut current = word;
    for _ in 0..8 {
        match links.get(current) {
            Some(link) => current = &link.stem,
            None => break,
        }
    }
    current
}

/// The word `word`'s family is rooted at, and every ending's keys along the way OR'd together -
/// `None` when any ending on the way is more than one stroke or touches a key outside the right
/// bank (`-ship`, `-hood`), or two endings share a key, since those can't fold into one stroke.
pub fn chain<'a>(links: &'a HashMap<String, Link>, word: &'a str) -> Option<(&'a str, u32)> {
    let right = parse_outline("-FRPBLGTSDZ").expect("the right bank parses")[0].0;
    let mut keys = 0u32;
    let mut current = word;
    for _ in 0..8 {
        let Some(link) = links.get(current) else { break };
        let [stroke] = link.strokes[..] else { return None };
        let bits = stroke.0 & !Stroke::JOIN;
        if bits & !right != 0 || bits & keys != 0 {
            return None;
        }
        keys |= bits;
        current = &link.stem;
    }
    (current != word).then_some((current, keys))
}

/// Every way to write a family member as its root's outline plus its
/// endings' keys, on the root's last stroke - the family rules, in order:
///
/// 1. the endings' keys added (`build` `PWHR` -> `builds` `PWHR-Z`, `building` `PWHR-G`);
/// 2. when the root already holds an ending's key, the star added too (`need` `TPHAED` ->
///    `needed` `TPHA*ED`, `think` `-PBG` -> `thinking` `*PBG`);
/// 3. "-ing" on a root ending in the K chord `-BG` swaps it for a bare `-G`, Plover's own
///    convention (`talk` `TAUBG` -> `talking` `TAUG`, `look` `HRAOBG` -> `looking` `HRAOG`).
pub fn shapes(root: &[Stroke], keys: u32) -> Vec<Vec<Stroke>> {
    let Some(&last) = root.last() else { return Vec::new() };
    let b = parse_outline("-B").expect("-B parses")[0].0;
    let g = parse_outline("-G").expect("-G parses")[0].0;
    let with_last = |stroke: u32| {
        let mut outline = root.to_vec();
        if let Some(l) = outline.last_mut() {
            l.0 = stroke;
        }
        outline
    };
    let mut out = Vec::new();
    if last.0 & keys == 0 {
        out.push(with_last(last.0 | keys));
    } else if last.0 & Stroke::STAR == 0 {
        out.push(with_last(last.0 | keys | Stroke::STAR));
    }
    if keys & g != 0 && last.0 & (b | g) == b | g {
        out.push(with_last((last.0 & !b) | keys));
    }
    out
}

/// Whether `member` follows its family: its outline is one of [`shapes`] of its root's, or it
/// keeps every key of its direct stem's outline ([`contains_stem`]).
pub fn follows_family(links: &HashMap<String, Link>, outlines: &HashMap<&str, &[Stroke]>, member: &str) -> bool {
    let Some(&own) = outlines.get(member) else { return false };
    if let Some((root, keys)) = chain(links, member)
        && let Some(&root_outline) = outlines.get(root)
        && shapes(root_outline, keys).iter().any(|s| s.as_slice() == own)
    {
        return true;
    }
    links
        .get(member)
        .and_then(|link| outlines.get(link.stem.as_str()))
        .is_some_and(|&stem| contains_stem(stem, own))
}

/// Whether `member`'s outline visibly starts from `stem`'s: stroke by stroke, every key of each
/// stem stroke (star and join aside) is also down in the member's stroke at the same position.
/// `start`/`STARTD`, `day`/`TKAEUZ`, and `starter`/`starters` (`SART/+-R`, `SART/+-RZ`) count;
/// `like`/`-FLD` does not.
pub fn contains_stem(stem: &[Stroke], member: &[Stroke]) -> bool {
    let mask = !(Stroke::STAR | Stroke::JOIN);
    !stem.is_empty()
        && member.len() >= stem.len()
        && stem[0].0 & mask != 0
        && stem.iter().zip(member).all(|(s, m)| s.0 & mask & m.0 == s.0 & mask)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pron(entries: &[(&str, &str)]) -> Pronunciations {
        entries
            .iter()
            .map(|(w, s)| ((*w).to_owned(), vec![s.split(' ').map(str::to_owned).collect()]))
            .collect()
    }

    #[test]
    fn spelling_changes_and_sound_check() {
        let p = pron(&[
            ("call", "K AO1 L"),
            ("called", "K AO1 L D"),
            ("make", "M EY1 K"),
            ("making", "M EY1 K IH0 NG"),
            ("run", "R AH1 N"),
            ("running", "R AH1 N IH0 NG"),
            ("happy", "HH AE1 P IY0"),
            ("happiness", "HH AE1 P IY0 N AH0 S"),
            ("us", "AH1 S"),
            ("use", "Y UW1 Z"),
            ("used", "Y UW1 Z D"),
            ("box", "B AA1 K S"),
            ("boxes", "B AA1 K S IH0 Z"),
            ("cat", "K AE1 T"),
            ("cats", "K AE1 T S"),
            ("dark", "D AA1 R K"),
            ("ne", "N IY1"),
            ("rob", "R AA1 B"),
            ("robb", "R AA1 B"),
            ("robber", "R AA1 B ER0"),
            ("bet", "B EH1 T"),
            ("better", "B EH1 T ER0"),
            ("need", "N IY1 D"),
            ("darkness", "D AA1 R K N AH0 S"),
        ]);
        let words: Vec<&str> = p.keys().map(String::as_str).collect();
        let mut count: HashMap<String, u64> = p.keys().map(|w| (w.clone(), 100)).collect();
        count.insert("ne".to_owned(), 1);
        count.insert("need".to_owned(), 1_000);
        count.insert("robb".to_owned(), 10);
        let links = links(&p, &count, words);
        let stem = |w: &str| links.get(w).map(|l| (l.stem.as_str(), l.suffix));
        assert_eq!(stem("called"), Some(("call", "ed")));
        assert_eq!(stem("making"), Some(("make", "ing")));
        assert_eq!(stem("running"), Some(("run", "ing")));
        assert_eq!(stem("happiness"), Some(("happy", "ness")));
        assert_eq!(stem("used"), Some(("use", "ed")), "not us + ed: the sound check rejects it");
        assert_eq!(stem("boxes"), Some(("box", "s")));
        assert_eq!(stem("darkness"), Some(("dark", "ness")));
        assert_eq!(stem("call"), None);
        assert_eq!(stem("need"), None, "ne is far too rare to be need's stem");
        assert_eq!(stem("robber"), Some(("rob", "er")), "the commoner rob, not the name robb");
        assert_eq!(stem("better"), None, "listed in rules/families.toml");
        let s = |w: &str| crate::stroke::render_outline(&links[w].strokes);
        assert_eq!(s("cats"), "+-S", "really ends in an s sound");
        assert_eq!(s("boxes"), "+-Z", "ends in a z sound");
    }

    #[test]
    fn contains_stem_checks_keys_not_text() {
        let o = |t: &str| parse_outline(t).unwrap();
        assert!(contains_stem(&o("SART"), &o("STARTD")));
        assert!(contains_stem(&o("AEU"), &o("TKAEUZ")));
        assert!(!contains_stem(&o("HRAOEU"), &o("-FLD")));
        assert!(contains_stem(&o("KAUL"), &o("KAUL/+-D")));
        assert!(contains_stem(&o("SART/+-R"), &o("SART/+-RZ")));
        assert!(!contains_stem(&o("SART/+-R"), &o("SART")));
        let text = |v: Vec<Vec<Stroke>>| v.iter().map(|s| crate::stroke::render_outline(s)).collect::<Vec<_>>();
        let g = o("-G")[0].0;
        let d = o("-D")[0].0;
        assert_eq!(text(shapes(&o("PWHR"), g)), ["PWHR-G"]);
        assert_eq!(text(shapes(&o("TPHAED"), d)), ["TPHA*ED"]);
        assert_eq!(text(shapes(&o("TAUBG"), g)), ["TA*UBG", "TAUG"]);
        let link = PrefixLink { prefix: "re", stroke: PrefixStroke::Fixed("RE"), stem: "build".to_owned() };
        assert_eq!(text(vec![prefixed(&link, &o("PWHR"), &HashMap::new()).unwrap()]), ["RE/+PWHR"]);
    }
}

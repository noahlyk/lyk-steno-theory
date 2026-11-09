//! The name lane: `#/<outline>` types a name, capitalized.
//!
//! lyk keeps one word for a sound and capitalizes with `KPA`; the lane is the second, separate way
//! to write a name: the stroke `#` alone, then the name struck as the rules strike the lowercase
//! word. This module decides which names there are and what each one is struck as; the solver's
//! names trie (`rules::solve`) reads the ones the dictionary files here do not list.
//!
//! The dictionary is generated, never borrowed, wherever the rules can strike the name:
//!
//! 1. **lyk's own outlines** for the lowercase word, when lyk has the word (`#/` + each outline of
//!    its words, short, alternates and extras entries);
//! 2. **the rules' reading** of every pronunciation of the name (plain and spelling variant);
//! 3. **its syllables**, one per stroke, and **a few lattice paths** (the cheapest ways to write its
//!    sounds as 2 to 4 strokes) for every pronunciation;
//! 4. for a name with **no pronunciation**, the cheapest ways to write its **spelling** as joined
//!    strokes ([`crate::rules::Rules::orthographic_paths`], the same chunks the spelled backups use).
//!
//! Only a name none of those can strike (more than one word, digits, a dot) is **borrowed**: the
//! outline Lapwing gives it after `#/`, in a file of its own so it is never mistaken for lyk's.
//! An outline already held by an earlier source (or by a commoner name) is never taken again.

use crate::analysis::Setup;
use crate::generate::{Entry, Origin, split_untypable, typable};
use crate::stroke::{Stroke, parse_outline, render_outline};
use anyhow::{Context, Result};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::path::Path;

/// The file of the name lane's lyk-rule entries. `03-` puts it after the dictionary files and
/// before the solver (`05-`), so the dictionary answers where the solver would guess.
pub const FILE: &str = "03-lyk-names.json";
/// The file of the entries borrowed from Lapwing, for the names the rules cannot strike.
pub const BORROWED_FILE: &str = "03-lyk-names-borrowed.json";

/// Lattice paths a name is given per pronunciation, for two strokes, three, four, five and six.
pub const LATTICE_QUOTAS: [usize; 5] = [3, 2, 1, 0, 0];
/// Spelled paths a name with no pronunciation is given, by stroke count.
pub const SPELLING_QUOTAS: [usize; 5] = [4, 4, 2, 1, 0];

/// One name: its lowercase key, how it is written, how common it is and Lapwing's outlines for it.
#[derive(Debug, Clone)]
pub struct Name {
    pub key: String,
    pub text: String,
    /// Position by how often the word is said (0 the commonest); names the list lacks come last
    pub rank: usize,
    /// Lapwing's outlines for this name after its `#/`
    pub lapwing: Vec<String>,
}

/// Lapwing's proper nouns: `(outline after "#/" if it has one, text)`. The prefixed file has the
/// outlines; the file with `#` held in the first stroke only adds texts.
pub fn load_proper_nouns(data: &Path) -> Result<Vec<(Option<String>, String)>> {
    let dir = data.join("dictionaries");
    let mut out = Vec::new();
    let prefixed = crate::data::load_dictionary(&dir.join("lapwing-prefixed-proper-nouns.json")).context("Lapwing's prefixed proper nouns")?;
    for (outline, text) in prefixed {
        if let Some(rest) = outline.strip_prefix("#/") {
            out.push((Some(rest.to_owned()), text));
        }
    }
    let held = crate::data::load_dictionary(&dir.join("lapwing-proper-nouns.json")).context("Lapwing's proper nouns")?;
    out.extend(held.into_values().map(|text| (None, text)));
    Ok(out)
}

fn capitalize(key: &str) -> String {
    let mut letters = key.chars();
    letters.next().map(|c| c.to_uppercase().chain(letters).collect()).unwrap_or_default()
}

/// Whether Lapwing's text is a name to write capitalized: it has a letter, it is not a plain
/// lowercase word (the file holds `college`) and not an acronym (`NBC`, `HTML`).
fn is_name_text(text: &str) -> bool {
    let letters = text.chars().filter(|c| c.is_alphabetic()).count();
    letters > 0 && text.chars().any(char::is_uppercase) && (letters == 1 || text.chars().any(char::is_lowercase))
}

/// Every name there is, commonest first: Lapwing's proper nouns, and the words Plover and Lapwing
/// write only capitalized.
pub fn collect(setup: &Setup) -> Vec<Name> {
    let mut by_key: BTreeMap<String, Name> = BTreeMap::new();
    let rank_of = |key: &str| setup.rank.get(key).copied().unwrap_or(usize::MAX);
    for (outline, text) in &setup.proper_nouns {
        if !is_name_text(text) {
            continue;
        }
        let key = text.to_lowercase();
        let name = by_key.entry(key.clone()).or_insert_with(|| Name { rank: rank_of(&key), key, text: text.clone(), lapwing: Vec::new() });
        // The usual capital letter wins over `MCDONALD` and the like, else the first seen
        if *text == capitalize(&name.key) {
            name.text.clone_from(text);
        }
        if let Some(outline) = outline
            && !name.lapwing.contains(outline)
        {
            name.lapwing.push(outline.clone());
        }
    }
    for key in setup.gold.names() {
        by_key.entry(key.clone()).or_insert_with(|| Name { rank: rank_of(key), key: key.clone(), text: capitalize(key), lapwing: Vec::new() });
    }
    let mut names: Vec<Name> = by_key.into_values().collect();
    names.sort_by(|a, b| (a.rank, &a.key).cmp(&(b.rank, &b.key)));
    names
}

/// Whether the rules can strike the name from its spelling: lowercase letters only.
pub fn spellable(key: &str) -> bool {
    !key.is_empty() && key.bytes().all(|c| c.is_ascii_lowercase())
}

/// Where a name's entries come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Source {
    /// An outline lyk already has for the lowercase word
    Own,
    /// The rules' reading of a pronunciation
    Rules,
    /// One syllable a stroke
    Syllables,
    /// A cheapest path over the sounds
    Lattice,
    /// A path over the spelling, for a name with no pronunciation
    Spelled,
    /// Lapwing's
    Borrowed,
}

/// The lane's entries: `#/OUTLINE` to the name, by source.
#[derive(Default)]
pub struct Lane {
    /// lyk's: every source but the borrowed one
    pub own: BTreeMap<String, String>,
    pub borrowed: BTreeMap<String, String>,
    /// How many entries each source added
    pub by_source: BTreeMap<Source, usize>,
    /// Names with at least one entry from a source
    pub names_by_source: BTreeMap<Source, usize>,
    /// Names whose outlines are only Lapwing's, and names with no way in at all
    pub borrowed_only: Vec<String>,
    pub nothing: Vec<String>,
    /// Outlines a commoner name or an earlier source already held
    pub skipped: usize,
}

fn lane_outline(outline: &[Stroke]) -> String {
    format!("#/{}", render_outline(outline))
}

/// The lane for `names`: first every name's own outlines, then the rules' readings, and so on down
/// the sources, commonest name first within each, so a clash goes to the earlier source and then
/// the commoner name. `entries` is the dictionary lyk generated.
pub fn generate(setup: &Setup, entries: &[Entry], names: &[Name]) -> Result<Lane> {
    let mut own_outlines: HashMap<&str, Vec<&Entry>> = HashMap::new();
    for e in entries {
        if !matches!(e.origin, Origin::Backup | Origin::Phrase | Origin::Suffix) {
            own_outlines.entry(e.word.as_str()).or_default().push(e);
        }
    }
    let reserved = crate::generate::reserved_outlines()?;
    let mut lane = Lane::default();
    let mut held: HashMap<String, usize> = HashMap::new();
    let mut gave: Vec<HashSet<Source>> = vec![HashSet::new(); names.len()];
    let rules = &setup.rules;
    let finish = |mut outline: Vec<Stroke>| -> Option<Vec<Stroke>> {
        rules.joined(&mut outline);
        if typable(&outline) { Some(outline) } else { split_untypable(&outline) }
    };
    let mut claim = |lane: &mut Lane, at: usize, source: Source, outline: Vec<Stroke>| {
        let key = lane_outline(&outline);
        if held.contains_key(&key) || reserved.contains_key(&key) {
            lane.skipped += 1;
            return;
        }
        held.insert(key.clone(), at);
        lane.own.insert(key, names[at].text.clone());
        *lane.by_source.entry(source).or_default() += 1;
        gave[at].insert(source);
    };
    // Tier by tier over every name, so an earlier source never loses an outline to a later one
    for source in [Source::Own, Source::Rules, Source::Syllables, Source::Lattice, Source::Spelled] {
        for (at, name) in names.iter().enumerate() {
            let prons = setup.pron.get(&name.key);
            let mut found: Vec<Vec<Stroke>> = Vec::new();
            match source {
                Source::Own => {
                    // Entries are already joined and typable
                    found.extend(own_outlines.get(name.key.as_str()).into_iter().flatten().map(|e| e.outline.clone()));
                }
                Source::Rules => {
                    for sounds in prons.into_iter().flatten() {
                        for outline in [rules.outline(&name.key, sounds), rules.variant(&name.key, sounds)].into_iter().flatten() {
                            found.extend(finish(outline));
                        }
                    }
                }
                Source::Syllables => {
                    for sounds in prons.into_iter().flatten() {
                        for outline in rules.syllable_paths(&name.key, sounds) {
                            found.extend(finish(outline));
                        }
                    }
                }
                Source::Lattice => {
                    for sounds in prons.into_iter().flatten() {
                        for (_, outline) in rules.lattice_paths(&name.key, sounds, &LATTICE_QUOTAS) {
                            found.extend(finish(outline));
                        }
                    }
                }
                Source::Spelled => {
                    if prons.is_none() && spellable(&name.key) {
                        for (_, outline) in rules.orthographic_paths(&name.key, &SPELLING_QUOTAS).into_iter().chain(rules.spelled_remainder(&name.key, &SPELLING_QUOTAS)) {
                            found.extend(finish(outline));
                        }
                    }
                }
                Source::Borrowed => {}
            }
            for outline in found {
                claim(&mut lane, at, source, outline);
            }
        }
    }
    for (at, name) in names.iter().enumerate() {
        for source in &gave[at] {
            *lane.names_by_source.entry(*source).or_default() += 1;
        }
        if !gave[at].is_empty() {
            continue;
        }
        // Only now Lapwing's, for a name the rules could not strike, and only an outline nobody holds
        let mut any = false;
        for outline in &name.lapwing {
            let Some(strokes) = parse_outline(outline) else { continue };
            let key = lane_outline(&strokes);
            if held.contains_key(&key) || reserved.contains_key(&key) {
                lane.skipped += 1;
                continue;
            }
            held.insert(key.clone(), at);
            lane.borrowed.insert(key, name.text.clone());
            any = true;
        }
        if any {
            *lane.by_source.entry(Source::Borrowed).or_default() += 1;
            lane.borrowed_only.push(name.text.clone());
        } else {
            lane.nothing.push(name.text.clone());
        }
    }
    // The borrowed count above is of names; entries are what the file holds
    lane.by_source.insert(Source::Borrowed, lane.borrowed.len());
    lane.names_by_source.insert(Source::Borrowed, lane.borrowed_only.len());
    Ok(lane)
}

/// Write the lane's two files into `dir`. Returns whether each changed.
pub fn write(lane: &Lane, dir: &Path) -> Result<(bool, bool)> {
    let own = crate::output::write_if_different(&dir.join(FILE), serde_json::to_string_pretty(&lane.own)?.as_bytes())?;
    let borrowed = crate::output::write_if_different(&dir.join(BORROWED_FILE), serde_json::to_string_pretty(&lane.borrowed)?.as_bytes())?;
    Ok((own, borrowed))
}

/// What the lane holds, for `build`: names and entries by source, and the names with no way in.
pub fn summary(lane: &Lane, names: &[Name], setup: &Setup) -> String {
    let with_pron = names.iter().filter(|n| setup.pron.contains_key(&n.key)).count();
    let mut text = format!(
        "{} names ({with_pron} with a pronunciation); {} lyk entries, {} borrowed from Lapwing, {} outlines skipped as held; {} names with no way in",
        names.len(),
        lane.own.len(),
        lane.borrowed.len(),
        lane.skipped,
        lane.nothing.len()
    );
    for (source, entries) in &lane.by_source {
        let names = lane.names_by_source.get(source).copied().unwrap_or(0);
        let _ = write!(text, "\n    {source:?}: {entries} entries for {names} names");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lane() -> &'static (Vec<Name>, Lane) {
        static LANE: std::sync::LazyLock<(Vec<Name>, Lane)> = std::sync::LazyLock::new(|| {
            let setup = crate::dictionary_fixture::setup();
            let names = collect(setup);
            let lane = generate(setup, &crate::dictionary_fixture::generated().entries, &names).expect("the lane generates");
            (names, lane)
        });
        &LANE
    }

    #[test]
    fn acronyms_and_plain_lowercase_words_are_not_names() {
        for (text, name) in [("Noah", true), ("McDonald", true), ("iPad", true), ("O'Brien", true), ("Adam Neely", true), ("NBC", false), ("college", false), (":)", false), ("A", true)] {
            assert_eq!(is_name_text(text), name, "{text}");
        }
    }

    #[test]
    fn every_entry_is_behind_the_hash_and_none_shadows_a_normal_entry() {
        let (_, lane) = lane();
        let held: HashSet<String> = crate::dictionary_fixture::generated().entries.iter().map(|e| render_outline(&e.outline)).collect();
        let reserved = crate::generate::reserved_outlines().unwrap();
        for outline in lane.own.keys().chain(lane.borrowed.keys()) {
            assert!(outline.starts_with("#/") && outline.len() > 2, "{outline}");
            assert!(!held.contains(outline) && !reserved.contains_key(outline), "{outline} shadows a normal entry");
            assert!(parse_outline(outline).is_some(), "{outline} is no outline");
        }
        assert!(lane.own.keys().all(|o| !lane.borrowed.contains_key(o)), "an outline is lyk's or borrowed, never both");
    }

    #[test]
    fn lyk_rules_come_first_and_lapwing_only_fills_what_they_cannot_strike() {
        let (names, lane) = lane();
        let text_has_own = |text: &str| lane.own.values().any(|t| t == text);
        // A name with a pronunciation is struck by the rules, never borrowed
        let setup = crate::dictionary_fixture::setup();
        for name in names.iter().filter(|n| setup.pron.contains_key(&n.key)).take(2_000) {
            assert!(text_has_own(&name.text), "{} has a pronunciation and no lyk entry", name.text);
            assert!(!lane.borrowed.values().any(|t| *t == name.text), "{} is borrowed though the rules strike it", name.text);
        }
        // A name lyk has a word for is `#/` and the word's own outline
        let cheapest = crate::dictionary_fixture::cheapest();
        let daniel = render_outline(cheapest["daniel"]);
        assert_eq!(lane.own.get(&format!("#/{daniel}")).map(String::as_str), Some("Daniel"));
        // A borrowed name is one the rules gave nothing for: no lyk entry at all
        for text in lane.borrowed.values() {
            assert!(!text_has_own(text), "{text} is borrowed though lyk strikes it");
        }
    }

    #[test]
    fn a_name_with_no_pronunciation_is_struck_by_its_spelling() {
        let setup = crate::dictionary_fixture::setup();
        let (names, lane) = lane();
        let name = names.iter().find(|n| !setup.pron.contains_key(&n.key) && spellable(&n.key) && n.key.len() >= 6).expect("there is one");
        assert!(lane.own.values().any(|t| *t == name.text), "{}", name.text);
    }
}

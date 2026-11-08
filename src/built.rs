//! The dictionary `build` already wrote, read back instead of generated again.
//!
//! Generating the dictionary is the slow part of every command that needs "the real outline for
//! this word" (`quote`, `lookup`, `resolve`): tens of seconds of solving, repeated from nothing
//! each run, to reach the answer `build` has already written to `out/lyk`. The files hold every
//! entry the generator made (words, short forms, alternates, backups, extras, phrases, suffixes),
//! so reading them back gives the same `cheapest` answer for every word in a couple of seconds
//! at most. `--rebuild` on those commands generates fresh instead, for a change to the rules or
//! briefs that `build` has not been run on yet.

use crate::generate::{Entry, Origin};
use crate::output::{BUILT_STAMP, FAMILY_FILE, Role};
use crate::rules::Step;
use crate::stroke::parse_outline;
use anyhow::{Context, Result};
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;
use std::time::SystemTime;

/// The origin an entry read from a role's file stands in for. Only the distinctions the pickers
/// make matter: another pronunciation and an extra never win against anything else
/// (`generate::priority`), and a suffix never counts as a word.
const fn origin_of(role: Role) -> Origin {
    match role {
        Role::Words => Origin::Rules,
        Role::Short => Origin::Short(Step::Skeleton),
        Role::Alternates => Origin::Alternate,
        Role::Backups => Origin::Backup,
        Role::Extras => Origin::Extra,
        Role::Phrases => Origin::Phrase,
        Role::Suffixes => Origin::Suffix,
    }
}

/// Whether `dir` holds a built dictionary: at least the words file.
pub fn exists(dir: &Path) -> bool {
    dir.join(Role::Words.file_name()).is_file()
}

/// The `"OUTLINE": "text"` pairs of one role's file, which `build` writes one pair per line
/// (`serde_json::to_string_pretty` of a map). Reading the lines directly instead of parsing the
/// JSON into a map is several times faster on the backups file, which holds millions of pairs.
/// An outline never contains a quote or an escape; a word's text sometimes does and is decoded.
fn pairs(text: &str) -> impl Iterator<Item = (&str, Cow<'_, str>)> {
    text.lines().filter_map(|line| {
        let line = line.trim().trim_end_matches(',');
        let line = line.strip_prefix('"')?;
        let at = line.find("\": \"")?;
        let raw = line[at + 4..].strip_suffix('"')?;
        let word = if raw.contains('\\') {
            Cow::Owned(serde_json::from_str::<String>(&format!("\"{raw}\"")).ok()?)
        } else {
            Cow::Borrowed(raw)
        };
        Some((&line[..at], word))
    })
}

fn read(dir: &Path, role: Role) -> Result<String> {
    let path = dir.join(role.file_name());
    std::fs::read_to_string(&path).with_context(|| path.display().to_string())
}

/// How many strokes an outline's text has, without parsing it.
fn stroke_count(outline: &str) -> usize {
    outline.matches('/').count() + 1
}

/// Every entry of the dictionary `build` wrote to `dir` that can be a word's cheapest outline.
/// Words are lowercase, as the generator keys them everywhere inside (the file writes the pronoun
/// `I` capitalized on the way out).
///
/// The backups file holds millions of `+`-joined fallbacks; a backup is only ever a word's
/// cheapest outline when it takes no more strokes than every other entry the word has, so any
/// backup with more strokes than its word's best so far is passed over unparsed. A backup tying
/// the best on strokes is kept so cost still decides between them, as for generated entries.
pub fn load(dir: &Path) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    let mut fewest: HashMap<String, usize> = HashMap::new();
    // A dictionary built before `build` wrote the family list reads back with every family shape
    // ranked as an ordinary primary, which can pick a different one of two tied one-stroke forms.
    let family: HashMap<String, String> = std::fs::read_to_string(dir.join(FAMILY_FILE))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    for role in Role::ALL.into_iter().filter(|r| *r != Role::Backups) {
        let text = read(dir, role)?;
        for (outline, word) in pairs(&text) {
            let Some(strokes) = parse_outline(outline) else { continue };
            let origin = if role == Role::Words && family.get(outline).is_some_and(|w| *w == *word) {
                Origin::Family
            } else {
                origin_of(role)
            };
            let word = word.to_lowercase();
            if matches!(role, Role::Words | Role::Short) {
                let held = fewest.entry(word.clone()).or_insert(usize::MAX);
                *held = (*held).min(strokes.len());
            }
            entries.push(Entry { word, outline: strokes, origin });
        }
    }
    let text = read(dir, Role::Backups)?;
    for (outline, word) in pairs(&text) {
        let strokes = stroke_count(outline);
        let lower;
        let word = if word.bytes().any(|b| b.is_ascii_uppercase()) {
            lower = word.to_lowercase();
            lower.as_str()
        } else {
            &*word
        };
        match fewest.get_mut(word) {
            Some(held) if strokes > *held => continue,
            Some(held) => *held = strokes,
            None => {
                fewest.insert(word.to_owned(), strokes);
            }
        }
        let Some(parsed) = parse_outline(outline) else { continue };
        entries.push(Entry { word: word.to_owned(), outline: parsed, origin: Origin::Backup });
    }
    Ok(entries)
}

/// A warning when something the dictionary is made from is newer than the files read back
/// (`rules/`, `src/generate/`, `src/rules/`), so a stale answer is never silent. `None` when the
/// files are at least as new as everything checked.
pub fn stale_note(dir: &Path) -> Option<String> {
    // The stamp is rewritten by every `build`; a dictionary from before it existed has only the
    // words file's own time to go by, which an unchanged rebuild leaves alone.
    let built = std::fs::metadata(dir.join(BUILT_STAMP))
        .or_else(|_| std::fs::metadata(dir.join(Role::Words.file_name())))
        .ok()?
        .modified()
        .ok()?;
    let newest = ["rules", "src/generate", "src/rules"]
        .iter()
        .filter_map(|folder| newest_in(Path::new(folder)))
        .max()?;
    (newest > built).then(|| {
        format!(
            "note: the rules or generator changed after {} was built; run `build` (or pass --rebuild) \
             for answers from the current code",
            dir.display()
        )
    })
}

fn newest_in(folder: &Path) -> Option<SystemTime> {
    let mut newest = None;
    for entry in std::fs::read_dir(folder).ok()?.flatten() {
        let path = entry.path();
        let modified = if path.is_dir() {
            newest_in(&path)
        } else {
            entry.metadata().ok().and_then(|m| m.modified().ok())
        };
        newest = newest.max(modified);
    }
    newest
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generate;

    fn write(dir: &Path, role: Role, body: &str) {
        std::fs::write(dir.join(role.file_name()), body).unwrap();
    }

    fn fresh_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("lyk-built-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for role in Role::ALL {
            write(&dir, role, "{}");
        }
        dir
    }

    #[test]
    fn a_built_dictionary_reads_back_with_the_same_cheapest_outline_per_word() {
        let dir = fresh_dir("cheapest");
        write(&dir, Role::Words, "{\n  \"KAT\": \"cat\",\n  \"S\": \"I\"\n}");
        write(&dir, Role::Backups, "{\n  \"KA/+T\": \"cat\"\n}");
        write(&dir, Role::Extras, "{\n  \"KAT*\": \"cat\"\n}");
        write(&dir, Role::Suffixes, "{\n  \"+-PBTS\": \"'s\"\n}");
        assert!(exists(&dir));

        let entries = load(&dir).unwrap();
        let cheapest = generate::cheapest(&entries);
        assert_eq!(cheapest["cat"], parse_outline("KAT").unwrap().as_slice());
        assert!(cheapest.contains_key("i"), "the pronoun is keyed lowercase");
        assert!(!cheapest.contains_key("'s"), "a suffix is never a word");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_backup_is_kept_when_it_is_the_words_only_or_shortest_way() {
        let dir = fresh_dir("backups");
        write(&dir, Role::Words, "{\n  \"TKOG\": \"dog\"\n}");
        write(
            &dir,
            Role::Backups,
            "{\n  \"TKO/+G\": \"dog\",\n  \"KA/+PBT\": \"cant\",\n  \"KA/+PB/+T\": \"cant\"\n}",
        );
        let entries = load(&dir).unwrap();
        let cheapest = generate::cheapest(&entries);
        assert_eq!(cheapest["dog"], parse_outline("TKOG").unwrap().as_slice(), "a longer backup loses");
        assert_eq!(cheapest["cant"], parse_outline("KA/+PBT").unwrap().as_slice(), "the shorter backup wins");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_word_with_an_escape_in_it_is_decoded() {
        let dir = fresh_dir("escape");
        write(&dir, Role::Words, "{\n  \"KR-GS\": \"say \\\"hi\\\"\"\n}");
        let entries = load(&dir).unwrap();
        assert!(entries.iter().any(|e| e.word == "say \"hi\""), "{:?}", entries.iter().map(|e| &e.word).collect::<Vec<_>>());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The reason reading back is allowed to replace generating: for every word, including the
    /// phrases, the cheapest outline from the files is the one the generator itself reports.
    #[test]
    fn reading_back_a_written_dictionary_gives_the_generators_cheapest_outline_for_every_word() {
        let generated = crate::dictionary_fixture::generated();
        let dir = std::env::temp_dir().join(format!("lyk-built-roundtrip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        crate::output::write_dictionary(generated, &dir).unwrap();
        let loaded = load(&dir).unwrap();
        let want = generate::cheapest(&generated.entries);
        let got = generate::cheapest(&loaded);
        // Two things the files cannot carry, neither a ranking difference: the pronoun is keyed
        // lowercase when read back, and a stroke of the join key alone has no notation to parse.
        let readable = |outline: &[crate::stroke::Stroke]| outline.iter().all(|s| s.keys().0 != 0);
        let differ: Vec<_> = want
            .iter()
            .filter(|(word, outline)| readable(outline) && word.chars().all(|c| !c.is_uppercase()))
            .filter(|(word, outline)| got.get(**word) != Some(*outline))
            .map(|(word, outline)| format!("{word}: wrote {outline:?}, read {:?}", got.get(*word)))
            .collect();
        assert!(differ.is_empty(), "{} of {} differ, e.g. {:?}", differ.len(), want.len(), &differ[..differ.len().min(5)]);
        assert!(got.len() + 5 >= want.len(), "{} read back of {} written", got.len(), want.len());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_without_the_words_file_is_not_a_built_dictionary() {
        assert!(!exists(Path::new("definitely/not/a/folder")));
    }
}

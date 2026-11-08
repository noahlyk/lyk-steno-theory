//! The generated dictionary as files: several small Plover JSON dictionaries, one per role, instead
//! of one big one.
//!
//! Every entry goes to the file for the way it was made (see [`Role`]). Outlines never repeat
//! across the files (the generator keeps exactly one word per outline), so which file loads first
//! never changes what a stroke means; the split is for people and for keymux. For a person, the
//! file says what kind of outline it is: `words` is where each word's own outline lives, `backups`
//! holds the `+` fallbacks, `extras` the extra one-stroke spellings. For keymux, each file is cached
//! and reloaded on its own (see its README), so a theory change that only moves some entries
//! rewrites only some files, and a file whose content did not change is not even touched: a build
//! that changes nothing leaves every file's modification time alone.

use crate::generate::{written, Generated, Origin};
use crate::stroke::render_outline;
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::Path;

/// The part of a dictionary a file holds. The file name carries the `02-lyk-` prefix keymux
/// orders the whole theory by (`01-` commands, `03-` punctuation, `04-` modifiers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    /// Where each word's own outline lives: gold briefs, the plain rules reading, spelling and
    /// voicing variants, ladder-settled clashes, and family members' shapes
    Words,
    /// Long words written shorter (the shortening steps)
    Short,
    /// Another pronunciation's outline for a word
    Alternates,
    /// `+`-joined fallbacks composed from other words' own outlines
    Backups,
    /// Extra single-stroke ways to write a word, filling strokes nothing else needed
    Extras,
    /// Two common words in one stroke
    Phrases,
    /// Standalone suffix strokes
    Suffixes,
}

impl Role {
    pub const ALL: [Self; 7] = [
        Self::Words,
        Self::Short,
        Self::Alternates,
        Self::Backups,
        Self::Extras,
        Self::Phrases,
        Self::Suffixes,
    ];

    /// The file this role is written to.
    pub const fn file_name(self) -> &'static str {
        match self {
            Self::Words => "02-lyk-words.json",
            Self::Short => "02-lyk-short.json",
            Self::Alternates => "02-lyk-alternates.json",
            Self::Backups => "02-lyk-backups.json",
            Self::Extras => "02-lyk-extras.json",
            Self::Phrases => "02-lyk-phrases.json",
            Self::Suffixes => "02-lyk-suffixes.json",
        }
    }

    /// Which file an entry made in this way belongs to.
    pub const fn of(origin: Origin) -> Self {
        match origin {
            Origin::Brief
            | Origin::Rules
            | Origin::Spelling
            | Origin::Voicing
            | Origin::Ladder(_)
            | Origin::Family => Self::Words,
            Origin::Short(_) => Self::Short,
            Origin::Alternate => Self::Alternates,
            Origin::Backup => Self::Backups,
            Origin::Extra => Self::Extras,
            Origin::Phrase => Self::Phrases,
            Origin::Suffix => Self::Suffixes,
        }
    }
}

/// The outlines (to their words) that are a family's shape: `build` writes it beside the
/// dictionary files so a dictionary read back ranks them as the generator did.
pub const FAMILY_FILE: &str = "family.json";

/// Rewritten by every `build`, even when no dictionary file changed, so its time says when the
/// dictionary was last checked against the rules (`crate::built::stale_note`).
pub const BUILT_STAMP: &str = ".built";

/// Whether `name` is one of the files this module writes, current or from an earlier layout.
pub fn is_generated_name(name: &str) -> bool {
    name.starts_with("02-lyk-") && Path::new(name).extension().is_some_and(|ext| ext == "json")
}

/// What was written for one role.
pub struct Written {
    pub role: Role,
    pub entries: usize,
    /// Whether the file's content is different from what was there (a new or changed file)
    pub changed: bool,
}

/// The dictionary in `dir`, one file per role, each sorted by outline so that the same entries
/// always give the same bytes. A file that would come out as it already is, is left alone; a
/// generated file in `dir` that no role writes (an earlier layout's) is removed.
pub fn write_dictionary(generated: &Generated, dir: &Path) -> Result<Vec<Written>> {
    let mut by_role: BTreeMap<Role, BTreeMap<String, String>> = BTreeMap::new();
    for entry in &generated.entries {
        by_role
            .entry(Role::of(entry.origin))
            .or_default()
            .insert(render_outline(&entry.outline), written(&entry.word));
    }
    std::fs::create_dir_all(dir).with_context(|| dir.display().to_string())?;
    let mut done = Vec::new();
    for role in Role::ALL {
        let map = by_role.remove(&role).unwrap_or_default();
        let path = dir.join(role.file_name());
        let content = serde_json::to_string_pretty(&map)?;
        let changed = write_if_different(&path, content.as_bytes())?;
        done.push(Written { role, entries: map.len(), changed });
    }
    // Which of the words file's entries are family shapes, so reading the files back
    // (`crate::built`) can rank them as the generator did. Not a dictionary file: it is not
    // `02-lyk-*`, so `deploy` never ships it and `remove_stale` never removes it.
    let family: BTreeMap<String, String> = generated
        .entries
        .iter()
        .filter(|e| e.origin == Origin::Family)
        .map(|e| (render_outline(&e.outline), written(&e.word)))
        .collect();
    write_if_different(&dir.join(FAMILY_FILE), serde_json::to_string_pretty(&family)?.as_bytes())?;
    std::fs::write(dir.join(BUILT_STAMP), "").with_context(|| dir.join(BUILT_STAMP).display().to_string())?;
    remove_stale(dir)?;
    Ok(done)
}

/// Write `content` to `path` unless the file already holds exactly that. Returns whether it wrote.
pub fn write_if_different(path: &Path, content: &[u8]) -> Result<bool> {
    if std::fs::read(path).is_ok_and(|existing| existing == content) {
        return Ok(false);
    }
    std::fs::write(path, content).with_context(|| path.display().to_string())?;
    Ok(true)
}

/// Remove the generated dictionary files in `dir` that no role writes any more.
pub fn remove_stale(dir: &Path) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| dir.display().to_string())?.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if is_generated_name(&name) && !Role::ALL.iter().any(|role| role.file_name() == name) {
            std::fs::remove_file(entry.path()).with_context(|| entry.path().display().to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_role_has_its_own_file_name() {
        let mut names: Vec<_> = Role::ALL.iter().map(|r| r.file_name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), Role::ALL.len());
        assert!(names.iter().all(|n| is_generated_name(n)));
    }

    #[test]
    fn static_reserved_files_are_not_generated_names() {
        for name in ["01-lyk-commands.json", "03-lyk-punctuation.json", "04-lyk-emilys-modifiers.json"] {
            assert!(!is_generated_name(name), "{name}");
        }
    }

    #[test]
    fn a_file_with_the_same_content_is_left_alone() {
        let dir = std::env::temp_dir().join(format!("lyk-output-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.json");
        assert!(write_if_different(&path, b"{}").unwrap());
        assert!(!write_if_different(&path, b"{}").unwrap());
        assert!(write_if_different(&path, b"{\"A\": \"a\"}").unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stale_generated_files_go_and_others_stay() {
        let dir = std::env::temp_dir().join(format!("lyk-output-stale-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["02-lyk-old-layout.json", "02-lyk-words.json", "01-lyk-commands.json", "notes.txt"] {
            std::fs::write(dir.join(name), "{}").unwrap();
        }
        remove_stale(&dir).unwrap();
        assert!(!dir.join("02-lyk-old-layout.json").exists());
        for kept in ["02-lyk-words.json", "01-lyk-commands.json", "notes.txt"] {
            assert!(dir.join(kept).exists(), "{kept}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

//! Which build of the program this is, and whether `out/lyk` came from its rules and generator.
//!
//! `build.rs` bakes in the version (`--version`, see its header for the format) and a content hash
//! of the generator's inputs (`LYK_SOURCE_HASH`: the Rust source, `rules/`, `Cargo.toml`/`.lock`).
//! `build` writes that hash into the `out/lyk/.built` stamp; a command that reads `out/lyk` back
//! compares it and says so, once, when the dictionary is from other rules or another generator
//! than the ones now running. The commit is not compared: a docs-only commit changes no hash.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// The version `--version` and `version` print, baked in by `build.rs`.
pub const VERSION: &str = env!("LYK_VERSION");
/// The hash of the generator's inputs this binary was compiled from, baked in by `build.rs`.
pub const SOURCE_HASH: &str = env!("LYK_SOURCE_HASH");

/// `lyk-steno-theory 0.1.0+g0a1387f`
pub fn version_line() -> String {
    format!("{} {VERSION}", env!("CARGO_PKG_NAME"))
}

static STALE_CHECK_OFF: AtomicBool = AtomicBool::new(false);

/// Silence the stale note (`--no-stale-check`, `LYK_NO_STALE_NOTE`).
pub fn silence_stale_note(off: bool) {
    STALE_CHECK_OFF.store(off, Ordering::Relaxed);
}

/// What `build` writes into the stamp.
pub fn stamp_text(source: &str) -> String {
    format!("source={source}\n")
}

fn field<'a>(stamp: &'a str, key: &str) -> Option<&'a str> {
    stamp.lines().find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
}

/// Why the dictionary stamped `stamp` is not from the rules and generator running now, `None` when
/// it is (or when either side cannot say: a `none` source hash).
pub fn stale_reason(stamp: &str, source: &str) -> Option<String> {
    let Some(built_from) = field(stamp, "source") else {
        return Some("it was built before builds recorded the source they came from".to_string());
    };
    (built_from != source && built_from != "none" && source != "none")
        .then(|| "the rules or the generator changed after it was built".to_string())
}

/// The one-line note for a dictionary folder, `None` when it is current, the check is off, or
/// stdout is not a terminal (a pipe gets the answer and nothing else).
pub fn stale_note(dir: &Path) -> Option<String> {
    use std::io::IsTerminal;
    if STALE_CHECK_OFF.load(Ordering::Relaxed) || !std::io::stdout().is_terminal() {
        return None;
    }
    let stamp = std::fs::read_to_string(dir.join(crate::output::BUILT_STAMP)).unwrap_or_default();
    let reason = stale_reason(&stamp, SOURCE_HASH)?;
    let note = format!("note: {} may not match this code: {reason}; run `build` (or pass --rebuild)", dir.display());
    let colour = std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    Some(if colour { format!("\x1b[2m{note}\x1b[0m") } else { note })
}

// The build script's own logic, run here so a missing program is tested without touching PATH.
#[cfg(test)]
#[allow(clippy::all, clippy::pedantic, clippy::nursery, dead_code)]
#[path = "../build.rs"]
mod script;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_line_names_the_program_and_the_baked_in_version() {
        let line = version_line();
        assert!(line.starts_with("lyk-steno-theory "), "{line}");
        assert!(line.ends_with(VERSION) && !VERSION.is_empty());
    }

    #[test]
    fn with_no_vcs_the_version_is_the_cargo_version_plus_unknown() {
        let version = super::script::describe("definitely-not-a-vcs-program", "0.1.0");
        assert_eq!(version, "0.1.0+unknown");
    }

    #[test]
    fn a_tag_gives_a_clean_version_and_commits_past_it_say_how_many() {
        assert_eq!(script::tag_version("v1.2.0-0-g0587d41", false).as_deref(), Some("1.2.0"));
        assert_eq!(script::tag_version("v1.2.0-8-g0587d41", false).as_deref(), Some("1.2.0+8.g0587d41"));
        assert_eq!(script::tag_version("v1.2.0-0-g0587d41", true).as_deref(), Some("1.2.0+dirty"));
        assert_eq!(script::tag_version("v1.2.0-8-g0587d41", true).as_deref(), Some("1.2.0+8.g0587d41.dirty"));
        assert_eq!(script::tag_version("nonsense", false), None);
    }

    #[test]
    fn a_matching_stamp_is_current_and_another_source_hash_is_stale() {
        let stamp = stamp_text("00ff");
        assert_eq!(stale_reason(&stamp, "00ff"), None);
        assert!(stale_reason(&stamp, "11aa").unwrap().contains("rules or the generator"));
    }

    #[test]
    fn what_cannot_be_compared_is_never_called_stale() {
        assert_eq!(stale_reason(&stamp_text("none"), "00ff"), None);
        assert_eq!(stale_reason(&stamp_text("00ff"), "none"), None);
    }

    #[test]
    fn a_stamp_from_before_the_source_hash_was_recorded_is_stale() {
        assert!(stale_reason("", "00ff").unwrap().contains("before"));
        assert!(stale_reason("binary=abc1234\nrules=00ff\n", "00ff").unwrap().contains("before"));
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("lyk-version-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for sub in ["src/sub", "rules", "docs", "site"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        std::fs::write(dir.join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::write(dir.join("src/sub/mod.rs"), "// m").unwrap();
        std::fs::write(dir.join("rules/a.toml"), "x = 1").unwrap();
        std::fs::write(dir.join("Cargo.toml"), "[package]").unwrap();
        dir
    }

    #[test]
    fn the_source_hash_ignores_docs_the_site_and_the_readme_but_not_rules_or_source() {
        let dir = scratch("hash");
        let before = script::source_hash(&dir);
        assert_ne!(before, "none");
        assert_eq!(before, script::source_hash(&dir));

        std::fs::write(dir.join("docs/decisions.md"), "words").unwrap();
        std::fs::write(dir.join("site/page.json"), "{}").unwrap();
        std::fs::write(dir.join("README.md"), "hello").unwrap();
        std::fs::write(dir.join("src/notes.txt"), "not rust").unwrap();
        assert_eq!(before, script::source_hash(&dir), "docs-only changes must not move the hash");

        std::fs::write(dir.join("rules/a.toml"), "x = 2").unwrap();
        let rules = script::source_hash(&dir);
        assert_ne!(before, rules);
        std::fs::write(dir.join("src/sub/mod.rs"), "// changed").unwrap();
        assert_ne!(rules, script::source_hash(&dir));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(script::source_hash(&dir), "none");
    }
}

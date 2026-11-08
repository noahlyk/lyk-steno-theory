//! Which build of the program this is, and whether `out/lyk` came from it.
//!
//! `build.rs` bakes in the version (`--version`, see its header for the format) and an id for the
//! exact source (`LYK_GIT_ID`). `build` writes that id and a hash of `rules/` into the
//! `out/lyk/.built` stamp; a command that reads `out/lyk` back compares them and says so, once,
//! when the dictionary is from other code or other rules than the ones now running.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// The version `--version` and `version` print, baked in by `build.rs`.
pub const VERSION: &str = env!("LYK_VERSION");
/// The exact source this binary was compiled from, baked in by `build.rs`.
pub const GIT_ID: &str = env!("LYK_GIT_ID");

/// `lyk-steno-theory 0.1.0+g0a1387f`
pub fn version_line() -> String {
    format!("{} {VERSION}", env!("CARGO_PKG_NAME"))
}

static STALE_CHECK_OFF: AtomicBool = AtomicBool::new(false);

/// Silence the stale note (`--no-stale-check`, `LYK_NO_STALE_NOTE`).
pub fn silence_stale_note(off: bool) {
    STALE_CHECK_OFF.store(off, Ordering::Relaxed);
}

fn fnv(hash: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(hash, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3))
}

/// A hash of every file under `folder` (names and contents, in name order), `none` when there is
/// no such folder.
pub fn inputs_hash(folder: &Path) -> String {
    fn walk(folder: &Path, hash: &mut u64) -> bool {
        let Ok(read) = std::fs::read_dir(folder) else { return false };
        let mut paths: Vec<_> = read.flatten().map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            if path.is_dir() {
                walk(&path, hash);
            } else if let Ok(bytes) = std::fs::read(&path) {
                *hash = fnv(*hash, path.to_string_lossy().as_bytes());
                *hash = fnv(*hash, &bytes);
            }
        }
        true
    }
    let mut hash = 0xcbf2_9ce4_8422_2325;
    if walk(folder, &mut hash) { format!("{hash:016x}") } else { "none".to_string() }
}

/// What `build` writes into the stamp.
pub fn stamp_text(id: &str, inputs: &str) -> String {
    format!("binary={id}\nrules={inputs}\n")
}

fn field<'a>(stamp: &'a str, key: &str) -> Option<&'a str> {
    stamp.lines().find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
}

/// The id without the hash of uncommitted changes, for showing.
fn shown(id: &str) -> &str {
    id.split('.').next().unwrap_or(id)
}

/// Why the dictionary stamped `stamp` is not from the code and rules running now, `None` when it
/// is (or when either side cannot say: `unknown` source, no `rules/` folder).
pub fn stale_reason(stamp: &str, id: &str, inputs: &str) -> Option<String> {
    let Some(built_by) = field(stamp, "binary") else {
        return Some("it was built before builds recorded which version made them".to_string());
    };
    if built_by != id && built_by != "unknown" && id != "unknown" {
        return Some(format!("it was built by {}, and this is {}", shown(built_by), shown(id)));
    }
    let built_rules = field(stamp, "rules").unwrap_or("none");
    (built_rules != inputs && built_rules != "none" && inputs != "none")
        .then(|| "the rules changed after it was built".to_string())
}

/// The one-line note for a dictionary folder, `None` when it is current, the check is off, or
/// stdout is not a terminal (a pipe gets the answer and nothing else).
pub fn stale_note(dir: &Path) -> Option<String> {
    use std::io::IsTerminal;
    if STALE_CHECK_OFF.load(Ordering::Relaxed) || !std::io::stdout().is_terminal() {
        return None;
    }
    let stamp = std::fs::read_to_string(dir.join(crate::output::BUILT_STAMP)).unwrap_or_default();
    let reason = stale_reason(&stamp, GIT_ID, &inputs_hash(Path::new("rules")))?;
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
    fn without_git_the_version_is_the_cargo_version_plus_unknown() {
        let (version, id) = super::script::describe("definitely-not-a-git-program", "0.1.0");
        assert_eq!((version.as_str(), id.as_str()), ("0.1.0+unknown", "unknown"));
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
    fn the_stamp_round_trips_and_a_matching_one_is_current() {
        let stamp = stamp_text("abc1234", "00ff");
        assert_eq!(stale_reason(&stamp, "abc1234", "00ff"), None);
    }

    #[test]
    fn another_binary_or_other_rules_make_it_stale() {
        let stamp = stamp_text("abc1234", "00ff");
        let other = stale_reason(&stamp, "def5678", "00ff").unwrap();
        assert!(other.contains("abc1234") && other.contains("def5678"), "{other}");
        assert!(stale_reason(&stamp, "abc1234", "11aa").unwrap().contains("rules"));
    }

    #[test]
    fn a_dirty_tree_is_compared_exactly_but_shown_without_its_change_hash() {
        let stamp = stamp_text("abc1234-dirty.11111111", "00ff");
        assert_eq!(stale_reason(&stamp, "abc1234-dirty.11111111", "00ff"), None);
        let reason = stale_reason(&stamp, "abc1234-dirty.22222222", "00ff").unwrap();
        assert!(reason.contains("abc1234-dirty") && !reason.contains("11111111"), "{reason}");
    }

    #[test]
    fn what_cannot_be_compared_is_never_called_stale() {
        assert_eq!(stale_reason(&stamp_text("unknown", "none"), "abc1234", "00ff"), None);
        assert_eq!(stale_reason(&stamp_text("abc1234", "00ff"), "unknown", "none"), None);
    }

    #[test]
    fn a_stamp_from_before_versions_were_recorded_is_stale() {
        assert!(stale_reason("", "abc1234", "00ff").unwrap().contains("before"));
    }

    #[test]
    fn the_rules_hash_follows_the_content_and_a_missing_folder_is_none() {
        let dir = std::env::temp_dir().join(format!("lyk-version-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.toml"), "x = 1").unwrap();
        let before = inputs_hash(&dir);
        assert_eq!(before, inputs_hash(&dir));
        std::fs::write(dir.join("a.toml"), "x = 2").unwrap();
        assert_ne!(before, inputs_hash(&dir));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(inputs_hash(&dir), "none");
    }
}

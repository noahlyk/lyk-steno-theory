//! Bakes the version the binary was compiled from into it (`lyk-steno-theory --version`), the way
//! keymux's `build.rs` does.
//!
//! `LYK_VERSION`, by priority:
//! 1. `LYK_VERSION` in the environment (a release build sets it).
//! 2. `git describe` on a checkout, against tags named `v<number>...`:
//!    - exactly on a tag:        `1.2.0`
//!    - commits past a tag:      `1.2.0+8.g0587d41`
//!    - uncommitted changes:     `dirty` added to the end of the `+` part (`1.2.0+dirty` on a tag)
//! 3. No tags: Cargo.toml's version and the short hash, `0.1.0+g0587d41` (`0.1.0+g0587d41.dirty`).
//! 4. No git (a tarball, or git missing): `0.1.0+unknown`.
//!
//! `LYK_SOURCE_HASH`: a content hash of what decides the dictionary's bytes (`src/**/*.rs`,
//! `rules/**`, `Cargo.toml`, `Cargo.lock`; not docs, the site, the README, tests directories or
//! git state). `build` stamps it into `out/lyk/.built` and the stale check compares it
//! (`src/version.rs`), so a docs-only commit never makes the dictionary look stale and any change
//! to the rules or the generator does.
//!
//! Re-runs when HEAD, the current branch's ref, tags, the index or an input file changes, so a
//! plain `cargo run` after a commit shows the new version. Git's own files are found with
//! `git rev-parse --git-path`, which is right inside a linked worktree (`.git` is a file there).

use std::path::{Path, PathBuf};
use std::process::Command;

/// What a dictionary is made from: a change to these is a change to the answers.
const INPUTS: [&str; 4] = ["src", "rules", "Cargo.toml", "Cargo.lock"];

fn git(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8(out.stdout).ok()?.trim().to_string())
}

fn walk(dir: &Path, only_rs: bool, files: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    for path in read.flatten().map(|e| e.path()) {
        if path.is_dir() {
            walk(&path, only_rs, files);
        } else if !only_rs || path.extension().is_some_and(|e| e == "rs") {
            files.push(path);
        }
    }
}

/// Hash of the generator's inputs under `root`.
///
/// Every `.rs` file under `src/`, every file under `rules/`, `Cargo.toml` and `Cargo.lock` (relative names and contents, in name order). `none`
/// when `root` holds none of them.
#[must_use]
pub fn source_hash(root: &Path) -> String {
    let mut files = Vec::new();
    walk(&root.join("src"), true, &mut files);
    walk(&root.join("rules"), false, &mut files);
    files.extend(["Cargo.toml", "Cargo.lock"].iter().map(|f| root.join(f)).filter(|p| p.is_file()));
    files.sort();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut any = false;
    for path in files {
        let Ok(bytes) = std::fs::read(&path) else { continue };
        any = true;
        let name = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
        for chunk in [name.as_bytes(), &bytes] {
            hash = chunk.iter().fold(hash, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3));
        }
    }
    if any { format!("{hash:016x}") } else { "none".to_string() }
}

/// The version string for the checkout in the current directory, with `program` as git.
#[must_use]
pub fn describe(program: &str, cargo_version: &str) -> String {
    let Some(hash) = git(program, &["rev-parse", "--short", "HEAD"]).filter(|h| !h.is_empty()) else {
        return format!("{cargo_version}+unknown");
    };
    // `status` refreshes the index first, so a file edited and then restored reads as clean.
    // If it cannot run, call the tree clean rather than fail the build.
    let dirty = git(program, &["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
    git(program, &["describe", "--tags", "--long", "--match", "v[0-9]*"])
        .and_then(|described| tag_version(&described, dirty))
        .unwrap_or_else(|| format!("{cargo_version}+g{hash}{}", if dirty { ".dirty" } else { "" }))
}

/// `v1.2.0-8-g0587d41` -> `1.2.0+8.g0587d41`; on the tag itself (`-0-`) just `1.2.0`.
#[must_use]
pub fn tag_version(described: &str, dirty: bool) -> Option<String> {
    let mut parts = described.rsplitn(3, '-');
    let hash = parts.next()?;
    let count: u32 = parts.next()?.parse().ok()?;
    let tag = parts.next()?.strip_prefix('v')?;
    let mut meta = Vec::new();
    if count > 0 {
        meta.push(format!("{count}.{hash}"));
    }
    if dirty {
        meta.push("dirty".to_string());
    }
    Some(if meta.is_empty() { tag.to_string() } else { format!("{tag}+{}", meta.join(".")) })
}

fn watch_git() {
    let mut internals =
        vec!["HEAD".to_string(), "packed-refs".to_string(), "index".to_string(), "refs/tags".to_string()];
    if let Some(branch_ref) = git("git", &["symbolic-ref", "-q", "HEAD"]) {
        internals.push(branch_ref);
    }
    for p in internals {
        if let Some(path) = git("git", &["rev-parse", "--git-path", &p]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    // The source hash depends on the contents of these real paths.
    for path in INPUTS {
        println!("cargo:rerun-if-changed={path}");
    }
}

fn main() {
    println!("cargo:rerun-if-env-changed=LYK_VERSION");
    watch_git();
    let cargo_version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".to_string());
    let version = std::env::var("LYK_VERSION")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| describe("git", &cargo_version));
    println!("cargo:rustc-env=LYK_VERSION={version}");
    println!("cargo:rustc-env=LYK_SOURCE_HASH={}", source_hash(Path::new(".")));
}

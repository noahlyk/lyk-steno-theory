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
//! `LYK_GIT_ID`: the short hash, plus a hash of the uncommitted changes to what the dictionary is
//! made from when the tree is dirty, or `unknown`. `build` stamps it into `out/lyk/.built` and
//! the stale check compares it (`src/version.rs`); two different dirty trees never look alike.
//!
//! Re-runs when HEAD, the current branch's ref, tags, the index or an input file changes, so a
//! plain `cargo run` after a commit shows the new version. Git's own files are found with
//! `git rev-parse --git-path`, which is right inside a linked worktree (`.git` is a file there).

use std::process::Command;

/// What a dictionary is made from: a change to these is a change to the answers.
const INPUTS: [&str; 5] = ["src", "rules", "build.rs", "Cargo.toml", "Cargo.lock"];

fn git(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8(out.stdout).ok()?.trim().to_string())
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3))
}

/// `(version, id)` for the checkout in the current directory, with `program` as git.
#[must_use]
pub fn describe(program: &str, cargo_version: &str) -> (String, String) {
    let Some(hash) = git(program, &["rev-parse", "--short", "HEAD"]).filter(|h| !h.is_empty()) else {
        return (format!("{cargo_version}+unknown"), "unknown".to_string());
    };
    // `status` refreshes the index first, so a file edited and then restored reads as clean.
    // If it cannot run, call the tree clean rather than fail the build.
    let dirty = git(program, &["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
    let id = if dirty {
        let mut diff = vec!["diff", "HEAD", "--"];
        diff.extend(INPUTS);
        let changes = git(program, &diff).unwrap_or_default();
        format!("{hash}-dirty.{:08x}", fnv(changes.as_bytes()) & 0xffff_ffff)
    } else {
        hash.clone()
    };
    let version = git(program, &["describe", "--tags", "--long", "--match", "v[0-9]*"])
        .and_then(|described| tag_version(&described, dirty))
        .unwrap_or_else(|| format!("{cargo_version}+g{hash}{}", if dirty { ".dirty" } else { "" }));
    (version, id)
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
    // The dirty state depends on the contents of the tracked inputs, which are real paths.
    for path in INPUTS {
        println!("cargo:rerun-if-changed={path}");
    }
}

fn main() {
    println!("cargo:rerun-if-env-changed=LYK_VERSION");
    watch_git();
    let cargo_version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".to_string());
    let (version, id) = std::env::var("LYK_VERSION")
        .ok()
        .filter(|v| !v.is_empty())
        .map_or_else(|| describe("git", &cargo_version), |v| (v.clone(), v));
    println!("cargo:rustc-env=LYK_VERSION={version}");
    println!("cargo:rustc-env=LYK_GIT_ID={id}");
}

//! Where the data and the build output live, so the same binary works from a checkout and from
//! anywhere once installed.
//!
//! Two folders, each resolved the same way by every command (`fetch`, `build`, `deploy`, `lookup`,
//! `solve`, ...), first match wins:
//!
//! | | data (downloaded by `fetch`) | out (written by `build`) |
//! |---|---|---|
//! | 1. flag / env | `--data DIR`, then `LYK_DATA` | `LYK_OUT` |
//! | 2. current folder is a checkout | `data` | `out` |
//! | 3. otherwise | `$XDG_DATA_HOME/lyk/data` | `$XDG_STATE_HOME/lyk/out` |
//!
//! A checkout is a folder whose `Cargo.toml` names this package and which has `rules/`. The XDG
//! homes default to `~/.local/share` and `~/.local/state`; with no `$HOME` either, the relative
//! `data` and `out` are used. Inside out: `lyk/` is the built dictionary, `cache/` the lookup
//! cache, `mistakes/` the tape reports.

use anyhow::{Result, bail};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// What decides the folders: the environment and whether the current folder is a checkout.
#[derive(Debug, Default, Clone)]
pub struct Env {
    pub lyk_data: Option<PathBuf>,
    pub lyk_out: Option<PathBuf>,
    pub xdg_data_home: Option<PathBuf>,
    pub xdg_state_home: Option<PathBuf>,
    pub home: Option<PathBuf>,
    pub checkout: bool,
}

/// Which rule picked a folder, for `where`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Flag,
    Env,
    Checkout,
    Xdg,
    Fallback,
}

impl Source {
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Flag => "--data",
            Self::Env => "environment",
            Self::Checkout => "current folder is a checkout",
            Self::Xdg => "XDG default",
            Self::Fallback => "no $HOME: relative",
        }
    }
}

fn non_empty(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// Whether `dir` is a checkout of this repository.
#[must_use]
pub fn is_checkout(dir: &Path) -> bool {
    let named = std::fs::read_to_string(dir.join("Cargo.toml"))
        .is_ok_and(|t| t.lines().any(|l| l.trim().replace(' ', "") == "name=\"lyk-steno-theory\""));
    named && dir.join("rules").is_dir()
}

impl Env {
    /// The real environment and current folder.
    #[must_use]
    pub fn current() -> Self {
        Self {
            lyk_data: non_empty("LYK_DATA"),
            lyk_out: non_empty("LYK_OUT"),
            xdg_data_home: non_empty("XDG_DATA_HOME").filter(|p| p.is_absolute()),
            xdg_state_home: non_empty("XDG_STATE_HOME").filter(|p| p.is_absolute()),
            home: non_empty("HOME"),
            checkout: is_checkout(Path::new(".")),
        }
    }

    fn xdg(&self, xdg_home: Option<&PathBuf>, under_home: &str, leaf: &str) -> Option<(PathBuf, Source)> {
        let base = xdg_home.cloned().or_else(|| self.home.as_ref().map(|h| h.join(under_home)))?;
        Some((base.join("lyk").join(leaf), Source::Xdg))
    }

    /// The data folder, `flag` being `--data` when given.
    #[must_use]
    pub fn data(&self, flag: Option<&Path>) -> (PathBuf, Source) {
        if let Some(flag) = flag {
            return (flag.to_path_buf(), Source::Flag);
        }
        if let Some(env) = &self.lyk_data {
            return (env.clone(), Source::Env);
        }
        if self.checkout {
            return (PathBuf::from("data"), Source::Checkout);
        }
        self.xdg(self.xdg_data_home.as_ref(), ".local/share", "data")
            .unwrap_or_else(|| (PathBuf::from("data"), Source::Fallback))
    }

    /// Where lyk's own records live: `$XDG_STATE_HOME/lyk` when installed, else the out folder.
    #[must_use]
    pub fn state(&self) -> PathBuf {
        let base = self.xdg_state_home.clone().or_else(|| self.home.as_ref().map(|h| h.join(".local/state")));
        match base {
            Some(base) if self.lyk_out.is_none() && !self.checkout => base.join("lyk"),
            _ => self.out().0,
        }
    }

    /// The out folder.
    #[must_use]
    pub fn out(&self) -> (PathBuf, Source) {
        if let Some(env) = &self.lyk_out {
            return (env.clone(), Source::Env);
        }
        if self.checkout {
            return (PathBuf::from("out"), Source::Checkout);
        }
        self.xdg(self.xdg_state_home.as_ref(), ".local/state", "out")
            .unwrap_or_else(|| (PathBuf::from("out"), Source::Fallback))
    }
}

static ENV: OnceLock<Env> = OnceLock::new();
static DATA_FLAG: OnceLock<PathBuf> = OnceLock::new();

fn env() -> &'static Env {
    ENV.get_or_init(Env::current)
}

/// Record `--data` (once, at start-up).
pub fn set_data_flag(flag: Option<&Path>) {
    if let Some(flag) = flag {
        let _ = DATA_FLAG.set(flag.to_path_buf());
    }
}

/// The data folder.
#[must_use]
pub fn data_dir() -> PathBuf {
    env().data(DATA_FLAG.get().map(PathBuf::as_path)).0
}

/// The out folder.
#[must_use]
pub fn out_dir() -> PathBuf {
    env().out().0
}

/// The practice history: `lyk/practice.jsonl` under the state home when installed (beside `out`,
/// not inside the rebuildable output), else inside out.
#[must_use]
pub fn practice_history() -> PathBuf {
    env().state().join("practice.jsonl")
}

/// The built dictionary, `out/lyk`.
#[must_use]
pub fn built_dir() -> PathBuf {
    out_dir().join("lyk")
}

/// The lookup cache, `out/cache`.
#[must_use]
pub fn cache_dir() -> PathBuf {
    out_dir().join("cache")
}

/// The tape reports, `out/mistakes`.
#[must_use]
pub fn mistakes_dir() -> PathBuf {
    out_dir().join("mistakes")
}

/// For a clap `default_value`: the built dictionary folder as text.
#[must_use]
pub fn built_dir_text() -> &'static str {
    static TEXT: OnceLock<String> = OnceLock::new();
    TEXT.get_or_init(|| built_dir().to_string_lossy().into_owned())
}

/// The file every command that needs the data reads first.
const MARKER: &str = "pronunciation/cmudict.dict";

/// Stop with a pointer to `lyk fetch` when `data` holds no downloaded data.
pub fn require_data(data: &Path) -> Result<()> {
    if data.join(MARKER).is_file() {
        return Ok(());
    }
    bail!(
        "no data in {}: run `lyk fetch` to download it (or point --data / LYK_DATA at a folder that has it); `lyk where` shows the folders in use",
        data.display()
    );
}

/// The `where` report.
#[must_use]
pub fn report(deploy_to: &Path) -> String {
    use std::fmt::Write;
    let env = env();
    let data_flag = DATA_FLAG.get().map(PathBuf::as_path);
    let (data, data_source) = env.data(data_flag);
    let (out, out_source) = env.out();
    let present = |p: &Path| if p.exists() { "present" } else { "missing" };
    let mut text = String::new();
    let _ = writeln!(text, "{}", crate::version::version_line());
    let _ = writeln!(
        text,
        "mode        {}",
        if env.checkout { "checkout (relative data/ and out/)" } else { "installed (XDG folders)" }
    );
    let _ = writeln!(
        text,
        "data        {}  [{}; {}]",
        data.display(),
        data_source.text(),
        if data.join(MARKER).is_file() { "downloaded" } else { "empty: run `lyk fetch`" }
    );
    let _ = writeln!(text, "out         {}  [{}; {}]", out.display(), out_source.text(), present(&out));
    let built = out.join("lyk");
    let _ = writeln!(text, "dictionary  {}  [{}; `lyk build` writes it]", built.display(), present(&built));
    let _ = writeln!(
        text,
        "deploy to   {}  [{}; `lyk deploy` writes keymux's lyk config folder, --to]",
        deploy_to.display(),
        present(deploy_to)
    );
    text.push_str("override    --data DIR, LYK_DATA, LYK_OUT; defaults XDG_DATA_HOME, XDG_STATE_HOME\n");
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(checkout: bool) -> Env {
        Env { home: Some("/home/u".into()), checkout, ..Env::default() }
    }

    #[test]
    fn a_checkout_keeps_the_relative_folders() {
        let e = env(true);
        assert_eq!(e.data(None), (PathBuf::from("data"), Source::Checkout));
        assert_eq!(e.out(), (PathBuf::from("out"), Source::Checkout));
    }

    #[test]
    fn elsewhere_the_xdg_folders_are_used() {
        let e = env(false);
        assert_eq!(e.data(None).0, PathBuf::from("/home/u/.local/share/lyk/data"));
        assert_eq!(e.out().0, PathBuf::from("/home/u/.local/state/lyk/out"));
        let e = Env { xdg_data_home: Some("/x/d".into()), xdg_state_home: Some("/x/s".into()), ..env(false) };
        assert_eq!(e.data(None).0, PathBuf::from("/x/d/lyk/data"));
        assert_eq!(e.out().0, PathBuf::from("/x/s/lyk/out"));
    }

    #[test]
    fn flag_beats_env_beats_checkout() {
        let e = Env { lyk_data: Some("/env/data".into()), lyk_out: Some("/env/out".into()), ..env(true) };
        assert_eq!(e.data(None), (PathBuf::from("/env/data"), Source::Env));
        assert_eq!(e.data(Some(Path::new("/flag"))), (PathBuf::from("/flag"), Source::Flag));
        assert_eq!(e.out(), (PathBuf::from("/env/out"), Source::Env));
    }

    #[test]
    fn records_sit_beside_out_when_installed_and_inside_it_otherwise() {
        assert_eq!(env(false).state(), PathBuf::from("/home/u/.local/state/lyk"));
        assert_eq!(env(true).state(), PathBuf::from("out"));
        let e = Env { lyk_out: Some("/env/out".into()), ..env(false) };
        assert_eq!(e.state(), PathBuf::from("/env/out"));
    }

    #[test]
    fn with_no_home_it_falls_back_to_relative() {
        let e = Env::default();
        assert_eq!(e.data(None), (PathBuf::from("data"), Source::Fallback));
        assert_eq!(e.out(), (PathBuf::from("out"), Source::Fallback));
    }

    #[test]
    fn only_this_package_with_rules_is_a_checkout() {
        let dir = std::env::temp_dir().join(format!("lyk-paths-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("rules")).unwrap();
        assert!(!is_checkout(&dir));
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"other\"\n").unwrap();
        assert!(!is_checkout(&dir));
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"lyk-steno-theory\"\n").unwrap();
        assert!(is_checkout(&dir));
        std::fs::remove_dir_all(dir.join("rules")).unwrap();
        assert!(!is_checkout(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_data_points_at_fetch() {
        let err = require_data(Path::new("/definitely/not/here")).unwrap_err().to_string();
        assert!(err.contains("lyk fetch"), "{err}");
    }
}

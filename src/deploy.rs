//! Installing a built theory into keymux's live folder without leaving it broken: stage every
//! file beside the folder, check the staged files with `keymux steno validate`, keep the previous
//! files for one deploy, swap file by file, and put the previous files back if anything fails.
//! `lyk deploy --rollback` restores the kept files.

use anyhow::{Context, Result, bail};
use std::ffi::OsStr;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// How many findings the summary prints.
const SHOWN_FINDINGS: usize = 10;

/// Where one file of the deployed folder comes from.
pub enum Source {
    /// Copied from a file `build` wrote
    File(PathBuf),
    /// Written from text the binary holds
    Text(&'static str),
}

/// One file of the deployed folder.
pub struct Item {
    pub name: String,
    pub source: Source,
}

/// Whether and how strictly the staged files are checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Validate {
    /// `--no-validate`
    Skip,
    /// Errors stop the deploy, warnings are printed
    Errors,
    /// `--strict-validate`: warnings stop it too
    Strict,
}

/// One thing the validator found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub file: String,
    pub outline: Option<String>,
    pub severity: String,
    pub message: String,
}

/// What `keymux steno validate --json` said.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub errors: usize,
    pub warnings: usize,
    pub info: usize,
    pub files: usize,
    pub entries: usize,
    pub findings: Vec<Finding>,
}

impl Report {
    /// Read the validator's `--json` output.
    pub fn parse(text: &str) -> Result<Self> {
        let json: serde_json::Value = serde_json::from_str(text).context("the validator's output is not JSON")?;
        let count = |key: &str| {
            json.get(key)
                .and_then(serde_json::Value::as_u64)
                .map(|n| usize::try_from(n).unwrap_or(usize::MAX))
        };
        let (Some(errors), Some(warnings)) = (count("errors"), count("warnings")) else {
            bail!("the validator's JSON has no `errors` and `warnings` counts");
        };
        let text_of = |finding: &serde_json::Value, key: &str| {
            finding.get(key).and_then(serde_json::Value::as_str).map(str::to_owned)
        };
        let findings = json
            .get("findings")
            .and_then(serde_json::Value::as_array)
            .map_or_else(Vec::new, |all| {
                all.iter()
                    .map(|f| Finding {
                        file: text_of(f, "file").unwrap_or_default(),
                        outline: text_of(f, "outline"),
                        severity: text_of(f, "severity").unwrap_or_default(),
                        message: text_of(f, "message").unwrap_or_default(),
                    })
                    .collect()
            });
        Ok(Self {
            errors,
            warnings,
            info: count("info").unwrap_or(0),
            files: count("files").unwrap_or(0),
            entries: count("entries").unwrap_or(0),
            findings,
        })
    }

    /// `Err` with the reason when this report should stop a deploy.
    pub fn verdict(&self, mode: Validate) -> Result<(), String> {
        if self.errors > 0 {
            return Err(format!("{} validation error(s)", self.errors));
        }
        if mode == Validate::Strict && self.warnings > 0 {
            return Err(format!("{} validation warning(s) with --strict-validate", self.warnings));
        }
        Ok(())
    }

    /// The terse summary: counts, then the first errors and warnings.
    pub fn summary(&self) -> String {
        let mut text = format!(
            "validate: {} error(s), {} warning(s), {} note(s) in {} file(s), {} entries",
            self.errors, self.warnings, self.info, self.files, self.entries
        );
        let mut shown: Vec<&Finding> = self.findings.iter().filter(|f| f.severity == "error").collect();
        shown.extend(self.findings.iter().filter(|f| f.severity == "warning"));
        for finding in shown.iter().take(SHOWN_FINDINGS) {
            let file = Path::new(&finding.file).file_name().map_or_else(
                || finding.file.clone(),
                |name| name.to_string_lossy().into_owned(),
            );
            let _ = write!(
                text,
                "\n  {} {} {}: {}",
                finding.severity,
                file,
                finding.outline.as_deref().unwrap_or("(file)"),
                finding.message
            );
        }
        let rest = (self.errors + self.warnings).saturating_sub(SHOWN_FINDINGS);
        if rest > 0 {
            let _ = write!(text, "\n  ... and {rest} more (run `keymux steno validate` for all)");
        }
        text
    }
}

/// The first executable called `name` in the directories of `path_var`.
pub fn find_on_path(name: &str, path_var: &OsStr) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    std::env::split_paths(path_var).map(|dir| dir.join(name)).find(|candidate| {
        candidate.metadata().is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
    })
}

/// What checking the staged files came to.
#[derive(Debug)]
pub enum Check {
    /// Not run, and why (one line)
    Skipped(String),
    Ran(Report),
}

/// Run `keymux steno validate --json` on `files`.
pub fn validate(keymux: &Path, files: &[PathBuf], strict: bool) -> Check {
    let run = || {
        let mut command = Command::new(keymux);
        command.args(["steno", "validate", "--json"]);
        if strict {
            command.arg("--strict");
        }
        command.args(files);
        // A script just written can still be held open by a forking thread: try again
        for _ in 0..20 {
            match command.output() {
                Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
                other => return other,
            }
        }
        command.output()
    };
    let output = match run() {
        Ok(output) => output,
        Err(e) => return Check::Skipped(format!("could not run {}: {e}", keymux.display())),
    };
    match output.status.code() {
        Some(0 | 1) => match Report::parse(&String::from_utf8_lossy(&output.stdout)) {
            Ok(report) => Check::Ran(report),
            Err(e) => Check::Skipped(format!("{e}")),
        },
        code => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let line = stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("no message");
            let code = code.map_or_else(|| "a signal".to_owned(), |c| format!("exit {c}"));
            Check::Skipped(format!("keymux steno validate could not run ({code}): {line}"))
        }
    }
}

/// The hidden folders beside the live one: the files being installed, and the previous set.
fn siblings(to: &Path) -> Result<(PathBuf, PathBuf)> {
    let (Some(parent), Some(name)) = (to.parent(), to.file_name()) else {
        bail!("{} has no parent folder to keep the previous files in", to.display());
    };
    let name = name.to_string_lossy();
    Ok((parent.join(format!(".{name}-staging")), parent.join(format!(".{name}-previous"))))
}

/// The regular files directly in `dir`, by name.
fn files_in(dir: &Path) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| dir.display().to_string())?.flatten() {
        if entry.file_type().is_ok_and(|t| t.is_file()) {
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    names.sort();
    Ok(names)
}

/// Put a copy of `from` at `dest` in one rename, so `dest` is never half written. A hard link
/// where the file system has them (nothing is copied), else a copy.
fn place(from: &Path, dest: &Path) -> Result<()> {
    let dir = dest.parent().unwrap_or_else(|| Path::new("."));
    let temp = dir.join(format!(".lyk-tmp-{}", dest.file_name().unwrap_or_default().to_string_lossy()));
    let _ = fs::remove_file(&temp);
    if fs::hard_link(from, &temp).is_err() {
        fs::copy(from, &temp).with_context(|| temp.display().to_string())?;
    }
    fs::rename(&temp, dest).with_context(|| dest.display().to_string())
}

/// Make `live` hold exactly the files of `previous`.
fn restore(previous: &Path, live: &Path) -> Result<()> {
    fs::create_dir_all(live)?;
    let keep = files_in(previous)?;
    for name in files_in(live)? {
        if !keep.contains(&name) {
            fs::remove_file(live.join(&name)).with_context(|| name.clone())?;
        }
    }
    for name in &keep {
        place(&previous.join(name), &live.join(name))?;
    }
    Ok(())
}

/// `lyk deploy --rollback`: put back the files the last deploy replaced.
pub fn rollback(to: &Path) -> Result<()> {
    let (_, previous) = siblings(to)?;
    if !previous.is_dir() {
        bail!("nothing to roll back to: {} does not exist", previous.display());
    }
    restore(&previous, to)?;
    println!("rolled back {} to the files kept in {}", to.display(), previous.display());
    Ok(())
}

/// Install `items` into `to`. Stages them in a hidden sibling folder, checks them (`mode`, with
/// the validator found on `path_var`), keeps the files now in `to` in `.NAME-previous` and
/// renames the staged files in; `cleanup` then removes what an older layout left. A failure after
/// the first rename puts the previous files back.
pub fn install(
    items: &[Item],
    to: &Path,
    mode: Validate,
    path_var: &OsStr,
    cleanup: &dyn Fn(&Path) -> Result<()>,
) -> Result<()> {
    let (staging, previous) = siblings(to)?;
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).with_context(|| staging.display().to_string())?;
    let staged = (|| -> Result<()> {
        for item in items {
            let dest = staging.join(&item.name);
            match &item.source {
                Source::File(from) => {
                    fs::copy(from, &dest).with_context(|| from.display().to_string())?;
                }
                Source::Text(text) => fs::write(&dest, text).with_context(|| dest.display().to_string())?,
            }
        }
        Ok(())
    })();
    if let Err(e) = staged {
        let _ = fs::remove_dir_all(&staging);
        return Err(e);
    }

    if mode != Validate::Skip {
        let json_files: Vec<PathBuf> =
            items.iter().filter(|i| Path::new(&i.name).extension().is_some_and(|e| e == "json")).map(|i| staging.join(&i.name)).collect();
        match find_on_path("keymux", path_var) {
            None => println!("validate: skipped, no `keymux` on PATH"),
            Some(keymux) => match validate(&keymux, &json_files, mode == Validate::Strict) {
                Check::Skipped(why) => println!("validate: skipped, {why}"),
                Check::Ran(report) => {
                    println!("{}", report.summary());
                    if let Err(why) = report.verdict(mode) {
                        let _ = fs::remove_dir_all(&staging);
                        bail!("not deployed, {why}; {} is unchanged (--no-validate skips the check)", to.display());
                    }
                }
            },
        }
    }

    fs::create_dir_all(to).with_context(|| to.display().to_string())?;
    let _ = fs::remove_dir_all(&previous);
    fs::create_dir_all(&previous).with_context(|| previous.display().to_string())?;
    for name in files_in(to)? {
        place(&to.join(&name), &previous.join(&name))?;
    }

    let swapped = (|| -> Result<()> {
        for item in items {
            let dest = to.join(&item.name);
            if fs::rename(staging.join(&item.name), &dest).is_err() {
                // another file system: copy beside, then rename
                place(&staging.join(&item.name), &dest)?;
            }
        }
        cleanup(to)
    })();
    let _ = fs::remove_dir_all(&staging);
    if let Err(e) = swapped {
        return match restore(&previous, to) {
            Ok(()) => Err(e.context("deploy failed; the previous files are back")),
            Err(again) => Err(e.context(format!(
                "deploy failed, and putting the previous files back failed too ({again}); they are in {}",
                previous.display()
            ))),
        };
    }
    Ok(())
}

// ---- targets: keymux and typeladder, each only when installed ----

/// A program `lyk deploy` can hand the theory to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Target {
    Keymux,
    Typeladder,
}

/// How one target was chosen on the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    /// Deployed to when detected (the default)
    Auto,
    /// `--only`: deployed to whether or not it is detected
    Force,
    /// `--no-keymux`, `--no-typeladder`, or left out by `--only`
    Off,
}

/// The targets as the command line chose them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Select {
    pub keymux: Pick,
    pub typeladder: Pick,
}

impl Select {
    pub fn from_flags(only: Option<Target>, no_keymux: bool, no_typeladder: bool) -> Self {
        let pick = |target: Target, off: bool| match only {
            Some(only) if only == target => Pick::Force,
            Some(_) => Pick::Off,
            None if off => Pick::Off,
            None => Pick::Auto,
        };
        Self { keymux: pick(Target::Keymux, no_keymux), typeladder: pick(Target::Typeladder, no_typeladder) }
    }
}

/// What detection looks at, injectable for tests.
pub struct Env<'a> {
    /// The value of `$PATH`
    pub path_var: &'a OsStr,
    /// Whether the package manager has this package installed (`pacman -Q`)
    pub probe: &'a dyn Fn(&str) -> bool,
    /// typeladder's config folder, if it can be worked out
    pub typeladder_dir: Option<PathBuf>,
}

/// `pacman -Q name` succeeding. False when there is no pacman.
pub fn pacman_has(name: &str) -> bool {
    use std::process::Stdio;
    Command::new("pacman")
        .args(["-Q", name])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Whether `name` is installed: an executable of that name in `path_var`, or `probe` says the
/// package is.
pub fn installed(name: &str, path_var: &OsStr, probe: &dyn Fn(&str) -> bool) -> bool {
    find_on_path(name, path_var).is_some() || probe(name)
}

/// typeladder's config folder: `$TYPELADDER_CONFIG_DIR`, else `$XDG_CONFIG_HOME/typeladder`, else
/// `$HOME/.config/typeladder`. An unset or empty variable counts as unset.
pub fn typeladder_config_dir(own: Option<&OsStr>, xdg: Option<&OsStr>, home: Option<&OsStr>) -> Option<PathBuf> {
    let set = |v: Option<&OsStr>| v.filter(|v| !v.is_empty()).map(PathBuf::from);
    set(own).or_else(|| set(xdg).map(|d| d.join("typeladder"))).or_else(|| set(home).map(|h| h.join(".config").join("typeladder")))
}

/// [`typeladder_config_dir`] from the environment.
pub fn typeladder_config_dir_from_env() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name);
    typeladder_config_dir(var("TYPELADDER_CONFIG_DIR").as_deref(), var("XDG_CONFIG_HOME").as_deref(), var("HOME").as_deref())
}

impl Env<'_> {
    /// Whether keymux is installed.
    pub fn has_keymux(&self) -> bool {
        installed("keymux", self.path_var, self.probe)
    }

    /// Whether typeladder is installed or already has a config folder.
    pub fn has_typeladder(&self) -> bool {
        installed("typeladder", self.path_var, self.probe) || self.typeladder_dir.as_ref().is_some_and(|d| d.is_dir())
    }
}

/// Whether a target is deployed to: never when off, always when forced, else when detected.
fn wants(pick: Pick, detected: impl FnOnce() -> bool) -> bool {
    match pick {
        Pick::Off => false,
        Pick::Force => true,
        Pick::Auto => detected(),
    }
}

/// The file typeladder reads, inside its config folder.
pub const CODES_DIR: &str = "dictionaries";
pub const CODES_FILE: &str = "lyk.codes.json";
const CODES_PREVIOUS: &str = "lyk.codes.json.previous";

/// The skip line for keymux.
pub const KEYMUX_SKIPPED: &str = "keymux: not installed, skipped (the dictionary is built in out/lyk)";

/// Whether `text` is something to type as words: letters, apostrophes and hyphens in words
/// separated by single spaces. Commands, braces, punctuation and digits are not.
fn is_plain_text(text: &str) -> bool {
    !text.is_empty()
        && text.split(' ').all(|word| {
            word.chars().any(char::is_alphabetic) && word.chars().all(|c| c.is_alphabetic() || c == '\'' || c == '-')
        })
}

/// Each plain word or phrase to the outline `lyk lookup` lists first for it (the deployed
/// dictionary's cheapest, `generate::cheapest_deployed`), sorted by text.
pub fn codes(entries: &[crate::generate::Entry]) -> std::collections::BTreeMap<String, String> {
    crate::generate::cheapest_deployed(entries)
        .into_iter()
        .map(|(word, outline)| (crate::generate::written(word), crate::stroke::render_outline(outline)))
        .filter(|(text, _)| is_plain_text(text))
        .collect()
}

/// Write `bytes` to `dest` in one rename.
fn write_atomic(dest: &Path, bytes: &[u8]) -> Result<()> {
    let name = dest.file_name().unwrap_or_default().to_string_lossy();
    let temp = dest.with_file_name(format!(".{name}.lyk-tmp"));
    fs::write(&temp, bytes).with_context(|| temp.display().to_string())?;
    fs::rename(&temp, dest)
        .inspect_err(|_| {
            let _ = fs::remove_file(&temp);
        })
        .with_context(|| dest.display().to_string())
}

/// Write the export into `config_dir/dictionaries/lyk.codes.json`, keeping the file it replaces
/// as `lyk.codes.json.previous` (not when the content is the same, so a repeat deploy keeps the
/// real previous). Returns the file written.
pub fn export_codes(config_dir: &Path, codes: &std::collections::BTreeMap<String, String>) -> Result<PathBuf> {
    let dir = config_dir.join(CODES_DIR);
    fs::create_dir_all(&dir).with_context(|| dir.display().to_string())?;
    let live = dir.join(CODES_FILE);
    let mut text = serde_json::to_string_pretty(codes)?;
    text.push('\n');
    match fs::read(&live) {
        Ok(old) if old == text.as_bytes() => return Ok(live),
        Ok(old) => write_atomic(&dir.join(CODES_PREVIOUS), &old)?,
        Err(_) => {}
    }
    write_atomic(&live, text.as_bytes())?;
    Ok(live)
}

/// Put `lyk.codes.json.previous` back as `lyk.codes.json`. The previous file stays.
pub fn rollback_codes(config_dir: &Path) -> Result<PathBuf> {
    let dir = config_dir.join(CODES_DIR);
    let previous = dir.join(CODES_PREVIOUS);
    let old = fs::read(&previous).with_context(|| format!("nothing to roll back to: {} is missing", previous.display()))?;
    let live = dir.join(CODES_FILE);
    write_atomic(&live, &old)?;
    Ok(live)
}

/// Deploy to every selected, detected target. `keymux` installs the keymux folder and `typeladder`
/// writes the export into the config folder it is given and returns the entry count. A target
/// that is not detected is skipped with one line, never an error.
pub fn run(
    select: Select,
    env: &Env<'_>,
    keymux: impl FnOnce() -> Result<()>,
    typeladder: impl FnOnce(&Path) -> Result<usize>,
) -> Result<()> {
    if wants(select.keymux, || env.has_keymux()) {
        keymux()?;
    } else if select.keymux == Pick::Auto {
        println!("{KEYMUX_SKIPPED}");
    }
    if wants(select.typeladder, || env.has_typeladder()) {
        let Some(dir) = &env.typeladder_dir else {
            bail!("typeladder: no config folder ($TYPELADDER_CONFIG_DIR, $XDG_CONFIG_HOME and $HOME are all unset)");
        };
        let count = typeladder(dir)?;
        println!("typeladder: wrote {count} entries to {}", dir.join(CODES_DIR).join(CODES_FILE).display());
    } else if select.typeladder == Pick::Auto {
        println!("typeladder: not installed, skipped");
    }
    Ok(())
}

/// `--rollback` over the selected, detected targets. A target with nothing to roll back is
/// reported; it is an error only when no selected target could be rolled back.
pub fn run_rollback(select: Select, env: &Env<'_>, keymux: impl FnOnce() -> Result<()>) -> Result<()> {
    let mut failures = Vec::new();
    let mut done = 0;
    if wants(select.keymux, || env.has_keymux()) {
        match keymux() {
            Ok(()) => done += 1,
            Err(e) => failures.push(format!("keymux: {e:#}")),
        }
    }
    if wants(select.typeladder, || env.has_typeladder()) {
        match env.typeladder_dir.as_deref().ok_or_else(|| anyhow::anyhow!("no config folder")).and_then(rollback_codes) {
            Ok(live) => {
                println!("typeladder: rolled back {}", live.display());
                done += 1;
            }
            Err(e) => failures.push(format!("typeladder: {e:#}")),
        }
    }
    for failure in &failures {
        println!("{failure}");
    }
    if done == 0 {
        if failures.is_empty() {
            println!("nothing to roll back: no target installed");
            return Ok(());
        }
        bail!("nothing rolled back");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lyk-deploy-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A `keymux` on a PATH of its own that prints `json` and exits with `code`.
    fn stub(root: &Path, json: &str, code: i32) -> PathBuf {
        let bin = root.join("bin");
        fs::create_dir_all(&bin).unwrap();
        let script = bin.join("keymux");
        fs::write(&script, format!("#!/bin/sh\ncat <<'EOF'\n{json}\nEOF\nexit {code}\n")).unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        bin
    }

    fn items(files: &[(&'static str, &'static str)]) -> Vec<Item> {
        files.iter().map(|(n, t)| Item { name: (*n).to_owned(), source: Source::Text(t) }).collect()
    }

    fn read(dir: &Path, name: &str) -> String {
        fs::read_to_string(dir.join(name)).unwrap_or_default()
    }

    const CLEAN: &str = r#"{"ok":true,"strict":false,"errors":0,"warnings":0,"info":2,"sets":[],"files":1,"entries":5,"findings":[]}"#;
    const WARN: &str = r#"{"ok":true,"strict":false,"errors":0,"warnings":1,"info":0,"sets":[],"files":1,"entries":5,"findings":[{"file":"/x/a.json","outline":"KAT","severity":"warning","kind":"unknown_meta","message":"not a meta","suggestion":null,"line":null,"column":null,"count":1,"layers":["steno"]}]}"#;
    const ERR: &str = r#"{"ok":false,"strict":false,"errors":1,"warnings":0,"info":0,"sets":[],"files":1,"entries":5,"findings":[{"file":"/x/a.json","outline":null,"severity":"error","kind":"unparsable_file","message":"not a JSON object","suggestion":null,"line":1,"column":1,"count":1,"layers":[]}]}"#;

    fn live_with_old(root: &Path) -> PathBuf {
        let live = root.join("steno").join("lyk");
        fs::create_dir_all(&live).unwrap();
        fs::write(live.join("a.json"), "{\"old\":1}").unwrap();
        fs::write(live.join("gone.json"), "{}").unwrap();
        live
    }

    #[test]
    fn a_report_is_read_from_the_validators_json() {
        let report = Report::parse(WARN).unwrap();
        assert_eq!((report.errors, report.warnings, report.info, report.files, report.entries), (0, 1, 0, 1, 5));
        assert_eq!(report.findings[0].outline.as_deref(), Some("KAT"));
        let summary = report.summary();
        assert!(summary.contains("1 warning(s)") && summary.contains("a.json KAT: not a meta"), "{summary}");
        assert!(Report::parse("not json").is_err());
        assert!(Report::parse("{}").is_err());
    }

    #[test]
    fn find_on_path_needs_an_executable() {
        let root = scratch("path");
        let bin = stub(&root, CLEAN, 0);
        assert_eq!(find_on_path("keymux", bin.as_os_str()), Some(bin.join("keymux")));
        assert_eq!(find_on_path("nothing-here", bin.as_os_str()), None);
        fs::set_permissions(bin.join("keymux"), fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(find_on_path("keymux", bin.as_os_str()), None);
    }

    #[test]
    fn clean_files_are_swapped_in_and_the_old_ones_kept() {
        let root = scratch("clean");
        let live = live_with_old(&root);
        let bin = stub(&root, CLEAN, 0);
        install(&items(&[("a.json", "{\"new\":1}"), ("b.json", "{}")]), &live, Validate::Errors, bin.as_os_str(), &|dir| {
            fs::remove_file(dir.join("gone.json")).map_err(Into::into)
        })
        .unwrap();
        assert_eq!(read(&live, "a.json"), "{\"new\":1}");
        assert!(live.join("b.json").exists() && !live.join("gone.json").exists());
        let previous = root.join("steno").join(".lyk-previous");
        assert_eq!(read(&previous, "a.json"), "{\"old\":1}");
        assert!(previous.join("gone.json").exists() && !previous.join("b.json").exists());
        assert!(!root.join("steno").join(".lyk-staging").exists(), "the staging folder is removed");
    }

    #[test]
    fn an_error_stops_the_deploy_and_writes_nothing() {
        let root = scratch("error");
        let live = live_with_old(&root);
        let bin = stub(&root, ERR, 1);
        let error = install(&items(&[("a.json", "{\"new\":1}")]), &live, Validate::Errors, bin.as_os_str(), &|_: &Path| Ok(()))
            .unwrap_err();
        assert!(format!("{error}").contains("1 validation error"), "{error}");
        assert_eq!(read(&live, "a.json"), "{\"old\":1}");
        assert!(live.join("gone.json").exists());
        assert!(!root.join("steno").join(".lyk-staging").exists());
        assert!(!root.join("steno").join(".lyk-previous").exists(), "the kept files are not touched either");
    }

    #[test]
    fn warnings_pass_unless_strict() {
        let root = scratch("warn");
        let live = live_with_old(&root);
        let bin = stub(&root, WARN, 0);
        let new = items(&[("a.json", "{\"new\":1}")]);
        let error = install(&new, &live, Validate::Strict, bin.as_os_str(), &|_: &Path| Ok(())).unwrap_err();
        assert!(format!("{error}").contains("warning"), "{error}");
        assert_eq!(read(&live, "a.json"), "{\"old\":1}");
        install(&new, &live, Validate::Errors, bin.as_os_str(), &|_: &Path| Ok(())).unwrap();
        assert_eq!(read(&live, "a.json"), "{\"new\":1}");
    }

    #[test]
    fn no_validator_on_path_skips_the_check() {
        let root = scratch("none");
        let live = live_with_old(&root);
        let empty = root.join("empty-bin");
        fs::create_dir_all(&empty).unwrap();
        install(&items(&[("a.json", "{\"new\":1}")]), &live, Validate::Strict, empty.as_os_str(), &|_: &Path| Ok(())).unwrap();
        assert_eq!(read(&live, "a.json"), "{\"new\":1}");
    }

    #[test]
    fn a_validator_that_cannot_run_skips_the_check() {
        let root = scratch("exit2");
        let bin = stub(&root, "", 2);
        let files = [root.join("a.json")];
        match validate(&bin.join("keymux"), &files, false) {
            Check::Skipped(why) => assert!(why.contains("could not run"), "{why}"),
            Check::Ran(report) => panic!("ran: {report:?}"),
        }
    }

    #[test]
    fn no_validate_never_calls_the_validator() {
        let root = scratch("skip");
        let live = live_with_old(&root);
        let bin = stub(&root, ERR, 1);
        install(&items(&[("a.json", "{\"new\":1}")]), &live, Validate::Skip, bin.as_os_str(), &|_: &Path| Ok(())).unwrap();
        assert_eq!(read(&live, "a.json"), "{\"new\":1}");
    }

    #[test]
    fn a_failure_after_the_swap_started_puts_the_old_files_back() {
        let root = scratch("midway");
        let live = live_with_old(&root);
        let empty = root.join("empty-bin");
        fs::create_dir_all(&empty).unwrap();
        let error = install(
            &items(&[("a.json", "{\"new\":1}"), ("b.json", "{}")]),
            &live,
            Validate::Skip,
            empty.as_os_str(),
            &|dir| {
                fs::remove_file(dir.join("gone.json"))?;
                bail!("disk full")
            },
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("previous files are back"), "{error:#}");
        assert_eq!(read(&live, "a.json"), "{\"old\":1}");
        assert!(live.join("gone.json").exists() && !live.join("b.json").exists());
    }

    #[test]
    fn rollback_restores_the_files_the_last_deploy_replaced() {
        let root = scratch("rollback");
        let live = live_with_old(&root);
        let empty = root.join("empty-bin");
        fs::create_dir_all(&empty).unwrap();
        install(&items(&[("a.json", "{\"new\":1}"), ("b.json", "{}")]), &live, Validate::Skip, empty.as_os_str(), &|dir| {
            fs::remove_file(dir.join("gone.json")).map_err(Into::into)
        })
        .unwrap();
        rollback(&live).unwrap();
        assert_eq!(read(&live, "a.json"), "{\"old\":1}");
        assert!(live.join("gone.json").exists() && !live.join("b.json").exists());
        let nothing = root.join("steno").join("other");
        assert!(rollback(&nothing).is_err(), "no previous set to restore");
    }

    // ---- targets ----

    fn fake(root: &Path, name: &str) -> PathBuf {
        let bin = root.join(format!("bin-{name}"));
        fs::create_dir_all(&bin).unwrap();
        let script = bin.join(name);
        fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        bin
    }

    fn entry(word: &str, outline: &str, origin: crate::generate::Origin) -> crate::generate::Entry {
        crate::generate::Entry { word: word.to_owned(), outline: crate::stroke::parse_outline(outline).unwrap(), origin }
    }

    #[test]
    fn detection_needs_the_binary_on_path_or_the_package() {
        let root = scratch("detect");
        let with = fake(&root, "keymux");
        let without = root.join("empty");
        fs::create_dir_all(&without).unwrap();
        let yes = |_: &str| true;
        let no = |_: &str| false;
        assert!(installed("keymux", with.as_os_str(), &no));
        assert!(!installed("keymux", without.as_os_str(), &no));
        assert!(installed("keymux", without.as_os_str(), &yes));
        let probe_only = |name: &str| name == "typeladder";
        assert!(!installed("keymux", without.as_os_str(), &probe_only));
        assert!(installed("typeladder", without.as_os_str(), &probe_only));
    }

    #[test]
    fn typeladder_is_also_detected_by_its_config_folder() {
        let root = scratch("detect-dir");
        let dir = root.join("typeladder");
        let no = |_: &str| false;
        let env = |dir: &Path| Env { path_var: OsStr::new(""), probe: &no, typeladder_dir: Some(dir.to_path_buf()) };
        assert!(!env(&dir).has_typeladder());
        fs::create_dir_all(&dir).unwrap();
        assert!(env(&dir).has_typeladder());
    }

    #[test]
    fn the_typeladder_folder_follows_its_variable_then_xdg_then_home() {
        let s = |t: &'static str| Some(OsStr::new(t));
        assert_eq!(typeladder_config_dir(s("/own"), s("/xdg"), s("/h")), Some(PathBuf::from("/own")));
        assert_eq!(typeladder_config_dir(None, s("/xdg"), s("/h")), Some(PathBuf::from("/xdg/typeladder")));
        assert_eq!(typeladder_config_dir(s(""), s(""), s("/h")), Some(PathBuf::from("/h/.config/typeladder")));
        assert_eq!(typeladder_config_dir(None, None, None), None);
    }

    #[test]
    fn flags_choose_the_targets() {
        let sel = Select::from_flags;
        assert_eq!(sel(None, false, false), Select { keymux: Pick::Auto, typeladder: Pick::Auto });
        assert_eq!(sel(None, true, false), Select { keymux: Pick::Off, typeladder: Pick::Auto });
        assert_eq!(sel(None, false, true), Select { keymux: Pick::Auto, typeladder: Pick::Off });
        assert_eq!(sel(Some(Target::Keymux), false, false), Select { keymux: Pick::Force, typeladder: Pick::Off });
        assert_eq!(sel(Some(Target::Typeladder), false, false), Select { keymux: Pick::Off, typeladder: Pick::Force });
    }

    #[test]
    fn the_export_holds_plain_words_and_phrases_with_their_best_outline() {
        use crate::generate::Origin::{Backup, Brief, Phrase, Rules, Suffix};
        let entries = [
            entry("cat", "KAT", Rules),
            entry("cat", "KA/TKPWAT", Brief),
            entry("cat", "K", Backup),
            entry("zebra", "SEB/RA", Backup),
            entry("i", "EU", Brief),
            entry("it's", "AOT", Rules),
            entry("well-known", "WEL", Rules),
            entry("to the", "TO", Phrase),
            entry("{:tape:toggle}", "TAEP", Rules),
            entry("{^ing}", "-G", Suffix),
            entry("2nd", "TWO", Rules),
            entry("a.m.", "APL", Rules),
            entry("ab  cd", "APB", Rules),
        ];
        let codes = codes(&entries);
        let got: Vec<(&str, &str)> = codes.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        assert_eq!(
            got,
            [("I", "EU"), ("cat", "KAT"), ("it's", "AOT"), ("to the", "TO"), ("well-known", "WEL"), ("zebra", "SEB/RA")]
        );
    }

    fn map(pairs: &[(&str, &str)]) -> std::collections::BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
    }

    #[test]
    fn the_export_is_sorted_atomic_and_keeps_the_previous_file() {
        let root = scratch("export");
        let dir = root.join("typeladder");
        let live = export_codes(&dir, &map(&[("zoo", "ZAO"), ("ant", "ANT")])).unwrap();
        let first = fs::read_to_string(&live).unwrap();
        assert!(first.find("ant").unwrap() < first.find("zoo").unwrap(), "sorted: {first}");
        assert!(first.ends_with("}\n"));
        assert!(!dir.join(CODES_DIR).join(CODES_PREVIOUS).exists());
        // the same content again changes nothing, and does not push the file into `.previous`
        export_codes(&dir, &map(&[("zoo", "ZAO"), ("ant", "ANT")])).unwrap();
        assert!(!dir.join(CODES_DIR).join(CODES_PREVIOUS).exists());
        // new content keeps the old file
        export_codes(&dir, &map(&[("ant", "AN")])).unwrap();
        assert_eq!(fs::read_to_string(dir.join(CODES_DIR).join(CODES_PREVIOUS)).unwrap(), first);
        assert!(fs::read_to_string(&live).unwrap().contains("\"AN\""));
        // no temp file is left, and nothing outside `dictionaries/` is made
        assert_eq!(files_in(&dir.join(CODES_DIR)).unwrap(), [CODES_FILE, CODES_PREVIOUS]);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        // rollback brings the first file back, and keeps the previous
        rollback_codes(&dir).unwrap();
        assert_eq!(fs::read_to_string(&live).unwrap(), first);
        assert!(dir.join(CODES_DIR).join(CODES_PREVIOUS).exists());
    }

    #[test]
    fn rolling_back_the_export_needs_a_previous_file() {
        let root = scratch("export-rollback");
        assert!(rollback_codes(&root.join("none")).is_err());
    }

    #[test]
    fn without_keymux_the_deploy_succeeds_and_makes_nothing_under_keymux() {
        let root = scratch("absent");
        let empty = root.join("empty-bin");
        fs::create_dir_all(&empty).unwrap();
        let no = |_: &str| false;
        let tdir = root.join("typeladder");
        let env = Env { path_var: empty.as_os_str(), probe: &no, typeladder_dir: Some(tdir.clone()) };
        let keymux_home = root.join("config").join("keymux").join("steno").join("lyk");
        let mut keymux_ran = false;
        let mut typeladder_ran = false;
        run(
            Select::from_flags(None, false, false),
            &env,
            || {
                keymux_ran = true;
                install(&items(&[("a.json", "{}")]), &keymux_home, Validate::Skip, empty.as_os_str(), &|_| Ok(()))
            },
            |_| {
                typeladder_ran = true;
                Ok(0)
            },
        )
        .unwrap();
        assert!(!keymux_ran && !typeladder_ran);
        assert!(!root.join("config").exists() && !tdir.exists());
    }

    #[test]
    fn each_detected_target_is_deployed_and_the_other_left_alone() {
        let root = scratch("present");
        let bin = fake(&root, "keymux");
        let no = |_: &str| false;
        let tdir = root.join("typeladder");
        let env = Env { path_var: bin.as_os_str(), probe: &no, typeladder_dir: Some(tdir.clone()) };
        let live = root.join("keymux-lyk");
        let go = |select| {
            run(
                select,
                &env,
                || install(&items(&[("a.json", "{}")]), &live, Validate::Skip, OsStr::new(""), &|_| Ok(())),
                |dir| export_codes(dir, &map(&[("ant", "AN")])).map(|_| 1),
            )
        };
        // keymux is on PATH, typeladder is not: only keymux
        go(Select::from_flags(None, false, false)).unwrap();
        assert!(live.join("a.json").exists() && !tdir.exists());
        // `--only typeladder` deploys it though undetected, and leaves keymux out
        fs::remove_dir_all(&live).unwrap();
        go(Select::from_flags(Some(Target::Typeladder), false, false)).unwrap();
        assert!(!live.exists() && tdir.join(CODES_DIR).join(CODES_FILE).exists());
        // `--no-keymux`
        go(Select::from_flags(None, true, false)).unwrap();
        assert!(!live.exists());
    }

    #[test]
    fn a_failed_keymux_deploy_is_still_an_error() {
        let root = scratch("keymux-fails");
        let bin = fake(&root, "keymux");
        let no = |_: &str| false;
        let env = Env { path_var: bin.as_os_str(), probe: &no, typeladder_dir: None };
        let result = run(Select::from_flags(None, false, true), &env, || bail!("boom"), |_| Ok(0));
        assert!(result.is_err());
    }

    #[test]
    fn rollback_covers_both_targets_and_errors_only_when_none_could() {
        let root = scratch("both-rollback");
        let bin = fake(&root, "keymux");
        let no = |_: &str| false;
        let tdir = root.join("typeladder");
        let env = Env { path_var: bin.as_os_str(), probe: &no, typeladder_dir: Some(tdir.clone()) };
        let all = Select::from_flags(None, false, false);
        // nothing kept anywhere (the typeladder folder does not exist, so it is not detected)
        assert!(run_rollback(all, &env, || bail!("no previous")).is_err());
        export_codes(&tdir, &map(&[("ant", "AN")])).unwrap();
        export_codes(&tdir, &map(&[("ant", "AN"), ("bat", "BAT")])).unwrap();
        // keymux has nothing, typeladder restores: fine
        run_rollback(all, &env, || bail!("no previous")).unwrap();
        let text = fs::read_to_string(tdir.join(CODES_DIR).join(CODES_FILE)).unwrap();
        assert!(!text.contains("bat"));
    }
}

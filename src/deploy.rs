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
}

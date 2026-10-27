//! Writes a plan to disk and lets cargo edit the manifests.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::plan::{manifest_commands, Plan, Status};
use crate::rewrite::rewrite;

pub struct Options<'a> {
    /// Directory cargo runs the final check in.
    pub root: &'a Path,
    pub manifest_path: Option<&'a Path>,
    pub source_only: bool,
    pub verify: bool,
}

pub struct Outcome {
    pub swaps: usize,
    pub files: usize,
    pub verified: Option<bool>,
}

pub fn apply(plan: &Plan, opts: &Options<'_>) -> Result<Outcome, String> {
    let mut outcome = Outcome {
        swaps: 0,
        files: 0,
        verified: None,
    };
    // Several swaps can touch one file. Each rewrite starts from the text the
    // previous one left, so the results compose instead of overwriting.
    let mut texts: BTreeMap<PathBuf, String> = BTreeMap::new();
    let mut changed: BTreeSet<PathBuf> = BTreeSet::new();
    for item in plan.items.iter().filter(|i| i.status == Status::Ready) {
        for f in &item.files {
            if f.rewrite.changes.is_empty() {
                continue;
            }
            let current = match texts.get(&f.path) {
                Some(t) => t.clone(),
                None => fs::read_to_string(&f.path)
                    .map_err(|e| format!("reading {}: {e}", f.path.display()))?,
            };
            let next = rewrite(&current, item.rule);
            if next.source != current {
                changed.insert(f.path.clone());
            }
            texts.insert(f.path.clone(), next.source);
        }
        outcome.swaps += 1;
    }
    for path in &changed {
        fs::write(path, &texts[path]).map_err(|e| format!("writing {}: {e}", path.display()))?;
        outcome.files += 1;
    }
    if !opts.source_only {
        for item in plan.items.iter().filter(|i| i.status == Status::Ready) {
            for args in manifest_commands(item) {
                run_cargo(&item.package_dir, &args)?;
            }
        }
    }
    if opts.verify && outcome.swaps > 0 && !opts.source_only {
        let mut args = vec![
            "check".to_string(),
            "--workspace".into(),
            "--all-targets".into(),
        ];
        if let Some(p) = opts.manifest_path {
            args.push("--manifest-path".into());
            args.push(p.display().to_string());
        }
        outcome.verified = Some(
            Command::new("cargo")
                .args(&args)
                .current_dir(opts.root)
                .status()
                .map_err(|e| format!("could not run cargo check: {e}"))?
                .success(),
        );
    }
    Ok(outcome)
}

fn run_cargo(dir: &Path, args: &[String]) -> Result<(), String> {
    let output = Command::new("cargo")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| format!("could not run cargo: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "`cargo {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// `Ok(true)` when the working tree is clean, `Ok(false)` when it has changes
/// or `dir` is not a git repository.
pub fn git_is_clean(dir: &Path) -> Result<bool, String> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir)
        .output();
    match output {
        Ok(o) if o.status.success() => Ok(o.stdout.is_empty()),
        _ => Ok(false),
    }
}

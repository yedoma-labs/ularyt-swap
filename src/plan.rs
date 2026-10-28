//! Turns the project layout and the rule catalogue into a list of swaps.

use std::fs;
use std::path::{Path, PathBuf};

use crate::project::{DepKind, Dependency, Package};
use crate::rewrite::{rewrite, Rewrite};
use crate::rules::{Action, Rule, RULES};

/// Rust version in which `std::io::IsTerminal` became stable.
const ISTERMINAL_MSRV: (u32, u32) = (1, 70);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Source and manifest can be changed automatically.
    Ready,
    /// Something needs a human; nothing is changed for this swap.
    Blocked(Vec<String>),
    /// The crate is a dependency but the source never names it.
    NoUsage,
    /// The successor's API differs; only reported.
    Manual,
}

#[derive(Debug, Clone)]
pub struct FileRewrite {
    pub path: PathBuf,
    pub rewrite: Rewrite,
}

#[derive(Debug, Clone)]
pub struct Item {
    pub package: String,
    pub package_dir: PathBuf,
    pub rule: &'static Rule,
    pub dependencies: Vec<Dependency>,
    /// Only files with at least one use of the old crate.
    pub files: Vec<FileRewrite>,
    pub status: Status,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub packages: usize,
    pub items: Vec<Item>,
}

impl Plan {
    pub fn count(&self, pick: impl Fn(&Status) -> bool) -> usize {
        self.items.iter().filter(|i| pick(&i.status)).count()
    }
}

pub fn build(packages: &[Package]) -> Result<Plan, String> {
    let mut plan = Plan {
        packages: packages.len(),
        ..Plan::default()
    };
    for package in packages {
        let mut sources: Option<Vec<(PathBuf, String)>> = None;
        for rule in RULES {
            let deps: Vec<Dependency> = package
                .dependencies
                .iter()
                .filter(|d| d.name == rule.old)
                .cloned()
                .collect();
            if deps.is_empty() {
                continue;
            }
            let files = match &sources {
                Some(s) => s,
                None => sources.insert(read_sources(&package.dir)?),
            };
            plan.items.push(item(package, rule, deps, files));
        }
    }
    Ok(plan)
}

fn item(
    package: &Package,
    rule: &'static Rule,
    deps: Vec<Dependency>,
    sources: &[(PathBuf, String)],
) -> Item {
    let mut files = Vec::new();
    for (path, text) in sources {
        let r = rewrite(text, rule);
        if r.uses > 0 || r.comment_mentions > 0 {
            files.push(FileRewrite {
                path: path.clone(),
                rewrite: r,
            });
        }
    }
    let uses: usize = files.iter().map(|f| f.rewrite.uses).sum();
    let mut warnings = Vec::new();
    let mentions: usize = files.iter().map(|f| f.rewrite.comment_mentions).sum();
    if mentions > 0 && rule.is_automatic() {
        warnings.push(format!(
            "{mentions} mention(s) in comments, doc comments or strings are not changed (doc tests may need a look)"
        ));
    }
    if rule.is_automatic() {
        warnings.push(rule.note.to_string());
    }
    if deps.iter().any(|d| !d.features.is_empty()) && matches!(rule.action, Action::Rename { .. }) {
        warnings.push(format!(
            "features enabled on {} ({}) are not carried over to the successor",
            rule.old,
            deps.iter()
                .flat_map(|d| d.features.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    let status = if !rule.is_automatic() {
        Status::Manual
    } else {
        let mut blockers = Vec::new();
        if deps.iter().any(|d| d.renamed) {
            blockers
                .push("the dependency is renamed with `package = ...` in Cargo.toml".to_string());
        }
        if rule.action == Action::Atty {
            if let Some(msrv) = package.msrv.filter(|m| *m < ISTERMINAL_MSRV) {
                blockers.push(format!(
                    "rust-version {}.{} is older than 1.70, which IsTerminal needs",
                    msrv.0, msrv.1
                ));
            }
        }
        for f in &files {
            for u in &f.rewrite.unresolved {
                blockers.push(format!(
                    "{}:{}: {} ({})",
                    short(&package.dir, &f.path),
                    u.line,
                    u.reason,
                    u.text
                ));
            }
        }
        if !blockers.is_empty() {
            Status::Blocked(blockers)
        } else if uses == 0 {
            Status::NoUsage
        } else {
            Status::Ready
        }
    };
    Item {
        package: package.name.clone(),
        package_dir: package.dir.clone(),
        rule,
        dependencies: deps,
        files,
        status,
        warnings,
    }
}

/// `cargo` arguments that replace the dependency, one command per entry.
pub fn manifest_commands(item: &Item) -> Vec<Vec<String>> {
    let mut commands = Vec::new();
    for dep in &item.dependencies {
        let mut scope = Vec::new();
        match dep.kind {
            DepKind::Dev => scope.push("--dev".to_string()),
            DepKind::Build => scope.push("--build".to_string()),
            DepKind::Normal => {}
        }
        if let Some(t) = &dep.target {
            scope.push("--target".to_string());
            scope.push(t.clone());
        }
        let mut remove = vec!["remove".to_string(), "-p".to_string(), item.package.clone()];
        remove.extend(scope.iter().cloned());
        remove.push(item.rule.old.to_string());
        commands.push(remove);
        if let Action::Rename { new_crate, .. } = item.rule.action {
            let mut add = vec!["add".to_string(), "-p".to_string(), item.package.clone()];
            add.extend(scope.iter().cloned());
            add.push(new_crate.to_string());
            if dep.optional {
                add.push("--optional".to_string());
            }
            commands.push(add);
        }
    }
    commands
}

/// Relative display path with `/` separators on every platform, so reports are
/// identical on Windows and Unix.
pub fn short(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .display()
        .to_string()
        .replace('\\', "/")
}

fn read_sources(dir: &Path) -> Result<Vec<(PathBuf, String)>, String> {
    let mut out = Vec::new();
    walk(dir, dir, &mut out).map_err(|e| format!("reading {}: {e}", dir.display()))?;
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<(PathBuf, String)>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let kind = entry.file_type()?;
        if kind.is_dir() {
            let nested_package = path != root && path.join("Cargo.toml").exists();
            if name == "target" || name.starts_with('.') || nested_package {
                continue;
            }
            walk(root, &path, out)?;
        } else if kind.is_file() && name.ends_with(".rs") {
            if let Ok(text) = fs::read_to_string(&path) {
                out.push((path, text));
            }
        }
    }
    Ok(())
}

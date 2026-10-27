//! Reads the workspace layout from `cargo metadata`.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepKind {
    Normal,
    Dev,
    Build,
}

impl DepKind {
    fn from_metadata(kind: Option<&str>) -> Self {
        match kind {
            Some("dev") => Self::Dev,
            Some("build") => Self::Build,
            _ => Self::Normal,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "dependencies",
            Self::Dev => "dev-dependencies",
            Self::Build => "build-dependencies",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Dependency {
    pub name: String,
    pub kind: DepKind,
    /// `cfg(...)` or target triple of a `[target.X.dependencies]` table.
    pub target: Option<String>,
    pub optional: bool,
    pub features: Vec<String>,
    /// The dependency is renamed with `package = "..."` in Cargo.toml.
    pub renamed: bool,
}

#[derive(Debug, Clone)]
pub struct Package {
    pub name: String,
    pub dir: PathBuf,
    /// `rust-version` as (major, minor).
    pub msrv: Option<(u32, u32)>,
    pub dependencies: Vec<Dependency>,
}

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<RawPackage>,
}

#[derive(Deserialize)]
struct RawPackage {
    name: String,
    manifest_path: PathBuf,
    rust_version: Option<String>,
    dependencies: Vec<RawDependency>,
}

#[derive(Deserialize)]
struct RawDependency {
    name: String,
    kind: Option<String>,
    target: Option<String>,
    optional: bool,
    features: Vec<String>,
    rename: Option<String>,
}

/// Packages of the workspace (or the single package) at `manifest_path`, or in
/// the current directory when it is `None`.
pub fn load(manifest_path: Option<&Path>) -> Result<Vec<Package>, String> {
    let mut cmd = Command::new("cargo");
    cmd.args(["metadata", "--no-deps", "--format-version", "1"]);
    if let Some(p) = manifest_path {
        cmd.arg("--manifest-path").arg(p);
    }
    let output = cmd
        .output()
        .map_err(|e| format!("could not run cargo: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let meta: Metadata = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("unexpected cargo metadata output: {e}"))?;
    Ok(meta.packages.into_iter().map(Package::from_raw).collect())
}

impl Package {
    fn from_raw(raw: RawPackage) -> Self {
        Self {
            name: raw.name,
            dir: raw
                .manifest_path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default(),
            msrv: raw.rust_version.as_deref().and_then(parse_version),
            dependencies: raw
                .dependencies
                .into_iter()
                .map(|d| Dependency {
                    name: d.name,
                    kind: DepKind::from_metadata(d.kind.as_deref()),
                    target: d.target,
                    optional: d.optional,
                    features: d.features,
                    renamed: d.rename.is_some(),
                })
                .collect(),
        }
    }
}

fn parse_version(v: &str) -> Option<(u32, u32)> {
    let mut parts = v.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().map_or(Some(0), |m| m.parse().ok())?;
    Some((major, minor))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rust_versions() {
        assert_eq!(parse_version("1.70"), Some((1, 70)));
        assert_eq!(parse_version("1.63.0"), Some((1, 63)));
        assert_eq!(parse_version("1"), Some((1, 0)));
        assert_eq!(parse_version("x"), None);
    }
}

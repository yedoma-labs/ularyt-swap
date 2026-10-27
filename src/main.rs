#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;

use ularyt::apply::{apply, git_is_clean, Options};
use ularyt::plan::{self, Status};
use ularyt::{project, report};

const HELP: &str = "\
cargo ularyt - swap unmaintained dependencies for maintained successors

USAGE:
    cargo ularyt [OPTIONS]

By default nothing is changed: the swaps are listed as a diff.

OPTIONS:
    --apply                 write the changes (source files and manifests)
    --check                 exit with 1 when any swap or manual finding exists
    --manifest-path <PATH>  Cargo.toml of the workspace or package
    --allow-dirty           apply even if the git working tree has changes
    --source-only           with --apply: rewrite source only, leave manifests
    --only <CRATE>          only handle this unmaintained dependency (e.g. atty)
    --no-verify             with --apply: skip the final `cargo check`
    --json                  print the plan as JSON (for CI); not with --apply
    -h, --help              show this help
    -V, --version           show the version
";

#[derive(Default)]
struct Args {
    apply: bool,
    check: bool,
    allow_dirty: bool,
    source_only: bool,
    no_verify: bool,
    json: bool,
    manifest_path: Option<PathBuf>,
    only: Option<String>,
}

fn parse() -> Result<Option<Args>, String> {
    let mut args = Args::default();
    let mut it = std::env::args().skip(1).peekable();
    // `cargo ularyt` passes the subcommand name as the first argument.
    if it.peek().map(String::as_str) == Some("ularyt") {
        it.next();
    }
    while let Some(a) = it.next() {
        match a.as_str() {
            "--apply" => args.apply = true,
            "--check" => args.check = true,
            "--allow-dirty" => args.allow_dirty = true,
            "--source-only" => args.source_only = true,
            "--no-verify" => args.no_verify = true,
            "--json" => args.json = true,
            "--only" => {
                let v = it.next().ok_or("--only needs a crate name")?;
                args.only = Some(v);
            }
            "--manifest-path" => {
                let v = it.next().ok_or("--manifest-path needs a value")?;
                args.manifest_path = Some(PathBuf::from(v));
            }
            "-h" | "--help" => {
                print!("{HELP}");
                return Ok(None);
            }
            "-V" | "--version" => {
                println!("cargo-ularyt {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            other => return Err(format!("unknown argument `{other}` (see --help)")),
        }
    }
    Ok(Some(args))
}

fn run() -> Result<ExitCode, String> {
    let Some(args) = parse()? else {
        return Ok(ExitCode::SUCCESS);
    };
    if args.json && args.apply {
        return Err("--json cannot be combined with --apply".into());
    }
    let packages = project::load(args.manifest_path.as_deref())?;
    let mut plan = plan::build(&packages)?;
    if let Some(name) = &args.only {
        plan.items.retain(|i| i.rule.old == name);
        if plan.items.is_empty() {
            return Err(format!(
                "`{name}` is not in the catalogue or not a dependency"
            ));
        }
    }
    if args.json {
        println!("{}", report::json(&plan));
    } else {
        print!("{}", report::render(&plan));
    }

    if args.apply {
        let root = match &args.manifest_path {
            Some(p) => p
                .parent()
                .filter(|d| !d.as_os_str().is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(".")),
            None => PathBuf::from("."),
        };
        if !args.allow_dirty && !git_is_clean(&root)? {
            return Err(
                "refusing to apply: the git working tree has changes or this is not a git \
                 repository (commit first, or pass --allow-dirty)"
                    .into(),
            );
        }
        let outcome = apply(
            &plan,
            &Options {
                root: &root,
                manifest_path: args.manifest_path.as_deref(),
                source_only: args.source_only,
                verify: !args.no_verify,
            },
        )?;
        println!(
            "\napplied {} swap(s), {} file(s) rewritten",
            outcome.swaps, outcome.files
        );
        match outcome.verified {
            Some(true) => println!("cargo check passed"),
            Some(false) => {
                println!("cargo check FAILED - review the changes (git diff) before committing");
                return Ok(ExitCode::from(2));
            }
            None => {}
        }
        return Ok(ExitCode::SUCCESS);
    }

    let findings = plan.items.iter().any(|i| i.status != Status::NoUsage);
    if args.check && findings {
        return Ok(ExitCode::from(1));
    }
    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}

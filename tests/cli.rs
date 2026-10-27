//! Runs the real binary against throw-away projects. `cargo metadata --no-deps`
//! needs no network, so these tests stay offline; manifest edits (cargo add /
//! remove) are covered by `--source-only` here and by the end-to-end run in the
//! README.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct Project(PathBuf);

impl Project {
    fn new(deps: &str, lib: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "ularyt-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            format!(
                "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n{deps}\n"
            ),
        )
        .unwrap();
        fs::write(dir.join("src/lib.rs"), lib).unwrap();
        Self(dir)
    }

    fn lib(&self) -> String {
        fs::read_to_string(self.0.join("src/lib.rs")).unwrap()
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_cargo-ularyt"))
            .args(args)
            .current_dir(&self.0)
            .output()
            .unwrap()
    }

    fn git(&self, args: &[&str]) {
        let ok = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .current_dir(&self.0)
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn dry_run_lists_the_swap_and_changes_nothing() {
    let src = "use instant::Instant;\npub fn t() -> Instant { Instant::now() }\n";
    let p = Project::new("instant = \"0.1\"", src);
    let out = p.run(&[]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("instant -> web-time"), "{text}");
    assert!(text.contains("[ready]"), "{text}");
    assert!(text.contains("- use instant::Instant;"), "{text}");
    assert!(text.contains("+ use web_time::Instant;"), "{text}");
    assert!(text.contains("cargo add -p demo web-time"), "{text}");
    assert_eq!(p.lib(), src);
}

#[test]
fn source_only_apply_rewrites_the_source() {
    let p = Project::new(
        "instant = \"0.1\"\natty = \"0.2\"",
        "use atty::Stream;\nuse instant::Instant;\npub fn t() -> (Instant, bool) {\n    (Instant::now(), atty::is(Stream::Stdout))\n}\n",
    );
    let out = p.run(&["--apply", "--source-only", "--allow-dirty"]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    let lib = p.lib();
    assert!(lib.contains("use web_time::Instant;"), "{lib}");
    assert!(lib.contains("std::io::stdout().is_terminal()"), "{lib}");
    assert!(lib.contains("use std::io::IsTerminal as _;"), "{lib}");
    assert!(!lib.contains("atty"), "{lib}");
    assert!(!lib.contains("instant::"), "{lib}");
    let manifest = fs::read_to_string(p.0.join("Cargo.toml")).unwrap();
    assert!(manifest.contains("instant = \"0.1\""), "manifest untouched");
}

#[test]
fn an_untranslatable_use_blocks_the_whole_swap() {
    let src =
        "use instant::Instant;\npub fn t() -> f64 { let _ = Instant::now(); instant::now() }\n";
    let p = Project::new("instant = \"0.1\"", src);
    let out = p.run(&["--apply", "--source-only", "--allow-dirty"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("[blocked]"), "{text}");
    assert!(text.contains("`now` has no counterpart"), "{text}");
    assert_eq!(p.lib(), src, "nothing is changed for a blocked swap");
}

#[test]
fn manual_findings_are_reported_but_never_rewritten() {
    let src = "pub fn r() { let _ = backoff::ExponentialBackoff::default(); }\n";
    let p = Project::new("backoff = \"0.4\"", src);
    let out = p.run(&["--apply", "--source-only", "--allow-dirty"]);
    let text = stdout(&out);
    assert!(text.contains("backoff -> backon  [manual]"), "{text}");
    assert_eq!(p.lib(), src);
}

#[test]
fn check_mode_fails_when_a_finding_exists() {
    let p = Project::new("paste = \"1\"", "pub fn f() { paste::paste! { } }\n");
    assert_eq!(p.run(&["--check"]).status.code(), Some(1));
    let clean = Project::new("", "pub fn f() {}\n");
    assert_eq!(clean.run(&["--check"]).status.code(), Some(0));
}

#[test]
fn apply_refuses_a_dirty_working_tree() {
    let p = Project::new("paste = \"1\"", "pub fn f() { paste::paste! { } }\n");
    p.git(&["init", "-q"]);
    let out = p.run(&["--apply", "--source-only"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr(&out).contains("refusing to apply"),
        "{}",
        stderr(&out)
    );
    p.git(&["add", "-A"]);
    p.git(&["commit", "-qm", "init"]);
    let out = p.run(&["--apply", "--source-only"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(p.lib().contains("pastey::paste!"));
}

#[test]
fn comments_and_strings_survive() {
    let src = "/// Uses instant::Instant.\npub const S: &str = \"instant::Instant\";\npub fn f() { let _ = instant::Instant::now(); }\n";
    let p = Project::new("instant = \"0.1\"", src);
    p.run(&["--apply", "--source-only", "--allow-dirty"]);
    let lib = p.lib();
    assert!(lib.contains("/// Uses instant::Instant."));
    assert!(lib.contains("\"instant::Instant\""));
    assert!(lib.contains("web_time::Instant::now()"));
}

#[test]
fn only_limits_the_report_to_one_crate() {
    let p = Project::new(
        "instant = \"0.1\"\npaste = \"1\"",
        "pub fn f() { paste::paste! { } let _ = instant::Instant::now(); }\n",
    );
    let out = p.run(&["--only", "paste"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("paste -> pastey"), "{text}");
    assert!(!text.contains("instant -> web-time"), "{text}");
}

#[test]
fn only_rejects_an_unknown_crate() {
    let p = Project::new("paste = \"1\"", "pub fn f() {}\n");
    let out = p.run(&["--only", "serde"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr(&out).contains("not in the catalogue"),
        "{}",
        stderr(&out)
    );
}

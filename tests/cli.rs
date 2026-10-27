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

fn json_of(o: &Output) -> serde_json::Value {
    assert!(o.status.success(), "{}", stderr(o));
    serde_json::from_slice(&o.stdout)
        .unwrap_or_else(|e| panic!("stdout is not JSON: {e}\n{}", stdout(o)))
}

#[test]
fn json_describes_a_ready_swap() {
    let p = Project::new(
        "instant = \"0.1\"",
        "use instant::Instant;\npub fn t() -> Instant { Instant::now() }\n",
    );
    let v = json_of(&p.run(&["--json"]));
    assert_eq!(v["packages"], 1);
    assert_eq!(v["summary"]["ready"], 1);
    assert_eq!(v["summary"]["blocked"], 0);
    let item = &v["items"][0];
    assert_eq!(item["package"], "demo");
    assert_eq!(item["crate"], "instant");
    assert_eq!(item["successor"], "web-time");
    assert_eq!(item["advisory"], "RUSTSEC-2024-0384");
    assert_eq!(item["status"], "ready");
    assert_eq!(item["uses"], 1);
    assert_eq!(item["changes"][0]["file"], "src/lib.rs");
    assert_eq!(item["changes"][0]["line"], 1);
    assert_eq!(item["changes"][0]["before"], "use instant::Instant;");
    assert_eq!(item["changes"][0]["after"], "use web_time::Instant;");
    assert_eq!(
        item["manifest_commands"][1],
        serde_json::json!(["add", "-p", "demo", "web-time"])
    );
    assert_eq!(
        p.lib(),
        "use instant::Instant;\npub fn t() -> Instant { Instant::now() }\n"
    );
}

#[test]
fn json_carries_the_reasons_for_a_blocked_swap() {
    let p = Project::new(
        "instant = \"0.1\"",
        "pub fn t() -> f64 { instant::now() }\n",
    );
    let v = json_of(&p.run(&["--json"]));
    let item = &v["items"][0];
    assert_eq!(item["status"], "blocked");
    assert_eq!(v["summary"]["blocked"], 1);
    assert!(
        item["reasons"][0].as_str().unwrap().contains("`now`"),
        "{v}"
    );
    assert_eq!(item["manifest_commands"], serde_json::json!([]));
}

#[test]
fn json_reports_manual_and_unused_items() {
    let p = Project::new(
        "backoff = \"0.4\"\npaste = \"1\"\nfxhash = \"0.1\"",
        "pub fn r() { let _ = backoff::ExponentialBackoff::default(); }\n",
    );
    let v = json_of(&p.run(&["--json"]));
    assert_eq!(v["summary"]["manual"], 1);
    assert_eq!(v["summary"]["no_usage"], 2);
    let manual = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["crate"] == "backoff")
        .unwrap();
    assert_eq!(manual["status"], "manual");
    assert_eq!(manual["uses"], 1);
    assert_eq!(manual["changes"], serde_json::json!([]));
}

#[test]
fn json_is_empty_when_nothing_is_found() {
    let p = Project::new("", "pub fn f() {}\n");
    let v = json_of(&p.run(&["--json"]));
    assert_eq!(v["items"], serde_json::json!([]));
    assert_eq!(v["summary"]["ready"], 0);
    assert_eq!(v["packages"], 1);
}

#[test]
fn json_respects_only_and_check() {
    let p = Project::new(
        "instant = \"0.1\"\npaste = \"1\"",
        "pub fn f() { paste::paste! { } let _ = instant::Instant::now(); }\n",
    );
    let v = json_of(&p.run(&["--json", "--only", "paste"]));
    assert_eq!(v["items"].as_array().unwrap().len(), 1);
    assert_eq!(v["items"][0]["crate"], "paste");
    assert_eq!(p.run(&["--json", "--check"]).status.code(), Some(1));
    let clean = Project::new("", "pub fn f() {}\n");
    assert_eq!(clean.run(&["--json", "--check"]).status.code(), Some(0));
}

#[test]
fn json_is_refused_together_with_apply() {
    let p = Project::new("paste = \"1\"", "pub fn f() { paste::paste! { } }\n");
    let out = p.run(&["--json", "--apply", "--source-only", "--allow-dirty"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr(&out).contains("cannot be combined with --apply"),
        "{}",
        stderr(&out)
    );
    assert!(p.lib().contains("paste::paste!"));
}

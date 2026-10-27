//! Human-readable rendering of a plan.

use crate::plan::{manifest_commands, short, Item, Plan, Status};
use crate::rules::Action;

pub fn render(plan: &Plan) -> String {
    let mut out = String::new();
    if plan.items.is_empty() {
        out.push_str(&format!(
            "No unmaintained dependencies from the catalogue in {} package(s).\n",
            plan.packages
        ));
        return out;
    }
    let mut last = "";
    for item in &plan.items {
        if item.package != last {
            out.push_str(&format!("\npackage `{}`\n", item.package));
            last = &item.package;
        }
        render_item(item, &mut out);
    }
    let ready = plan.count(|s| *s == Status::Ready);
    let blocked = plan.count(|s| matches!(s, Status::Blocked(_)));
    let manual = plan.count(|s| *s == Status::Manual);
    let unused = plan.count(|s| *s == Status::NoUsage);
    out.push_str(&format!(
        "\n{ready} ready, {blocked} blocked, {manual} manual, {unused} without usages in source\n"
    ));
    out
}

fn render_item(item: &Item, out: &mut String) {
    let tag = match &item.status {
        Status::Ready => "ready",
        Status::Blocked(_) => "blocked",
        Status::NoUsage => "no usages in source",
        Status::Manual => "manual",
    };
    out.push_str(&format!(
        "  {} -> {}  [{tag}]  ({})\n",
        item.rule.old,
        item.rule.successor(),
        item.rule.advisory
    ));
    match &item.status {
        Status::Ready => {
            for f in &item.files {
                for c in &f.rewrite.changes {
                    let file = short(&item.package_dir, &f.path);
                    if !c.before.is_empty() {
                        out.push_str(&format!("    {file}:{}  - {}\n", c.line, c.before.trim()));
                    }
                    if !c.after.is_empty() {
                        out.push_str(&format!("    {file}:{}  + {}\n", c.line, c.after.trim()));
                    }
                }
            }
            for args in manifest_commands(item) {
                out.push_str(&format!("    cargo {}\n", args.join(" ")));
            }
            if matches!(item.rule.action, Action::Atty) {
                out.push_str("    (run `cargo fmt` afterwards)\n");
            }
        }
        Status::Blocked(reasons) => {
            for r in reasons {
                out.push_str(&format!("    ! {r}\n"));
            }
        }
        Status::NoUsage => {
            out.push_str(
                "    the source never names it; check with cargo-machete before removing\n",
            );
        }
        Status::Manual => {
            let uses: usize = item.files.iter().map(|f| f.rewrite.uses).sum();
            out.push_str(&format!(
                "    {} (used in {uses} place(s))\n",
                item.rule.note
            ));
        }
    }
    for w in &item.warnings {
        if item.status == Status::Ready || matches!(item.status, Status::Blocked(_)) {
            out.push_str(&format!("    note: {w}\n"));
        }
    }
}

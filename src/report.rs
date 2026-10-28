//! Human-readable and JSON rendering of a plan.

use serde::Serialize;

use crate::plan::{manifest_commands, short, Item, Plan, Status};
use crate::rules::Action;

#[derive(Serialize)]
struct JsonPlan {
    packages: usize,
    summary: JsonSummary,
    items: Vec<JsonItem>,
}

#[derive(Serialize)]
struct JsonSummary {
    ready: usize,
    blocked: usize,
    manual: usize,
    no_usage: usize,
}

#[derive(Serialize)]
struct JsonItem {
    package: String,
    #[serde(rename = "crate")]
    crate_name: String,
    successor: String,
    advisory: String,
    status: &'static str,
    /// Why the swap is blocked; empty otherwise.
    reasons: Vec<String>,
    uses: usize,
    changes: Vec<JsonChange>,
    /// Arguments of each `cargo` call that edits the manifest.
    manifest_commands: Vec<Vec<String>>,
    notes: Vec<String>,
}

#[derive(Serialize)]
struct JsonChange {
    file: String,
    line: usize,
    before: Option<String>,
    after: Option<String>,
}

/// The plan as pretty-printed JSON, for CI and other tools. Same content as
/// [`render`], without the diff formatting.
pub fn json(plan: &Plan) -> String {
    let count = |tag: &str| {
        plan.items
            .iter()
            .filter(|i| status_tag(&i.status) == tag)
            .count()
    };
    let report = JsonPlan {
        packages: plan.packages,
        summary: JsonSummary {
            ready: count("ready"),
            blocked: count("blocked"),
            manual: count("manual"),
            no_usage: count("no_usage"),
        },
        items: plan.items.iter().map(json_item).collect(),
    };
    serde_json::to_string_pretty(&report).expect("plain data always serializes")
}

fn json_item(item: &Item) -> JsonItem {
    // Only a ready swap is applied; a blocked one changes nothing, so it lists
    // no changes (the translatable lines are not written either).
    let mut changes = Vec::new();
    let ready = item.status == Status::Ready;
    for f in item.files.iter().filter(|_| ready) {
        for c in &f.rewrite.changes {
            changes.push(JsonChange {
                file: short(&item.package_dir, &f.path),
                line: c.line,
                before: non_empty(&c.before),
                after: non_empty(&c.after),
            });
        }
    }
    JsonItem {
        package: item.package.clone(),
        crate_name: item.rule.old.to_string(),
        successor: item.rule.successor().to_string(),
        advisory: item.rule.advisory.to_string(),
        status: status_tag(&item.status),
        reasons: match &item.status {
            Status::Blocked(reasons) => reasons.clone(),
            _ => Vec::new(),
        },
        uses: item.files.iter().map(|f| f.rewrite.uses).sum(),
        changes,
        manifest_commands: if item.status == Status::Ready {
            manifest_commands(item)
        } else {
            Vec::new()
        },
        notes: item.warnings.clone(),
    }
}

/// `None` for an empty line (an import that was removed or added).
fn non_empty(s: &str) -> Option<String> {
    (!s.is_empty()).then(|| s.to_string())
}

fn status_tag(status: &Status) -> &'static str {
    match status {
        Status::Ready => "ready",
        Status::Blocked(_) => "blocked",
        Status::NoUsage => "no_usage",
        Status::Manual => "manual",
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_of_an_empty_plan_has_no_items() {
        let v: serde_json::Value = serde_json::from_str(&json(&Plan {
            packages: 3,
            items: Vec::new(),
        }))
        .unwrap();
        assert_eq!(v["packages"], 3);
        assert_eq!(v["items"], serde_json::json!([]));
        assert_eq!(v["summary"]["ready"], 0);
    }
}

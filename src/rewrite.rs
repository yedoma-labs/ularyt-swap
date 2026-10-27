//! Source rewriting for one file and one rule.
//!
//! Only code is touched (see [`crate::lexer`]). A use the rule cannot translate
//! with certainty is left as it is and reported as [`Unresolved`]; the caller
//! then keeps the dependency.

use std::collections::BTreeSet;

use crate::lexer::{code_spans, is_ident_byte};
use crate::rules::{Action, Rule};

/// One changed line, for the dry-run report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub line: usize,
    pub before: String,
    pub after: String,
}

/// A use of the old crate that was not rewritten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unresolved {
    pub line: usize,
    pub text: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default)]
pub struct Rewrite {
    /// The source after all rewrites (equal to the input when nothing changed).
    pub source: String,
    pub changes: Vec<Change>,
    pub unresolved: Vec<Unresolved>,
    /// Paths of the old crate found in code (rewritten or not).
    pub uses: usize,
    /// Mentions in comments and strings, which are never changed.
    pub comment_mentions: usize,
}

struct PathUse {
    start: usize,
    end: usize,
    /// Offset just after the `::` that follows the crate name.
    after: usize,
}

pub fn rewrite(src: &str, rule: &Rule) -> Rewrite {
    let old = rule.old_ident();
    let mut out = Rewrite {
        source: src.to_string(),
        comment_mentions: comment_mentions(src, &old),
        ..Rewrite::default()
    };
    let uses = find_uses(src, &old);
    out.uses = uses.len();

    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    match rule.action {
        Action::Manual { .. } => return out,
        Action::Rename { new_crate, allowed } => {
            let new_ident = new_crate.replace('-', "_");
            for u in &uses {
                match check_allowed(src, u.after, allowed) {
                    Ok(()) => edits.push((u.start, u.end, new_ident.clone())),
                    Err(reason) => out.unresolved.push(unresolved(src, u.start, reason)),
                }
            }
        }
        Action::Atty => {
            let mut covered = 0;
            for u in &uses {
                // The `atty::Stream` inside a call that was just rewritten, and the
                // import line, are handled with the call (see `finish_atty`).
                if u.start < covered || is_atty_import(line_at(src, line_number(src, u.start))) {
                    continue;
                }
                match parse_atty_call(src, u.after) {
                    Some((end, text)) => {
                        covered = end;
                        edits.push((u.start, end, text));
                    }
                    None => out.unresolved.push(unresolved(
                        src,
                        u.start,
                        "only atty::is / atty::isnt with a literal Stream are rewritten".into(),
                    )),
                }
            }
        }
    }
    bare_references(src, &old, &mut out);

    let mut text = src.to_string();
    let mut touched = BTreeSet::new();
    for (start, end, replacement) in edits.iter().rev() {
        text.replace_range(*start..*end, replacement);
    }
    for (start, ..) in &edits {
        touched.insert(line_number(src, *start));
    }
    for line in touched {
        out.changes.push(Change {
            line,
            before: line_at(src, line).to_string(),
            after: line_at(&text, line).to_string(),
        });
    }

    if rule.action == Action::Atty {
        text = finish_atty(text, &old, !edits.is_empty(), &mut out);
    }
    out.source = text;
    out
}

/// Removes `use atty::Stream;` once nothing else refers to atty, and adds the
/// `IsTerminal` import that the new calls need.
fn finish_atty(mut text: String, old: &str, rewrote: bool, out: &mut Rewrite) -> String {
    if !out.unresolved.is_empty() {
        return text;
    }
    let mut occurrence_lines = BTreeSet::new();
    for span in code_spans(&text) {
        for pos in word_positions(&text, span, old) {
            occurrence_lines.insert(line_number(&text, pos));
        }
    }
    let mut remove = BTreeSet::new();
    for &line in &occurrence_lines {
        if is_atty_import(line_at(&text, line)) {
            remove.insert(line);
        } else {
            out.unresolved.push(Unresolved {
                line,
                text: line_at(&text, line).trim().to_string(),
                reason: "other use of atty".into(),
            });
        }
    }
    if !out.unresolved.is_empty() {
        return text;
    }
    for &line in &remove {
        out.changes.push(Change {
            line,
            before: line_at(&text, line).to_string(),
            after: String::new(),
        });
    }
    if !remove.is_empty() {
        let kept: Vec<&str> = text
            .split_inclusive('\n')
            .enumerate()
            .filter(|(i, _)| !remove.contains(&(i + 1)))
            .map(|(_, l)| l)
            .collect();
        text = kept.concat();
    }
    if rewrote && !text.contains("IsTerminal") {
        let (at, has_header) = header_end(&text);
        let line = line_number(&text, at.min(text.len())) + usize::from(has_header);
        let import = if has_header {
            "\nuse std::io::IsTerminal as _;\n"
        } else {
            "use std::io::IsTerminal as _;\n"
        };
        text.insert_str(at, import);
        out.changes.push(Change {
            line,
            before: String::new(),
            after: "use std::io::IsTerminal as _;".into(),
        });
    }
    text
}

/// Byte offset just after the last non-blank line of the file header (`//!`
/// docs, `#![...]` attributes, plain `//` comments), and whether there is one.
fn header_end(text: &str) -> (usize, bool) {
    let mut offset = 0;
    let mut end = 0;
    let mut in_attr = false;
    for line in text.split_inclusive('\n') {
        let t = line.trim();
        let header = if in_attr {
            if t.ends_with(']') {
                in_attr = false;
            }
            true
        } else if t.starts_with("#![") {
            in_attr = !t.ends_with(']');
            true
        } else {
            t.is_empty() || t.starts_with("//!") || (t.starts_with("//") && !t.starts_with("///"))
        };
        if !header {
            break;
        }
        offset += line.len();
        if !t.is_empty() {
            end = offset;
        }
    }
    (end, end > 0)
}

fn is_atty_import(line: &str) -> bool {
    matches!(line.trim(), "use atty::Stream;" | "use atty;")
}

fn parse_atty_call(src: &str, after: usize) -> Option<(usize, String)> {
    let b = src.as_bytes();
    let mut i = skip_ws(b, after);
    let (func, len) = read_ident(src, i);
    let negate = match func {
        "is" => false,
        "isnt" => true,
        _ => return None,
    };
    i = skip_ws(b, i + len);
    if b.get(i) != Some(&b'(') {
        return None;
    }
    i = skip_ws(b, i + 1);
    let (mut word, mut len) = read_ident(src, i);
    if word == "atty" {
        i = skip_ws(b, i + len);
        if !src[i..].starts_with("::") {
            return None;
        }
        i = skip_ws(b, i + 2);
        (word, len) = read_ident(src, i);
    }
    if word != "Stream" {
        return None;
    }
    i = skip_ws(b, i + len);
    if !src[i..].starts_with("::") {
        return None;
    }
    i = skip_ws(b, i + 2);
    let (stream, len) = read_ident(src, i);
    let handle = match stream {
        "Stdin" => "stdin",
        "Stdout" => "stdout",
        "Stderr" => "stderr",
        _ => return None,
    };
    i = skip_ws(b, i + len);
    if b.get(i) != Some(&b')') {
        return None;
    }
    let end = i + 1;
    let call = format!("std::io::{handle}().is_terminal()");
    let text = if !negate {
        call
    } else if matches!(b.get(skip_ws(b, end)), Some(b'.' | b'?')) {
        format!("(!{call})")
    } else {
        format!("!{call}")
    };
    Some((end, text))
}

fn find_uses(src: &str, ident: &str) -> Vec<PathUse> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    for span in code_spans(src) {
        for s in word_positions(src, span, ident) {
            let e = s + ident.len();
            let prev_ok = s == 0 || {
                let c = b[s - 1];
                let absolute = c == b':'
                    && s >= 2
                    && b[s - 2] == b':'
                    && (s == 2 || (!is_ident_byte(b[s - 3]) && b[s - 3] != b':'));
                absolute || (!is_ident_byte(c) && c != b':' && c != b'.' && c != b'$')
            };
            if !prev_ok {
                continue;
            }
            let j = skip_ws(b, e);
            if src[j..].starts_with("::") {
                out.push(PathUse {
                    start: s,
                    end: e,
                    after: j + 2,
                });
            }
        }
    }
    out
}

/// Start offsets of `ident` as a whole word inside `span`.
fn word_positions(src: &str, span: std::ops::Range<usize>, ident: &str) -> Vec<usize> {
    let b = src.as_bytes();
    let text = &src[span.clone()];
    let mut from = 0;
    let mut found = Vec::new();
    while let Some(p) = text[from..].find(ident) {
        let s = span.start + from + p;
        let e = s + ident.len();
        from += p + ident.len();
        let before = s == 0 || !is_ident_byte(b[s - 1]);
        let after = e >= b.len() || !is_ident_byte(b[e]);
        if before && after {
            found.push(s);
        }
    }
    found
}

fn bare_references(src: &str, old: &str, out: &mut Rewrite) {
    for (i, line) in src.lines().enumerate() {
        let t = line.trim();
        let bare = t.strip_prefix("extern crate ").or_else(|| {
            t.strip_prefix("pub use ")
                .or_else(|| t.strip_prefix("use "))
        });
        let Some(rest) = bare else { continue };
        let (word, len) = read_ident(rest, 0);
        let tail = rest[len..].trim_start();
        if word == old && (tail.starts_with(';') || tail.starts_with("as ")) {
            out.uses += 1;
            out.unresolved.push(Unresolved {
                line: i + 1,
                text: t.to_string(),
                reason: "crate imported by name".into(),
            });
        }
    }
}

fn comment_mentions(src: &str, ident: &str) -> usize {
    let pattern = format!("{ident}::");
    let spans = code_spans(src);
    let mut total = 0;
    let mut cursor = 0;
    let mut gaps: Vec<std::ops::Range<usize>> = Vec::new();
    for s in &spans {
        if s.start > cursor {
            gaps.push(cursor..s.start);
        }
        cursor = s.end;
    }
    if cursor < src.len() {
        gaps.push(cursor..src.len());
    }
    for gap in gaps {
        let text = &src[gap];
        let mut from = 0;
        while let Some(p) = text[from..].find(&pattern) {
            let boundary = (from + p == 0) || !is_ident_byte(text.as_bytes()[from + p - 1]);
            if boundary {
                total += 1;
            }
            from += p + pattern.len();
        }
    }
    total
}

fn check_allowed(src: &str, after: usize, allowed: &[&str]) -> Result<(), String> {
    let b = src.as_bytes();
    let i = skip_ws(b, after);
    match b.get(i) {
        Some(&c) if is_ident_byte(c) => {
            let (word, _) = read_ident(src, i);
            if allowed.contains(&word) {
                Ok(())
            } else {
                Err(format!("`{word}` has no counterpart in the successor"))
            }
        }
        Some(b'{') => check_group(src, i, allowed),
        Some(b'*') => Err("glob import".into()),
        _ => Err("unrecognised path".into()),
    }
}

fn check_group(src: &str, open: usize, allowed: &[&str]) -> Result<(), String> {
    let b = src.as_bytes();
    let mut i = open + 1;
    let mut depth = 1;
    let mut item_start = true;
    while i < b.len() && depth > 0 {
        let c = b[i];
        if c == b'/' {
            return Err("comment inside an import group".into());
        }
        if depth == 1 && item_start {
            if c == b'*' {
                return Err("glob import".into());
            }
            if is_ident_byte(c) {
                let (word, len) = read_ident(src, i);
                if word != "self" && !allowed.contains(&word) {
                    return Err(format!("`{word}` has no counterpart in the successor"));
                }
                item_start = false;
                i += len;
                continue;
            }
        }
        match c {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            b',' if depth == 1 => item_start = true,
            _ => {}
        }
        i += 1;
    }
    Ok(())
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && b[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

fn read_ident(src: &str, i: usize) -> (&str, usize) {
    let b = src.as_bytes();
    let mut j = i;
    while j < b.len() && is_ident_byte(b[j]) {
        j += 1;
    }
    (&src[i..j], j - i)
}

fn line_number(src: &str, offset: usize) -> usize {
    src[..offset].bytes().filter(|&c| c == b'\n').count() + 1
}

fn line_at(src: &str, line: usize) -> &str {
    src.lines().nth(line - 1).unwrap_or("")
}

fn unresolved(src: &str, offset: usize, reason: String) -> Unresolved {
    let line = line_number(src, offset);
    Unresolved {
        line,
        text: line_at(src, line).trim().to_string(),
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::lookup;

    fn run(rule: &str, src: &str) -> Rewrite {
        rewrite(src, lookup(rule).unwrap())
    }

    #[test]
    fn renames_paths_but_not_variables() {
        let r = run(
            "instant",
            "use instant::Instant;\nfn f() { let instant = instant::Instant::now(); }\n",
        );
        assert_eq!(
            r.source,
            "use web_time::Instant;\nfn f() { let instant = web_time::Instant::now(); }\n"
        );
        assert!(r.unresolved.is_empty());
        assert_eq!(r.uses, 2);
        assert_eq!(r.changes.len(), 2);
    }

    #[test]
    fn leaves_comments_and_strings_alone() {
        let src = "// instant::Instant\nlet s = \"instant::Instant\";\n";
        let r = run("instant", src);
        assert_eq!(r.source, src);
        assert_eq!(r.uses, 0);
        assert_eq!(r.comment_mentions, 2);
    }

    #[test]
    fn items_without_a_counterpart_are_unresolved() {
        let r = run("instant", "let t = instant::now();\n");
        assert_eq!(r.unresolved.len(), 1);
        assert!(r.unresolved[0].reason.contains("`now`"));
        assert!(r.changes.is_empty());
    }

    #[test]
    fn checks_every_item_of_an_import_group() {
        let ok = run("fxhash", "use fxhash::{FxHashMap, FxHashSet as S};\n");
        assert!(ok.unresolved.is_empty());
        assert_eq!(ok.source, "use rustc_hash::{FxHashMap, FxHashSet as S};\n");
        let bad = run("fxhash", "use fxhash::{FxHashMap, hash64};\n");
        assert_eq!(bad.unresolved.len(), 1);
        assert!(bad.unresolved[0].reason.contains("hash64"));
        let glob = run("fxhash", "use fxhash::*;\n");
        assert_eq!(glob.unresolved.len(), 1);
    }

    #[test]
    fn handles_absolute_and_nested_paths() {
        let r = run("instant", "let a = ::instant::Instant::now();\n");
        assert_eq!(r.source, "let a = ::web_time::Instant::now();\n");
        let r = run("instant", "let a = other::instant::Instant::now();\n");
        assert_eq!(r.uses, 0);
    }

    #[test]
    fn crate_imported_by_name_is_unresolved() {
        let r = run("instant", "extern crate instant;\n");
        assert_eq!(r.unresolved.len(), 1);
        let r = run("instant", "use instant as i;\n");
        assert_eq!(r.unresolved.len(), 1);
    }

    #[test]
    fn rewrites_atty_calls() {
        let r = run(
            "atty",
            "fn main() {\n    if atty::is(atty::Stream::Stdout) { }\n}\n",
        );
        assert_eq!(
            r.source,
            "use std::io::IsTerminal as _;\nfn main() {\n    if std::io::stdout().is_terminal() { }\n}\n"
        );
        assert!(r.unresolved.is_empty());
    }

    #[test]
    fn atty_isnt_is_negated_and_parenthesised_before_a_method_call() {
        let r = run("atty", "let a = atty::isnt(atty::Stream::Stderr);\n");
        assert!(r
            .source
            .contains("let a = !std::io::stderr().is_terminal();"));
        let r = run(
            "atty",
            "let a = atty::isnt(atty::Stream::Stdin).then(|| 1);\n",
        );
        assert!(r.source.contains("(!std::io::stdin().is_terminal()).then"));
    }

    #[test]
    fn atty_import_line_is_removed_and_header_is_respected() {
        let src = "//! docs\n#![forbid(unsafe_code)]\n\nuse atty::Stream;\n\nfn f() -> bool { atty::is(Stream::Stdout) }\n";
        let r = run("atty", src);
        assert!(r.unresolved.is_empty());
        assert_eq!(
            r.source,
            "//! docs\n#![forbid(unsafe_code)]\n\nuse std::io::IsTerminal as _;\n\n\nfn f() -> bool { std::io::stdout().is_terminal() }\n"
        );
    }

    #[test]
    fn atty_with_a_variable_stream_is_unresolved() {
        let r = run("atty", "fn f(s: atty::Stream) -> bool { atty::is(s) }\n");
        assert!(!r.unresolved.is_empty());
    }

    #[test]
    fn existing_is_terminal_import_is_not_duplicated() {
        let src = "use std::io::IsTerminal;\nfn f() -> bool { atty::is(atty::Stream::Stdin) }\n";
        let r = run("atty", src);
        assert_eq!(r.source.matches("IsTerminal").count(), 1);
    }

    #[test]
    fn manual_rules_only_count_uses() {
        let src = "let b = backoff::ExponentialBackoff::default();\n";
        let r = run("backoff", src);
        assert_eq!(r.source, src);
        assert_eq!(r.uses, 1);
    }
}

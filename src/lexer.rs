//! Splits Rust source into code and non-code (comments, strings, char
//! literals) so that rewrites only ever touch code.

use std::ops::Range;

pub(crate) fn is_ident_byte(b: u8) -> bool {
    b == b'_' || b.is_ascii_alphanumeric() || b >= 0x80
}

/// Byte ranges of `src` that are code. Everything between the ranges is a
/// comment, a string literal or a char literal.
pub fn code_spans(src: &str) -> Vec<Range<usize>> {
    let b = src.as_bytes();
    let mut spans = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < b.len() {
        let skip_to = match b[i] {
            b'/' if b.get(i + 1) == Some(&b'/') => Some(line_end(b, i)),
            b'/' if b.get(i + 1) == Some(&b'*') => Some(block_comment_end(b, i)),
            b'"' => Some(string_end(b, i + 1)),
            b'r' | b'b' if i == 0 || !is_ident_byte(b[i - 1]) => raw_string_end(b, i),
            b'\'' => char_literal_end(src, i),
            _ => None,
        };
        match skip_to {
            Some(end) => {
                if start < i {
                    spans.push(start..i);
                }
                i = end.max(i + 1);
                start = i;
            }
            None => i += 1,
        }
    }
    if start < b.len() {
        spans.push(start..b.len());
    }
    spans
}

fn line_end(b: &[u8], from: usize) -> usize {
    b[from..]
        .iter()
        .position(|&c| c == b'\n')
        .map_or(b.len(), |p| from + p)
}

fn block_comment_end(b: &[u8], from: usize) -> usize {
    let mut depth = 0usize;
    let mut i = from;
    while i < b.len() {
        if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
            depth += 1;
            i += 2;
        } else if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
            depth -= 1;
            i += 2;
            if depth == 0 {
                return i;
            }
        } else {
            i += 1;
        }
    }
    b.len()
}

/// `from` points just after the opening quote.
fn string_end(b: &[u8], from: usize) -> usize {
    let mut i = from;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'"' => return i + 1,
            _ => i += 1,
        }
    }
    b.len()
}

/// `r"..."`, `r#"..."#`, `br"..."`, `b"..."`. Returns the end if one starts at `i`.
fn raw_string_end(b: &[u8], i: usize) -> Option<usize> {
    let mut j = i;
    if b[j] == b'b' {
        j += 1;
        if b.get(j) == Some(&b'"') {
            return Some(string_end(b, j + 1));
        }
    }
    if b.get(j) != Some(&b'r') {
        return None;
    }
    j += 1;
    let hashes = b[j..].iter().take_while(|&&c| c == b'#').count();
    j += hashes;
    if b.get(j) != Some(&b'"') {
        return None;
    }
    j += 1;
    while j < b.len() {
        if b[j] == b'"'
            && b[j + 1..]
                .iter()
                .take(hashes)
                .filter(|&&c| c == b'#')
                .count()
                == hashes
        {
            return Some(j + 1 + hashes);
        }
        j += 1;
    }
    Some(b.len())
}

/// A char literal at `i` (`'x'`, `'\n'`, `'\u{1F600}'`), or `None` for a lifetime.
fn char_literal_end(src: &str, i: usize) -> Option<usize> {
    let b = src.as_bytes();
    if b.get(i + 1) == Some(&b'\\') {
        let mut j = i + 3;
        while j < b.len() && b[j] != b'\'' {
            j += 1;
        }
        return Some((j + 1).min(b.len()));
    }
    let c = src[i + 1..].chars().next()?;
    let close = i + 1 + c.len_utf8();
    (b.get(close) == Some(&b'\'')).then_some(close + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(src: &str) -> String {
        code_spans(src)
            .into_iter()
            .map(|r| &src[r])
            .collect::<Vec<_>>()
            .join("|")
    }

    #[test]
    fn skips_comments_and_strings() {
        assert_eq!(code("a // x\nb"), "a |\nb");
        assert_eq!(code("a /* x /* y */ z */ b"), "a | b");
        assert_eq!(code(r#"a "q\"x" b"#), "a | b");
        assert_eq!(code(r##"a r#"q"x"# b"##), "a | b");
    }

    #[test]
    fn tells_char_literals_from_lifetimes() {
        assert_eq!(code("f('a', 'b)"), "f(|, 'b)");
        assert_eq!(code(r"f('\n', x)"), "f(|, x)");
        assert_eq!(code("fn f<'a>(x: &'a str)"), "fn f<'a>(x: &'a str)");
    }

    #[test]
    fn keeps_identifiers_that_end_in_r_or_b() {
        assert_eq!(code("bar\"x\""), "bar");
    }
}

//! The swap catalogue: which unmaintained crate maps to which successor, and
//! whether the swap is mechanical enough to automate.

/// How a dependency is handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// The successor exposes the same items under a new crate name. Paths are
    /// renamed, but only for the items in `allowed`; anything else is left
    /// alone and reported.
    Rename {
        new_crate: &'static str,
        allowed: &'static [&'static str],
    },
    /// `atty::is(atty::Stream::X)` becomes `std::io::x().is_terminal()`.
    Atty,
    /// The successor's API differs. Reported, never rewritten.
    Manual { successor: &'static str },
}

#[derive(Debug, Clone, Copy)]
pub struct Rule {
    pub old: &'static str,
    pub action: Action,
    /// RustSec advisory that marks `old` as unmaintained.
    pub advisory: &'static str,
    pub note: &'static str,
}

impl Rule {
    /// Name under which the old crate is used in source (`number-prefix` -> `number_prefix`).
    pub fn old_ident(&self) -> String {
        self.old.replace('-', "_")
    }

    pub fn successor(&self) -> &'static str {
        match self.action {
            Action::Rename { new_crate, .. } => new_crate,
            Action::Atty => "std::io::IsTerminal",
            Action::Manual { successor } => successor,
        }
    }

    pub fn is_automatic(&self) -> bool {
        !matches!(self.action, Action::Manual { .. })
    }
}

pub const RULES: &[Rule] = &[
    Rule {
        old: "atty",
        action: Action::Atty,
        advisory: "RUSTSEC-2024-0375",
        note: "std::io::IsTerminal is stable since Rust 1.70",
    },
    Rule {
        old: "instant",
        action: Action::Rename {
            new_crate: "web-time",
            allowed: &["Instant"],
        },
        advisory: "RUSTSEC-2024-0384",
        note: "web-time needs no features; the wasm-bindgen / stdweb / inaccurate features of instant are dropped",
    },
    Rule {
        old: "fxhash",
        action: Action::Rename {
            new_crate: "rustc-hash",
            allowed: &["FxHashMap", "FxHashSet", "FxHasher", "FxBuildHasher"],
        },
        advisory: "RUSTSEC-2025-0057",
        note: "rustc-hash 2 uses a different algorithm: hash values and iteration order change",
    },
    Rule {
        old: "number_prefix",
        action: Action::Rename {
            new_crate: "unit-prefix",
            allowed: &["NumberPrefix", "Prefix"],
        },
        advisory: "RUSTSEC-2025-0119",
        note: "recommended successor with the same API",
    },
    Rule {
        old: "paste",
        action: Action::Rename {
            new_crate: "pastey",
            allowed: &["paste"],
        },
        advisory: "RUSTSEC-2024-0436",
        note: "pastey is a drop-in fork",
    },
    manual("backoff", "backon", "RUSTSEC-2025-0012", "retry API differs"),
    manual(
        "rustls-pemfile",
        "rustls-pki-types (PemObject)",
        "RUSTSEC-2025-0134",
        "use the PemObject trait from rustls-pki-types",
    ),
    manual(
        "derivative",
        "derive-where, derive_more or educe",
        "RUSTSEC-2024-0388",
        "attribute syntax differs",
    ),
    manual(
        "bincode",
        "postcard, bitcode, wincode or rkyv",
        "RUSTSEC-2025-0141",
        "the wire format differs between all successors",
    ),
    manual("async-std", "smol", "RUSTSEC-2025-0052", "runtime API differs"),
    manual(
        "proc-macro-error",
        "manyhow or proc-macro2-diagnostics",
        "RUSTSEC-2024-0370",
        "diagnostics API differs",
    ),
    manual(
        "gcc",
        "cc",
        "RUSTSEC-2025-0121",
        "gcc::Config is cc::Build",
    ),
    manual("basic-toml", "toml", "RUSTSEC-2026-0374", "serde API differs slightly"),
    manual("gumdrop", "clap or bpaf", "RUSTSEC-2026-0214", "argument parsing is rewritten"),
    manual(
        "smartstring",
        "compact_str or smol_str",
        "RUSTSEC-2026-0249",
        "string API differs",
    ),
    manual("im", "imbl (maintained fork)", "RUSTSEC-2026-0248", "check imbl's changelog"),
    manual(
        "chrono-english",
        "interim (fork with the same API)",
        "RUSTSEC-2024-0395",
        "rename the crate and re-run your tests",
    ),
    manual(
        "trust-dns-proto",
        "hickory-proto",
        "RUSTSEC-2025-0017",
        "renamed upstream",
    ),
];

const fn manual(
    old: &'static str,
    successor: &'static str,
    advisory: &'static str,
    note: &'static str,
) -> Rule {
    Rule {
        old,
        action: Action::Manual { successor },
        advisory,
        note,
    }
}

pub fn lookup(crate_name: &str) -> Option<&'static Rule> {
    RULES.iter().find(|r| r.old == crate_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique() {
        for (i, a) in RULES.iter().enumerate() {
            assert!(RULES[i + 1..].iter().all(|b| b.old != a.old), "{}", a.old);
        }
    }

    #[test]
    fn idents_replace_dashes() {
        assert_eq!(
            lookup("number_prefix").unwrap().old_ident(),
            "number_prefix"
        );
        assert_eq!(
            lookup("rustls-pemfile").unwrap().old_ident(),
            "rustls_pemfile"
        );
    }
}

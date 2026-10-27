# ularyt-swap

<picture>
  <source media="(max-width: 640px)" srcset="https://raw.githubusercontent.com/yedoma-labs/assets/main/resized/banner-resized-mobile.png">
  <img src="https://raw.githubusercontent.com/yedoma-labs/assets/main/resized/banner-resized.png" alt="Project Header">
</picture>

> **`cargo ularyt`: swap unmaintained dependencies for their maintained successors** - manifest and source, as a reviewable diff

[![CI](https://github.com/yedoma-labs/ularyt-swap/actions/workflows/ci.yml/badge.svg)](https://github.com/yedoma-labs/ularyt-swap/actions/workflows/ci.yml)
[![Security Audit](https://github.com/yedoma-labs/ularyt-swap/actions/workflows/audit.yml/badge.svg)](https://github.com/yedoma-labs/ularyt-swap/actions/workflows/audit.yml)
[![Crates.io](https://img.shields.io/crates/v/ularyt-swap.svg)](https://crates.io/crates/ularyt-swap)
[![License](https://img.shields.io/crates/l/ularyt-swap.svg)](LICENSE-MIT)
[![Conventional Commits](https://img.shields.io/badge/commits-conventional-fe5196?logo=conventionalcommits&logoColor=white)](https://www.conventionalcommits.org/)

*ularyt* (уларыт; Sakha) = "to change"

**Status: prototype.** Five swaps are automated, thirteen more are reported. See [What is verified](#what-is-verified) before you trust it.

## Why

`cargo audit` and `cargo deny` tell you that `atty`, `instant`, `paste` or `fxhash` are unmaintained. They do not change a line. For most of these crates the successor is known and the change is mechanical, but you still do it by hand, in every project: edit `Cargo.toml`, rename paths, fix imports.

`ularyt-swap` does the mechanical part and refuses the rest.

## Usage

```bash
cargo install ularyt-swap      # installs the `cargo-ularyt` binary

cargo ularyt                   # dry run: lists every swap as a diff, changes nothing
cargo ularyt --apply           # writes source and manifests, then runs `cargo check`
cargo ularyt --check           # exit code 1 if anything is found (for CI)
```

Dry-run output:

```
package `demo`
  atty -> std::io::IsTerminal  [ready]  (RUSTSEC-2024-0375)
    src/lib.rs:12  - atty::is(Stream::Stdout) && !atty::is(atty::Stream::Stderr)
    src/lib.rs:12  + std::io::stdout().is_terminal() && !std::io::stderr().is_terminal()
    cargo remove -p demo atty
  paste -> pastey  [ready]  (RUSTSEC-2024-0436)
    src/lib.rs:7  - paste::paste! {
    src/lib.rs:7  + pastey::paste! {
    cargo remove -p demo paste
    cargo add -p demo pastey
  backoff -> backon  [manual]  (RUSTSEC-2025-0012)
    retry API differs (used in 2 place(s))
```

Options: `--manifest-path <Cargo.toml>`, `--allow-dirty` (by default `--apply` refuses a dirty git tree, so you can always `git diff` or revert), `--source-only`, `--no-verify`.

## What it changes

| Old | New | How |
|---|---|---|
| `atty` | `std::io::IsTerminal` | `atty::is(Stream::Stdout)` becomes `std::io::stdout().is_terminal()`; `isnt` is negated; the import is added |
| `instant` | `web-time` | `instant::Instant` becomes `web_time::Instant` |
| `fxhash` | `rustc-hash` | `FxHashMap`, `FxHashSet`, `FxHasher`, `FxBuildHasher` |
| `number_prefix` | `unit-prefix` | `NumberPrefix`, `Prefix` |
| `paste` | `pastey` | `paste::paste!` |

Reported only, with the successor the RustSec advisory recommends: `backoff`, `rustls-pemfile`, `derivative`, `bincode`, `async-std`, `proc-macro-error`, `gcc`, `basic-toml`, `gumdrop`, `smartstring`, `im`, `chrono-english`, `trust-dns-proto`. Their APIs differ, so a rename would not compile or would change behaviour.

## How it stays safe

- **Code only.** A small lexer skips comments, strings and char literals, so `"instant::Instant"` in a message or a doc comment is never touched. Mentions there are counted and reported (doc tests may need a look).
- **All or nothing per package.** If one use cannot be translated with certainty (`instant::now()`, `use fxhash::*`, `extern crate instant;`, a non-literal `atty::Stream`, a renamed dependency, a `rust-version` below 1.70 for `atty`), the whole swap for that package is blocked and nothing is changed. The dependency never ends up half-moved.
- **Allow-lists, not guesses.** A rename only applies to items the successor is known to export (see `src/rules.rs`).
- **Manifest edits go through cargo.** `cargo remove` / `cargo add` keep formatting, workspace tables and target-specific sections intact. Dev, build, optional and `cfg` dependencies keep their kind.
- **Clean tree required.** `--apply` verifies with `cargo check --workspace --all-targets` and tells you when it fails.

## What is verified

- 18 unit tests (lexer, rewrite rules) and 7 end-to-end tests that run the real binary against throw-away projects, including blocked swaps, comment safety, the dirty-tree refusal and `--check`.
- One end-to-end run on a demo crate that uses all five automated crates (including `NumberPrefix::binary`, `FxHashMap::default()` and `pastey::paste!`): after `--apply`, `cargo check` passes.
- The dry run was run against two public projects that use `atty` (a CLI and a REPL). It found the calls and produced the expected replacements. **It was not compiled there**, and no real-world project has been migrated end to end yet.

Known limits:

- `rustc-hash` 2 uses a different hashing algorithm than `fxhash`: hash values and the iteration order of maps change. The tool says so in a note.
- `instant` features (`wasm-bindgen`, `stdweb`, `inaccurate`) are not carried over; the tool warns when they were set.
- Dependencies inherited from `[workspace.dependencies]` are removed from the member, but the workspace entry stays.
- Macro-generated paths and code in `include!`d files are not visible to the lexer.
- Run `cargo fmt` afterwards: inserted imports are not sorted.

## Not in scope (yet)

`chrono` to `jiff` is the biggest migration in the ecosystem, but the two have different models (`NaiveDateTime` vs. `civil::DateTime`, time zones, errors), so it cannot be a rename. A guided mode that rewrites only the unambiguous calls and marks the rest is the planned next step.

## Other tools

`cargo-unmaintained`, `cargo audit` and `cargo deny` find these dependencies. `ast-grep` and rust-analyzer's structural search-and-replace can express single rewrites. I found no tool that ships a curated, verified catalogue of swaps for Rust and applies the manifest and source changes together; if you know one, please open an issue.

## Adding a rule

Add an entry to `RULES` in `src/rules.rs`. A `Rename` rule needs an allow-list of the items that exist in both crates; verify it by compiling a project that uses every listed item. Anything you cannot verify belongs in `Manual`.

## License

MIT OR Apache-2.0

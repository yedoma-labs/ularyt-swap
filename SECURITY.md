# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |

## Scope

`ularyt-swap` rewrites files in the project you point it at and runs `cargo add` / `cargo remove` / `cargo check` there. It makes no network calls itself (cargo does), has no `unsafe` code and reads only `.rs` files below the package directory.

- `cargo check` runs the build scripts and proc macros of the project's dependencies, like any build. Only run `--apply` on code you trust.
- `--apply` refuses a dirty git tree unless you pass `--allow-dirty`.

## Reporting a Vulnerability

**Do not** open a public issue for security vulnerabilities. Use a private GitHub Security Advisory on this repository, or email yedoma-labs@proton.me. Include a description, steps to reproduce and the impact. We acknowledge within 48 hours.


# Contributing to ularyt-swap

1. Fork and clone the repository
2. Enable the commit hook: `git config core.hooksPath .githooks`
3. Create a branch, make your change, add tests (unit tests next to the code, end-to-end tests in `tests/`)
4. Run the checks CI runs:
   ```bash
   cargo fmt --all -- --check
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test --all-targets --all-features
   RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
   ```
5. Commit with a [Conventional Commit](https://www.conventionalcommits.org/) message and open a pull request

## Commit messages

`<type>(<scope>)?: <description>`, for example `feat(rules): swap number_prefix for unit-prefix`. Types: `feat fix docs style refactor perf test build ci chore revert`. `fix:` bumps the patch version, `feat:` the minor version (while 0.x), `feat!:` or a `BREAKING CHANGE` footer the major version.


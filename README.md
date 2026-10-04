# maestro-rust-workflows

One shared Rust CI, with three checks:

| Check | Command | Systems |
| --- | --- | --- |
| Format | `cargo fmt --all --check` | Linux |
| Lint | `cargo clippy --workspace --all-targets --locked -- -D warnings` | Linux |
| Tests | `cargo test --workspace --locked` | Linux, macOS, Windows |

## Adoption

The organization rules run `.github/workflows/ci.yml` for repositories with
`stack=rust`. Consumers need only standard Rust files: no workflow to copy,
no settings file and no workflow inputs. A repository's `rust-toolchain.toml`
selects its toolchain through rustup.

The approved [shared CI spec](https://github.com/Orchestration-Maestro/.github/blob/main/docs/specs/2026-10-04-shared-rust-ci.md)
defines the scope. Organization pins move separately from this repository's
releases; publishing the first release does not change existing consumers.

## Self-test

`ci-internal.yml` calls the shared workflow on `fixtures/sample`, which must
pass on all three systems. The three broken copies are independent packages,
not members of a root workspace. Each fails only its intended command:
format, lint or tests. The self-test reads these commands from `ci.yml` and
runs all nine combinations on Linux, because reusable workflow calls cannot
mark expected failures with `continue-on-error`.

The final checks are `Required repository quality` and
`Required consumer tests`.

## Versions

Release-please opens release pull requests and tags merged releases, starting
at `v0.1.0`. The organization rules pin a version tag; releasing here does not
move those pins. CI changes, features and fixes appear in the changelog.

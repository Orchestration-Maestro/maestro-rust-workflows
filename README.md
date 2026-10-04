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

### Verify a release

Releases include `source.tar.gz` (a git archive of the tag), `sbom.spdx.json`
(GitHub's dependency graph SPDX export), and `SHA256SUMS` covering both files.
All three assets have SLSA build provenance attestations, including the checksum
file. The SBOM describes GitHub's dependency graph at export time.

With GitHub CLI authenticated, set `tag` to the release to verify and run in an
empty directory:

```bash
set -euo pipefail
repo=Orchestration-Maestro/maestro-rust-workflows
tag=v0.1.1
gh release download "$tag" --repo "$repo" \
  --pattern source.tar.gz --pattern sbom.spdx.json --pattern SHA256SUMS
for asset in source.tar.gz sbom.spdx.json SHA256SUMS; do
  gh attestation verify "$asset" --repo "$repo" \
    --signer-workflow "$repo/.github/workflows/release-please.yml"
done
sha256sum -c SHA256SUMS
```

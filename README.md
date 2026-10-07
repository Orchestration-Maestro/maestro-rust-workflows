# maestro-rust-workflows

One shared Rust CI. The original checks remain required:

| Check | Command | Systems |
| --- | --- | --- |
| Format | `cargo fmt --all --check` | Linux |
| Lint | `cargo clippy --workspace --all-targets --locked -- -D warnings` | Linux |
| Docs | `cargo doc --workspace --no-deps --locked` | Linux |
| Tests | `cargo test --workspace --locked` | Linux |

Docs uses `RUSTDOCFLAGS="-D warnings -D missing_docs"`: every public item must
be documented, and documentation warnings fail the check. Tests run on Linux
for now; macOS and Windows return before the first release.
A new push to a pull request cancels that pull request's older run.

## Adoption

The organization rules run `.github/workflows/ci.yml` for repositories with
`stack=rust`. Consumers need only standard Rust files: no workflow to copy,
no required settings file and no workflow inputs. The optional
[caller declaration](docs/caller-checks.md) enables isolated tests, real search
tools, caller-pinned test tools, listed wasm builds, native foreign-target
checks and a pinned browser recipe. A repository's `rust-toolchain.toml`
selects its toolchain through rustup.

Every caller also gets one dependency, source and coverage job: cargo-deny
(licenses, bans, sources and advisories), cargo-machete (unused dependencies),
typos (spelling), gitleaks (checked-out secrets), zizmor and actionlint (workflow
security and correctness when workflows exist), and at least 80% line coverage
with `cargo llvm-cov --workspace --locked --fail-under-lines 80`. All seven tools
are exact release pins, verified against embedded SHA-256 checksums before
execution. Their defaults are embedded in the shared workflow, not loaded from
consumer files. The caller declaration can add spelling words, exact-version
license exceptions and exact git sources, or raise coverage; it cannot lower
the floor or disable a check. Declared callers retain test isolation and their
opted-in search/developer tools during coverage.

The approved [shared CI spec](https://github.com/Orchestration-Maestro/.github/blob/main/docs/specs/2026-10-04-shared-rust-ci.md)
defines the scope. Organization pins move separately from this repository's
releases; publishing the first release does not change existing consumers.

## Self-test

`ci-internal.yml` calls the shared workflow on `fixtures/sample`, which must
pass every check on Linux. The eleven broken copies are independent packages,
not members of a root workspace. Each fails only its intended command:
format, lint, tests, an undocumented public item or a broken documentation link.
The self-test reads the commands and documentation flags from `ci.yml` and
runs all twenty original combinations on Linux, because reusable workflow calls cannot
mark expected failures with `continue-on-error`. Six additional copies each
fail only license policy, unused dependencies, spelling, secrets, unsafe
workflows or low coverage; the self-test runs all sixty combinations of their
new and original checks. Permanent workflow-body tests also exercise caller
extensions, rejected suppressions, independent workflow correctness failures,
verified installations and the sample job's added execution time.

The configured `fixtures/declared` caller also passes all declared checks.
Workflow-body probes cover declarations, isolation, real tools, wasm and
browser failures and pinned tooling. A developer-tool fixture proves isolated
tests can spawn caller-pinned `just` and `prek` only when opted in. Native
runner proofs detect deliberate Windows/macOS-only failures. These feed the same required results.

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

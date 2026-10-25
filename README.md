# maestro-rust-workflows

One shared Rust CI. The original checks remain required:

| Check | Command | Systems |
| --- | --- | --- |
| Format | `cargo fmt --all --check` | Linux |
| Lint | `cargo clippy --workspace --all-targets --locked -- -D warnings` | Linux |
| Docs | `cargo doc --workspace --no-deps --locked` | Linux |
| Tests | `cargo nextest run` partitions and `cargo test --doc` | Linux |

Docs uses `RUSTDOCFLAGS="-D warnings -D missing_docs"`: every public item must
be documented, and documentation warnings fail the check. Tests run on Linux
for now; macOS and Windows return before the first release.
A new push to a pull request cancels that pull request's older run.

Pull requests run declaration validation, formatting, Clippy, docs, the
dependency and source checks, and declared Windows target checks in parallel.
Test shards, coverage, other foreign-target checks (including macOS),
wasm/browser builds and Chromium tests run on merge groups and main pushes,
not on pull-request heads. Those full events also keep the Windows check.

Tests run with checksum-pinned cargo-nextest 0.9.148 on four Linux runners,
using automatic count partitions rather than package lists. Doctests run in a
parallel `Doctests` matrix leg. The aggregate `Tests (ubuntu-latest)` requires
all five legs to succeed on full runs and accepts event-skipped shards on
pull requests.
Declared callers can retain [per-test diagnostics](docs/caller-checks.md#test-diagnostics)
and the reporter log after isolated tests fail or time out.

Each nextest test can retry twice; flaky passes are named in job summaries,
while permanent failures still fail. Merge groups and main pushes run the full
suite with coverage. See [test selection and retries](docs/caller-checks.md#pull-request-selection-and-retries)
for the declaration's package-selection rules and self-test event mode.

Declared callers measure coverage in those partitions with `cargo llvm-cov
nextest`. Only the four numeric shards upload LCOV; the aggregate counts a source line as covered
if any shard covers it and enforces the union's 80% floor (or caller's higher
threshold). Missing shard reports fail. Undeclared callers retain single-job
coverage in `Dependency, source and coverage checks`.

For an independent workspace built by tests, extend the optional declaration:

```toml
wasm_crates = []
check_targets = []
additional_locked_manifests = ["fixtures/author build/Cargo.toml"]
```

The additional locked fetch runs before tests; missing or stale locks fail rather
than being repaired. Adoption needs a workflows release and organization re-pin.

Pinned tool downloads use curl's `--retry 3` for transient failures without
changing their pins or integrity checks.

## Compile cache

Linux compiling jobs and target checks on Linux and arm64 macOS restore one
compiled-cache snapshot through GitHub's cache action, then run
checksum-pinned sccache 0.18.0 from
`${runner.temp}/maestro-sccache`. Compiler lookups use local disk, not GitHub
requests per object. Incremental compilation stays disabled. See
[Caller compile cache](docs/caller-checks.md#compile-cache) for wrapper settings
and coverage behaviour. Pinned assets are `x86_64-unknown-linux-musl`,
`aarch64-unknown-linux-musl` and `aarch64-apple-darwin`. Windows target
checks run uncached because sccache expands response files beyond Windows'
command-line limit; [Windows caching](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/166)
tracks the upstream fix. The checks job's
secret scan runs before restoration; isolated tests receive only non-secret
local-cache and diagnostic settings, including after automatic server restarts.

Compatibility prefixes identify the caller repository and directory, host,
compiler and job shape. Coverage shards share a prefix with the optional
`warm-cache: true` build, which compiles the full coverage test graph without
producing a report. By default it runs no tests; an optional caller
[`warm_test_filter`](docs/caller-checks.md#warm-test-filter) runs selected tests
to include their child compiles in an eligible snapshot. Plain selected-package builds and doctests
share a plain shape. Other jobs use their own shape, including the native target
where needed. Immutable save keys distinguish runs, attempts, jobs, matrix legs
and reusable calls without splitting the shared restore prefix. GitHub's
branch/default-branch visibility and newest-compatible-prefix lookup apply.
Only pushes to the caller's default branch save snapshots. Pull-request and
merge-group jobs restore but never save. Each test shape has one writer per
call: the warm build, or shard 1 plus the separate plain doctest leg when the
numeric shards use coverage. Other compiling jobs each write their own shape.
A default-branch push warm call also runs the declared target checks to seed
the Linux and macOS shapes. Warm pull-request calls skip targets because they
cannot save.

Missing snapshots build cold. Failed restores discard partial data before
startup, including warning-only failures with no matched-cache output. Only
archive transfers may fail open; installation, checksum, compiler, test and
policy failures still fail CI. No workflow retry, sleep or per-object fallback
is added. Statistics print raw diagnostics with Actions command processing
disabled; cache write errors retain their diagnostic artifact.

After a successful job, the server stops before one inspection walk. Empty
payloads, stop/inspection failures and native payloads above 1 GiB skip saving.
The archive contains compiler objects, never `target/`, registry sources,
credentials or diagnostics. Compressed archive bytes are a separate measurement.
This per-save bound does not bound the repository's 10 GB allowance: seven
1-GiB entries total 7,516,192,768 bytes, and two generations total
15,032,385,536 bytes before additional shapes or targets. GitHub evicts by
last access, so even a shape's only seed can disappear and cause a cold build.
The optional warmer is not a required consumer check. Merging this workflow
does not release it or change consumer pins.

## Adoption

The organization rules run `.github/workflows/ci.yml` for repositories with
`stack=rust`. Consumers need only standard Rust files: no workflow to copy,
no required settings file and no workflow inputs. The optional
[caller declaration](docs/caller-checks.md) enables isolated tests, real search
tools, caller-pinned build and test tools, reserved test ports, listed wasm builds, native foreign-target
checks, [additional locked manifests](docs/caller-checks.md#additional-locked-manifests)
for independent workspace dependencies, a caller-pinned browser build recipe,
optional [Chromium browser tests](docs/caller-checks.md#chromium-browser-tests)
with shared verified runtime pins, and a [nightly fuzz job](docs/caller-checks.md#nightly-fuzz-job)
that files one issue per distinct finding. A repository's `rust-toolchain.toml`
selects its toolchain through rustup.

Every caller also gets the dependency and source checks: cargo-deny
(licenses, bans, sources and advisories; pull requests and merge groups check
advisories when they change `Cargo.lock` or the advisory or yanked-crate exceptions
in `.github/ci.toml`, when the base is absent, or for merge groups when the base
cannot be read. Pull requests with an unreadable nonempty base fail package
selection. The [hourly advisory scan](docs/caller-checks.md#required-dependency-source-and-coverage-checks)
of Maestro's `main` opens an issue per new finding), cargo-machete (unused dependencies; a package named by `hakari-package` in
`.config/hakari.toml` is not checked and its member dependency edge is accepted),
cargo-hakari (`cargo hakari verify` when `.config/hakari.toml` exists),
typos (spelling), gitleaks (checked-out source, before build-cache restore),
zizmor and actionlint (workflow
security and correctness when workflows exist), plus at least 80% line coverage on merge groups and main pushes
(measured in isolated nextest partitions, or in the single undeclared coverage
job). All check tools
are exact release pins, verified against embedded SHA-256 checksums before
execution. Their defaults are embedded in the shared workflow, not loaded from
consumer files. The caller declaration can add spelling words, exact-version
license exceptions, named advisory exceptions tied to reported package versions,
[time-boxed yanked-crate exceptions](docs/caller-checks.md#declaration-extensions)
for exact crate versions and exact git sources, or raise coverage. It can also
list exact privileged-trigger workflow paths to ignore only the
`dangerous-triggers` audit for those files; all other workflow audits stay
active. It cannot lower the floor or disable a check. Declared callers retain
test isolation and their opted-in search/developer tools during coverage.

The approved [shared CI spec](https://github.com/Orchestration-Maestro/.github/blob/main/docs/specs/2026-10-04-shared-rust-ci.md)
defines the scope. Creating a release requests Quality sync, which owns eligible
organization re-pins. Publication itself does not update consumer pins.

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
verified installations and the sample job's added execution time. The secrets
probe proves restored `target/` output is not scanned while the same secret in
tracked source is caught.

The search-release seam also inventories workflow deadlines, retries, report
polling and caller fuzz-limit forwarding. Each occurrence names its workflow,
job, step, consuming command, occurrence, value and recorded reason. Temporary
workflow fixtures reject unlisted, moved, changed, duplicated or unexplained
controls. The runner-provided Mike Farah `yq` reads YAML nodes, including flow
mappings and aliases. Check tools installs checksum-verified shfmt v3.14.1; the
same installer supplies the self-test. Its Bash syntax tree identifies executable
commands, including substitutions inside unquoted heredocs, without treating
quoted heredoc payloads or string contents as commands. Python steps and stdin
heredocs owned by `python -` or `python3 -` are inspected through Python's syntax
tree. A consumer table binds shell options to the commands that interpret them;
arguments to other commands remain data. The checker follows `run_isolated` and
`timeout` wrappers. An unresolved dynamic command carrying a control option
fails closed. Missing tools and parse failures also fail the check. This is
static discovery, not shell evaluation.

The configured `fixtures/declared` caller also passes all declared checks.
Workflow-body probes cover declarations, isolation, real tools, wasm and
browser failures and pinned tooling. The required consumer result also includes
[the pinned Maestro browser qualification](docs/caller-checks.md#local-build-and-runtime-command),
using extracted shared setup and recipe bodies. A developer-tool fixture proves build-script
tool availability across compiling jobs. Isolated tests spawn caller-pinned
`just` and `prek` only when opted in. Native
runner proofs detect deliberate Windows/macOS-only failures. The `fixtures/fuzz`
caller runs the fuzz job on hosted runners: its planted crash, stall and large
allocation each give one finding, its clean target gives none, and the report
code files them through a fake `gh`, so the self-test never opens a real issue.
These feed the same required results.

The final checks are `Required repository quality` and
`Required consumer tests`.

## Versions

Release-please opens release pull requests and tags merged releases, starting
at `v0.1.0`. Each new release requests Quality sync before asset generation. A
notification failure fails the release job; the published release remains, but
asset generation does not start. CI changes, features and fixes appear in the
changelog.

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

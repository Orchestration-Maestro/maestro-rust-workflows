# Caller-declared checks

Add `.github/ci.toml` at the caller repository root to opt into isolated tests,
real search tools and optional builds. Organization calls use that root; the
workflow's directory input exists only for its fixture self-test, pinned
workspace proof, and advisory and fuzz scans. Optional `checkout-repository` and
`checkout-ref` inputs serve the same four: when unset they check out the caller
repository and commit. `checkout-base-ref` and `test-event` are overrides
restricted to this workflow repository; they default to the real PR base and event.

The full and selected-package proof consumers use one immutable Maestro source
snapshot. A warm build uses its parent commit; selected-package proof sets that
parent as its PR base. The warmer is non-required, and consumers still run if it
fails. See [Compile cache](../README.md#compile-cache) for snapshot shapes,
fail-open transfers and storage limits.
Advance all proof checkout references together after checking locked dependencies.
The proof uses `12614d3236fba10d81afd2191a32ade98700f5e2`, which includes the
native residency-helper compilation repair. Its parent,
`cb438128e64e27be85dcafbdf3275dfa058e2781`, supplies the warm build and
selected-package base.
The coverage proof runs only on pushes to `ci/228-merge-proof`, not on pull requests.
Check its results before landing.

```toml
wasm_crates = ["my-browser-library"]
check_targets = ["x86_64-pc-windows-msvc"]
browser_build = false
browser_test_crates = []
test_tools = false
```

## Coverage baseline and caller permissions

When updating the shared workflow release pin, callers grant `actions: read`
to the reusable-workflow call. The shared test-selection job needs it to read
coverage artifacts from earlier runs; all other jobs retain their existing
permissions. A missing caller grant can prevent a reusable workflow from
starting, so add the grant in the same PR as the release-pin update.

The workspace coverage floor remains 80% (or the caller's higher floor).
A narrowed merge group combines fresh selected-package LCOV with unchanged
packages' LCOV from the latest successful full run on main's ancestry.
Narrowing requires a `.github/ci.toml` declaration; undeclared callers run the
full suite. Selected-package coverage is reported separately, for information
only; it has no per-package floor. A selected package with no instrumented lines
reports `N/A (0/0)`; only the workspace union is gated. Selection compares the
baseline head with the group head, so packages changed since the baseline also
run.

A successful full aggregate publishes `coverage-<key>-full`, containing the
source head, archive compiler checkout root, package roots and shard count. The
aggregate uses the compiler job's root, even when its own runner has a different
workspace path. The selector accepts
only this manifest together with all of its run's LCOV shard artifacts. Missing
or expired artifacts, unavailable API access and an unrelated baseline cause a
logged full-suite fallback. Historical runs without a manifest are not used;
the first full run after rollout supplies the initial baseline. Main pushes and
full fallbacks retain full-workspace coverage.

## Warm test filter

`warm_test_filter` is an optional nextest filterset string used only by
`warm-cache: true`. With no declaration or no key, it defaults to `none()`:
compile the full coverage test graph, but run no tests. For example:

```toml
wasm_crates = []
check_targets = []
warm_test_filter = 'test(child_build)'
```

Choose tests that start child compiles. The warm build still compiles the full
coverage test graph, then runs only the selected tests under the existing
isolated coverage runner. It ignores the caller's default test filter and
produces no coverage report or doctest run. Ordinary tests and package selection
ignore this key.

Declaration validation checks only that the value is a string. Nextest parses
its grammar during the warm run: an invalid expression or an explicit empty
string fails that run. Whitespace, quotes, backslashes, Unicode and multiline
expressions pass through unchanged; the value is never evaluated as shell code.
A valid filter matching no tests succeeds under the existing no-tests policy.

Running tests adds execution time and compiler objects. Keep the filter narrow
enough for the warm build to finish between default-branch pushes and for the
native payload to stay within 1 GiB. The existing writer and size rules still
apply: only an eligible successful default-branch warm build saves a snapshot;
a pull-request run cannot prove saving. More cached compiles are useful only
when that save succeeds and later jobs restore the compatible snapshot.

## Reusable-call concurrency

The optional workflow input `concurrency-key` defaults to an empty string,
keeping the existing per-ref concurrency group. If a workflow calls this
reusable workflow more than once, give each calling job a distinct, stable
key (case-insensitive), such as `concurrency-key: quality`. Reuse that key on
later pushes so a new pull-request run cancels the older invocation of the
same job, not another job in its own run. Do not use a run ID as the key.
Merge-queue runs never cancel running checks. The caller must not share the
same concurrency group with its called workflow; see GitHub's
[reusable workflow concurrency rules](https://docs.github.com/en/actions/reference/reusable-workflows-reference#limitations-of-reusable-workflows).

## Declaration

The `wasm_crates` and `check_targets` arrays are required, including when empty. `browser_build` and
`test_tools` are optional booleans that default to `false`. The
[schema](../.github/ci.schema.json) describes the parsed TOML shape; there is no
version key or older-format reader.

`reserved_ports` is an optional array of unique integers from 1 to 65535. When
set, every Linux test job runs
`sudo sysctl -w net.ipv4.ip_local_reserved_ports=<list>` before the tests, so
tests that must bind a fixed local port are not displaced by an outbound
connection that the kernel assigned that port. Without it nothing runs.

An absent file runs the standard checks without isolation, search tool
installation or optional jobs. After declaration validation, pull requests
run formatting, Clippy, docs, dependency/source checks and declared Windows
target checks in parallel. Merge groups and main pushes also run tests, coverage and
all declared foreign-target checks. Other foreign-target checks (including
macOS), wasm/browser builds and Chromium tests run only on those full events.
An unreadable file, malformed TOML,
duplicate key, unknown key, missing array, scalar array value, non-string
member or non-boolean tool/build flag fails declaration validation. An empty
present document is invalid. Integer and string booleans are not accepted.

Names are exact, without whitespace or Unicode normalization. Each wasm name
must match a package in the caller's locked Cargo metadata. Each target must
match the caller compiler's target list and the runner table below. An unknown
package, unknown target or recognized target without a runner fails with a
file/key diagnostic before optional work is emitted. Metadata or compiler
inspection failures also fail validation. Wasm and target array order and repetitions are
retained; declaration text is never executed as shell syntax.

## Compile cache

Cached compiling jobs export `CARGO_BUILD_RUSTC_WRAPPER=sccache`, so callers do not
need to set `RUSTC_WRAPPER` for caching. A build that a test starts after removing
`RUSTC_WRAPPER` keeps the cache without coverage instrumentation. Coverage runs
still instrument workspace builds and send their compiles through the cache.

| Jobs | Host | Restore | Save |
| --- | --- | --- | --- |
| Format and lint, Docs, Tests, Doctests, Dependency/source/coverage checks, Wasm and browser build, Chromium browser tests | Linux | Every call that runs the job | Default-branch pushes only; one writer per test shape |
| Declared target checks | Linux, arm64 macOS | Every call that runs the job | Default-branch pushes only; each target-matrix entry saves, including repeated targets |
| Declared target checks | x86_64 Windows | Never | Never |

Windows checks run uncached because sccache expands response files beyond
Windows' command-line limit; [Windows caching](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/166)
tracks the upstream fix.

A push with `warm-cache: true`, including a feature-branch push, also runs the
declared target checks; warm calls for other events skip them. Only
default-branch pushes save those snapshots. Host-specific, checksum-pinned assets
are listed in [Compile cache](../README.md#compile-cache).

The 1 GiB native-payload bound applies to each save, not the repository's
10 GB cache allowance. Compressed snapshot sizes are measured separately;
GitHub eviction can remove even the newest snapshot of a shape.

## Test shards

Tests use checksum-pinned cargo-nextest 0.9.148 with sixteen automatic
`--partition count:N/16` partitions. The caller's `.config/nextest.toml` supplies
test-suite runner metadata such as resource groups for fixed-port tests. Shared
CI controls partitioning and coverage; profile default filters are ignored so
they cannot omit tests. Runner user configuration is not loaded. `test_shards` was removed; setting it fails
with a clear diagnostic. Unit and integration tests run exactly once; doctests
run once with `cargo test --locked --doc` in a parallel `Doctests` matrix leg
for merge groups and main pushes, using the selected packages described below.
Main pushes always select every workspace package.
Pull requests skip tests unless the [included-input rule](#pull-request-selection-and-retries) applies.

Declared callers run `cargo llvm-cov nextest` in the same isolated environment
with their opted-in tools. Only the sixteen numeric shards upload LCOV, without
individual thresholds. `Tests (ubuntu-latest)` requires all matrix legs,
including `Doctests`, to succeed on full runs. On pull requests it requires a
successful declaration and either event-skipped shards or successful included-input shards. On full runs it downloads all
sixteen reports, counts each source line once (covered if any shard hits it), and
enforces `coverage_min_lines` on the union. Missing reports or low coverage fail.
Undeclared callers use nextest partitions but retain their single coverage job
for merge groups and main pushes. Both coverage modes run tests through nextest,
so caller setup scripts run in either mode (reusing prepared outputs in shards). Single-job coverage uses one partition
(`count:1/1`); declared callers combine sixteen reports. Both modes use the same
line-counting policy: normalize source paths, count each line once, and enforce
the coverage floor on the union of hits. PRs skip coverage for all callers.

### Build once, test many

One `Build test archive` job compiles the selected unit and integration tests with
`cargo nextest archive` (or `cargo llvm-cov nextest-archive` for declared coverage).
Numeric shards download that archive and run `--archive-file`, keeping the same
partitions, retries and coverage reports. Existing locked dependency fetches stay
available for tests that intentionally start child builds. `Doctests` compiles and
runs doctests once in parallel with the build, including on included-input PR runs.
Warm-cache runs keep their existing compile route and cache shape.

A caller that needs prepared files can declare one standalone shell command:

```toml
wasm_crates = []
check_targets = []
test_prepare = "sh .config/prepare-tests.sh"
```

`Prepare test setup` runs that command once, in parallel with host compilation,
after the caller's declared tools and locked manifests are available. It starts
from the caller's working directory. `TEST_PREPARE_OUTPUT` names the fixed
`target/test-setup` output directory there; write only reusable runtime outputs
inside it. `CARGO_TARGET_DIR` points at a separate temporary build directory.
Preparation must not depend on the host test binaries or coverage instrumentation.
A failing command fails the required test result. The default is no preparation.

The workflow transfers the prepared directory separately, preserving file modes,
and restores it at `target/test-setup` in each numeric shard. Archived isolated
shards receive `TEST_PREPARE_OUTPUT` as the absolute path to that restored
directory, replacing any inherited value. Callers may rely on it to locate
prepared files during archived execution when `test_prepare` is declared. Warm-cache
runs also receive this path, but do not run preparation or restore prepared files.
Archived runs without `test_prepare` likewise have no prepared files restored.
The variable alone does not guarantee files exist; callers must check for the
files they need before reusing them. No nextest archive include is needed.
Nextest setup scripts can export paths into that directory;
they must reuse those files instead of preparing them again. The caller owns the
preparation logic and exported environment. Shared CI does not interpret its
setup-script definitions. The existing `concurrency-key` distinguishes both
artifacts when a workflow makes several calls.

### Test diagnostics

Declared callers receive `MAESTRO_TEST_DIAGNOSTICS_DIR`, an absolute path to
`target/browser-author-diagnostics` under the caller's working directory. The
workflow creates it outside disposable HOME/TMP isolation and forwards it to
partition runners and the doctest owner described in [Test shards](#test-shards).
An inherited value is not used.

Caller tests own transcript production: write regular files there, flush each
phase before waiting on a child, and exclude credentials and other sensitive
content. Use distinct files for tests and attempts. The workflow does not create
transcripts or collect the environment. Callers without transcripts need no
changes.

After isolated tests, `Print test diagnostics` prints regular transcript files,
including unfinished final lines, with Actions command processing disabled.
`Upload test diagnostics` retains only that directory and `nextest.log`, even
when the isolated step fails or reaches its existing 45-minute cap. Missing or
empty diagnostics are allowed; retention does not turn a failed test green.
Runner loss, cancellation or an enclosing job timeout can prevent these steps
from running.

Artifact names are
`test-diagnostics-<repository-id>-<caller-digest>-shard-<shard>-run-<run-id>-attempt-<run-attempt>`.
The shard field is `1` through `16`, or `doctests` for the parallel doctest leg
(`shard-doctests`). That leg normally has no `nextest.log`; the same missing-file
tolerance applies. The caller digest is the full lowercase SHA-256 of the UTF-8 compact JSON array
`[checkout-repository-or-github.repository, working-directory-or-".", concurrency-key-or-""]`,
with Unicode unescaped. It distinguishes repositories, caller directories and
reusable call keys without exposing path characters in artifact names.

Find the artifact name in the run's artifact list, download it, and read the
caller's marker and last phase (replace the example records with your own):

```bash
gh api "repos/$repo/actions/runs/$run/artifacts" --jq '.artifacts[].name'
gh run download "$run" --repo "$repo" --name "$artifact" --dir "$download"
cat "$download/nextest.log"
grep -RFn -- 'CHILD-READY-MARKER' "$download/target/browser-author-diagnostics"
grep -RFn -- 'phase: waiting-for-child-exit' "$download/target/browser-author-diagnostics"
```

### Pull-request selection and retries

Pull requests run fast checks only, except that changes to literal `include_str!` or `include_bytes!` inputs also run test shards for the reading crates and their reverse dependencies, without coverage.
The declaration still computes changed workspace crates and their reverse
dependencies for the self-test event mode. A crate manifest selects its
crate; lockfile changes compare the resolved transitive dependency identities
and features at base and head, selecting affected crates. Root manifest,
toolchain or CI workflow/schema changes select everything. Non-test declaration
keys (such as spelling, wasm and foreign-target lists) select no extra tests;
changes to test tools, coverage policy, additional locked manifests, declared
test inputs or declaration presence select everything.
For paths outside a crate, fallback selection skips undeclared Markdown files
and paths under `docs/`; other undeclared files select everything. The workflow
and configuration rules above take precedence. Declared paths select their
readers, including for Markdown changes.
Every enabled shard fetches locked dependencies.
Declared merge groups select workspace crates changed between the verified
coverage baseline and the group head, plus every direct or transitive workspace
dependent from locked Cargo metadata. Normal, build, development and
target-specific dependency edges all count; unrelated crates are not selected.
Changes to `Cargo.lock`, `.config/`, `.github/` (including the caller declaration),
the toolchain or any root file select the full suite. Unknown paths, an absent or
unreadable merge-group base, an unresolved dependency graph, or a selection with
no workspace test packages also select the full suite and log the fallback reason. Unlike pull requests, merge groups do not
skip undeclared documentation changes.

Narrowed coverage tests run the selected packages. The aggregate filters their
own source records and combines them with unchanged baseline packages for the
workspace floor; selected-package coverage is informational. See
[coverage baseline and caller permissions](#coverage-baseline-and-caller-permissions)
for the artifact contract and fallback rules. Main pushes always test and measure
the full workspace after every landing; that full-suite safeguard must be deployed before narrowed merge-group
testing is enabled. Its failure reporting and revert handling live in the
post-landing workflow. Release graph selection together with the single-build
test archive so the shared test build compiles only selected packages.

`test_inputs` maps repository-relative files or directories outside the
workspace members to the exact package names whose tests read them. A change
under a declared path selects those packages and their reverse dependencies,
including Markdown files there. An empty list marks a path no workspace test
reads, such as a separate fuzz workspace. Paths must stay inside the repository
and names must be workspace packages; anything else fails the declaration.

```toml
test_inputs = { "packages/" = ["packages-crate"], "layers.json" = ["conventions"], "fuzz/" = [] }
```

CI allows two retries for each nextest test. A pass on retry remains a flaky
pass, named in the job summary; permanent failures fail the shard. Caller
resource groups are retained, but default filters cannot suppress tests and
caller retry counts cannot override this policy.

## Required dependency, source and coverage checks

No consumer file is required. The shared workflow embeds the defaults and
installs checksum-verified official releases: cargo-deny 0.20.2,
cargo-machete 0.9.2, cargo-hakari 0.9.39, typos 1.51.1, gitleaks 8.30.1, zizmor 1.30.1,
actionlint 1.7.12 and cargo-llvm-cov 0.9.1. Checksums and download URLs are
pinned together in the workflow. LLVM tools come from the caller's rustup
component distribution, not a caller-provided executable.

- cargo-deny checks licenses, bans, sources and advisories across the workspace
  with all features. Advisories and yanked crates without declared exceptions fail. Registries
  other than crates.io and undeclared git sources fail. Licenses, bans and sources run on
  every event. A pull request or merge group checks advisories and yanked crates when it changes
  `Cargo.lock`, or `advisory_exceptions` or `yanked_crate_exceptions`
  in `.github/ci.toml`, including exception removal. An absent base also selects
  the check. An unreadable nonempty base selects the check for merge groups;
  pull requests fail package selection before the advisory check. Other declaration
  changes, such as `spelling_words`, don't select the check, and the step prints that
  advisories were not checked.
  Pushes to `main`, other events and the scheduled scan always check them, so they still
  catch an advisory that reaches `main`.
- The scheduled advisory scan (`.github/workflows/advisories.yml`) runs this workflow in
  advisory-scan mode against Maestro's `main` every hour and on manual dispatch: only the
  declaration and the advisory check run, with the declared exceptions. Each undeclared
  advisory or yanked crate opens one issue in that repository, titled `Priority: <identifier> on main`
  and its crates, unless an open issue title already starts with `Priority: <identifier> on main`; the
  run fails while any remains. Pull requests that change the workflow run the scan without
  opening issues.
- The cargo-deny license allowlist is MIT, Apache-2.0, ISC, BSD-2-Clause, BSD-3-Clause, Zlib,
  Apache-2.0 WITH LLVM-exception, Unicode-DFS-2016, Unicode-3.0, CC0-1.0,
  CDLA-Permissive-2.0, MPL-2.0, MIT-0 and BSL-1.0. BSL-1.0 is the Boost Software
  License; the Business Source License (BUSL-1.1) is absent from this default list.
  Callers can allow it for an exact package version through `license_exceptions`.
- cargo-machete fails on unused dependencies. Nonempty `ignored` or `renamed`
  package/workspace cargo-machete metadata is rejected; ignored source files
  remain checked, but generated target directories are skipped.
- cargo-hakari runs `cargo hakari verify` only when the caller has
  `.config/hakari.toml`; a stale generated feature-unification package fails.
- typos checks spelling with its defaults and no implicit consumer config.
- gitleaks scans the checked-out tree with the default rules and redacted
  findings. Consumer secret configs, inline allows and ignore fingerprints
  cannot suppress findings; a nonempty root `.gitleaksignore` is rejected.
- zizmor and actionlint check the caller's workflow files when any exist.
  Consumer security configurations are not loaded. Inline zizmor ignore
  comments are rejected with a file/line diagnostic; declared exceptions go
  through the caller declaration below.
  actionlint's optional external shellcheck/pyflakes integrations are disabled;
  its built-in correctness checks remain active.

Undeclared coverage runs `cargo llvm-cov --workspace --locked
--fail-under-lines 80`. Declared coverage runs within the test partitions and
its threshold is enforced by `Tests (ubuntu-latest)`, not by the dependency job.
Neither accepts consumer coverage exclusions or external LLVM overrides.
The Tests, Isolated tests and undeclared Coverage steps retain their 45-minute
timeouts to bound hung caller tests.

### Declaration extensions

See [Additional locked manifests](#additional-locked-manifests) for independent
workspace dependency provisioning.

The following optional root keys extend the defaults. Both existing arrays
remain required when the declaration is present:

```toml
wasm_crates = []
check_targets = []
additional_locked_manifests = ["fixtures/author build/Cargo.toml"]
coverage_min_lines = 90
spelling_words = ["Maestro"]
git_sources = ["https://github.com/example/library"]
privileged_trigger_workflows = [".github/workflows/review-gate.yml"]
license_exceptions = [
  { package = "specific-library", version = "1.2.3", licenses = ["0BSD"] },
]
advisory_exceptions = [
  { id = "RUSTSEC-2025-0141", package = "bincode", version = "1.3.3", reason = "Packaged syntax and theme dumps; reviewed exception" },
  { id = "RUSTSEC-2025-0141", package = "bincode", version = "2.0.1", reason = "Protocol encoding; separately reviewed exception" },
]
# Time-boxed; remove when the dependency is upgraded.
yanked_crate_exceptions = [
  { package = "specific-library", version = "1.2.3" },
]
```

`coverage_min_lines` must be a number from 80 through 100; boolean, non-finite
and lower values fail validation. The default is 80. `spelling_words` and
`git_sources` are arrays of nonempty strings. Repeated spelling words count
once, and Unicode words are preserved. Quality string values cannot contain
control characters or surrogate code points. Git sources must be exact HTTPS
repository URLs without credentials, query strings or fragments, never
organization-wide or prefix exceptions. License exceptions require exactly
`package`, an exact `version` (not a range), and a nonempty `licenses` array;
they add licenses for only that package version. All arrays default to empty.
Malformed extension values fail declaration validation, and invalid license
expressions fail cargo-deny. Advisory exceptions excuse only named advisories;
no key disables bans, source checks, spelling defaults, other workflow audits or
secret detection.

`advisory_exceptions` defaults to an empty array. Each entry requires exactly
`id`, `package`, `version` and `reason`, all nonempty strings subject to the
quality-string rules above. The ID must match `RUSTSEC-YYYY-NNNN`; the version
must be an exact semantic version, not a range. The package/version pair must
remain in the caller's `Cargo.lock`; otherwise the required check fails with a
stale-exception diagnostic naming the advisory, package and version. Wherever
advisories are checked, the pinned cargo-deny first reports the advisories without
any ignore. Each declared ID must be reported for its exact declared package and
version; otherwise the check fails naming the ID and the package and version
reported, or "no such finding". Every package/version pair reported for an ignored
ID needs its own exact, reasoned entry, including separate entries when multiple
versions of the same package are affected. Missing entries fail naming the
advisory, package and version; unaffected locked versions need no exception.
When advisories are skipped because dependencies are unchanged, the workflow does
not probe or require reported-pair coverage, but still rejects stale entries.
The shared workflow renders each declared ID and reason into cargo-deny's advisory
ignore list.
Unlisted advisories remain checked wherever advisories are checked, including the
scheduled scan; yanked crates require their own exceptions below.

`yanked_crate_exceptions` defaults to an empty array and is intended for
reviewed, time-boxed use while an upgrade is prepared. Each entry requires
exactly `package` and `version`, both strings: an exact ASCII crate name
(letters, digits, hyphens and underscores, starting with a letter or digit)
and a complete semantic version. Bare names, ranges, unknown fields and
malformed entries fail declaration validation. Before rendering, each package
and complete version string, including any build metadata, must match a pair
in the caller's `Cargo.lock`; otherwise the check fails with a stale-exception
diagnostic naming the package and version. The workflow renders each pair
only as a cargo-deny yanked-crate ignore (`crate@=version`); another version
of the same crate or an unlisted yanked crate still fails wherever advisories are checked. Advisory, license,
ban and source policies are unchanged. Track the removal deadline and upgrade
in the consuming repository's declaration comment and issue.

`privileged_trigger_workflows` is an optional array of unique, exact, existing
`.github/workflows/<name>.yml` or `.yaml` file paths. It defaults to empty.
Each listed file ignores only zizmor's `dangerous-triggers` audit, allowing
reviewed privileged-trigger workflows such as fork-request gates. Every other
security audit and actionlint correctness check remains active on those files;
unlisted workflows retain all audits. Review these workflows to ensure they
never check out or execute contributor code.

Empty entries, globs, `..`, nested or outside paths, colons, backslashes,
control characters, non-string entries, missing files and duplicates fail
validation with the key named. Colons are forbidden because zizmor uses them
for line/column ignore selectors. The shared workflow generates its own
explicit zizmor config containing only these basenames; repository zizmor
configs cannot add exceptions. Inline ignore comments are forbidden, including
on declared files. No declaration means no audit exceptions.

### Additional locked manifests

`additional_locked_manifests` is an optional array of nonempty strings, defaulting to `[]`. Use it when tests build an independent Cargo workspace whose dependencies are not covered by the repository root's lockfile. For example:

```toml
wasm_crates = []
check_targets = []
additional_locked_manifests = ["fixtures/author build/Cargo.toml"]
```

Each value must name an existing `Cargo.toml` file by a relative path from the caller directory. Absolute paths, missing files, directories, control characters and paths that resolve outside that directory, including through symlinks, fail declaration validation. Internal symlinks and relative components that still resolve inside the directory are allowed. Paths are literal: whitespace, Unicode and shell punctuation are not expanded or normalized. Declaration order and repeated entries are retained.

Every test shard fetches each declared manifest with `cargo fetch --locked --manifest-path <path>` before starting its tests or declared coverage. Existing root-workspace fetching remains unchanged. The step is skipped when the list is omitted or empty. Cargo chooses the manifest's workspace and lockfile; a missing or stale lock fails provisioning with the manifest identified, Cargo's diagnostics visible and its nonzero status preserved. Provisioning may use the network and write Cargo's dependency cache; it does not generate, update or repair source manifests or lockfiles. Tests can subsequently build offline from the acquired sources, including by staging a temporary directory-source bundle; a cold, empty Cargo home alone does not contain that cache.

Changes to this input select all workspace test packages in the declaration's pull-request selection mode, like other test-affecting declaration changes. Pull requests follow the [included-input rule](#pull-request-selection-and-retries); merge groups and main pushes run full tests and coverage. Adoption requires a release containing this input and the organization rules' version pin to that release.

## Isolated tests

Opted-in Linux tests and coverage install ripgrep 15.2.0 and fd 10.5.0 from
pinned Linux x86_64 musl release archives, verifying their published SHA-256
digests before extraction. The dedicated binary directory exposes `rg` and
`fd` directly on `PATH`. Every tool-installation step has a 10-minute timeout
to fail stalled downloads or installs. Before tests start, a presence guard rejects exported provider API/OAuth
keys, GitHub/Hugging Face tokens, cloud project/profile/region selectors,
cloud credential paths/URIs and the extensive-model selector listed in the
[shared specification](https://github.com/Orchestration-Maestro/.github/blob/main/docs/specs/2026-10-04-shared-rust-ci.md).
It additionally rejects every exported name ending in `_API_KEY`, including
empty values. Diagnostics identify variable names, never credential values.
The exact guarded list is in the shared workflow's isolated-test step.

The child keeps only `PATH`, resolved `CARGO_HOME` and `RUSTUP_HOME`, the
checkout-selected `RUSTUP_TOOLCHAIN`, fresh `HOME`, `TMPDIR`, `TMP`, `TEMP` and all XDG configuration/data/cache/state/runtime
locations, plus `MAESTRO_NO_LOCAL_LLM=1` and the workflow-computed
`MAESTRO_TEST_DIAGNOSTICS_DIR` described in [Test diagnostics](#test-diagnostics). `TMP` and `TEMP` equal `TMPDIR`; the
other locations are separate, initially empty directories beneath a unique
runner-temp parent. Unrelated CI state, proxy settings and application/provider
overrides are not inherited. The partition argv uses `cargo llvm-cov nextest`; doctests use
`cargo test --workspace --locked --doc`.

The test setup exports Rustup's active toolchain before compilation; both test
routes pass it to child builds, including dependencies outside the checkout.
Explicit Bash run defaults preserve runner failures through log capture. A failed
runner fails its test step, so coverage uploads run only after success; a missing
report after success fails the upload separately. On full runs the required aggregate accepts
only successful matrix legs, including `Doctests`, and requires the sixteen numeric
reports when merging coverage. On pull requests the aggregate requires a successful
declaration and either skipped shards or successful included-input shards.

The original home and auth file are never read, moved or modified. Only the
isolated directories are cleaned on success or failure; other action steps
retain the runner's normal home and cache access.

## Caller-pinned build and test tools

With `test_tools = true`, every enabled caller-compiling job installs the
caller's `mise.toml` tools before lint, documentation, test/coverage partitions,
foreign-target checks and wasm builds, including their build scripts. The shared
checksum-verified mise 2026.10.2 bootstrap selects the actual runner host: Linux
x64/ARM64, macOS x64/ARM64 or Windows x64, independently of the Rust target.
Provide pins for the tools your builds and tests spawn, for example:

```toml
[tools]
just = "1.58.0"
prek = "0.5.3"
```

Build-tool versions come from the caller. The optional
[browser runtime tools](#chromium-browser-tests) have shared pins instead. Missing
`mise.toml` or a failed installation fails the job. Installing pins does not
supply undeclared build tools. False or omitted `test_tools` skips this opt-in;
a `mise.toml` alone does not enable it. Noncompiling jobs install no caller tools.
An absent declaration still runs the original unisolated tests.
The nextest partition and doctest argv, credential guards,
fresh directories and the child environment allowlist are unchanged. Only the
installed tools' executable paths are added, not mise's environment settings.
Windows paths are published in native format for later steps.

## Wasm and browser builds

For each listed package, Linux runs:

```bash
cargo build --locked --target wasm32-unknown-unknown -p "$CRATE"
```

The caller toolchain installs wasm support only when there are listed
packages. Either `test_tools = true` or `browser_build = true` installs caller
tools once before wasm compilation. The browser flag is independent: it does
not enable tools in other jobs. With `browser_build = true`, the workflow runs
`mise exec -- just browser-build` after successful wasm builds, including when
the wasm array is empty. A failed wasm build prevents the recipe. False or
omitted `browser_build` skips the recipe and its `just`-pin validation, even
when tools are enabled for wasm compilation.

Provide a `mise.toml` pinning the recipe's tools, including `just`, and a
`justfile` with `browser-build`. A missing tool configuration, installation
failure, missing recipe or nonzero recipe result fails CI. The shared workflow
bootstraps mise 2026.10.2, reusing that exact installed version or verifying the
published SHA-256 of its official runner-host binary before execution. It does
not override the caller's tool versions; the self-test uses just 1.58.0.

The caller owns the browser application, entry, temporary bundles and
application-specific diagnostics. A wasm build or recipe invocation alone is
not proof of browser runtime behavior.

## Chromium browser tests

`browser_test_crates` is an optional array of exact locked workspace package
names, defaulting to `[]`. Repeated names request one job, not repeated recipe
invocations. An absent declaration, omitted key or empty array schedules no
browser runtime job. Wrong types and unknown names fail declaration validation.
The existing `wasm_crates` and `check_targets` arrays remain required.

One Linux `browser-tests` job invokes `just browser-test` on merge groups and
main pushes, which select the full requested suite. Ordinary pull requests skip
it, and advisory-only runs never start Chromium. The pull-request selection rule
is self-test behavior only: a requested package in the
changed-package/reverse-dependency selection, or a change to a nonempty browser
request, selects the job without widening native package selection, and
removing the request skips it.
`browser_build` and `test_tools` remain independent opt-ins. Native coverage
selection and its floor do not change.

The caller supplies a `mise.toml` just pin and a `browser-test` recipe. The job
installs caller tools first, then these shared runtime tools:

| Tool | Pin | Archive verification |
| --- | --- | --- |
| Chromium Linux x64 | 156.0.8078.4, revision 1248 | SHA-256 `43fd31766c7533a02dda2ded5ad50c2f861cd5400be7509c0abad71129f79d79` |
| Playwright | 1.64.1 | SHA-512 `HCrVhHr5QQuwYoswEUGTb7gGp522yflbv/ST8nF4sFWGuLjQCt17Q/EubpGGSNKYn1tsTCPZxqHhHCfVURu7UA==` |
| Playwright core | 1.64.1 | SHA-512 `BC4s8y6SEVA6Q1rSxZTto8hI1KUI3xXlHGZgvGqP4IqPdI/a6FX9H29IDNz7vKo9l21b6LfYosScPL6M3xxyDQ==` |
| wasm-bindgen CLI | 0.2.129 | SHA-256 `82d12bb940e2d4e72e0d5605387fc1b8ca179044e012b620f0ce4e7440e8320e` |

The [production installer](../.github/workflows/ci.yml) contains the exact
archive URLs. It verifies each archive before unpacking, installs no npm
lifecycle scripts or second browser, and checks the two-package JavaScript
runtime closure. Node must be at least 20; the job logs its actual version,
Playwright, Chromium and binding CLI versions. Playwright's public `install-deps
chromium` command installs Linux system libraries and may require sudo.

The recipe receives absolute `PLAYWRIGHT_MODULE` and `CHROMIUM_PATH` values.
The verified binding CLI directory precedes caller tools on PATH. The job runs
`just browser-test` directly, without a later mise PATH rewrite. The consumer
must check its resolved binding crate against CLI 0.2.129, reject missing or
empty export selections, await each export and report completed/expected counts.
Download, installation, missing just pin/recipe, version mismatch and nonzero
recipe results fail the job and its required caller result.

### Local build and runtime command

From the consumer checkout on Linux, set `WORKFLOW_CHECKOUT` to a checkout of
the workflow release being qualified. This runs the same verified installer
(including system-library setup), then the same fixed recipe. It requires
Python, Node >=20, curl, unzip, tar and the caller-pinned just on PATH:

```bash
export WORKFLOW_CHECKOUT="$HOME/src/maestro-rust-workflows"
python3 - <<'PYTHON'
import os
import pathlib
import re
import subprocess
import tempfile
import textwrap
source = (pathlib.Path(os.environ['WORKFLOW_CHECKOUT']) / '.github/workflows/ci.yml').read_text(encoding='utf-8')
step = source.split('      - name: Browser runtime tools\n', 1)[1]
command = textwrap.dedent(re.search(r'        run: \|\n((?:          .*\n|\n)+)', step)[1])
with tempfile.TemporaryDirectory() as temporary:
    paths, values = pathlib.Path(temporary) / 'paths', pathlib.Path(temporary) / 'env'
    env = os.environ | {'RUNNER_TEMP': temporary, 'GITHUB_PATH': str(paths), 'GITHUB_ENV': str(values)}
    subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', command], env=env, check=True)
    env.update(line.split('=', 1) for line in values.read_text(encoding='utf-8').splitlines())
    env['PATH'] = os.pathsep.join(reversed(paths.read_text(encoding='utf-8').splitlines())) + os.pathsep + env['PATH']
    env['CARGO_BUILD_JOBS'] = '3'
    subprocess.run(['capped', 'just', 'browser-test'], env=env, check=True)
PYTHON
```

In Maestro, the recipe builds an unregistered scratch cdylib from unchanged
sandbox test sources with root-only test configuration and locked external
dependency identities. It packages web bindings with `--split-linked-modules`
to preserve relative fixture imports, then awaits 26 execution and 19 loading
exports with a fresh page per export. The registered integration target remains
compile/lint proof only: its libtest initialization starts native cases before
browser exports. Offline-card exports are excluded while their dependency graph
contains egui/epaint; session-header canvas tests are excluded. Transcript
execution waits for [the transcript delivery](https://github.com/Orchestration-Maestro/maestro/issues/801).
See [Maestro's consumer harness](https://github.com/Orchestration-Maestro/maestro/issues/1809)
for the build/package/driver owner and its development documentation.

The declared fixture proves reusable-job wiring with a real browser assertion,
not the consumer's Rust assertions. Real-consumer shared-job-body qualification
uses the immutable main commit containing the harness, all 45 exports, seeded
failures in both families, missing/zero exports and asynchronous rejection,
followed by restored sources. The internal self-test runs this matrix against a
pinned Maestro main commit and includes it in Required consumer tests. Browser
setup and the recipe are extracted from the shared workflow with the same
extractor as the coverage proof; seam tests reject copied setup bodies. Panic
controls require the failing export name and WebAssembly trap, not assertion
text, which the driver does not expose. Release waits for hosted qualification.
Caller activation is separate: release, re-pin, opt in, then observe the first
actual Chromium job pass. Workflow landing or a job-body proof is not activation;
[the delivery issue](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/98)
stays open until that observation. Record cold/warm cost from hosted runs rather
than treating estimates as deadlines.

## Foreign-target checks

The caller's rustup toolchain installs each requested target and Clippy, then runs:

```bash
cargo clippy --workspace --all-targets --locked --target "$TARGET" -- -D warnings
```

| Target | Runner |
| --- | --- |
| `aarch64-unknown-linux-gnu` | `ubuntu-24.04-arm` |
| `aarch64-apple-darwin` | `macos-latest` |
| `x86_64-apple-darwin` | `macos-latest` (requested target installed) |
| `x86_64-pc-windows-msvc` | `windows-latest` |

There is no Linux fallback for Apple/MSVC targets. These compile checks do not
restore macOS/Windows runtime tests, which remain a before-first-release
requirement. Merge groups and main pushes run every opted-in target check.
Ordinary pull requests run only a declared `x86_64-pc-windows-msvc` check.
Declaration errors fail CI even when dependent jobs skip.

## Nightly fuzz job

A caller declares its cargo-fuzz targets with two keys that go together;
declaring one without the other fails validation:

```toml
fuzz_manifest = "fuzz/Cargo.toml"
fuzz_targets = [
  { name = "parse", seconds = 600, max_len = 4096, timeout = 10, rss_limit_mb = 2048 },
]
```

`fuzz_manifest` is the literal relative path of the fuzz crate's `Cargo.toml`.
The declaration checks it like an additional locked manifest: it must be an
existing file inside the repository. Keep that crate outside the caller
workspace, with its own empty `[workspace]` table as cargo-fuzz lays it out, so
no pull-request job builds it. The nightly license check runs locked over that
crate, so it needs an allowed `license` field in its `Cargo.toml` and a
committed `Cargo.lock`, as `fixtures/fuzz/fuzz/` has. A crate made by
`cargo fuzz init` has neither, and every nightly run fails until both exist.
Every `fuzz_targets` entry needs all five fields, and each number is a positive
integer:

| Field | libFuzzer flag | Meaning |
| --- | --- | --- |
| `name` | | Fuzz target (`[[bin]]`) name: letters, digits, `_` and `-`, unique |
| `seconds` | `-max_total_time` | Total fuzzing time in seconds |
| `max_len` | `-max_len` | Largest generated input in bytes |
| `timeout` | `-timeout` | Seconds one input may run before it counts as a timeout |
| `rss_limit_mb` | `-rss_limit_mb` | Memory in megabytes before an input counts as out of memory |

Each target gets its own job, the jobs start together, and an empty array runs
none. The committed `<fuzz dir>/corpus/<name>/` directory seeds each run; the
job does not keep the corpus between nights.

Pull requests, merge groups and pushes only validate the keys: a malformed
entry or a missing manifest fails the declaration, and nothing is fuzzed. Only
the workflow's fuzz-scan mode runs the fuzz job, next to the declaration, and
the organization's required workflow never sets that mode. This repository's
own pull requests also run the `fixtures/fuzz` planted targets to test the
workflow code; callers' pull requests do not.
`.github/workflows/fuzz.yml` calls the workflow in that mode for Maestro's
`main` every night at 04:41 UTC and on manual dispatch. Each target job installs
cargo-fuzz 0.13.2, a checksum-verified release that only this job downloads,
and the `nightly-2026-09-15` toolchain, which replaces the caller's
`rust-toolchain.toml` for that job only. It fuzzes on Linux x86_64
(`--target x86_64-unknown-linux-gnu`).

A crash, timeout, out-of-memory result or leak is a finding. The job minimizes
its input with `cargo fuzz tmin` under the same time and memory limits, then
uploads the raw and minimized inputs, both logs and `finding.json` as the
artifact `fuzz-<concurrency key>-<name>`. The report job files one issue in the
caller repository per distinct finding, titled `Fuzz <kind> in <target>`, with
` at <file>:<line>:<column>` added for a Rust panic. The body holds the run
link, the artifact name, the minimized input in base64 and the stack, within
GitHub's 65,536-character limit; an input too large for the body stays in the
artifact. When an open issue has exactly that title, the job comments on it
instead of opening another. Any finding or failed scan turns the run red. A tool
or build error fails only that target's job, and the report job still files
the other targets' findings.

The `libfuzzer-sys` crate that cargo-fuzz uses is licensed
`(MIT OR Apache-2.0) AND NCSA`. Before fuzzing, each target job runs the
cargo-deny license check on the fuzz crate with the embedded allow list and the
caller's license exceptions, plus NCSA for `libfuzzer-sys` alone, so any other
NCSA crate fails the job. Keep the NCSA notice in the fuzz crate, as
`fixtures/fuzz/fuzz/NOTICE` does, and nothing you ship may depend on the fuzz
crate. The workspace license check never sees the fuzz crate and still rejects
NCSA in anything the caller ships.

## Adoption and verification

Keep standard Rust files and the existing organization-managed workflow call;
no extra workflow or ruleset is needed. The four original quality/docs/tests
commands and the dependency, source and coverage job remain required. Self-tests exercise production step bodies,
isolated positive/negative controls, real tools, wasm success/failure, caller
pins, bootstrap verification and native platform diagnostics. Both existing
required results aggregate the new positive caller and workflow proofs.

Releases and organization/caller pins are coordinated separately. A code
change or release tag alone does not prove adoption: observe the immutable
release, pin/ruleset read-back and required checks after repinning.

## Ephemeral Linux runners

The hosted caller-declaration job selects the organization's `maestro-aws`
runner group for x64 Linux jobs when its published online count is positive
and no more than five minutes old. Missing, expired or invalid availability,
or an exact caller-workflow revision that declares, inherits or references repository secrets,
selects GitHub-hosted Linux instead. The declaration and the runner controller
use the same pinned secret-reference policy. The automatic job token is allowed.
The selector uses the platform YAML parser without installing packages; a missing
parser selects hosted Linux. The provider's own self-tests stay hosted because
their synthetic PR refs cannot be admitted as pinned runner-group workflows.
Native ARM Linux, Windows, macOS and CodeQL remain GitHub-hosted.

Each AWS runner accepts one job. It has a separate unprivileged identity,
filesystem and network namespace, with no access to the registration credential,
build-user state, another runner, private networks or instance metadata.
Only HTTPS and DNS leave its network. Existing memory admission reserves each
runner's cap and gives waiting compile jobs priority. Four idle runners are the
initial target; admission can delay replacements. Repository secrets are not
passed to these jobs, and external contributors require approval before running.
Availability is checked before scheduling; a host lost afterward fails or leaves
an already-selected job queued, not silently rerun on another host.

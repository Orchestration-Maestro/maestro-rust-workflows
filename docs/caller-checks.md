# Caller-declared checks

Add `.github/ci.toml` at the caller repository root to opt into isolated tests,
real search tools and optional builds. Organization calls use that root; the
workflow's directory input exists only for its fixture self-test, pinned
workspace proof and advisory scan. Optional `checkout-repository` and
`checkout-ref` inputs serve the same three: when unset they check out the caller
repository and commit. `checkout-base-ref` and `test-event` are overrides
restricted to this workflow repository; they default to the real PR base and event.

Both coverage proof jobs use one immutable Maestro snapshot for comparable runs.
Advance both checkout references together to the smallest main commit that resolves
the reason for the move, after checking its locked dependencies. For example,
`85dc98f2c993437238168528dc081b828048a3f1` updates wasmtime and wasmtime-wasi to
49.0.3 to resolve the advisories without taking unrelated later changes.
Changes to either `.github/workflows/ci.yml` or `.github/workflows/ci-proof.yml`
schedule the proof; check its results before landing.

```toml
wasm_crates = ["my-browser-library"]
check_targets = ["x86_64-pc-windows-msvc"]
browser_build = false
test_tools = false
```

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

Both arrays are required, including when empty. `browser_build` and
`test_tools` are optional booleans that default to `false`. The
[schema](../.github/ci.schema.json) describes the parsed TOML shape; there is no
version key or older-format reader.

`reserved_ports` is an optional array of unique integers from 1 to 65535. When
set, every Linux test job runs
`sudo sysctl -w net.ipv4.ip_local_reserved_ports=<list>` before the tests, so
tests that must bind a fixed local port are not displaced by an outbound
connection that the kernel assigned that port. Without it nothing runs.

An absent file runs the original four checks plus the dependency, source and
coverage job, without isolation, search tool installation or optional jobs. An unreadable file, malformed TOML,
duplicate key, unknown key, missing array, scalar array value, non-string
member or non-boolean tool/build flag fails declaration validation. An empty
present document is invalid. Integer and string booleans are not accepted.

Names are exact, without whitespace or Unicode normalization. Each wasm name
must match a package in the caller's locked Cargo metadata. Each target must
match the caller compiler's target list and the runner table below. An unknown
package, unknown target or recognized target without a runner fails with a
file/key diagnostic before optional work is emitted. Metadata or compiler
inspection failures also fail validation. Array order and repetitions are
retained; declaration text is never executed as shell syntax.

## Test shards

Tests use checksum-pinned cargo-nextest 0.9.148 with four automatic
`--partition count:N/4` partitions. The caller's `.config/nextest.toml` supplies
test-suite runner metadata such as resource groups for fixed-port tests. Shared
CI controls partitioning and coverage; profile default filters are ignored so
they cannot omit tests. Runner user configuration is not loaded. `test_shards` was removed; setting it fails
with a clear diagnostic. Unit and integration tests run exactly once; doctests
run once on shard 1 with `cargo test --workspace --locked --doc`.

Declared callers run `cargo llvm-cov nextest` in the same isolated environment
with their opted-in tools. Shards upload LCOV without individual thresholds.
`Tests (ubuntu-latest)` requires all shards, downloads all four reports, counts
each source line once (covered if any shard hits it), and enforces
`coverage_min_lines` on the union. Missing reports or low coverage fail.
Undeclared callers use nextest partitions but retain their single coverage job
for merge groups and main pushes. PRs skip coverage for all callers.

### Pull-request selection and retries

On pull requests, tests run only changed workspace crates and their reverse
dependencies, without the coverage threshold. A crate manifest selects its
crate; lockfile changes compare the resolved transitive dependency identities
and features at base and head, selecting affected crates. Root manifest,
toolchain or CI workflow/schema changes select everything. Non-test declaration
keys (such as spelling, wasm and foreign-target lists) select no extra tests;
changes to test tools, coverage policy, additional locked manifests or declaration presence select everything.
Docs-only changes outside a crate select no tests. Unowned non-document files
conservatively select everything. Every shard still fetches locked dependencies.
Merge groups and main pushes run the whole suite with coverage.

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
  every event. A pull request or merge group checks advisories and yanked crates only when it changes
  `Cargo.lock` or `.github/ci.toml`; otherwise the step prints that advisories were not
  checked. Pushes and other events always check them.
- The scheduled advisory scan (`.github/workflows/advisories.yml`) runs this workflow in
  advisory-scan mode against Maestro's `main` every hour and on manual dispatch: only the
  declaration and the advisory check run, with the declared exceptions. Each undeclared
  advisory or yanked crate opens one issue in that repository, titled `Priority: <identifier> on main`
  and its crates, unless an open issue title already starts with `Priority: <identifier> on main`; the
  run fails while any remains. Pull requests that change the workflow run the scan without
  opening issues.
- The cargo-deny license allowlist is MIT, Apache-2.0, ISC, BSD-2-Clause, BSD-3-Clause, Zlib,
  Apache-2.0 WITH LLVM-exception, Unicode-DFS-2016, Unicode-3.0, CC0-1.0,
  CDLA-Permissive-2.0, MPL-2.0 and MIT-0.
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
  { package = "specific-library", version = "1.2.3", licenses = ["BSL-1.0"] },
]
advisory_exceptions = [
  { id = "RUSTSEC-2025-0141", package = "bincode", version = "1.3.3", reason = "Packaged syntax and theme dumps; reviewed exception" },
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
stale-exception diagnostic naming the advisory, package and version. If another
version of the same package is also locked, the check rejects the exception and
names the locked versions. Wherever advisories are checked, the pinned cargo-deny
first reports the advisories without any ignore; each declared ID must be reported for exactly the
declared package and version, otherwise the check fails naming the ID and the
package and version reported, or "no such finding". The shared workflow renders only that ID and reason
into cargo-deny's advisory ignore list.
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

Changes to this input select all workspace test packages on pull requests, like other test-affecting declaration changes. Merge groups and main pushes retain full tests and coverage. Adoption requires a release containing this input and the organization rules' version pin to that release.

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

The child keeps only `PATH`, resolved `CARGO_HOME` and `RUSTUP_HOME`, fresh
`HOME`, `TMPDIR`, `TMP`, `TEMP` and all XDG configuration/data/cache/state/runtime
locations, plus `MAESTRO_NO_LOCAL_LLM=1`. `TMP` and `TEMP` equal `TMPDIR`; the
other locations are separate, initially empty directories beneath a unique
runner-temp parent. Unrelated CI state, proxy settings and application/provider
overrides are not inherited. The partition argv uses `cargo llvm-cov nextest`; doctests use
`cargo test --workspace --locked --doc`.

The original home and auth file are never read, moved or modified. Only the
isolated directories are cleaned on success or failure; other action steps
retain the runner's normal home and cache access.

## Caller-pinned test tools

With `test_tools = true`, the Tests job reuses the browser build's
checksum-verified mise 2026.10.2 bootstrap, runs `mise install` from the caller
directory, and adds the installed tool bin directories to `PATH` before
isolated tests. Provide a `mise.toml` pinning the tools your tests spawn, for
example:

```toml
[tools]
just = "1.58.0"
prek = "0.5.3"
```

Tool versions come from the caller, not the shared workflow. Missing
`mise.toml` or a failed installation fails the job. False or omitted
`test_tools` skips the bootstrap and tool installation, preserving the existing
test path. An absent declaration still runs the original unisolated tests.
The nextest partition and doctest argv, credential guards,
fresh directories and the child environment allowlist are unchanged. Only the
installed tools' executable paths are added, not mise's environment settings.

## Wasm and browser builds

For each listed package, Linux runs:

```bash
cargo build --locked --target wasm32-unknown-unknown -p "$CRATE"
```

The caller toolchain installs wasm support only when there are listed
packages. With `browser_build = true`, the workflow subsequently runs
`mise install`, then `mise exec -- just browser-build` from the caller directory,
including when the wasm array is empty. A failed wasm build prevents the
recipe. False or omitted flags skip the browser tooling and recipe.

Provide a `mise.toml` pinning the recipe's tools, including `just`, and a
`justfile` with `browser-build`. A missing tool configuration, installation
failure, missing recipe or nonzero recipe result fails CI. The shared workflow
bootstraps mise 2026.10.2, reusing that exact installed version or verifying the
published SHA-256 of its official Linux x64 binary before execution. It does
not override the caller's tool versions; the self-test uses just 1.58.0.

The caller owns the browser application, entry, temporary bundles and
application-specific diagnostics. A wasm build or recipe invocation alone is
not proof of browser runtime behavior.

## Foreign-target checks

The caller's rustup toolchain installs each requested target, then runs:

```bash
cargo check --workspace --all-targets --locked --target "$TARGET"
```

| Target | Runner |
| --- | --- |
| `aarch64-unknown-linux-gnu` | `ubuntu-24.04-arm` |
| `aarch64-apple-darwin` | `macos-latest` |
| `x86_64-apple-darwin` | `macos-latest` (requested target installed) |
| `x86_64-pc-windows-msvc` | `windows-latest` |

There is no Linux fallback for Apple/MSVC targets. These compile checks do not
restore macOS/Windows runtime tests, which remain a before-first-release
requirement. All opted-in jobs run in both ordinary pull requests and merge
queue checks. Declaration errors fail CI even when dependent jobs skip.

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

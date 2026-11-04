# Caller-declared checks

Add `.github/ci.toml` at the caller repository root to opt into isolated tests,
real search tools and optional builds. Organization calls use that root; the
workflow's directory input exists only for its fixture self-test, pinned
workspace proof, and advisory and fuzz scans. Optional `checkout-repository` and
`checkout-ref` inputs serve the same four: when unset they check out the caller
repository and commit. `checkout-ref` also selects the caller revision for a
[manual browser proof](#manual-browser-red-and-green-proofs). `checkout-base-ref`
and `test-event` are overrides restricted to this workflow repository; they
default to the real PR base and event.

The full and selected-package proof consumers use one immutable Maestro source
snapshot. A warm build uses its parent commit; selected-package proof sets that
parent as its PR base. The warmer is non-required, and consumers still run if it
fails. See [Compile cache](../README.md#compile-cache) for snapshot shapes,
fail-open transfers and storage limits.
Advance all proof checkout references together after checking locked dependencies.
The proof uses `cd95c368dc1c7a0c265f5ed6303cd0fa8e7f1cc8`, the Maestro main
revision that qualifies the current release, and it still includes the native
residency-helper compilation repair. Its first parent,
`7b3a82bbc647320524044dc33198ed75669c8e17`, supplies the warm build and
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

## Test policy and caller permissions

Pull requests run declaration validation, formatting, Clippy, docs,
dependency/source checks and declared macOS and Windows target checks. Host Clippy
and these target checks select changed crates and their workspace dependents;
formatting still checks the whole workspace. Pull requests skip the test archive,
test setup, test shards, doctests and wasm/browser builds. Every required check
name still reports; skipped jobs count as success. Merge groups and main pushes
run every workspace test with the full shard matrix, doctests, coverage and all
declared builds and foreign-target checks, except that a main push reuses tests
when its exact commit already passed a completed merge-group run of the same
shared workflow revision with the full workspace selected. That push skips the
test shards, doctests, coverage and wasm/browser legs. The archive and preparation
jobs still compile and save their compiled-cache snapshots for the next merge
group. The archive keeps the same instrumentation and cache shape as that group.
Formatting, lint, docs, dependency/source checks and target checks still run.
No caller setting changes this policy.

Every merge-group declaration uploads a small identity artifact: the shared
workflow's full repository path, exact revision and whether it declared the full
workspace test set. The artifact is scoped to the caller repository, directory,
call key and run attempt. A main push paginates successful merge-group runs for
its exact commit and reads that evidence with the existing job token's
`actions: read` permission. Different calling workflows can reuse the same
shared revision; a successful PR run, a different revision or a selected subset
cannot qualify.

Only merge-group runs whose head repository is the caller repository can supply
evidence; fork runs and runs without that repository identity do not qualify.
The artifact is a self-report, not an independent attestation. This trust boundary
accepts workflow authors in the same repository: a workflow forging evidence
would have to be reviewed code at that exact commit, and its author could
already edit the caller. The read-only job token gains no additional permissions.

Writer and reader resolve the shared identity once. The raw workflow context
qualifies only when it names this shared workflow's full repository path.
Reusable callers resolve that same path from the current run's referenced
workflows, with exactly one matching entry. Missing, conflicting or ambiguous
identities, missing or expired artifacts, unreadable evidence and API errors all
keep the full suite. Runs whose merge group left no evidence keep the full suite.
Fixture checkout/event overrides, scan calls and `warm-cache` calls do not use
the result lookup.
The independent `warm-cache` call still runs on every main push and seeds its
existing cache shapes, including its normal doctest and browser commands.

Below, main-push test and coverage behaviour describes pushes without a matching
merge-queue result. The independent `warm-cache` call runs on every main push.

See [Test shards](#test-shards) for the required test result and coverage rules.

Callers grant `actions: read` to the reusable-workflow call so the shared
workflow can reuse prepared test bundles from earlier runs. Other jobs retain
their existing permissions. Add this grant with the release-pin update; a
missing grant can prevent the reusable workflow from starting.

The workspace coverage floor remains 80% (or the caller's higher floor).
Declared callers collect fresh coverage for the full workspace on merge groups
and main pushes. Every shard must provide its report, and the aggregate applies
the floor to their union. Pull requests collect no coverage and need no previous
run's coverage artifacts.

## Native C compilation

Linux and macOS compilation jobs use the existing compiler cache for native C
builds, including coverage builds. When neither `HOST_CC` nor `CC` is set, the
workflow exports `HOST_CC="sccache cc"`. Native C objects use the same local
cache and compiled-cache snapshot as the job's Rust objects; no separate cache
is created.

An explicit `HOST_CC` or `CC` in the environment stays unchanged, including an
empty value. If the caller's `.cargo/config` or `.cargo/config.toml` declares
either key in `[env]`, the workflow leaves compiler selection to Cargo and logs
that decision. Other `[env]` keys do not disable the default. A caller-selected
compiler is not wrapped automatically.

The `cc` build library uses `HOST_CC` only when the build host matches the target.
Foreign-target compiler discovery and target-specific compiler settings stay
unchanged. Isolated test children retain the configured `HOST_CC` and `CC` values.
This setting does not change Rust's coverage wrapper or instrument C sources.

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
native payload to stay within the coverage shape's 1 GiB cap. The existing
writer and size rules still apply: only an eligible successful default-branch
warm build saves a snapshot; a pull-request run cannot prove saving. More cached
compiles are useful only when that save succeeds and later jobs restore the
compatible snapshot.

## Reusable-call concurrency

Every run except a push is grouped by ref. A push run is grouped by
the calling workflow's name and the pushed commit, so two caller workflows on
one commit run at the same time and later landings never wait for each
other. The optional workflow input `concurrency-key` defaults to an empty
string, which adds nothing to the group. If a workflow calls this
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
run formatting, Clippy, docs, dependency/source checks and declared macOS and
Windows target checks in parallel. Merge groups and main pushes also run the test
archive, test setup, full shard matrix, doctests, coverage, wasm/browser builds,
Chromium tests and all other declared foreign-target checks.
Pull requests skip these jobs without removing their required check names.
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

macOS target checks use the runner's built-in Bash. Callers do not need to
install a newer shell.

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

Windows checks run without the compile cache because sccache expands response
files beyond Windows' command-line limit; [Windows caching](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/166)
tracks the upstream fix.

A push with `warm-cache: true`, including a feature-branch push, also runs the
declared target checks; warm calls for other events skip them. Only
default-branch pushes save those snapshots. Host-specific, checksum-pinned assets
are listed in [Compile cache](../README.md#compile-cache).

Only a default-branch push warm call also runs Format and lint, Docs,
Doctests, Wasm and browser build, and declared Chromium browser tests.
They run their existing checks, not substitute builds: the paths and compiler
output modes match subsequent readers. Doctests use the plain test shape,
not the instrumented coverage shape. Browser tests and doctests execute during
these warm calls; the coverage warm build still uses its optional test filter.
Other warm calls skip these additional jobs. No extra cache writer is enabled
for pull requests, merge groups, feature branches or manual runs.

### Snapshot size

Each job shape has one cache cap. Compile cache identity sets it as the `size`
output, Compile cache hands it to sccache as `SCCACHE_CACHE_SIZE`, and Seal
compile cache reads that same variable:

| Shape | Cap |
| --- | --- |
| `test-prepare` (Prepare test setup) | 1,610,612,736 bytes (1.5 GiB) |
| Every other shape | 1,073,741,824 bytes (1 GiB) |

sccache applies the cap twice. Its compiled objects stay within the cap, and so
do the preprocessor entries of C and assembler compiles, which it keeps in a
separate `preprocessor` directory and leaves out of its reported cache size. The
seal measures each part and skips the snapshot when either one exceeds the cap,
so it refuses only a cache that sccache would not have left. The
`Compile cache snapshot:` line prints both parts; in Prepare test setup the
preprocessor directory held about 6 MB. A snapshot grows toward its cap, because
objects that later builds stop using stay until sccache evicts the least
recently used ones, so a full cap alone is normal. A cap is too small when a
build that starts from an empty cache fills it: the earliest objects are
evicted, and every reader then misses the objects it needs first.

Prepare test setup builds every wasm example extension as well as the setup
tools, so its shape has the larger cap. Source: in seven Maestro default-branch
pushes on 2026-11-03, the job started from an empty cache, filled its 1 GiB cap,
evicted 32 to 38 of the roughly 2,080 entries it wrote, and left 1,076,511,005
to 1,079,794,032 bytes on disk. The seal refused all seven, so the merge groups
compiled that shape cold (2 cache hits and 1,673 misses), and Prepare test setup
took a median 28.5 minutes across 15 of them. A merge-group run that reused the
example extensions held 1,026,339,681 bytes with nothing evicted. The first
default-branch push with the 1.5 GiB cap also starts from an empty cache, so its
`Compile cache snapshot:` line gives the shape's whole working set, and that
number sets the cap.

The bound applies to each save, not the repository's 10 GB cache allowance.
Compressed snapshot sizes are measured separately, and GitHub eviction can
remove even the newest snapshot of a shape. On 2026-11-03 one Maestro generation
held 5,606,616,026 bytes: the other compile snapshots, three Cargo git entries
and one test setup bundle. The setup snapshot adds one entry of about 1.1 GB to
each generation: at most 1.6 GB of objects, plus about 6 MB of preprocessor
entries. A generation then stays near 7.2 GB, under the limit. GitHub evicts by
last access, so the previous generation goes first: once a shape saves its new
snapshot, no reader restores the old one. The extra entry makes those old
snapshots leave sooner on each push.

### Cargo git dependency cache

No caller setting is needed. Lint, Docs, dependency/source/coverage checks,
declared target checks, Build test archive, Prepare test setup and every test
shard restore Cargo's `git/db` and `git/checkouts` before resolving dependencies.
Doctests share the test-shard route. A hit containing the locked revision avoids
fetching that git source; Cargo still downloads missing registry packages.

All Cargo jobs use Git's CLI transport through
`CARGO_NET_GIT_FETCH_WITH_CLI=true`. Isolated test children keep this setting.
Cargo retries failed Git fetches, including submodule fetches, with its default
three-retry budget. TLS verification stays enabled. An exhausted retry budget
fails the check; there is no workflow retry loop.

The key hashes the caller's `Cargo.lock` and the `Cargo.lock` of every
`additional_locked_manifests` entry. Entries stay separate by caller repository,
directory, workflow concurrency key and runner operating system. A changed lock
file falls back to the newest entry in that scope; Cargo fetches any missing
revision normally. A cache miss or transfer failure does not bypass a check or
change its result.

Only a default-branch push that missed the exact key saves an entry. Build test
archive seeds Linux; native target checks seed macOS and Windows after a
successful check. Each uses its native Cargo home. Archives never cross operating
systems. Windows reuses git sources without enabling the compile cache.

These entries count against the 10 GB repository allowance. One measured Linux
entry for Maestro's `wasmtime` revision and submodules is 436 MB compressed.
Allow for a separate entry per native host and retained lock-file revision;
measure native sizes and restore time before claiming a saving. Cache eviction
can remove an entry before the next reader runs.

## Test shards

Tests use checksum-pinned cargo-nextest 0.9.148 with automatic
`--partition slice:N/COUNT` partitions, where `COUNT` is the declared
`test_shard_count`. The caller's `.config/nextest.toml` supplies
test-suite runner metadata such as resource groups for fixed-port tests. Shared
CI controls partitioning and coverage; profile default filters are ignored so
they cannot omit tests. Runner user configuration is not loaded. `test_shards` was removed; setting it fails
with a clear diagnostic. Unit and integration tests run exactly once; doctests
run once with `cargo test --locked --doc` in a parallel `Doctests` matrix leg
for merge groups and main pushes. These events always select every workspace
package. Pull requests skip both test shards and doctests.

`test_shard_count` sets the number of shards. It is an optional whole number
from 1 to 60 and defaults to 16. Any other value, including a float, a string or
a boolean, fails the declaration job with `test_shard_count must be a whole number from 1 to 60`.

```toml
test_shard_count = 32
```

More shards spread the tests over more runners, and each shard repeats its own
setup. The shard matrix, the partitions, the job names and the coverage union
all use the declared count on full runs, so the union expects exactly that many
reports. Pull requests run no test shards and upload no coverage.

Declared callers run `cargo llvm-cov nextest` in the same isolated environment
with their opted-in tools. Only the numeric shards upload LCOV, without
individual thresholds. `Tests (ubuntu-latest)` requires all matrix legs,
including `Doctests`, to succeed on full runs. On pull requests it requires a
successful declaration and skipped test and doctest jobs, regardless of package
selection. On full runs it downloads all reports, counts each source line once
(covered if any shard hits it), and enforces `coverage_min_lines` on the union.
Missing reports or low coverage fail.
Undeclared callers use nextest partitions but retain their single coverage job
for merge groups and main pushes. Both coverage modes run tests through nextest,
so caller setup scripts run in either mode (reusing prepared outputs in shards). Single-job coverage uses one partition
(`slice:1/1`); declared callers combine one report per shard. Both modes use the same
line-counting policy: normalize source paths, count each line once, and enforce
the coverage floor on the union of hits. PRs skip coverage for all callers.

### Build once, test many

On merge groups and main pushes, one `Build test archive` job compiles the
workspace's unit and integration tests with `cargo nextest archive` (or
`cargo llvm-cov nextest-archive` for declared coverage).
Numeric shards download that archive and run `--archive-file`, keeping the same
partitions and coverage reports. Existing locked dependency fetches stay
available for tests that intentionally start child builds. `Doctests` compiles and
runs doctests once on every merge group and on main pushes that do not reuse
merge-queue tests, without waiting for the archive build.
Warm-cache runs keep their existing compile route and cache shape.

Both archive commands pass `--timings` to Cargo. Download the
`test-build-timings-<concurrency-key>` artifact (`test-build-timings-root` when no
key is set) and open `cargo-timing.html` to inspect per-crate build times.
The artifact includes the report from `target/cargo-timings/` for plain builds or
`target/llvm-cov-target/cargo-timings/` for coverage builds. A successful archive
build must produce a timing report; a missing report fails the upload.

Each shard fetches the archive and the test setup bundle in up to 16 parallel byte ranges
from artifact storage, checks the size and SHA-256 digest the Actions API reports, then
extracts. Storage without range support supplies one stream. A digest or size mismatch
fails the step before anything is extracted. If the listing has no digest or the transfer
fails, the step hands over to the single-stream `download-artifact` action without retrying
individual ranges.

Each download prints one line, `artifact-download name=<artifact> bytes=<n> seconds=<s>
mb_per_second=<rate> streams=<n>`. The rate uses decimal MB (1,000,000 bytes) and measures
transfer time only, excluding verification, extraction and test work. `streams=0` and
`mb_per_second=0.00` mark a handover, not a measured single-stream transfer.

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
runs also receive this path, but run preparation and restore prepared files only
when `test_prepare_inputs` is declared.
Archived runs without `test_prepare` likewise have no prepared files restored.

Archived isolated numeric shards receive `TEST_ARCHIVE=1` on both plain and
coverage routes. Warm-cache runs, direct non-archived runs, doctests and coverage
runs that do not extract an archive leave it unset in the isolated environment.
Callers may use `TEST_ARCHIVE=1` to require their declared prepared files and fail
if any are missing, rather than silently rebuilding them in each shard.
It identifies archived execution, not the presence of prepared files.

`TEST_PREPARE_OUTPUT` alone does not guarantee files exist; callers must check for the
files they need before reusing them. No nextest archive include is needed.
Nextest setup scripts can export paths into that directory;
they must reuse those files instead of preparing them again. The caller owns the
preparation logic and exported environment. The existing `concurrency-key`
distinguishes both artifacts when a workflow makes several calls.

To prepare only when selected tests need the files, set `test_prepare_setups` to
the nextest setup-script names that read them:

```toml
test_prepare_setups = ["native-fixture", "author-component"]
```

The declaration reads the caller's `.config/nextest.toml`. It collects packages
from every `[[profile.default.scripts]]` entry whose `setup` names a listed
script, either as a string or in an array. Other profiles and unlisted setups
do not contribute packages.

Each filter must be a union of terms. Every term must conjoin exactly one
`package(NAME)` or `package(=NAME)` atom with expressions containing no package
atom. For example:

```toml
[[profile.default.scripts]]
filter = "package(wasm-host) | (package(wasm-guest) & (test(component::) | !test(slow::)))"
setup = "author-component"
```

This filter names `wasm-host` and `wasm-guest` as packages that need preparation.
The declaration refuses a listed setup with no default-profile script entry, a
rule without a filter, a term without a package, or a term with more than one
package conjunct. It also refuses package atoms under `!`, `not` or the right
side of `-`, package glob, regex and `~` substring matchers, and `deps()` or
`rdeps()`. Unsupported atoms such as `default()` or `group()` are refused, and
unreadable filters fail with the offset where parsing stopped. Errors name the
nextest file, the setup and the reason. Every named package must belong to the
workspace.

Pull requests always skip preparation. On runs that test, when none of these
packages is selected, `Prepare test setup` does not start, and the shards skip
the download and restore. Selecting any of them keeps the
command. Every merge group and main push selects the whole workspace, so it
prepares whenever a consumer exists. An empty setup list disables preparation;
without the key every run that tests prepares.

A change to `.config/nextest.toml`, `test_prepare_setups`, `test_prepare` or
`test_prepare_inputs` selects the whole workspace on pull requests. This also
applies when a source includes the nextest file or a `test_inputs` entry routes
it to specific packages.

`test_prepare_inputs` lists the files the `test_prepare` command reads, as
repository-relative path globs from the caller's working directory, such as
`.config/prepare-tests.sh` or `tools/**/*.toml`. `**` matches any depth, hidden
files included. The prepared directory is cached under a key built from the
contents of every matching file, the `test_prepare` command, the compile cache
identity (repository, directory, host and compiler), and the checkout path. A
glob that matches no file fails the job. On reader runs, `Prepare test setup`
restores the bundle on an exact hit and skips the `test_prepare` command.
When preparation is enabled, default-branch pushes run the command even on an
exact hit, including warm builds and pushes that reuse merge-queue tests.
These writers check whether the bundle is cached without downloading it and
pass no `TEST_PREPARE_BASE`, so preparation compiles the inputs and seeds its
compiled-cache snapshot rather than copying prepared files. A cached bundle
alone does not contain those compiler objects. Without the list a warm build
does not prepare. Every numeric shard downloads the prepared directory as before.
Only pushes to the default branch write the cache, so a merge group reads what
the last push saved.
Pull requests skip preparation. Declare every input the command reads: an
unlisted input does not change the key and can leave a stale directory in place.
Without the list nothing is cached.

On reader runs, when the exact key misses, `Prepare test setup` restores the
closest earlier bundle saved under the same compile cache identity, checkout
path and `test_prepare` command, which is the last push's bundle. A merge group
adds bundles that earlier successful runs of this repository uploaded at the
commit being merged, whichever workflow ran them:

- A merge group adds, for each pull request that is not on the default branch
  yet, whether in this group or queued ahead of it (found from the `(#N)` that
  ends its commit subject), the bundle of that pull request's newest successful
  run at its head.
- Pushes add no candidates: writers must populate compiler objects.
- A pull request run adds nothing, so every added bundle was built by the code
  being merged, starting from a bundle the default branch saved. Runs from forks
  are never added: a bundle holds executables that the run then starts.

When the runs cannot be listed, the job logs a warning that names the failed
request. Reader runs prepare from the bundles found before the failure;
writers still prepare without bases.

On reader runs, `Prepare test setup` extracts each bundle to its own directory,
separate from `TEST_PREPARE_OUTPUT`, and sets `TEST_PREPARE_BASE` to the list of those
directories before the command runs: the restored bundle first, then the added
ones, joined with `:`. A bundle that is missing, expired, fails its size or
digest check or cannot be read is left out, and the later ones are still tried.
The command may copy or reuse any file in a listed directory whose own inputs
are unchanged, and must still write every file it needs to
`TEST_PREPARE_OUTPUT`. When no bundle is available, `TEST_PREPARE_BASE` is
unset. Reader runs still skip the command on an exact hit. Writers leave
`TEST_PREPARE_BASE` unset. Pushes that reuse merge-queue tests save compiled-cache
snapshots without uploading the test archive or setup artifacts, since no test
shard in those runs needs them.

```toml
test_prepare = "sh .config/prepare-tests.sh"
test_prepare_setups = ["author-component"]
test_prepare_inputs = [".config/prepare-tests.sh", "tools/**/*.toml"]
```

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
The shard field is `1` through the declared `test_shard_count`, or `doctests`
for the parallel doctest leg (`shard-doctests`). That leg normally has no
`nextest.log`; the same missing-file tolerance applies. The caller digest is the
full lowercase SHA-256 of the UTF-8 compact JSON array
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

Pull-request host Clippy, macOS and Windows checks select changed workspace crates,
literal `include_str!`, `include_bytes!` or `#[path = "..."]` input readers and
declared test-input readers, plus their direct or transitive workspace dependents. Normal, build,
development and target-specific dependency edges count. The deepest package root
owns a changed source file. Including a package input adds its readers without
dropping its owner or the owner's dependents. Non-root packages own their whole
directory. A root package owns `Cargo.toml`, its Cargo target source directories
recursively, and its build script itself. Literal `#[path = "..."]` module targets
also select their declaring packages and those packages' dependents, even outside
target source directories. Other root paths, such as external docs or fixtures,
select only their compile-input readers and explicit test-input routes when
included.
Root manifest and lockfile changes still follow the rules below.

Host Clippy and declared macOS and Windows target checks lint the selected packages
with `--all-targets`. Merge groups and main pushes lint the whole workspace.
An empty PR selection performs no Clippy compilation; the named checks still
report success. An absent base selects every package for lint. Pull requests
skip the test archive, preparation, test shards, doctests and wasm/browser
builds regardless of package selection.

A crate manifest selects its crate; lockfile changes compare the resolved transitive dependency identities
and features at base and head, selecting affected crates. Root manifest,
toolchain or CI workflow/schema changes select everything. Non-test declaration
keys (such as spelling, wasm and foreign-target lists) select no extra packages;
changes to test tools, coverage policy, additional locked manifests, declared
test inputs or declaration presence select everything.
For paths outside a crate, fallback selection skips undeclared Markdown files
and paths under `docs/`; other undeclared files select everything. The workflow
and configuration rules above take precedence. Declared paths select their
readers, including for Markdown changes.
Every enabled shard fetches locked dependencies.

Merge groups and main pushes always run the full workspace suite, doctests,
declared Chromium tests and coverage. They do not narrow tests using a diff or
previous coverage. The queue is the merge gate; a suite failure stops the merge.
Main-push failure reporting and revert handling remain in the post-landing
workflow.

`test_inputs` maps repository-relative files or directories outside the
workspace members to the exact package names whose tests read them. A change
under a declared path selects those packages and their reverse dependencies,
including Markdown files there. An empty list marks a path no workspace test
reads, such as a separate fuzz workspace. Paths must stay inside the repository
and names must be workspace packages; anything else fails the declaration.

```toml
test_inputs = { "packages/" = ["packages-crate"], "layers.json" = ["conventions"], "fuzz/" = [] }
```

CI does not retry tests. Every nextest run passes `--retries 0`, so caller
retry settings, including per-test overrides, cannot enable retries. A failed
test fails its job. Caller resource groups are retained, but default filters
cannot suppress tests.

## Required dependency, source and coverage checks

No consumer file is required. The shared workflow embeds the defaults and
installs checksum-verified official releases: cargo-deny 0.20.2,
cargo-machete 0.9.2, cargo-hakari 0.9.39, typos 1.51.1, gitleaks 8.30.1, zizmor 1.30.1,
actionlint 1.7.12 and cargo-llvm-cov 0.9.1. Checksums and download URLs are
pinned together in the workflow. LLVM tools come from the caller's rustup
component distribution, not a caller-provided executable.

Pinned tool downloads use curl's `--retry 3 --retry-all-errors --retry-delay 30`.
After an error, curl uses a 30-second retry interval, up to three times. A server's
`Retry-After` can extend the wait. This covers receive resets and gateway errors lasting
60 seconds without increasing the retry count.
Permanent failures still fail after the last attempt, including a missing release asset.
Downloaded bytes pass the same pinned checksum check before installation; corrupt bytes
never become tools. Existing installation deadlines still apply.

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
- The scheduled advisory scan (`.github/workflows/advisories.yml`) runs the
  [released workflow](../README.md#versions), pinned by full commit SHA, in
  advisory-scan mode against Maestro's `main` every hour and on manual dispatch: only the
  declaration and the advisory check run, with the declared exceptions. Each undeclared
  advisory or yanked crate opens one issue in that repository, titled `Priority: <identifier> on main`
  and its crates, unless an open issue title already starts with `Priority: <identifier> on main`; the
  run fails while any remains. Pull requests that change `advisories.yml` run the pinned
  scan without opening issues.
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

Changes to this input select all workspace packages for pull-request lint, like other test-affecting declaration changes. Pull requests follow the [included-input rule](#pull-request-selection-and-retries); merge groups and main pushes run full tests and coverage. Adoption requires a release containing this input and the organization rules' version pin to that release.

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
only successful matrix legs, including `Doctests`, and requires one report per
numeric shard when merging coverage. On pull requests the aggregate requires a
successful declaration and skipped test and doctest jobs.

The original home and auth file are never read, moved or modified. Only the
isolated directories are cleaned on success or failure; other action steps
retain the runner's normal home and cache access.

Before cleanup, the step kills every process still using the isolated
directories: as its working directory, executable or open file, or through the
isolated environment it inherited. Processes running the compile cache
executable are left running for the statistics and seal steps. The log lists
each stopped process as `Stopped test process <pid>: <command>`, so the test
that leaked it can be found. If the directories still cannot be removed, the
step logs `Could not remove <directory>`. Cleanup never changes the exit status
of the test command, whether it passed or failed.

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

On merge groups and main pushes, Linux runs this command for
each listed package:

```bash
cargo build --locked --target wasm32-unknown-unknown -p "$CRATE"
```

The caller toolchain installs wasm support only when there are listed
packages. Either `test_tools = true` or `browser_build = true` installs caller
tools once before wasm compilation. The browser flag is independent: it does
not enable tools in other jobs. With `browser_build = true`, the workflow runs
`mise exec -- just browser-build` after successful wasm builds, including when
the wasm array is empty. Both wasm builds and the browser recipe run on every
merge group and main push. Pull requests, advisory scans, fuzz scans and warm
calls outside a default-branch push skip them. A failed wasm build prevents
the recipe. False or omitted `browser_build` skips the recipe and its
`just`-pin validation, even when tools are enabled for wasm compilation.

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
main pushes whenever the caller declares a nonempty browser test list. Every
full run covers those tests, even when only an unrelated crate changed.

Ordinary pull requests, advisory-only and fuzz-only runs skip the job; warm-cache
runs start it only on default-branch pushes. Removing the request skips it.
An intentional browser skip does not block required checks; a recipe failure
still fails CI. `browser_build` and `test_tools` remain independent opt-ins.

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

### Manual browser red and green proofs

Run Chromium tests on a named caller branch, tag or commit without joining the
merge queue. The dispatch calls this same reusable workflow with
`browser-only: true` and `checkout-ref`; only declaration validation and the
existing Chromium job run. Keep the caller's nonempty `browser_test_crates`
declaration, pinned tools and `browser-test` recipe. No new declaration key or
command hook is needed. Leave `warm-cache`, `advisory-scan` and `fuzz-scan` off.
The declaration job fails if the proof cannot select browser tests or is
combined with any of those modes.

Add `.github/workflows/browser-proof.yml` to the caller's default branch. Replace
`RELEASE_COMMIT` with the full commit SHA of a released workflow version that
supports `browser-only`, using the same release as the caller's ordinary CI:

```yaml
name: Browser proof
on:
  workflow_dispatch:
    inputs:
      ref:
        description: Caller branch, tag or commit to test
        required: true
        type: string
permissions:
  contents: read
jobs:
  browser:
    permissions:
      contents: read
      actions: read
    uses: Orchestration-Maestro/maestro-rust-workflows/.github/workflows/ci.yml@RELEASE_COMMIT
    with:
      browser-only: true
      checkout-ref: ${{ inputs.ref }}
      concurrency-key: browser-proof
```

This entry has no checkout, test commands or secrets of its own. The automatic
job token has read-only contents and Actions access. Dispatch only caller refs
you intend to execute; those refs supply the recipe and test code. Keep the
workflow selection on `main` (or the caller's default branch) while the `ref`
input selects the source under test. A dispatch does not cancel an in-progress
proof. All dispatches share one concurrency group, so a newer dispatch replaces
any queued proof.

The caller's `browser-test` recipe must report every selected case even after a
failure, and return a failing status if any case fails. For a nextest browser
run, pass `--no-fail-fast`. Other browser drivers must collect all case results
before returning. The shared workflow runs the recipe as written; it does not
change the caller's test-runner settings. The caller-side recipe update and
live red/green proof are tracked in
[Orchestration-Maestro/maestro#4015](https://github.com/Orchestration-Maestro/maestro/issues/4015).

The job captures both output streams from `just browser-test` in
`browser-test.log`. After a passing or failing recipe, it uploads
`browser-test-browser-proof-<attempt>`; without that concurrency key the middle
part is `root`. Setup failures before the recipe have no recipe log. The log
upload does not turn failing tests green. Manual proofs restore compatible
compiled caches but never publish cache snapshots.

To save a red and green pair:

1. Push a throwaway caller branch with one deliberate assertion mutation. Keep
   the test registered and keep its browser selection unchanged. Record the
   mutated commit SHA and the test's name.
2. Dispatch from the caller checkout and record the new run ID and URL:

   ```bash
   gh workflow run browser-proof.yml --ref main -f ref=ci/browser-red
   gh run list --workflow browser-proof.yml --event workflow_dispatch --limit 5
   ```

3. Wait for that run to finish. It must fail at the mutated browser test, not
   during compilation or setup. Download its artifact, replacing `RED_RUN` and
   the attempt number with that run's values:

   ```bash
   gh run download RED_RUN --name browser-test-browser-proof-1 --dir proof/red
   ```

4. Restore the assertion on a green caller ref and dispatch again with the same
   workflow release. Record the source SHA, run ID and URL. The browser job must
   pass. Download `browser-test-browser-proof-1` from `GREEN_RUN` into
   `proof/green` with the same command.
5. Save `proof/red/browser-test.log` as `<test>-red.log` and
   `proof/green/browser-test.log` as `<test>-green.log` with the change's test
   evidence. Check that the red log names the expected assertion failure and
   reports all selected cases, and that the green log passes the same test set.
   Keep both run URLs, source SHAs and the shared release SHA beside the logs.

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

On full runs, the caller's rustup toolchain installs each requested target and
Clippy, then runs:

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
Ordinary pull requests run declared `aarch64-apple-darwin`,
`x86_64-apple-darwin` and `x86_64-pc-windows-msvc` checks in parallel.
PR macOS and Windows checks use the same selected packages as host lint instead
of `--workspace`; an empty selection keeps the check result without compiling.
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
`.github/workflows/fuzz.yml` calls the [released workflow](../README.md#versions),
pinned by full commit SHA, in that mode for Maestro's `main` every night at
04:41 UTC and on manual dispatch. Each target job installs
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

For ordinary CI, keep standard Rust files and the existing organization-managed
workflow call; no extra workflow or ruleset is needed. Manual browser proofs use
the thin dispatch entry above. The four original quality/docs/tests
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

## Full suite after landing

A caller can run `ci.yml` on every push to `main`, without `warm-cache` or scan
inputs. Checkout uses the push's exact commit. A completed successful merge-group
run of the same shared workflow revision at that exact commit, with evidence
that the full workspace was selected, lets the push skip the
test legs described in [Test policy](#test-policy-and-caller-permissions).
Otherwise it runs every test shard, doctests and declared browser tests.
Keep the independent cache warm-up on every main push. The landing suite and
merge queue can use different calling workflows; their shared workflow revision
and evidence scope must match.
Push runs have separate concurrency groups by calling workflow and commit: later
landings cannot cancel a run or replace a queued landing.

Maestro's `Full suite after landing` workflow owns that call. Its
`Failed landing reports` workflow listens for completed main-push runs with
`workflow_run` and calls `post-landing.yml` with the completed `run-id`.
Keep both shared-workflow references pinned to reviewed commits. There is no
new `.github/ci.toml` setting for test selection or commands; list the caller's
reporting workflow in `privileged_trigger_workflows` for the existing trigger
audit. GitHub activates `workflow_run` listeners only after they reach the
default branch.

The reporter needs `contents: read` and `actions: read` on the job token to
read the exact run attempt and failed job logs. It uses the existing
`RELEASE_APP_CLIENT_ID` organization variable and `RELEASE_APP_PRIVATE_KEY`
secret to mint a repository-scoped App token with contents, workflows, issues and pull
requests write access. Workflow write access lets it revert a landing that
changed CI files. The App must be installed in the caller repository and
its secret must be available there. Do not substitute the automatic job token
for these writes: it does not trigger required checks on the new revert PR.
No credential is passed to the full-suite test jobs.

A failed suite creates one issue per landing commit, naming the exact merged
PR (or every PR in a batched push), commit, failed-test log lines and original run. The reporter paginates jobs
and reads only the current run attempt. It opens `ci/revert-<full-commit>` against
`main`, reverting only that landing (first parent for a merge commit), from the
current main tip. The PR links the original PR and failure issue. It uses the
normal required checks and merge queue; the reporter never pushes to `main`
or merges the revert. Existing issues and open or closed revert PRs are reused
on repeated failure handling. If a push succeeded before PR creation failed,
the next attempt reuses the landing's existing revert branch.

A merge queue can squash several PRs in one push, and only the push's last
commit gets a suite run. The reporter walks back from that commit to the
previous commit with its own main-push run of the same workflow (within 50
commits; it fails with the run link if none is found). When it finds more than
one PR, the issue names all of them, newest first, and the reporter fails with
the run link without a revert, because the failing PR is unknown. Revert the
culprit in a normal reviewed PR.

A passing suite creates neither artifact. Unknown or ambiguous PR attribution,
unreadable logs, missing permissions and revert conflicts fail with the original
run link. A conflict can leave the failure issue without a revert PR: resolve
the conflict in a normal reviewed PR, not by force-pushing main. Non-suite job
failures are reported as safeguard failures rather than blamed on another PR.

## CI speed guards

Six guards keep the merge queue from slowing down again. Each one fails a check
or files an issue on its own; none waits for someone to notice. The numbers
below are the first values measured on Maestro, the largest caller, and each has
its source next to it. Tighten them with `with:` inputs once the 32-shard and
build-once changes have run on that caller.

`ci-guards.yml` holds the guards that judge a finished run, plus the exception
check. A caller gives it the completed run to judge from a `workflow_run`
listener, and runs it without a run id on pull requests for the exception check:

```yaml
on:
  workflow_run:
    workflows: [Rust CI]
    types: [completed]
jobs:
  guards:
    if: >-
      github.event.workflow_run.event == 'merge_group' ||
      (github.event.workflow_run.event == 'push' &&
       github.event.workflow_run.head_branch == github.event.repository.default_branch)
    permissions:
      contents: read
      actions: read
      issues: write
    uses: Orchestration-Maestro/maestro-rust-workflows/.github/workflows/ci-guards.yml@<reviewed commit>
    with:
      run-id: ${{ github.event.workflow_run.id }}
```

The listener passes default-branch pushes as well as merge groups, because those
runs save the compile snapshots every other run restores, and it passes a
cancelled one too: a cancelled seeding run is a finding, not silence. A
default-branch push that calls the shared suite through one reusable job is
judged too. GitHub titles its jobs `<calling job> / <shared job>`, and every
guard matches a job against a pinned name after the last ` / `, so
`suite / Caller declaration` counts as `Caller declaration`. The reuse,
required-checks, time-per-unit, shard-balance and slow-test guards read those
names. When the `Reuse merge queue tests` step in `Caller declaration`
succeeded, the skipped test shards and doctests leave the pinned list, exactly
as they do in a run with no prefix.

Pin the reference to a reviewed commit and list the listener workflow in
`privileged_trigger_workflows`, as for the post-landing reporter. Issues are
filed with the automatic job token. An open issue with the same title is never
filed twice, so a breach that persists leaves one issue, and a job that cannot
read the run fails instead of passing.

| Input | Default | Used by |
| --- | --- | --- |
| `selection-window` | 10 | Selection check |
| `selection-share-ceiling-percent` | 25 | Selection check |
| `shard-balance-margin` | 1.5 | Shard balance |
| `slow-test-seconds` | 30 | Test-time limit |
| `hang-limit-seconds` | 120 | Test-time limit |
| `cache-storage-limit-gib` | 10 | Cache usage |

### Time per unit of work

The guard holds time per unit of work, not raw wall time. A run that does twice
the work and takes twice as long is natural growth: it is accepted without
anyone acting and files nothing. Two kinds of work are measured:

- Test jobs, in seconds per test run. Each test shard ends with a `Work units`
  notice, `tests: N`, read from the nextest summary; the rate is the seconds of
  the step that ran the tests (`Tests` or `Isolated tests`) over the shards'
  test runs, within a runner band. Downloads and setup are never test work, and
  shards that run side by side each add their own seconds.
- Build jobs, in seconds per compile unit. The `Build test archive` cache step
  prints `compiles: N; hit-rate: P`, the executed compile requests and the hit
  rate of the sccache statistics.

The seconds are work time, never wall time: a gap with no job executing counts
for nothing, and skipped and cancelled jobs are not work. The union of the jobs'
start-to-end intervals is the run's work time, which only the runner wait below
subtracts. A run is compared only
with earlier runs that executed the same jobs (the same job names), so a run
that added or lost a job has no like-for-like baseline until three runs have the
new set. The stored ratchet level of a band, kind and workflow is shared across
job sets.

A run whose rate for either kind is more than 10% over its baseline, and
whose gap is more than 2 s per executed job in total (the rate gap times the
run's units of that kind, against two seconds for each job that did them), is a
regression and files "CI run slower than its baseline: run N" at once. The
bound is the measurement's own error (supervisor ruling S7759): GitHub stamps
job and step times to the second, 1 s at each boundary, so a work-time
difference is measurable only above 2 s per executed job. A difference within
it is neither a regression nor an escalation, for a rate below its spread as well;
anything above it at +10% fires, and anything above it outside the band's
spread escalates. The
issue names the run and the band, gives each regressed rate against its baseline
and links the latest baseline runs. Fix it at its cause; nothing accepts it.

Hosted Linux runners come in two speed bands: the same work, cache and image
ran 1.45 to 1.6 times slower on some runners (maestro-rust-workflows runs
40990998136 and 40931459907). Every Linux job therefore starts with a `Runner
hardware` step that prints the CPU model and core count, adds a notice
annotation and appends the line to the job summary, with a stable label:

```text
Runner hardware: AMD EPYC 7763 64-Core Processor, 4 cores (runner-band: amd-epyc-7763-64-core-4c)
```

A run's runner band is the label of its slowest labelled job, which sets the
run's pace. The guard reads it from the annotation (`runner-band: <label>`); a
run with no label is in the band `unknown`. Of the last `selection-window`
earlier successful runs of the same workflow and event (10 by default), only
those in the same band count, and a run newer than the judged one never does.

The recent baseline of a kind is the median of its rates in the previous five
successful runs in the band. The guard also keeps the lowest such median it has
seen for each level, a workflow, event, band and kind, in one open issue titled
"CI time ratchet". The baseline is the lower of the two, so a staircase of small
regressions cannot raise the line, and the limit is the baseline plus 10%.
Fewer than three earlier rates in the band means no baseline and no issue.
Merge groups, pull requests and pushes to `main` are judged; CodeQL and every
other run are not, because they gate nothing. Keep the ratchet issue open: the
guard rebuilds a missing one from the next runs, and a lasting slowdown it has
not seen is then taken as the new level.

A stored level has one writer: only the guard lowers it, when the recent median
is lower, and nothing raises it; no input does. A lasting slowdown keeps firing
until its cause is fixed. A deliberate new cost that is not natural growth needs
its own owner decision first.

The runner wait is its own measure: the run's wall time minus its work time,
which is the time spent pending for a concurrency group or a runner, before the
first job or between jobs. A run ends at its last job's end, never at its
`updated_at`, which keeps moving after the last job when a check is re-requested
or the pull request changes: run 41098096027 ended its last job at 17:38:57,
`updated_at` read 17:56:00, and no job waited over 10 s. It is never CI slowness. It is out of pattern only
when it is over the median wait of the like-for-like baseline runs by more than
10% and by more than 2 minutes, and then the guard files "Escalation: runner
wait over its median" (a capacity issue, never a regression). Source: Maestro
run 41006271785 waited 36.8 min before its first job and worked 17.0 min, and
the advisory run 41059668738 of this repository waited 16 s and 28 s longer for
runners than before while its work time matched.

Source: no rate is fixed in advance; the first baseline of a band is its first
three runs. Maestro merge-group runs 40990557025 and 40990555729 took 39.3 and
39.8 minutes with 16 shards, and the baseline moves down by itself as the
32-shard change makes runs shorter. A judged run
reads the jobs of at most `selection-window` earlier runs, the annotations of
the test shards and the archive build of the judged run and of at most five
baseline runs, and shard logs only on a merge group or main push, or when the
shards are unbalanced. With 32 shards and the default window that is at most
about 285 calls for the time guards and about 315 when the shards are
unbalanced, against the job token's 1,000 calls an hour for the whole
repository. The cache reads add one call (cache usage), up to six pages of the
cache list (the largest entries past 90%, and the snapshots of a default-branch
run), and the floor annotations of the other jobs that ran a floor step, about
15, so about 310 and 340. To respond, open the run link and fix the cause of the
kind that regressed.

### Compile-cache hit-rate floor

Every job that saves a compile snapshot warns when its compile-cache hit rate is
under 90%: Format and lint, Docs, Build test archive, Prepare test setup, the
test shards and Doctests, Dependency, source and coverage checks, each Linux and
macOS target check (the Windows target checks skip the statistics step, so they
are never judged) and the Wasm and Chromium browser builds. A default-branch push
seeds every one of these shapes (`tests-coverage`, `tests-plain`, `quality`,
`docs`, `checks`, `targets-clippy-<target>`, `browser`, `browser-tests` and
`test-prepare`), so each is judged on merge groups, pull requests and
default-branch runs alike; only a run in which no cacheable compilation ran is
not judged. It never fails the build: a change can invalidate the cache itself,
such as a toolchain bump, and only a push to the default branch saves a
snapshot again, so a failing floor would block the merge group on every retry.
The floor is fixed in `ci.yml`. Source: the archive build of merge-group runs
40990557025, 40990555729, 40968689273 and 40946549381 hit 100.00%, 100.00%,
100.00% and 99.67%. Before the check covered every shape, Format and lint, Docs,
Doctests and the browser builds of Maestro merge groups 40322701820, 40620121647,
40870001533 and 40993559130 compiled cold, about 4,040 misses per group, because
the warm-up stayed on a release without their writer, and the first group after a
default-branch push (40996420703) restored all five at 100%. The warning is an
annotation that names the shape and one cause. The guards read it from a
merge-group or main-push run, and from a pull-request run when a caller passes
one, and file "Compile cache below its hit-rate floor", or add a comment to the
open issue listing each job and its warning, so repeated cold builds on
default-branch pushes, which are the regression, show as a growing list of
comments:

- **Cold snapshot:** a snapshot was restored but lacks this build's objects.
  Run the warm build on the default branch to seed it again.
- **Key change:** no snapshot matches the job shape, though others of the same
  job exist. The compiler, runner or caller directory changed; the next warm
  build on the default branch seeds the new key.
- **No writer:** the repository cache holds snapshots of other jobs but none of
  this shape, so no default-branch push saves it. Check that the caller's warm-up
  runs a release of this workflow that has the writer.
- **Eviction:** the repository cache holds no compile snapshot at all. Check the
  cache size and the seven-day expiry, then run the warm build.

### Compile snapshots and cache usage

A push to the default branch is the only run that saves compile snapshots, so
the guards judge those runs themselves, cancelled ones included, and the
repository cache they fill.

- "Escalation: default-branch run did not seed its compile snapshots" is filed
  when the run's jobs reported more saved snapshots than the repository cache
  holds for the run (the save step never fails the build, so a lost save shows
  only here), when jobs measured their compile cache and none saved, or when the
  run was cancelled with jobs unfinished. The cache list is read for the run's
  default branch and matched by the run id in the snapshot key. Source: on
  2026-10-26 between 16:13 and 21:00 UTC other caches filled the 10 GiB limit and
  every default-branch snapshot was evicted, and eight merge groups
  (40369902252 to 40395312817) compiled the test shape cold at 0% to 28.8%, their
  test shards taking a median 17.4 min against 13.2 min afterwards.
- "Repository cache above 90% of its limit" is filed when the cache in use
  (`actions/cache/usage`) is over 90% of the repository's storage limit, with
  the five largest entries by ref. The limit is the `cache-storage-limit-gib`
  input, 10 GiB by default: the platform default, and Maestro's limit. A caller
  that raised its limit in the repository's Actions settings passes the new
  value. The guard can't read the setting itself. GitHub refuses the job
  token's read of `actions/cache/storage-limit` with HTTP 403 (Maestro run
  41873062666): the read needs administration read, and no job token can hold
  administration. A value that is not a positive number fails the job before
  any read. The guard fires at 90% of the storage limit the caller declares,
  because over the limit GitHub evicts the oldest snapshots, which is how the
  2026-10-26 cold groups happened, and the margin gives the guard time to file
  before that. The usage is read for every judged run.
  Source: Maestro held 10,928,201,953 bytes (10.18 GiB) in 22 entries on
  2026-10-30 against its 10 GiB limit. The guide and the finding use GiB. To respond, delete the pull-request entries the issue lists, or
  shrink what the snapshots hold.

Both titles are standing: a repeat adds a comment to the open issue.

### Selection check

The selection step logs and summarizes `Selection share: N% (a/b test
packages)`. When a change selects every package it also logs
`Full-suite fallback: <trigger>`, with the file when one caused it. The
selection guard judges pull requests only. Merge groups and main pushes
intentionally run 100% of the workspace.

The guard reads the last `selection-window` completed PR runs of the same
workflow and files "Test selection share above its range" when their median
share exceeds `selection-share-ceiling-percent`. It also escalates a PR share
outside the earlier runs' normal range. The issue lists each run's share and
trigger. Read those triggers to find shared inputs that need a `test_inputs`
route. Source: of 14 measured PR runs, 11 selected 1 to 3 of 51 packages
(median 2%) and 3 selected all 51.

### Shard balance

When the slowest `Test shard` job takes more than `shard-balance-margin` times
the median shard, the guard files "Test shards unbalanced: run N". The issue
reads the blamed shard's own log, so only tests that ran on that shard appear:
the ones that are new, or reached a higher slow mark since the last green run of
the same event. Nextest prints only slow marks, so growth shows as a higher mark
rather than exact seconds.

Beside the tests, the issue lists the fixed phases the same log times: the
extraction of the instrumented targets, each test setup, and the tests' own
seconds from the nextest summary. A shard past the margin is often slow from
that fixed work before its first test runs, so read those lines before blaming a
test.

The default is 1.5. Source: the slowest shard was 1.18 and 1.20 times the median
in runs 40990557025 and 40990555729, and 1.32 times in the 32-shard proof run
40974924871. To respond, fix or split the named tests, then check priorities in
the nextest configuration; when the fixed phases hold most of the shard's time,
the cause is the shard's composition or its setups.

### Test-time limit

Nextest flags a test slow after `slow-test-seconds` (30), and the caller's
`slow-timeout` period should match. A merge-group or main run files one issue
for every test it flagged, "Test over 30 seconds: <test>", with no cap on how many one
run files; an open issue for the test is never filed twice. Fix the test at its
cause; do not raise a timeout.

The kill is one hang limit, `hang-limit-seconds` (120). A nextest override may
exceed it only for a test that needs more, and the comment directly above its
`slow-timeout` must name the issue that removes it. The `Test-time exceptions` job
fails for an override that names no issue or only closed ones, and names the
filter. Close the issue only after removing the override. Source: 30 and 120
seconds are the limits Maestro chose after a single 30-second kill failed 79 of
99 shard jobs in one day; its one exception, `browser_author_bootstraps_from_empty_build_state`,
needs 208 seconds.

### Required checks

For a full merge-group or main run, `ci-guards.yml` pins the jobs that must run:
`Caller declaration`, `Format and lint`, `Docs`, `Build test archive`, the
`Test shard` jobs, `Doctests`, `Tests (ubuntu-latest)` and `Dependency, source
and coverage checks`. A pinned job that is missing or skipped files an issue
titled `Required checks missing: Docs, Doctests`, with the missing set filled
in: `Test shard (*` stands for any shard, and `Dependency, source and coverage
checks` is one check despite its comma. The title never carries the run id, so
a set that stays missing across landings keeps one issue; each body names the
judged run. A failed job is not a missing one; the run already reports it. The
self-test checks every pinned name against `ci.yml`, and checks that this
repository's ruleset checks, `Required repository quality` and `Required
consumer tests`, still exist, so renaming a job fails there first. When you
rename a job on purpose, change the pin and the ruleset in one change.

On a main push that reuses merge-queue tests, the successful `Reuse merge queue
tests` step in `Caller declaration` records the decision. The required-checks
guard permits shard and doctest skips only with that marker. The archive and
every other pinned check must still run. Job-shape comparisons keep reused runs
separate from full-suite runs, so the deliberate skips do not report removals.

Pull requests require `Caller declaration`, `Format and lint`, `Docs`,
`Dependency, source and coverage checks` and each declared native target result:
`Check (aarch64-apple-darwin)`, `Check (x86_64-apple-darwin)` and
`Check (x86_64-pc-windows-msvc)`. A failed target check fails the workflow.
Undeclared targets do not create required results. Pull requests keep the test
and build job names but skip their work. The required-checks guard enforces
execution only on merge groups and main pushes, so intentional PR skips do not
file missing-check findings.

### Escalations

Anything that stands out of the pattern is escalated explicitly: the guard files
an issue with the label `escalation`, or adds a comment to the open one with the
same title, naming the run, its baseline runs and what departed from them.
Nothing outside the pattern is absorbed silently. The normal spread of a rate is
the range of the band's earlier rates widened by 10% of their median; for a hit
rate or a share it is the range of the earlier values widened by 10 points.
Three earlier values are needed for a spread. Each title is standing, so a
departure that repeats shows as comments on one issue:

- "Escalation: time per test run outside its normal spread" and "Escalation:
  time per compile unit outside its normal spread": a rate below the band's
  spread, which means work that did not run. A rate above the spread is the
  regression above, filed once with the same label.
- "Escalation: jobs or steps changed without matching work": a job or step
  that appears, disappears or grows (past the earlier range plus 10% and 30
  seconds) in a successful run. The seconds a rate judges (the archive build,
  each shard job and the `Tests` or `Isolated tests` step of a shard) are only
  checked for appearing and disappearing. Every other shard step, such as setup,
  tools, coverage or a download, is judged for growth like any step: a slow
  download is a transfer finding, never a regression of the tests rate.
- "Escalation: compile cache hit rate outside its usual range": the hit rate of
  the archive build, read from its `Work units` notice, outside the earlier
  hit rates widened by 10 points.
- "Escalation: test selection share outside its usual range": the run's share
  of workspace test packages selected, outside the earlier runs' shares
  widened by 10 points.

To respond, open the run link, find what departed and either fix its cause or,
if the change is deliberate, take it to the owner; the guard stays quiet again
once the earlier runs show the new pattern.


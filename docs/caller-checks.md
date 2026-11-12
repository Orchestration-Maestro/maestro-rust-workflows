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
The proof uses `0027a687e4f844031159535ad2a43325e2457305`, the Maestro main
revision that qualifies the current release, and it still includes the native
residency-helper compilation repair. Its first parent,
`55305aa7f2bfd38723b119fb757a1ffa396e80d7`, supplies the warm build and
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
dependency/source checks and the declared Windows target check. Host
Clippy and the Windows target check select changed crates and their workspace
dependents; formatting still checks the whole workspace. A declared target
check prints the selected packages, then lints each one's tests and examples in
its own run. One run over several packages shares a feature set and would hide a
package that lacks a dependency feature its tests need. Pull requests skip the
test archive, test setup, test shards, doctests and wasm/browser builds. Two
exceptions still compile: the declared [example witnesses](#wasm-example-witnesses),
and the [Emscripten crates](#emscripten-builds), which the sandbox runtime job
builds, or restores from its cache, on every run when the list is non-empty.
Every required check name still reports; skipped jobs count as success.
Merge groups run the full Linux shard matrix, Linux doctests,
coverage and all declared builds and foreign-target checks except the native
arm64 macOS route. They keep `Check (x86_64-apple-darwin)`. Main pushes also
run the claimed macOS host's test archive, preparation, four test shards and
doctests. The arm64 macOS archive replaces its separate target check there.
A macOS test failure after landing goes through the existing
[post-landing safeguard](#full-suite-after-landing).

A main push reuses Linux tests when its exact commit already passed a completed
merge-group run of the same shared workflow revision with the full workspace
selected. That push skips Linux test shards, Linux doctests and coverage, but
still runs the macOS tests and doctests the group skipped. The archive and
preparation jobs still run on every test host and save compiled-cache snapshots;
they upload test archives and setup only for hosts whose tests run. A merge
group cannot save snapshots itself: only default-branch pushes write them. The
Wasm and browser build and Chromium browser tests jobs also still run, so browser
tests run a second time on that commit. The Linux archive keeps the same
instrumentation and cache shape as the merge group.
Formatting, lint, docs, dependency/source checks and target checks still run. No
caller setting changes this policy.

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

Every step that holds the job token or a caller secret and starts Python from
standard input or `-c` uses isolated mode, `python3 -I`, so the current directory
stays off `sys.path`. A module at the caller's checkout root, such as `json.py`
or `subprocess.py`, is not imported while the step holds that token. Runner
selection, merge-queue evidence, the content check's caller-file check, the
compile-cache floor, test setup and archive downloads, cache pruning, and the
release SBOM check run this way. A seam proof reads every token-bearing step in
`.github/workflows/` and fails on any Python start without `-I`. It also plants
a `json.py` in a scratch checkout: the compile-cache floor step ignores it, and
the same body without `-I` does not.

Writer and reader resolve the shared identity once. The raw workflow context
qualifies only when it names this shared workflow's full repository path.
Reusable callers resolve that same path from the current run's referenced
workflows, with exactly one matching entry. Missing, conflicting or ambiguous
identities, missing or expired artifacts, unreadable evidence and API errors all
keep the full suite. Runs whose merge group left no evidence keep the full suite.
Fixture checkout/event overrides, scan calls and `warm-cache` calls do not use
the result lookup.
The independent `warm-cache` call still runs on every main push and seeds the
cache shapes that keep a snapshot; its normal doctest and browser commands run
without saving one ([Jobs that keep no snapshot](#jobs-that-keep-no-snapshot)).

Below, main-push test and coverage behaviour describes pushes without a matching
merge-queue result. The independent `warm-cache` call runs on every main push.

See [Test shards](#test-shards) for the required test result and coverage rules.

Callers grant `contents: read` and `actions: write` to the reusable-workflow
call. Read access lets the shared workflow reuse prepared test bundles from
earlier runs. Only two kinds of job take write access, and neither checks out
anything or runs caller code: the pruning jobs in
[One entry per prefix](#one-entry-per-prefix), on default-branch pushes, and the
canceller in [Superseded merge-group runs](#superseded-merge-group-runs), on
merge groups. Every other job declares `actions: read`. GitHub checks each
job's permissions against the call's grant when the run starts, even for jobs
that will skip, so a call that grants less cannot start. Add this grant before
the release-pin update.

The workspace coverage floor remains 80% (or the caller's higher floor).
Declared callers collect fresh coverage for the full workspace on merge groups
and main pushes. Every shard must provide its report, and the aggregate applies
the floor to their union. Pull requests collect no coverage and need no previous
run's coverage artifacts.

## Release PR self-test reuse

Release pull requests need no extra configuration. When a PR from this
repository's `release-please--branches--main` branch changes only release
metadata, `CI self-test` reuses a successful run that tested its base commit
or an identical repository tree.
The allowed files are the release manifest and the version file and changelog
named by the base commit's release configuration. The default changelog is
`CHANGELOG.md`. For example, a release changing only that changelog,
`.github/release-please/manifest.json` and `version.txt` can reuse its base run.

Reuse requires a completed, successful `ci-internal.yml` run in the same
repository. An exact-base `push` run qualifies directly. Otherwise, selection
looks up a small evidence artifact named for the base tree SHA. The artifact
must identify this workflow, record a full self-test and contain the same tree
SHA. The matching run can be a PR run on a different commit with identical
files. For example, the merged PR's tested tree can qualify while the new main
push is still running. Release creation stays on main pushes; it does not wait
for that push's self-test.

After all fixture and proof jobs and repository quality pass, a full self-test
records its checked-out tree and workflow identity. It uploads the evidence
under `self-test-evidence-<tree SHA>`; a rerun replaces that run's artifact.
Selection reads only the evidence for the requested tree, checks the producing
run and accepts one JSON file of at most 4 KiB. The evidence is a same-repository
workflow self-report, not an independent attestation. A run that reuses evidence
does not upload fresh full-self-test evidence.

Missing, expired or invalid evidence, failed or unfinished runs and lookup
errors keep the full self-test. A change outside the metadata files also keeps
the full self-test. Other PRs, merge groups and main pushes keep their normal
runs; caller repositories and release-pin updates are unchanged.

`Self-test selection` shows `Reuse release base self-test` only when it reuses
successful evidence. On reuse, the fixture and proof jobs skip their work. Both
required checks, `Required repository quality` and `Required consumer tests`,
still run and accept those skips only when selection succeeded and proved reuse.
Without reuse, repository quality requires the sample, declared caller and
caller test tools to succeed. Consumer tests require repository quality and every
workflow proof to succeed. Failures and cancellations still fail the required
checks.

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

## Superseded merge-group runs

A merge group that the queue drops can never merge. The queue drops a group
when it rebuilds (main moved, or an entry ahead of it left), when its pull
request leaves the queue, and when the pull request is closed. Every
merge-group run starts a `Cancel replaced merge groups` job. The job cancels
the unfinished runs of the same workflow whose group the queue no longer holds,
and their runners, macOS included, go to the live groups.

A queued group runs on a ref named `gh-readonly-queue/<base>/pr-<number>-<hash>`.
When the queue drops a group, it deletes that ref. If it later queues the same
pull request on the same base, it reuses the name for a new commit. A run is
replaced when its ref is missing or points at a commit other than the run's
own. The job lists this workflow's queued and in-progress merge-group runs,
reads each run's ref and cancels every replaced run. A live group's ref still
points at its own commit, so live groups are never cancelled.

The job has no `needs`, so it starts with the run, beside the declaration. It
holds `contents: read` and `actions: write`, checks out nothing and runs no
caller code. A failed read or cancellation prints a `Replaced merge groups`
warning, marks the step failed and leaves the job green, so it never fails the
live group that ran it. A run that finished between the listing and the
cancellation is not an error. Pull requests, pushes, and scan, warm or
browser-only calls do not run the job.

Concurrency groups cannot do this. GitHub cancels a job only when a later job
joins its group, and a dropped group may have no successor: a closed pull
request has none, and a rebuild builds only as many groups as the queue's build
limit allows. Queue positions and bases shift as groups merge, so any group
name read from the ref that ties a dropped group to a live one also ties two
live groups together.

Each run also checks its own ref, right after the declaration is read and again
as the last step of `test-build` and `test-prepare`, the two jobs `tests` waits
for. A missing or moved ref fails the job and skips the jobs that need it, so a
run replaced before its builds end never starts its test shards. Any other API
failure lets the run continue.

Merge-group runs are grouped by the calling workflow's name and commit, so a
new run starts its canceller even when the queue reuses the old run's ref name.

Cancellation happens when a new group starts. A pull request removed from the
end of the queue starts no new group, so the jobs its run already started run
to completion, but test shards that had not started never start. The next
group's run cancels it, or cancel it by hand.

## Reusable-call concurrency

Push and merge-group runs are grouped by the calling workflow's name and
commit; other runs are grouped by ref. Two caller workflows on one commit run
at the same time, later landings never wait for each other, and a rebuilt merge
group starts even when the queue reuses its ref name. The optional workflow
input `concurrency-key` defaults to an empty string, which adds nothing to the
group. If a workflow calls this
reusable workflow more than once, give each calling job a distinct, stable
key (case-insensitive), such as `concurrency-key: quality`. Reuse that key on
later pushes so a new pull-request run cancels the older invocation of the
same job, not another job in its own run. Do not use a run ID as the key.
A job, not concurrency, cancels the runs of merge groups that the queue
dropped; see [Superseded merge-group runs](#superseded-merge-group-runs). The
caller must not share the same concurrency group with its called workflow; see
GitHub's [reusable workflow concurrency rules](https://docs.github.com/en/actions/reference/reusable-workflows-reference#limitations-of-reusable-workflows).

## Declaration

The `wasm_crates` and `check_targets` arrays are required, including when empty.
`browser_build` and `test_tools` are optional booleans that default to `false`.
The test run reaches the pinned `rg` and `fd` only through files the
`test_prepare` command copied into `TEST_PREPARE_OUTPUT`, never through `PATH`.
`wasm_examples` is an optional table that maps package names to example names
and defaults to `{}`. `emscripten_crates` is an optional array of package names
and defaults to `[]`; a non-empty list also makes every compiling job wait for the
[generated runtime script](#generated-runtime-script). The [schema](../.github/ci.schema.json) describes the
parsed TOML shape; there is no version key or older-format reader.

`check_targets` also selects native test hosts: `aarch64-apple-darwin` adds
`macos-latest`. These test legs run after landing on main pushes, including
pushes that reuse the merge group's Linux results. Merge groups, pull requests
and warm calls keep Linux-only tests. The other supported targets,
including `x86_64-pc-windows-msvc`, remain compile-only. The test jobs already
carry a Windows leg, but `windows-latest` joins the native test hosts only once
its cold preparation fits the job's 45-minute step limit.

`reserved_ports` is an optional array of unique integers from 1 to 65535. When
set, every Linux test job runs
`sudo sysctl -w net.ipv4.ip_local_reserved_ports=<list>` before the tests, so
tests that must bind a fixed local port are not displaced by an outbound
connection that the kernel assigned that port. This setting does not reserve
ports on macOS or Windows. Without it nothing runs.

An absent file runs the standard checks without isolation, search tool
installation or optional jobs. After declaration validation, pull requests
run formatting, Clippy, docs, dependency/source checks and the declared
Windows target check in parallel. Merge groups and main pushes also run the Linux
test archive, test setup, full shard matrix, doctests, coverage, wasm/browser
builds, Chromium tests and declared foreign-target checks. The native macOS
test route runs only after landing on main; merge groups keep the x86_64 macOS
compile check. Main pushes may reuse the Linux results as described above.
Pull requests skip these jobs without removing their required check names.
An unreadable file, malformed TOML,
duplicate key, unknown key, missing array, scalar array value, non-string
member or non-boolean tool/build flag fails declaration validation. An empty
present document is invalid. Integer and string booleans are not accepted.

Names are exact, without whitespace or Unicode normalization. Each wasm name
must match a package in the caller's locked Cargo metadata, as must each
`emscripten_crates` name (which may also come from an additional locked
manifest). Each `wasm_examples`
example must be an example target of its package. Each target must match the
caller compiler's target list and the runner table below. An unknown package,
unknown target or recognized target without a runner fails with a file/key
diagnostic before optional work is emitted. Metadata or compiler inspection
failures also fail validation. Wasm and target array order and repetitions are
retained; declaration text is never executed as shell syntax.

macOS target checks use the runner's built-in Bash. Callers do not need to
install a newer shell.

## Tool downloads

No caller setting is needed. Release downloads use the job token. The Rust
workflow scopes download credentials to installer steps; caller compilation does
not inherit them. The compile-cache daemon starts without either download token.
Mise uses `MISE_GITHUB_TOKEN` for GitHub tool installs.

GitHub release requests send an Authorization header and allow only HTTPS
requests and redirects. Curl drops the header when GitHub redirects to another
host. Release-asset storage (`objects.githubusercontent.com` or
`release-assets.githubusercontent.com`) uses the signed URL GitHub supplies,
not a forwarded job token. The pinned checksums are verified before tools run,
including when an archive is restored.

The download inventory is:

| Download | Hosts | Credentials |
| --- | --- | --- |
| sccache; mise bootstrap in Rust and content checks | `github.com`, redirected release-asset storage | Job token on GitHub; signed storage URL |
| cargo-nextest; cargo-llvm-cov; ripgrep; fd | `github.com`, redirected release-asset storage | Job token on GitHub; signed storage URL |
| cargo-deny; cargo-hakari; cargo-machete; typos; gitleaks; zizmor; actionlint; cargo-fuzz; shfmt | `github.com`, redirected release-asset storage | Job token on GitHub; signed storage URL |
| wasm-bindgen browser runtime | `github.com`, redirected release-asset storage | Job token on GitHub; signed storage URL |
| Chromium | `cdn.playwright.dev`, `storage.googleapis.com` | Public download; no job token |
| Playwright and Playwright Core | `registry.npmjs.org` | Public download; no job token |
| Caller tools installed by mise | Hosts selected by the caller's pinned `mise.toml` | Mise uses the job token for GitHub; other hosts use their own authentication |
| Rust toolchains and components | `static.rust-lang.org` | Public download; no job token |
| Cargo dependencies | Registry and locked source hosts selected by the caller | Cargo's configured authentication; not a release-asset download |
| Actions, source checkouts, caches and run artifacts | GitHub APIs and their storage services | Actions' built-in authentication or the explicit job token; signed storage URLs |

Test archive builds share checksum-addressed release archives as a run artifact,
once per runner host and workflow call. Release archives and compiled test
archives use the repository's default artifact retention, so both remain available
when failed shards are rerun. Test shards restore the bundle before installing
tools and still run each installer's original checksum verification. The artifact
name includes the operating system, architecture and call key; Actions reads it
only from the current run. Entries
contain downloads, not credentials or installed paths. Using artifacts leaves
the repository's limited cache storage available for compiled snapshots.

Parallel compilation jobs, doctests, warm builds, standalone content checks and
browser runtime downloads remain independent. Caller-declared mise installs also
run per job, authenticated for GitHub: their caller-owned mutable state and
platform-specific paths do not belong in the shared archive artifact. Only the
workflow's checksum-pinned release archives are shared across test shards.
No download retry or timeout policy changes with authentication.

## Compile cache

Cached compiling jobs export `CARGO_BUILD_RUSTC_WRAPPER=sccache`, so callers do not
need to set `RUSTC_WRAPPER` for caching. A build that a test starts after removing
`RUSTC_WRAPPER` keeps the cache without coverage instrumentation. Coverage runs
still instrument workspace builds and send their compiles through the cache.

| Jobs | Host | Restore | Save |
| --- | --- | --- | --- |
| Warm coverage build, Build test archive, Prepare test setup, Dependency/source/coverage checks | Linux | Every call that runs the job | Default-branch pushes only; one writer per test shape |
| Wasm and browser build | Linux | Every call that runs the job | Default-branch pushes only; the one writer of the `wasm32` shape |
| Chromium browser tests | Linux | Every call that runs the job | Never; they read the `wasm32` shape the Wasm build saves |
| Format and lint, Docs, Doctests, declared target checks | Linux | Never | Never; see [Jobs that keep no snapshot](#jobs-that-keep-no-snapshot) |
| Native test archive builds and test preparation | arm64 macOS | Every call that runs the job | Default-branch pushes only, including those that reuse merge-queue tests |
| Native doctests | arm64 macOS | Every call that runs the job | Never; they restore the native archive's snapshot |
| Declared target checks | arm64 macOS | Every call that runs the job | Default-branch pushes only; each target-matrix entry saves, including repeated targets |
| Declared target checks and native test jobs | x86_64 Windows | Never | Never |

Windows compilation runs uncached because sccache expands response files beyond
Windows' command-line limit; [Windows caching](https://github.com/Orchestration-Maestro/maestro-rust-workflows/issues/166)
tracks the upstream fix.

Because Windows has no compile cache, a Windows run with no earlier prepared
bundle builds everything cold on four cores. The cold preparation of a large
workspace sits near the 45-minute step limit and varies with the runner's
processor, so Windows tests wait until it fits.

A push with `warm-cache: true`, including a feature-branch push, also runs the
declared target checks; warm calls for other events skip them. Only
default-branch pushes save those compiled-cache snapshots. Host-specific,
checksum-pinned assets are listed in [Compile cache](../README.md#compile-cache).

Only a default-branch push warm call also runs Format and lint, Docs,
Doctests, Wasm and browser build, and declared Chromium browser tests.
They run their existing checks, not substitute builds. Browser tests and
doctests execute during these warm calls; the coverage warm build still uses
its optional test filter. The Wasm build seeds the `wasm32` snapshot there and
the other Linux jobs keep none, so the warm call saves only that shape. Other
warm calls skip these additional jobs.
No extra cache writer is enabled for pull requests, merge groups, feature
branches or manual runs.

### Jobs that keep no snapshot

A Linux job saves a snapshot only when a job restores it: the warm coverage
build, Build test archive and Prepare test setup feed the test shards of the
same run, the Dependency, source and coverage checks job is the whole build of
a caller without the archive, and the Wasm build's `wasm32` snapshot is what the
Chromium browser tests restore on a later run. Format and lint, Docs, Doctests
and the Linux target checks restore nothing and save nothing, because one
snapshot for each of them would not fit the cache limit next to the others (see
[One entry per prefix](#one-entry-per-prefix)). They compile cold, and a job on
another host keeps its snapshot. Compile cache identity decides this once, in
its `snapshot` output. The restore step runs only when `snapshot` is `true`, the
save step needs the `writer` output that is `true` only for a snapshot-keeping
job on a default-branch push, and the hit-rate floor reports the job's compile
count without judging a hit rate it cannot reach.

The Wasm build and the Chromium browser tests compile the same wasm32 graph, so
they share one shape and one entry. The Wasm build is its only writer, on
default-branch pushes. The Chromium job restores it read-only, and because it
does not wait for the Wasm build it reads the entry the previous push saved. Its
cap is the default 1 GiB.

Cold times measured on Maestro, against the warm time of the same job:

| Job | Cold | Warm |
| --- | --- | --- |
| Docs | 7.0 min | 3.9 min |
| Doctests | 6.8 to 8.0 min | 2.8 min |
| Wasm and browser build | 8.3 to 12.3 min | 3.1 to 3.5 min |
| Check (aarch64-unknown-linux-gnu) | 4.7 to 5.5 min | 3.0 min |

The table records Linux job costs; macOS tests run after landing. The extra
Linux minutes add no cost on a public repository. The Chromium browser tests
have no warm row yet: their cold baseline is the compile work inside the
`browser-exports` setup, and the first default-branch push that writes the
`wasm32` snapshot measures their warm time.

An entry a job saved before it stopped keeping a snapshot stays in the cache
until GitHub's seven-day expiry removes it, because nothing restores it and the
pruners keep the newest entry of each prefix.

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
held 5,606,616,026 bytes: the other compile snapshots, the Cargo git entry
and one test setup bundle. The setup snapshot adds one entry of about 1.1 GB to
each generation: at most 1.6 GB of objects, plus about 6 MB of preprocessor
entries. GitHub evicts by last access, so the previous generation goes first:
once a shape saves its new snapshot, no reader restores the old one. The pruning
jobs below delete that old generation after each shape saves, so it no longer
waits for eviction. What one generation adds up to is the budget in
[One entry per prefix](#one-entry-per-prefix).

### One entry per prefix

Cache entries cannot be overwritten, and every default-branch save uses a new
key, so each push adds one entry per shape beside the one it replaces. A restore
reads only the newest entry of a prefix. After each job that saves a cache, a
pruning job named `Prune superseded caches (<job>)` lists the default branch's
entries, keeps the newest entry of each compile, Cargo git, test setup and
browser packages prefix and deletes the others. It runs only on default-branch
pushes after its saving job succeeds, holds only `actions: write` and checks out
nothing. Another pruner may delete an entry first; that is not an error. Any
other failed deletion prints a `Cache prune` warning, marks the step failed and
leaves the job green, and the next pruner tries again. Entries under other
names, such as a caller's own caches, are never touched.

The repository cache holds one generation, so the snapshots every kept shape
saves, plus the Cargo git entries, the test setup bundles and the browser
packages, must add up to under 90% of the 10 GiB limit, the level at which the
usage guard files an issue. Over the limit GitHub evicts the newest snapshots of
the shapes restored least recently, and those shapes then compile cold. One
snapshot for each of the twelve shapes Maestro compiled needed 11.66 GiB, so the
Linux jobs listed in [Jobs that keep no snapshot](#jobs-that-keep-no-snapshot)
stopped saving. Measured from Maestro's default-branch push 42428071607 on
2026-11-08, with the `wasm32` shape at its cap in place of the two native Cargo
git entries, one entry per kept shape holds 8.19 GiB; with the browser packages
entry, 8.21 GiB, 82.1% of the limit:

| Entry | Host | GiB |
| --- | --- | --- |
| `test-prepare` | Linux | 1.35 |
| `tests-coverage` | Linux | 0.98 |
| `test-prepare` | macOS | 1.33 |
| `tests-plain` | macOS | 0.81 |
| `targets-clippy-x86_64-apple-darwin` | macOS | 0.37 |
| `targets-clippy-aarch64-apple-darwin` | macOS | 0.36 |
| `wasm32` | Linux | 1.00 |
| `cargo-git` | Linux | 0.41 |
| `test-setup` | Linux | 0.82 |
| `test-setup` | macOS | 0.76 |
| `browser-packages` | Linux | 0.02 |

The `browser-packages` row is a bound taken from the uncompressed download size
of the Chromium job's package step on 2026-11-11 (9 packages, 21.9 MB), not a
measured cache entry. While a new runner image rolls out, each of the two
images keeps an entry, 0.04 GiB together, until GitHub evicts the retired
image's entry after 7 days unread. A shape that joins Maestro needs a
row here, and the self-test fails when a kept shape has no row or the rows pass
90%. While a run saves, a shape's new entry and the one it replaces exist
together until the pruner that follows its saving job runs. That job waits for
the whole matrix, so a Linux entry can outlive its replacement by as long as the
macOS leg takes. GitHub evicts by last access, so the replaced entry, which
nothing has read since the run began, normally goes first.

### Cargo git dependency cache

No caller setting is needed. Linux jobs restore Cargo's `git/db` and
`git/checkouts` before resolving dependencies: Lint, Docs,
dependency/source/coverage checks, declared target checks, Build test archive,
Prepare test setup and every test shard. Doctests share the test-shard route. A hit containing the locked
revision avoids fetching that git source; Cargo still downloads missing registry
packages.

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

Only a default-branch push that missed the exact key saves an entry, and only
Build test archive on Linux saves it. The locked sources are the same on every
host, and one entry per native host does not fit the cache beside the compile
snapshots, so macOS and Windows write none and restore none: their target
checks, archive builds and test shards fetch those git sources cold. Only Linux
jobs restore the entry, because archives never cross operating systems.

These entries count against the 10 GB repository allowance. One measured Linux
entry for Maestro's `wasmtime` revision and submodules is 436 MB compressed.
Allow one entry per caller scope and retained lock-file revision; measure a new
lock file's size before claiming a saving. Cache eviction
can remove an entry before the next reader runs.

## Test shards

Tests use checksum-pinned cargo-nextest 0.9.148. Each of the declared
`test_shard_count` shards runs test binaries, balanced by the seconds
their tests took on an earlier run when the tests are built (see
[Build once, test many](#build-once-test-many)). Each claimed native test host
runs `min(4, test_shard_count)` shards of its own. The `warm-cache` call is
the exception: one job compiles the tests and runs its warm test filter with
no archive. The
caller's `.config/nextest.toml` supplies
test-suite runner metadata such as resource groups for fixed-port tests. Shared
CI controls shard grouping and coverage; profile default filters are ignored so
they cannot omit tests. Runner user configuration is not loaded. `test_shards` was removed; setting it fails
with a clear diagnostic. Unit and integration tests run exactly once per test host; doctests
run once with `cargo test --locked --doc` in a parallel `Doctests` matrix leg
for merge groups and main pushes. These events always select every workspace
package. Pull requests skip both test shards and doctests.
Native shards are named `Test shard (<runner> k/n)`, with `Doctests (<runner>)`
for their doctests. Linux names remain `Test shard (k/N)` and `Doctests`.
Native legs run only on main pushes, including those that reuse merge-queue
Linux results.

`test_shard_count` sets the number of shards. It is an optional whole number
from 1 to 60 and defaults to 16. Any other value, including a float, a string or
a boolean, fails the declaration job with `test_shard_count must be a whole number from 1 to 60`.

```toml
test_shard_count = 32
```

More shards spread the tests over more runners, and each shard repeats its own
setup. The shard matrix, the binary groups, the job names and the coverage union
all use the declared count on full runs, so the union expects exactly that many
reports. Each claimed native test host runs its own shard matrix of
`min(4, test_shard_count)` shards. Only Linux uploads coverage. Pull requests
run no test shards and upload no coverage.

Declared Linux callers run `cargo llvm-cov nextest` in the same isolated environment
with their opted-in tools. Only the Linux numeric shards upload LCOV, without
individual thresholds. The run writes its raw profiles without a report, the
step checks every profile, and `cargo llvm-cov report` merges them into the
report. A process that a test stops while it exits leaves its profile empty or
cut. The check fails the step for an empty profile or one the toolchain's own
merge tool rejects, the same tool the report merges with, so another
`llvm-profdata` earlier on `PATH` reads nothing here. It names each file with
the binary whose merge signature it carries, without the pool index the runtime
appends. It finds the binary by running each program of the instrumented build
once, and only when a profile is damaged; it prints the seconds it took either
way. The merge then runs as before: no profile is skipped or deleted.
`Tests (ubuntu-latest)` requires all matrix legs,
including native shards and every `Doctests` leg, to succeed on full runs.
Native legs run plain nextest without coverage instrumentation. On a main push
that reuses merge-queue Linux results, the aggregate requires successful native
shards and `Doctests` legs when the caller claims native test hosts. Without
native hosts, it requires skipped test and doctest jobs. On pull requests it
requires a successful declaration and skipped test and doctest jobs, regardless
of package selection. On full runs it downloads all reports, counts each source line once
(covered if any shard hits it), and enforces `coverage_min_lines` on the union.
Missing reports or low coverage fail.
Undeclared callers use the same test shards but retain their single coverage job
for merge groups and main pushes. Both coverage modes run tests through nextest,
so caller setup scripts run in either mode (reusing prepared outputs in shards). Single-job coverage uses one partition
(`slice:1/1`); declared callers combine one report per shard. Both modes use the same
line-counting policy: normalize source paths, count each line once, and enforce
the coverage floor on the union of hits. PRs skip coverage for all callers.

### Build once, test many

On merge groups and main pushes, one `Build test archive` job compiles the
workspace's unit and integration tests once, with `cargo nextest list`. Declared
coverage builds in `target/llvm-cov-target` with the environment
`cargo llvm-cov show-env` reports, less show-env's own `CARGO_LLVM_COV_SHOW_ENV`,
`CARGO_LLVM_COV_TARGET_DIR` and `CARGO_LLVM_COV_BUILD_DIR`. That is the
instrumentation, directory and compile-cache key set `cargo llvm-cov` builds
with, so shards report coverage from the paths it reads. The job then splits the test binaries into one group per
shard by the seconds their tests took before. A binary whose tests take longer
than the mean shard load cannot fit one shard, so its tests are placed one by
one. The slowest item goes first, and each goes to the group with the fewest
seconds so far. Shard 1 gets the slowest group. A binary stays whole when it has
one test or a test name holding a character outside letters, digits, `_` and `:`.
A filterset travels on a command line, which Windows caps at 32767 characters, so
a split binary whose test names would push a shard's filterset past 30000
characters stays whole as well, the one adding the most name characters first. The job log prints each shard's tests and binaries, with its
seconds when an earlier run timed them, and `cargo nextest archive -E` writes
one archive per group from the binaries already built.

Each host's job groups the binaries into that host's own shard count: the
declared `test_shard_count` on Linux and `min(4, test_shard_count)` on a claimed
native host. That way every group has a shard that runs it.

The job keeps each shard's archive filter on its own line of `target/test-filters.txt`
with a Unix line end, and reads the file with a plain `while read` loop. Windows
Python would otherwise end each line with a carriage return that nextest rejects,
and the Bash 3.2 on macOS runners has no `mapfile`.

The seconds come from the `Earlier test seconds` step, before the build. It takes
the newest earlier successful merge group or push run of the same workflow in
which every test shard of this caller on the same host succeeded, adds up the
`PASS` seconds those shards' logs print for each test, and logs the run
it read with its branch, commit and date. Each host reads only its own shards:
`Build test archive (macos-latest)` times its binaries from the earlier
`Test shard (macos-latest 1/4)` to `(macos-latest 4/4)` jobs, never from the
Linux shards, so a run without macOS shards is passed over. A test that run
did not time weighs the timed tests' mean seconds. With no
such run, or when the lookup fails, the step says so and every test weighs the
same: the binaries go by test count. The timing decides only which shard runs
each test, never which tests run. This step holds the run's token, and its
Python runs isolated, so no module in the caller's checkout is imported while it
holds the token.

Each numeric shard downloads only its own archive and runs the tests its
filterset selects with `--archive-file`. No partition flag applies. A split binary
sits in every archive that holds a part of it, and each filterset names that
shard's tests of it, so every test runs on exactly one shard. Its coverage report reads that same archive with
`--nextest-archive-file`, so the report finds the binaries the shard ran. When a
caller has fewer placed items (whole binaries and split tests) than shards, the
extra shards hold no binary: they run nothing and upload an empty coverage report, so the union
still receives one report per shard. Existing locked dependency fetches stay
available for tests that intentionally start child builds. `Doctests` compiles and
runs doctests once per tested host on every merge group and main push. On a main
push that reuses merge-queue Linux results, the Linux leg skips and the claimed
native legs still run. It does not wait for the archive build.

Native hosts follow the same route: one `Build test archive (<runner>)` job and
one `Prepare test setup (<runner>)` job per claimed host, each with its own
shard count. Their artifacts carry the same host suffix, so each shard reads
only its host's archives and prepared files. Linux coverage records only the
Linux checkout root, regardless of which archive build finishes last. Shards
wait for every host's archive and preparation; a failed host build fails the
required result.
Warm-cache runs keep their existing compile route and cache shape.

The compiling `cargo nextest list` passes `--timings` to Cargo. Download the
`test-build-timings-<concurrency-key>` artifact (`test-build-timings-root` when no
key is set) and open `cargo-timing.html` to inspect per-crate build times.
The artifact includes the report from `target/cargo-timings/` for plain builds or
`target/llvm-cov-target/cargo-timings/` for coverage builds. A successful archive
build must produce a timing report; a missing report fails the upload.

Each shard fetches the test setup bundle in up to 16 parallel byte ranges from artifact
storage, checks the size and SHA-256 digest the Actions API reports, then extracts.
Storage without range support supplies one stream. A digest or size mismatch fails the
step before anything is extracted. If the listing has no digest or the transfer fails,
the step hands over to the single-stream `download-artifact` action without retrying
individual ranges.

The test archive artifact holds every shard's archive and `test-binaries.json`, which
records for each archive the number of test binaries in it and the filterset of the
tests it runs. A shard reads the artifact's zip directory, then
fetches only `shard-<n>.tar.zst` and `test-binaries.json` in parallel ranges and checks
each against the size and CRC-32 the zip records. A mismatch fails the step. A
compressed member, an unreadable directory or a failed range hands over to the
single-stream action, which downloads every shard's archive.

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

`Prepare test setup` runs that command once per test host, in parallel with host compilation,
after the caller's declared tools and locked manifests are available. It starts
from the caller's working directory. `TEST_PREPARE_OUTPUT` names the fixed
`target/test-setup` output directory there; write only reusable runtime outputs
inside it. `CARGO_TARGET_DIR` points at a separate temporary build directory.
Preparation must not depend on the host test binaries. A failing command fails
the required test result. The default is no preparation.

On the Linux host of a run that collects coverage, `TEST_PREPARE_COVERAGE_ENV`
names a file of `export` lines: the compiler wrapper and flags the coverage build
uses. A command that prepares a test child running workspace code sources the
file around that child's build only, never around its own tooling. Before the
report, each shard copies every instrumented executable left in
`TEST_PREPARE_OUTPUT` into the coverage build directory, so the child's lines
count. The child must exit on its own: a profile cut by a kill fails the coverage
step and names the child. On other hosts and on runs without coverage the
variable is unset. A bundle prepared for coverage has its own cache key.

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
identity (repository, directory, host and compiler), the checkout path and the
pinned `rg` and `fd`. A glob that matches no file fails the job. On reader
runs, `Prepare test setup` restores the bundle on an exact hit and skips the
`test_prepare` command.
When preparation is enabled, default-branch pushes run the command even on an
exact hit, including warm builds and pushes that reuse merge-queue tests.
These writers download the closest bundle as a base only when the compiled-cache
snapshot they restored is warm: an earlier writer saved the compiler objects of
every component that bundle holds, and the digest check builds each component
whose inputs changed, so what the writer compiles seeds the rest. When the
snapshot is cold, the writer passes no `TEST_PREPARE_BASE` and compiles every
input. A cached bundle alone does not contain compiler objects. Without the
list a warm build does not prepare. Every numeric shard downloads the prepared directory as before.
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
- Pushes add no candidates: a merge group saves no snapshot, so no snapshot holds
  the compiler objects of the components its bundle built.
- A pull request run adds nothing, so every added bundle was built by the code
  being merged, starting from a bundle the default branch saved. Runs from forks
  are never added: a bundle holds executables that the run then starts.

When the runs cannot be listed, the job logs a warning that names the failed
request. Reader runs prepare from the bundles found before the failure;
writers never list candidates.

On reader runs, `Prepare test setup` extracts each bundle to its own directory,
separate from `TEST_PREPARE_OUTPUT`, and sets `TEST_PREPARE_BASE` to the list of those
directories before the command runs: the restored bundle first, then the added
ones, joined with the host path separator (`:` on Linux and macOS, `;` on Windows). A bundle that is missing, expired, fails its size or
digest check or cannot be read is left out, and the later ones are still tried.
The command may copy or reuse any file in a listed directory whose own inputs
are unchanged, and must still write every file it needs to
`TEST_PREPARE_OUTPUT`. When no bundle is available, `TEST_PREPARE_BASE` is
unset. Reader runs still skip the command on an exact hit. A writer on a warm
snapshot lists only the restored bundle; on a cold snapshot it leaves
`TEST_PREPARE_BASE` unset. Pushes that reuse merge-queue tests save compiled-cache
snapshots without uploading the test archive or setup artifacts, since no test
shard in those runs needs them.

Every step that runs the workspace tests starts them with `TEST_PREPARE_BASE`
set to `<runner temp>/maestro-test-prepare-base`, a path that holds no bundle.
The build host exports the variable for every job, so without it a test that
starts a setup child passes here and fails after landing: the child reads a
prepared bundle the test never asked for. A test that clears the variable before
it starts its child is unaffected, and nothing reads the path. The isolated test
environment starts empty, so the tests step passes the variable in explicitly.

```toml
test_prepare = "sh .config/prepare-tests.sh"
test_prepare_setups = ["author-component"]
test_prepare_inputs = [".config/prepare-tests.sh", "tools/**/*.toml"]
```

### Test diagnostics

Declared callers receive `MAESTRO_TEST_DIAGNOSTICS_DIR`, an absolute path to
`target/browser-author-diagnostics` under the caller's working directory. The
workflow creates it outside disposable HOME/TMP isolation and forwards it to
test shards and the doctest owner described in [Test shards](#test-shards).
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

### Files the tests leave behind

A test that writes into the caller's checkout can be staged and committed, so a
shard fails when its tests leave files there. `Test checkout baseline` records
`git status --porcelain --untracked-files=all` for the caller's working
directory before either test route starts. `Tests leave no files behind`
compares the same list after them and names every entry the tests added as
`::error file=<path>::The tests left <path> in the checkout`, then fails the
shard. The comparison subtracts the baseline, so a checkout that was changed
before the tests is not blamed on them, and a deleted tracked file fails like an
added one.

The build directory `target/`, the test log `nextest.log`, the shard reports
`shard-<n>.lcov` and the `mise.lock` that `Test tools` writes are the workflow's
own files in the checkout. They are declared once in `TEST_CHECKOUT_EXCLUSIONS`
and never fail a shard. Git-ignored files are not compared because they cannot
be committed; nextest extracts its archive into the build directory, so archive
members never appear either. A downloaded `shard-<n>.tar.zst` and
`test-binaries.json` are in the baseline.

The two steps sit in the step list that the `Test shard` and `Doctests` jobs
share, so both test routes and the warm-cache run are checked. The check runs
even when a test failed, which names the files a failing test left. A caller
fixes the test, not the check: a test writes into a temporary directory it owns,
and once the test stops writing there, the next run is green.

### Pull-request selection and retries

Pull-request host Clippy and Windows checks select changed workspace crates,
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

Host Clippy and the declared Windows target check lint the selected packages
with `--all-targets`. Merge groups and main pushes select every workspace crate:
host Clippy lints them in one workspace run, and the target check lints each one
in its own run.
An empty PR selection performs no host or target Clippy compilation; the named
checks still report success. An absent base selects every package for lint.
Wasm Clippy ignores the selection and lints every `wasm_crates` package on every
run; see [Wasm and browser builds](#wasm-and-browser-builds). Pull requests
skip the test archive, preparation, test shards, doctests and wasm/browser
builds regardless of package selection, except for the example witnesses. The
Emscripten crates compile in the sandbox runtime job, which runs on every pull
request that lists them, so no browser job installs the compiler.

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
declared Chromium tests and coverage, except a main push that reuses
merge-queue tests: it takes the Linux suite from the merge group, skips the
Linux shards and doctests, turns coverage off, and runs the declared Chromium
tests and the claimed native hosts' shards and doctests. They do
not narrow tests using a diff or previous coverage. The queue is the merge gate;
a suite failure stops the merge.
Main-push failure reporting and revert handling remain in the post-landing
workflow.

`test_inputs` maps repository-relative files or directories to the exact
package names whose tests read them. A change under a declared path selects
those packages and their reverse dependencies, including Markdown files there.
A path inside a member directory also selects the member that owns the file, so
a test that reads every crate's sources lists the crates' parent directory. An
empty list marks a path no workspace test reads, such as a separate fuzz
workspace. Paths must stay inside the repository
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
  CDLA-Permissive-2.0, MPL-2.0, MIT-0, BSL-1.0 and 0BSD. BSL-1.0 is the Boost Software
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
--fail-under-lines 80`. Declared coverage runs within the test shards and
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
  { package = "specific-library", version = "1.2.3", licenses = ["Unlicense"] },
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

Opted-in tests install ripgrep 15.2.0 and fd 10.5.0 from pinned release archives
for their host: Linux x86_64 musl, macOS arm64 or Windows x86_64 MSVC.
Installation verifies their published SHA-256
digests before extraction. The dedicated binary directory exposes `rg` and
`fd` on the `PATH` of the setup and prepare steps. The test run does not
inherit it, so `test_prepare` copies the programs into `TEST_PREPARE_OUTPUT`:
see [Caller-pinned build and test tools](#caller-pinned-build-and-test-tools).
Every tool-installation step has a 10-minute timeout
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
overrides are not inherited. The test shard argv uses `cargo llvm-cov nextest`; doctests use
`cargo test --workspace --locked --doc`. Native numeric shards use plain
nextest instead of the Linux coverage route.

Windows keeps its system locations (`SYSTEMROOT`, `WINDIR`, `SYSTEMDRIVE`,
`COMSPEC`, `PATHEXT`, program and program-data directories), processor metadata
and `OS`. The workflow matches these system-variable names without regard to
case and keeps their values unchanged. `USERPROFILE` points to the fresh
isolated home. The workflow creates
`AppData/Roaming` and `AppData/Local` beneath it and sets `APPDATA` and
`LOCALAPPDATA` to those directories. Windows known-folder APIs require these
directories to exist; they resolve inside the isolated profile rather than the
runner's application settings or caches. XDG locations remain separate.
Paths passed to native programs use Windows mixed separators.

The test setup exports Rustup's active toolchain before compilation; both test
routes pass it to child builds, including dependencies outside the checkout.
Explicit Bash run defaults preserve runner failures through log capture. A failed
runner fails its test step, so coverage uploads run only after success; a missing
report after success fails the upload separately. On full runs the required aggregate accepts
only successful matrix legs, including `Doctests`, and requires one report per
numeric shard when merging coverage. On a main push that reuses merge-queue
Linux results, Linux shards and `Doctests` skip; claimed native test hosts still
run their shards and doctests, and the aggregate requires success. Without
native hosts, it requires skipped test and doctest jobs. On pull requests the
aggregate requires a successful declaration and skipped test and doctest jobs.

The original home and auth file are never read, moved or modified. Only the
isolated directories are cleaned on success or failure; other action steps
retain the runner's normal home and cache access.

On Linux, before cleanup, the step kills every process still using the isolated
directories: as its working directory, executable or open file, or through the
isolated environment it inherited. Processes running the compile cache
executable are left running for the statistics and seal steps. The log lists
each stopped process as `Stopped test process <pid>: <command>`, so the test
that leaked it can be found. If the directories still cannot be removed, the
step logs `Could not remove <directory>`. Cleanup never changes the exit status
of the test command, whether it passed or failed.

## Caller-pinned build and test tools

With `test_tools = true`, every enabled caller-compiling job installs the
caller's `mise.toml` tools before lint, documentation, test/coverage shards,
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
The nextest shard and doctest argv, credential guards,
fresh directories and the child environment allowlist are unchanged. Only the
installed tools' executable paths are added, not mise's environment settings.
Windows paths are published in native format for later steps.

The test run reaches the pinned `rg` and `fd` only through files your
`test_prepare` command copies into `TEST_PREPARE_OUTPUT`, for example
`cp "$(command -v rg)" "$TEST_PREPARE_OUTPUT/"`. Nextest setup scripts run
inside the test run, so they and the tests read those copies. A setup script
can export the paths as `MAESTRO_TEST_RG` and `MAESTRO_TEST_FD` for tests that
expect them. The shared pins of `rg` and `fd` are part of the prepared bundle's
cache key, so a pin change prepares again. For programs you pin yourself, list
the files that fix their versions, such as `mise.toml`, in `test_prepare_inputs`.

The `Ambient path` step records `PATH` before any tool step runs. The isolated
test command then removes each directory a tool step added afterwards that holds
`rg` or `fd`, whether it comes from the shared search tools or from your
`mise.toml` pins. A test that looks up `rg` or `fd` on `PATH` fails unless the
runner image itself provides it. The tool steps and `test_prepare` keep the full
`PATH`; nextest setup scripts and tests get the filtered one. Directories that
were on `PATH` before the tool steps, and added directories that hold neither
program, stay. Linux, macOS and Windows follow the same rule.

## Documentation

Every call that runs the Docs job builds the workspace documentation with
`RUSTDOCFLAGS="-D warnings -D missing_docs"`:

```bash
cargo doc --workspace --no-deps --locked
```

An undocumented public item or a documentation warning fails the job. Merge
groups and default-branch pushes that build the tests also document every
feature, so an item that only a non-default feature compiles is documented
before it merges. A pull request documents the default features only, which
keeps its Docs job as fast as before.

## Wasm and browser builds

Every call that runs Format and lint, pull requests included, lints the listed
packages for the wasm target with warnings denied, in one Clippy run with a `-p`
for each package:

```bash
cargo clippy --locked --target wasm32-unknown-unknown -p "$FIRST" -p "$SECOND" -- -D warnings
```

Host Clippy never compiles code that only a WebAssembly build sees. A lint
finding or a compile error in a wasm-only body fails Format and lint before the
change merges. The run checks library targets only, as the wasm build does,
because test targets may depend on native-only crates. It lints every listed
package whatever the pull request changed; an empty list compiles nothing.

On merge groups and main pushes, Linux also runs this command for
each listed package:

```bash
cargo build --locked --target wasm32-unknown-unknown -p "$CRATE"
```

Each listed package is then documented for the same target with
`RUSTDOCFLAGS="-D warnings -D missing_docs"`:

```bash
cargo doc --locked --no-deps --target wasm32-unknown-unknown -p "$CRATE"
```

That covers the code that only a WebAssembly build compiles, which the host Docs
job never sees. Missing documentation or a documentation warning fails the job
before the browser recipe runs.

The caller toolchain installs wasm support only when there are listed
packages. Either `test_tools = true` or `browser_build = true` installs caller
tools once before wasm compilation. The browser flag is independent: it does
not enable tools in other jobs. With `browser_build = true`, the workflow runs
`mise exec -- just browser-build` after successful wasm builds, including when
the wasm array is empty. Both wasm builds and the browser recipe run on every
merge group and main push. Pull requests, advisory scans, fuzz scans and warm
calls outside a default-branch push skip them, except for the
[example witnesses](#wasm-example-witnesses). A failed wasm build prevents
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

## Wasm example witnesses

Quick start: declare the examples whose body compiles only for
`wasm32-unknown-unknown`. Native Clippy never builds that body, so only a wasm
check sees it break.

```toml
wasm_crates = ["my-browser-library"]
wasm_examples = { "my-browser-library" = ["browser_check", "import_check"] }
```

`wasm_examples` is an optional table that maps exact locked package names to
arrays of exact example names. It defaults to `{}`.

The Wasm and browser build job runs this command for every pair, after the
`wasm_crates` builds, in declaration order (packages as written, then each
package's examples):

```bash
cargo check --locked --target wasm32-unknown-unknown -p "$CRATE" --example "$EXAMPLE"
```

Each compile starts with the log line `wasm example <package>::<example>`. The
job stops at the first failure; the last `wasm example` line names the witness
that broke. Declaring examples installs the wasm target even when `wasm_crates`
is empty.

Witnesses compile on pull requests, so a broken one fails the pull request that
breaks it. That run does only the witness compile. The crate builds,
documentation, `browser_build` tools and the browser recipe stay on merge groups
and main pushes. `test_tools = true` still installs the caller's tools first.

Declaration validation fails with the file and key when a package is not in the
locked Cargo metadata or an example is not a target of its package. Repetitions
are kept. Declaration text reaches cargo as arguments and never runs as shell
syntax.

## Emscripten builds

Quick start: list the packages that compile for `wasm32-unknown-emscripten`.
Maestro installs the pinned Emscripten SDK and the target's standard library,
then builds each package.

```toml
wasm_crates = []
check_targets = []
emscripten_crates = ["my-sandbox-runtime"]
```

`emscripten_crates` is an optional array of exact locked package names, in
build order. It defaults to `[]`. A non-empty list also requires a
`sandbox-runtime` recipe in the caller's `justfile`; see the
[generated runtime script](#generated-runtime-script). Repetitions are kept, and the names reach
cargo as arguments, never as shell syntax. A name outside the root or the
additional locked metadata fails declaration validation with the file and key.
With an empty list or no key, no job installs or builds anything for
Emscripten.

A package that lives in its own locked workspace works the same way once that
workspace's manifest is in `additional_locked_manifests`. The name is checked
against that manifest's own `cargo metadata --no-deps --locked`, and the build
passes `--manifest-path` for it:

```toml
wasm_crates = []
check_targets = []
additional_locked_manifests = ["runtime/Cargo.toml"]
emscripten_crates = ["my-sandbox-runtime", "my-host"]
```

Here `my-sandbox-runtime` is a package of `runtime/Cargo.toml` and `my-host`
belongs to the root workspace. Listing a manifest twice, or under another
spelling, still names one package. Two different packages with the same name
fail validation, because the build would reach only one of them.

A non-empty list makes the [`Sandbox runtime script`](#generated-runtime-script)
job the one place the listed packages compile. On a cache miss it installs the
caller's toolchain and tools, the SDK and the target, and runs the caller's
`sandbox-runtime` recipe. No other job runs `emcc` or builds for
`wasm32-unknown-emscripten`; every other job downloads the file the recipe
writes. The Chromium browser tests job installs no SDK: a browser
recipe that needs it runs its own build.

The SDK comes from the emsdk 6.0.10 release archive on github.com. That request
is the only one that carries the job token, and the step drops the token before
the installer runs. Each archive is verified by SHA-256 before it is unpacked:

| Archive | SHA-256 |
| --- | --- |
| emsdk 6.0.10 release | `09cbafdf00e5a7b4275fc27229d69befafc5ca6070543124219977101310dade` |
| Node 24.19.0, Linux x64 | `14b342e71204f811bde6153be8e04b62aef63c236fef92b55f9c83154b409647` |
| Emscripten 6.0.10 toolchain binaries, Linux x64 | `a51dd2829bdf725d268974728e032533b106ea01cb482a62288ec12627a7abbe` |

The two archives the installer would fetch itself are downloaded and verified
first, then placed in its download folder, so the installer never reads an
unchecked file. The step then checks that `emcc` reports 6.0.10 and exports
`EMSDK` and `EMSDK_NODE` for later steps. The SDK is MIT and NCSA licensed.

The caller owns the binding step and the browser application. The shared
workflow provides the toolchain, the build of the listed packages and the run of
the caller's [`sandbox-runtime` recipe](#generated-runtime-script).

### Generated runtime script

Quick start: define a `sandbox-runtime` recipe in the caller's `justfile` that
writes `target/sandbox-runtime/maestro-sandbox-runtime.js`, and pin `just` and
any other tool the recipe runs in `mise.toml`. Maestro runs the recipe once per
run, before any job compiles the caller.

```toml
# .github/ci.toml
emscripten_crates = ["my-sandbox-runtime"]
```

```just
# justfile
sandbox-runtime:
    cargo run --quiet --locked -p my-tooling --bin development -- sandbox-runtime
```

A caller that embeds a generated file in a crate cannot compile until the file
exists. When `emscripten_crates` is non-empty, the `Sandbox runtime script` job
builds that file once and every compiling job fetches it. It runs on pull
requests too, so a change outside the listed packages still finds the file. An
empty list skips the job and every other job runs as before.

| Step | What it does |
| --- | --- |
| Script identity | Hashes every input of the script into one cache key. |
| Restore sandbox runtime | Restores the file on an exact hit. A cache it cannot read costs the preparation, never the job. |
| Toolchain, Mise bootstrap, Test tools | Install the caller's pinned toolchain and `mise.toml` tools, also when `test_tools` is `false`, because the recipe needs them. |
| Emscripten SDK, Emscripten target | Install Emscripten 6.0.10 and put `emcc` on the `PATH`, then `rustup target add wasm32-unknown-emscripten`. |
| Build sandbox runtime script | Runs `mise exec -- just sandbox-runtime`, then fails if the file is missing. |
| Save sandbox runtime | Saves the file under the same key, on a push to the default branch. |
| Upload sandbox runtime | Publishes the file as the `sandbox-runtime-<concurrency key or root>` artifact of the run, on a hit and on a miss. |

On a hit the job costs the checkout, the key and the restore: the toolchain,
the tools, the SDK, the target and the build are skipped, about 110 to 125
seconds of a 145-second job. On a miss every one of them runs, so a miss costs
what the job cost before the cache. A merge group reads what the last push to
the default branch saved; a pull request reads the same entry.

The key covers every input of the script:

- the directories of the packages `emscripten_crates` lists, and the manifests
  and lock files that declare them;
- the directories of the packages the `sandbox-runtime` recipe runs, found from
  the `-p` and `--package` arguments of its recipe body, so a change to the
  generator misses the cache;
- every path dependency the named packages declare, followed through
  the caller's `cargo metadata`, so a change to a crate the generator builds on
  misses too;
- the `justfile`, the root `Cargo.toml` and `Cargo.lock`, `mise.toml` and
  `rust-toolchain.toml`;
- the revision of this workflow, resolved once per run from the run's referenced
  workflows, the way the merge-queue evidence resolves it, so a caller's commit
  never enters the key. A revision that cannot be resolved makes the key new on
  every run, so such a run never serves a script it did not build.

A recipe that names no package cannot be read that way, so the whole checkout
decides its key: such a caller never serves an old script, and never hits
either.

Every job that compiles the caller waits for `Sandbox runtime script`: Format
and lint, Docs, Build test archive, Prepare test setup, the test shards,
Doctests, Dependency, source and coverage checks, the target checks, Wasm and
browser build, Chromium browser tests and the fuzz job. Each one runs the step
`Sandbox runtime script` straight after its checkout. It downloads the artifact
into `target/sandbox-runtime`, before the first cargo command.

A failed generator does not skip those jobs: they start, the download fails
them, and each required check reports the failure. The job keeps no compile
snapshot, so a miss runs the recipe's cargo build cold.

### Emscripten crates on pull requests

The listed packages compile in `Sandbox runtime script`, which runs on every
pull request that lists them, so a pull request always builds the script it
changes and no browser job installs the compiler.

`Wasm and browser build` runs on a pull request only for the declared
[example witnesses](#wasm-example-witnesses). A listed Emscripten crate has
nothing left to build there, so a change that reaches one does not start the
job; the sandbox job still compiles it. The Chromium browser tests job does not
run on pull requests.

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
Playwright, Chromium and binding CLI versions. The Linux system libraries the
browser needs install with sudo; see [System packages](#system-packages).

### System packages

The Chromium job installs the Linux packages the browser links against from a
cache, so a merge group runs its browser tests even when the Ubuntu archive is
unreachable. The packages are the ones Playwright 1.64.1 names for its tools and
for Chromium on Ubuntu 24.04, listed in the job's `BROWSER_PACKAGES`
environment. Four steps handle them:

| Step | What it does |
| --- | --- |
| `Browser runtime packages identity` | Builds the cache key from the runner image (`ImageOS` and `ImageVersion`), the Playwright version and the package list, and decides whether this run saves |
| `Restore browser runtime packages` | Restores the package files saved under that exact key into `$RUNNER_TEMP/browser-packages` |
| `Browser runtime packages` | Downloads the packages on a miss, installs the files with `dpkg --install`, then runs Playwright's `install-deps --dry-run chromium` check |
| `Save browser runtime packages` | Saves the package files on a default-branch push that missed |

On a hit, the step installs the restored files and contacts no archive: it runs
no `apt-get update` and no download. On a miss, it downloads them. Apt
resolves the list against the packages the runner image already holds and
downloads the rest, every transfer bounded:

```text
apt-get -o Acquire::http::Timeout=15 -o Acquire::https::Timeout=15 -o Acquire::Retries=3 update
apt-get -o Acquire::http::Timeout=15 -o Acquire::https::Timeout=15 -o Acquire::Retries=3 \
  -o Dir::Cache::Archives=<staging>/ install -y --no-install-recommends --download-only <packages>
```

The staging directory is a fresh `browser-packages.XXXXXX` directory in `/tmp`
that anyone may enter, because apt downloads as its own unprivileged user. The
step moves the downloaded `.deb` files into `$RUNNER_TEMP/browser-packages` and
installs them from there, so a miss and a hit run the same install. The staging
directory stays on the machine after the run with only apt's lock and its empty
`partial` directory in it. A stalled connection fails over in seconds, and the
step's `timeout-minutes: 5` still fails a mirror that streams too slowly to trip
those timeouts.

Playwright's dry run simulates the install of every package it names and fails
the step when one is missing, so a Playwright upgrade that names a new package
fails here until the list follows it. It reads only the local package state and
downloads nothing.

Only an exact key is a hit: files saved on another runner image could replace
the packages the image already holds with older ones. A new image version, a
Playwright upgrade or a list change starts a new key, and the first
default-branch push after it saves the new entry. Merge groups and later
default-branch runs read that entry. Each image version has a prefix of its
own. The pruning job that follows the Chromium job keeps the newest entry of
each prefix, so a Playwright upgrade or a list change replaces the image's
entry; see [One entry per prefix](#one-entry-per-prefix). A staged rollout runs
two images at once, and each keeps its own entry. GitHub evicts a retired
image's entry after 7 days unread. A failed restore or save never fails the
job; after a failed restore, the step downloads.

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
      actions: write
    uses: Orchestration-Maestro/maestro-rust-workflows/.github/workflows/ci.yml@RELEASE_COMMIT
    with:
      browser-only: true
      checkout-ref: ${{ inputs.ref }}
      concurrency-key: browser-proof
```

This entry has no checkout, test commands or secrets of its own. The call grants
Actions write access only so the shared workflow can start; the pruning jobs
that use it skip on a manual dispatch, and the Chromium job keeps read-only
contents and Actions access. Dispatch only caller refs you intend to execute;
those refs supply the recipe and test code. Keep the workflow selection on
`main` (or the caller's default branch) while the `ref` input selects the source
under test. A dispatch does not cancel an in-progress proof. All dispatches
share one concurrency group, so a newer dispatch replaces any queued proof.

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
Python, Node >=20, curl, unzip, tar, sudo for the system packages and the
caller-pinned just on PATH:

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
def step_body(name):
    step = source.split('      - name: ' + name + '\n', 1)[1]
    pattern = re.compile(r'        run: \|\n((?:          .*\n|\n)+)')
    return textwrap.dedent(pattern.search(step)[1])
# The tools and then the install of the packages they need: the two production steps in order.
commands = [step_body(name) for name in ['Browser runtime tools', 'Browser runtime packages']]
# The packages step reads its list from the Chromium job's environment.
packages = re.search(r'\n      BROWSER_PACKAGES: >-\n((?:        .*\n)+)', source)[1].split()
with tempfile.TemporaryDirectory() as temporary:
    paths, values = pathlib.Path(temporary) / 'paths', pathlib.Path(temporary) / 'env'
    env = os.environ | {'RUNNER_TEMP': temporary, 'GITHUB_PATH': str(paths), 'GITHUB_ENV': str(values),
                        'BROWSER_PACKAGES': ' '.join(packages)}
    for command in commands:
        subprocess.run(['bash', '-e', '-o', 'pipefail', '-c', command], env=env, check=True)
        # Each step exports its own environment for the steps after it.
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

The caller's rustup toolchain installs each requested target and Clippy. The
check prints the selected packages, then runs, for each one:

```bash
cargo clippy -p "$PACKAGE" --all-targets --locked --target "$TARGET" -- -D warnings
```

Merge groups and main pushes select every workspace crate, so the check lints
them one after another. Run alone, a package builds with only the dependency
features its own manifest asks for, so a test target missing a feature it needs
fails here, as it does in its own tests.

| Target | Runner |
| --- | --- |
| `aarch64-unknown-linux-gnu` | `ubuntu-24.04-arm` |
| `aarch64-apple-darwin` | `macos-latest` |
| `x86_64-apple-darwin` | `macos-latest` (requested target installed) |
| `x86_64-pc-windows-msvc` | `windows-latest` |

There is no Linux fallback for Apple/MSVC targets. Runtime tests on a claimed
macOS arm64 host use the separate [test shard route](#test-shards).
Merge groups and main pushes run opted-in target checks, except for
`aarch64-apple-darwin`: the main push's native macOS test archive compiles that
target after landing. Merge groups compile macOS through the separate
`x86_64-apple-darwin` check. Warm calls keep both Apple checks because they do
not build native test archives. The four native macOS test shards and macOS
doctests run on main pushes, not in merge groups.

Ordinary pull requests run only the declared `x86_64-pc-windows-msvc` target
check. Apple checks wait for the full run; no macOS compile-only job starts on
a pull request. Apple dependencies with C build scripts need a cross toolchain
and an Apple SDK to compile on Linux; this workflow does not install one.
The PR Windows check uses the same selected packages as host lint instead of
`--workspace`; an empty selection keeps the check result without compiling.
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

## Runners

Every job runs on a GitHub-hosted runner: x64 Linux jobs state
`runs-on: ubuntu-latest`, the target matrix names a hosted image per target
(`ubuntu-24.04-arm`, `macos-latest`, `windows-latest`), and the CodeQL scan
states `runs-on: ubuntu-latest` too. The declaration job reads the caller's
settings, test selection and coverage scope.

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

What the report does depends on the job that failed:

| Failed job | Report |
| --- | --- |
| Test shard, doctests or Chromium tests | The failure issue and the revert PR described above |
| Any other job, such as the dependency, source and coverage checks | One issue per landing commit naming the job, its failed step, the commit and the run; no revert, because a check job does not run the landed change's tests |
| None identifiable, with the run not passed | The report fails with the run link |

A passing suite creates neither artifact. Unknown or ambiguous PR attribution,
unreadable logs, missing permissions and revert conflicts fail with the original
run link. A conflict can leave the failure issue without a revert PR: resolve
the conflict in a normal reviewed PR, not by force-pushing main. A check-job
issue is reused on repeated handling, as the failure issue is.

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
    workflows: [Rust CI, Full suite after landing]
    types: [completed]
jobs:
  guards:
    if: >-
      github.event.workflow_run.event == 'pull_request' ||
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

List every workflow whose runs hold the shared jobs. On Maestro those are
`Rust CI`, the shared workflow itself, for pull requests and merge groups, and
`Full suite after landing`, its caller for main pushes. The macOS test shards
run only in the second, so a listener that leaves it out never judges them. The
listener passes pull-request runs, the only runs the selection guard judges. It
passes default-branch pushes as well as merge groups, because those runs save
the compile snapshots every other run restores, and it passes a cancelled one
too: a cancelled seeding run is a finding, not silence. A default-branch push
that calls the shared suite through one reusable job is judged too. GitHub
titles its jobs `<calling job> / <shared job>`, and every guard matches a job
against a pinned name after the last ` / `, so `suite / Caller declaration`
counts as `Caller declaration`. The reuse, required-checks, time-per-unit,
shard-balance and slow-test guards read those names. When the
`Reuse merge queue tests` step in `Caller declaration` succeeded, the skipped
Linux test shards and Linux doctests leave the pinned list, exactly as they do
in a run with no prefix. Native test jobs still run.

Pin the reference to a reviewed commit and list the listener workflow in
`privileged_trigger_workflows`, as for the post-landing reporter. Issues are
filed with the automatic job token, and a job that cannot read the run fails
instead of passing.

Every finding has a kind, and most have a subject. The guards keep one open
issue per kind: the kind is its title and each subject is one section in it.
One slow build behind four slow tests, or one cold snapshot behind five cache
shapes, opens one issue that lists them. The run is evidence in the body, never
part of the title:

| Guard | Issue title, the kind | Subject |
| --- | --- | --- |
| Time per unit | `CI run slower than its baseline` | `Test jobs on <band>`, or `Build jobs on <band>` for the compile units |
| Shard balance | `Test shards unbalanced` | the blamed shard job |
| Slow test | `Test over 30 seconds` | `<binary> <test>` |
| Cache floor | `Compile cache below its hit-rate floor` | the cache shape |
| Required checks | `Required checks missing` | the missing checks |
| Snapshot seed | `Escalation: default-branch run did not seed its compile snapshots` | none |
| Cache usage | `Repository cache above 90% of its limit` | none |
| Selection | `Test selection share above its range` | none |
| Escalation | `Escalation: <what departed from the pattern>` | none |

The issue body is the finding's current state. Each section holds the subject,
`Breaching` or `Passed`, the run that first breached it, and the latest
evidence, which opens with the run that last judged the subject. A kind without
subjects has one section and no subject heading.

The guard reads the open issues once per run, then handles each kind the run
judged:

- With no open issue, a breach opens one that lists every subject that breached
  in the run.
- A run records in hidden comments, the record, the sections a later run cannot
  rebuild from the body: the subject the write adds, which the body does not list
  yet, and a subject whose state changed since the guard recorded it. It writes
  the record before it writes the body, in as many comments as GitHub's
  65,536-character comment limit needs, and the guard rebuilds the body from all
  of them. GitHub has no write that refuses a body built from an older read, so two
  runs can write one body at the same time, and the run that read it first writes
  back what it read, dropping a subject the other run had just added. The record
  keeps that subject: the next run lists it again. A subject the body already lists
  needs no entry until its state changes. Only a comment the guard wrote itself is
  a record, so a marker in anyone else's comment changes nothing.
- Two runs can finish together and judge a kind before either has opened its
  issue, so both open one. The lower number holds the kind: the guard writes
  that run's subjects onto it and comments a pointer on the duplicate it just
  opened. The duplicate keeps its own copy of those findings and stays open
  until a later run folds its subjects into the record and closes it.
- With an open issue, the guard rewrites its body in place. A breach adds its
  subject, or replaces that subject's evidence and keeps the run that first
  breached it. A pass updates only a subject the issue already lists.
- A comment marks a change only: subjects listed for the first time
  (`New subjects: ...`), or the run after which no listed subject is breaching
  (`passed this guard: every subject listed here has passed`). A repeat breach,
  a pass while another subject still breaches, and a breach of a subject that
  had passed update the body and announce nothing. A record is a comment too, and
  a run records only a subject the body does not list yet or a subject whose state
  changed: a repeat breach leaves the issue's comment count where it was, and so
  does the first repeat on an issue written before the guard kept a record.

A body stays within GitHub's 65,536-character limit. When a rewrite would pass
it, the guard drops subjects that passed, the earliest listed first. The record
keeps the state it holds for a subject, so a recorded subject the body drops comes
back as passed when the body has room again, and one that breaches again is listed
again with the run that first breached it. A subject the record never held is not
rebuilt once the body drops it, so only a recorded pass survives the limit. The
guard never drops a breaching subject, so when breaching subjects alone would pass
the limit, the run fails and the issue stays as it was.

The guard never closes a kind's issue. Close it once the fix is confirmed; the
next breach of that kind opens a new one. Its only close is a duplicate, and only
once the record holds that duplicate's subjects; until then the duplicate points
at that issue. A pass with no open issue, or for a subject the issue does not
list, changes nothing. A body the guard did not write still stops the run before
any write: an open issue with a kind's title and no subject section is refused,
untouched.

A subject is as wide as the cause: one cache shape is one subject, however many
jobs judge it, so sixteen test shards on one cold snapshot are one section with
sixteen job lines. A rate is one subject per kind of work and runner band, a
shard balance one per blamed shard, and a slow test one per test.

Source: on 2026-11-06 and 2026-11-07, while each issue carried one subject, one
slow browser build in Maestro's tooling tests opened four slow-test issues and a
second shard issue, one cold compile snapshot opened five cache-floor issues,
and the open issues gained a comment on nearly every merge group, up to 26 each.
The self-test replays those verdicts run by run
(`fixtures/guard-fan-out/verdicts.json`) and ends with one issue per kind and 13
comments in all.

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
regression: the guard lists `Test jobs on <band>` at once in the `CI run slower
than its baseline` issue, or `Build jobs on <band>` when the compile units are
what regressed. Each kind of work keeps its own section, because each has its
own limit. The bound is the measurement's own error (supervisor ruling S7759):
GitHub stamps job and step times to the second, 1 s at each boundary, so a
work-time difference is measurable only above 2 s per executed job. A difference
within it is neither a regression nor an escalation, for a rate below its spread
as well; anything above it at +10% fires, and anything above it outside the
band's spread escalates. The issue names the run and the band, gives the
regressed rate against its baseline and links the latest baseline runs. Fix it
at its cause; nothing accepts it.

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
15, so about 310 and 340. While a cache-floor issue is open, the guard also
reads the log of each job that ran a floor step, about fifteen more calls, to
name the shape that recovered; with no floor issue open it reads none. Keeping
the issues costs two calls for each open issue whose kind the run judged (the
body is read, then rewritten) and one more for each comment. To respond, open
the run link and fix the cause of the kind that regressed.

### Compile-cache hit-rate floor

Every job that keeps a compile snapshot warns when its compile-cache hit rate is
under 90%: Build test archive, Prepare test setup, the test shards, Dependency,
source and coverage checks, and each arm64 macOS target check (the Windows target
checks skip the statistics step, so they are never judged). Format and lint,
Docs, Doctests, the Wasm and Chromium browser builds and the Linux target checks
keep no snapshot, so they report their compile count and no hit rate. A
default-branch push, including one that reuses merge-queue tests, seeds every
judged shape on every host (`tests-coverage`, `tests-plain`,
`checks`, `targets-clippy-<target>` and `test-prepare`), so each is judged on
merge groups, pull requests and default-branch runs alike; only a run in which no
cacheable compilation ran is not judged. It never fails the build: a change can invalidate the cache itself,
such as a toolchain bump, and only a push to the default branch saves a
snapshot again, so a failing floor would block the merge group on every retry.
The floor is fixed in `ci.yml`. Source: the archive build of merge-group runs
40990557025, 40990555729, 40968689273 and 40946549381 hit 100.00%, 100.00%,
100.00% and 99.67%. The warning is an
annotation that names the shape and one cause. The guards read it from a
merge-group or main-push run, and from a pull-request run when a caller passes
one, and list each shape that fell cold in the `Compile cache below its hit-rate
floor` issue, with every job that judged that shape. The shape is the subject
because one snapshot serves every job that compiles it: the sixteen test shards
of a cold `tests-coverage` snapshot are one cause. A repeated cold build updates
its shape's section, and a shape that turns cold for the first time adds a
section and one comment. A run whose floor step stayed over the limit names its
shape on its own log line, so a job that judged its cache and passed marks that
shape passed. The causes a warning names, with their fixes:

- **Cold snapshot:** a snapshot was restored but lacks this build's objects.
  Run the warm build on the default branch to seed it again.
- **Key change:** no snapshot matches the job shape, though others of the same
  job on the same operating system exist. The compiler, runner or caller
  directory changed; the next warm build on the default branch seeds the new
  key. Every key names its host's operating system, so the Linux snapshot of a
  job never counts as a key change for its macOS leg.
- **No writer:** the repository cache holds snapshots of other jobs, or of the
  same job on another host, but none of this shape, so no default-branch push
  saves it. Check that the caller's warm-up
  runs a release of this workflow that has the writer.
- **Eviction:** the repository cache holds no compile snapshot at all. Check the
  cache size and the seven-day expiry, then run the warm build.

### Compile snapshots and cache usage

A push to the default branch is the only run that saves compile snapshots, so
the guards judge those runs themselves, cancelled ones included, and the
repository cache they fill.

- "Escalation: default-branch run did not seed its compile snapshots" is filed
  when the run's jobs reported more saved snapshots than the repository cache
  holds for the run or a newer one (the save step never fails the build, so a
  lost save shows only here; a newer run's snapshot of the same job replaces
  this run's, and its pruner deletes the older one), when jobs measured their
  compile cache and none saved, or when the run was cancelled with jobs
  unfinished. The cache list is read for the run's default branch and matched by
  the run id in the snapshot key. Source: on 2026-10-26 between 16:13 and 21:00
  UTC other caches filled the 10 GiB limit and every default-branch snapshot was
  evicted, and eight merge groups (40369902252 to 40395312817) compiled the test
  shape cold at 0% to 28.8%, their test shards taking a median 17.4 min against
  13.2 min afterwards.
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
  before that. The usage is read for every judged run. The save window does not
  count: a push leaves the snapshots it replaces on the default branch until its
  pruning jobs finish, so the guard sets those aside (the older entries of each
  compile, Cargo git, test setup and browser packages prefix on that branch) and
  judges what stays,
  the at-rest total in [One entry per prefix](#one-entry-per-prefix). An entry
  of another ref, or under another name, counts in full. The issue states the
  reading, the part that is window and the part that stays.
  Source: Maestro held 10,928,201,953 bytes (10.18 GiB) in 22 entries on
  2026-10-30 against its 10 GiB limit, and 10,484,269,311 bytes (9.76 GiB,
  97.6%) on 2026-11-08, when one snapshot for each of twelve shapes needed
  11.66 GiB and GitHub evicted the newest Linux snapshots. The guide and the
  finding use GiB. To respond, delete the pull-request entries the issue lists,
  or shrink what the snapshots hold: a shape that restores nothing later needs no
  snapshot ([Jobs that keep no snapshot](#jobs-that-keep-no-snapshot)).

Both are kinds without subjects: a repeat rewrites the open issue in place.

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
the median shard on the same host, the guard lists that shard in the
`Test shards unbalanced` issue, and every shard inside the margin of that same
run is marked passed there. A second shard over the margin is an imbalance of
its own, so it is never marked passed. Its section reads the blamed shard's
own log, so only tests that ran on that shard appear: the ones that are new, or
reached a higher slow mark since
the last green run of the same event. Nextest prints only slow marks, so growth
shows as a higher mark rather than exact seconds.

Beside the tests, the section lists the fixed phases the same log times: the
extraction of the instrumented targets, each test setup, and the tests' own
seconds from the nextest summary. A shard past the margin is often slow from
that fixed work before its first test runs, so read those lines before blaming a
test.

The default is 1.5. Source: the slowest shard was 1.18 and 1.20 times the median
in runs 40990557025 and 40990555729, and 1.32 times in the 32-shard proof run
40974924871. To respond, fix or split the named tests, then check priorities in
the nextest configuration; when the fixed phases hold most of the shard's time,
the cause is the shard's composition or its setups.

Test and archive-build rate baselines and ratchet levels are also separate
for Linux, macOS and Windows.

### Test-time limit

Nextest flags a test slow after `slow-test-seconds` (30), and the caller's
`slow-timeout` period should match. A merge-group or main run lists every test
it flagged, `<binary> <test>`, in the `Test over 30 seconds` issue, with no cap
on how many; a listed test is never listed twice, and the next run that flags it
updates its numbers there. A run that reports the test again without a slow mark
marks it passed, and the issue gets its recovery comment once no listed test is
still slow. Fix the test at its cause; do not raise a timeout.

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
and coverage checks`. A pinned job that is missing or skipped is listed in the
`Required checks missing` issue, with the missing set as its subject, such as
`Docs, Doctests`: `Test shard (*` stands for any shard, and `Dependency, source
and coverage checks` is one check despite its comma. The title never carries
the run id, so a set that stays missing across landings keeps one section,
naming the latest
judged run. A failed job is not a missing one; the run already reports it. The
pinned `Test shard (*` prefix includes native shard names, and
`Tests (ubuntu-latest)` requires every native shard and doctest leg to succeed;
a failure, cancellation or unexpected skip fails the aggregate. The
self-test checks every pinned name against `ci.yml`, and checks that this
repository's ruleset checks, `Required repository quality` and `Required
consumer tests`, still exist, so renaming a job fails there first. When you
rename a job on purpose, change the pin and the ruleset in one change.

On a main push that reuses merge-queue tests, the successful `Reuse merge queue
tests` step in `Caller declaration` records the decision. The required-checks
guard permits Linux shard and doctest skips only with that marker. The archive and
every other pinned check must still run. Job-shape comparisons keep reused runs
separate from full-suite runs, so the deliberate skips do not report removals.

Pull requests require `Caller declaration`, `Format and lint`, `Docs`,
`Dependency, source and coverage checks` and the declared Windows target result,
`Check (x86_64-pc-windows-msvc)`. `Check (x86_64-apple-darwin)` runs on merge
groups and main pushes. The native test archive covers arm64 Apple compilation
on main pushes only. A failed target check fails the workflow.
Undeclared targets do not create required results. Pull requests keep the test
and build job names but skip their work, except that `Wasm and browser build`
compiles the declared [example witnesses](#wasm-example-witnesses) and reports
its required name. The
required-checks guard enforces execution only on merge groups and main pushes,
so intentional PR skips do not file missing-check findings.

### Escalations

Anything that stands out of the pattern is escalated explicitly: the guard files
an issue with the label `escalation`, or rewrites the open one with the same
title, naming the run, its baseline runs and what departed from them.
Nothing outside the pattern is absorbed silently. The normal spread of a rate is
the range of the band's earlier rates widened by 10% of their median; for a hit
rate or a share it is the range of the earlier values widened by 10 points.
Three earlier values are needed for a spread. Each departure is a kind of its
own with a standing title, so one that repeats keeps one issue, rewritten in
place:

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


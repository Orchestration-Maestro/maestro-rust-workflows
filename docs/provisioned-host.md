# Provisioned-host ownership

This is an ownership transfer, not an exclusion or manufactured LLVM hit.
No consumer enables it yet. The host executor and Ubuntu job are a separate
slice. Until they exist, configured ownership fails Required Rust CI with
`provisioned-host mutation result is missing; no host executor ran`.

## Tested-head policy

The optional `[ci.mutation-provisioned-host]` table in `maestro-quality.toml`
is always read from the tested checkout, including called workflows. Its bytes,
owned sources and provisioner must match tracked HEAD blobs. There is no caller
input to override ownership. The table takes exactly these fields:

| Field | Value |
| --- | --- |
| `files` | Nonempty unique exact tracked Rust paths inside the project |
| `features` | Nonempty unique declared workspace `package/feature` strings |
| `provisioner` | One exact tracked regular script path, never command text |

Paths with symlink components, escapes, globs or control characters refuse.
Host files cannot disable mutation or coverage. Windows and engine owners
cannot overlap. Mutation testing must stay enabled. A removed entry transfers
the still-existing file back to full ordinary or new-owner mutation discovery,
even if the source did not change. A deleted source has no remaining obligation.

Host discovery always lists full files with pinned cargo-mutants 27.1.0,
`--list --json --no-shuffle --no-config --cargo-arg=--locked`, selected owning
packages and qualified features. It never inherits exclusions, changed-line
filters or prior iteration evidence. Ordinary mutation excludes only the exact
transferred host files. The early planner runs independently of coverage.

## Schema 1 plans and pending coverage

`mutation-host-plan.json` contains `schema`, `identity`, `mutants`, `files`,
`features`, `packages`, `source_sha256`, `policy_sha256`, `provisioner`,
`provisioner_sha256` and `workflow_revision`. `identity` is the existing
mutation manifest: SHA, first parent, directory, compiler, cargo-mutants version,
run ID, attempt, configuration/diff digests and ordinary counts. `mutants`
retains the complete listing, including patches and enclosing-function spans.
`source_sha256` maps each owned path to the tested bytes' SHA-256. Every owned
file requires nonempty discovery. Raw discovery and diagnostics remain in
`mutation-host-list.json` and `mutation-host-plan.log`.

`changed-coverage-pending.json` contains `schema`, `state: pending-host`, `sha`,
`run_id`, `attempt`, `plan_sha256`, `policy_sha256`, `target`, `coverable`,
`ordinary`, `host`, `ordinary_uncovered`, `ordinary_allowed` and `full_allowed`.
Each line record is `{file, line, hits}` with real LCOV counters. The full set
is the disjoint union of ordinary and exact host identities. The ordinary
subset independently keeps the existing title target, rounding and minimum-one
allowance. Host lines cannot increase that allowance. Every owned file must
have LCOV line records. Pushes have empty changed identities but still require
complete host mutation evidence. Raw global LLVM coverage stays unchanged.

## Schema 1 outcomes

The executor produces `host-outcomes.json`, never cargo-mutants' native outcomes.
Its fields are `schema`, `sha`, `run_id`, `attempt`, `plan_sha256`,
`policy_sha256`, `provisioner_sha256`, `source_sha256`, `baseline_before`,
`baseline_after` and `outcomes`. Both baselines and every mutant receipt retain:

| Field | Meaning |
| --- | --- |
| `build`, `provision`, `test`, `cleanup` | Separate `passed`, `failed` or `not-run` phase statuses |
| `selected_tests`, `passed`, `failed`, `ignored` | Named positive test selection and exact integral counts |
| `phases` | `normal`, `abandon`, `preparing`, `recover_abandon`, `recover_preparing` assertion statuses |
| `test_sha256`, `bootstrap_sha256` | Current built test and bootstrap byte digests |
| `logs` | Nonempty map of artifact-relative raw phase log paths to SHA-256 digests |
| `test_failure` | Nonempty unique array of exact selected test or failed phase names |

Each `outcomes` entry contains the complete planned `mutant` identity,
`outcome`, `patched_source_sha256` and `receipt`. Exact planned and executed
sets must match without duplicates. Every outcome must be `caught`. Both
baselines require successful tests and every phase passed. Mutants run every
phase, with only passed or failed statuses. Every failed phase must be named in
`test_failure`; a phase-only failure or multiple named phase failures can prove
a kill after successful build, provision and cleanup. A Rust test failure must
name a selected test with a positive failed count. Ignored or zero tests,
unviable, timeout, partial, missing, stale and mismatched evidence refuse.
Raw logs must be nonempty regular files inside a checked safe artifact tree.

## Final join

Aggregation runs for host-only, ordinary-inline, ordinary-empty and sharded
plans, with or without Windows and engine owners. It rechecks the plan digest,
SHA/run/attempt, exact outcome population, phase receipts and raw log digests.
Every host changed line must have an enclosing function in that complete caught
population. Unmapped lines or functions refuse; one unrelated caught mutant
does not credit an entire file.

The final `changed-coverage-final.json` retains the pending document and changes
its state to `passed`, with `host_verified`. `changed-coverage.txt` reports the
unchanged full denominator, ordinary misses/allowance, host-verified count, raw
host hits and exact mutant totals. `HOST_BEHAVIOUR_VERIFIED` is distinct from
`LLVM_EXECUTED`; neither LCOV nor Codecov gains invented hits.

The scorecard keeps pending changed coverage and mutation `not-run`. Only the
same-attempt successful host job, aggregation and coverage join can satisfy
Required Rust CI. R4 adds the executor job and its dependency edge together;
there is no placeholder success or refusing job in this slice. The executor
must run every mandatory phase for each scenario in fresh units. Existing job
budgets, thresholds and artifact retention are unchanged. Pi has no equivalent
coverage-owner or provisioning policy.

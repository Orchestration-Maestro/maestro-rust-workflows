# Shared Rust CI

This repository maintains the organization's shared Rust CI workflows.

## Language

**Shared CI**: The organization's common Rust checks, maintained here and used by caller repositories.

**Caller**: A repository using the shared CI, including a fixture repository used for its self-test.

_Avoid_: Consumer, when it means a repository using this CI.

**Caller declaration**: The caller's optional declaration of supported checks and permitted extensions to the shared defaults.

_Avoid_: Settings file, when it means the caller declaration.

**Release pin**: A fixed release selected for a workflow or check tool. Releasing a new workflow version and changing callers' workflow pins are separate actions.

_Avoid_: Version pin; consumer pin, when it means a caller's workflow release selection.

**Required check**: A CI result that must succeed for a change to merge.

**Fixture**: A small example package or test input used to exercise a shared-CI outcome.

**Self-test**: This repository's validation of the shared workflow using passing and deliberately broken fixtures.

**Proof**: A validation run demonstrating a workflow outcome on a pinned workspace or a host-specific fixture.

**Isolated tests**: Caller tests run with a restricted child environment and fresh home and temporary directories.

_Avoid_: Test isolation, when it names this execution mode.

**Test shard**: One numbered partition of the caller's unit and integration tests on one test runner; doctests have a separate execution route.

_Avoid_: Partition, when it names a numbered CI test shard.

**Coverage floor**: The minimum line coverage required by the shared CI for runs that enforce coverage.

_Avoid_: Coverage threshold, when it means this minimum.

**Compiled-cache snapshot**: A saved collection of compiler objects used to seed a compatible compilation job. It is distinct from a source checkout or Cargo's build directory.

_Avoid_: Snapshot, when it means a compiler-cache snapshot rather than a source snapshot.

**Job shape**: The compilation characteristics used to decide whether jobs can share a compiled-cache snapshot.

**Warm build**: An optional compilation that seeds the coverage cache. It runs no tests unless the caller declares a warm test filter.

_Avoid_: Warmer; warm-up, when they mean this cache-seeding build.

**Foreign-target check**: A compile-and-lint check for a requested native target on its supported runner, not a runtime test suite.

**Browser build**: The caller-owned build recipe for its browser application, separate from browser runtime tests.

**Chromium browser tests**: The opted-in caller browser tests run with the shared workflow's verified browser runtime.

**Advisory scan**: The scheduled check of the caller's locked dependencies for advisories and yanked crates.

**Nightly fuzz job**: The scheduled work that exercises declared fuzz targets and reports discovered findings.

**Finding**: A reported dependency problem or a fuzz-discovered crash, timeout, memory failure or leak. A fuzz tool or build failure is a failed scan, not a discovered finding.

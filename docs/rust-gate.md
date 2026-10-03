# The gate: one binary instead of step bodies

`rust-gate` is a standard-library-only Rust binary in `gate/`. The workflows
build it from this repository at their own commit, `job.workflow_sha`, through the
`.github/actions/gate` action, put it on the PATH, and every step then runs
one subcommand: `rust-gate validate`, `rust-gate stage`, and so on. Inputs
reach it exactly as the step bodies received them, through `env:`.

## Why

A reusable workflow runs in the consumer's checkout and cannot read a script
from this repository, so every step body was Bash inline in YAML: 510 lines
in `ci.yml` alone, tested only through a harness that stubs `curl`, `tar` and
`sha256sum`. A binary built from the workflow's own commit has the same trust model as
that Bash (the commit SHA that pins the workflow pins it), is typed, has unit tests for its own
helpers, is exercised end to end by the same contract tests that held the
Bash, and leaves each workflow step one line long.

## Invariants

1. Standard library only. `cargo build --offline` must work: the gate is built
   before job-local Cargo state is configured, and a dependency would be a second
   supply chain to review. JSON is delegated to the pinned `jaq`, and digests
   to `sha256sum` in the steps that run on Linux alone: `install-tools`,
   `verify-payload` and `hardening`. The gate computes the others itself:
   mise's, in `rust-gate setup` and the `hooks` step, verified before any tool
   that could verify it is installed, and the artifact name's in `validate`
   and the payload's checksums in `stage`, which `rust-gate ci --local` runs on
   macOS and Windows too, where `sha256sum` does not exist.
2. Behaviour ported verbatim: same inputs, same reports, same messages, same
   refusals. The contract tests execute step bodies as black boxes, so a
   ported step passes the same tests as the Bash it replaces; that is the
   safety net of this migration. Four differences were taken on purpose,
   each stricter or cosmetic: refusal messages are plain lines on stderr,
   where the two former actions prefixed theirs with `::error::`;
   `verify-payload` names its `REQUIRE_BINARIES` variable rather than the
   former input; the unsafe audit counts whole words, so two `unsafe` on one
   line count twice where grep counted one; the platform check reads the
   build target rather than `uname`.
3. One subcommand per step. A `ci.yml` step body is `rust-gate <id>`; a step
   of any other workflow is `rust-gate <workflow> <step>`, and nothing else.
4. The compiler pin is the `rust-toolchain.toml` at the repository root, the
   one file rustup reads for the gate, the tests and the action's build;
   `the_local_toolchain_pin_has_one_copy` keeps it the only copy.
5. A step is data before it is code: its `STEPS` declaration names the
   workflow, the id, the summary, the inputs it reads, the tools it runs and
   the reports it writes, next to the function that does it. The runner
   refuses anything undeclared while the step runs, `rust-gate describe`
   renders the declarations as `docs/steps.md`, and tests keep that document
   current, every declared name used, every used name declared, and every
   workflow body pointing at a registered step.

## Layout

Three layers under `gate/src/`, each entered through its own `mod.rs`, with
imports flowing one way only:

- `gate/src/runner/` is the runner as the gate sees it: `outcome.rs` (how a
  step ends: complete, or failed with the status of the tool that failed or
  with one message), `github_actions.rs` (inputs, the `GITHUB_*` files, the
  step summary, masking, the job's three directories), `commands.rs` (`Cmd`, tools run with their own exit status,
  the trace) and `step_declaration.rs` (a step as data, and the refusal of
  anything undeclared). Standard library only, nothing above it.
- `gate/src/checks/` is what the steps share: `checkout_paths.rs` (canonical
  forms, containment in the checkout, the project directory, symlinks, the
  Rust sources of a tree), `simple_names.rs` (one validator for every
  simple-name rule, hex strings), `rust_versions.rs` (version strings, the
  channel line of a toolchain file), `release_boundary.rs` (what both
  publishers ask of a release), `private_directories.rs` (private temporary
  directories), `cargo_metadata.rs` (the jaq programs several steps read over
  Cargo's records) and `inputs.rs` (the `ci.yml` inputs with a shape of their
  own: the three policies, the coverage threshold and the artifact key, each
  read and refused in one place and typed for the steps). Built on the
  runner, never on a step. The door offers the modules, not their names, so
  an import says which kind of rule it reaches for:
  `use crate::checks::checkout_paths::{canonical, inside};`.
- `gate/src/steps/` is one module per step, private to the directory and
  named after what the step does. A step that keeps a seam of its own becomes
  a directory: `quality_scorecard/step.rs` is the step and
  `quality_scorecard/scorecard.rs` is its internal seam, reaching no further
  than its parent. Its `STEPS` declaration names the step:
  `validate_inputs.rs` holds `rust-gate validate`, `format_lint_test.rs`
  `rust-gate quality`, `publish_crate.rs` every `rust-gate publish-crate
  <step>`, `install_tools.rs` and `verify_payload.rs` the two commands
  several workflows share. Each module exposes one declaration, `STEPS`;
  `steps/registry.rs` holds the registry and `run` and `describe`, the two
  doors `main.rs` uses.

### Mutation workflow steps

`rust-gate mutants-plan` writes the full filtered listing and immutable run
manifest in the early `mutation-plan` job for opted-in automatic or fixed sharding;
the serial default and
disabled mode write routing outputs without invoking cargo-mutants. The manifest
binds workers to the tested revision, first parent, project directory, compiler,
tool version, run attempt, config and diff digests. `rust-gate mutants` keeps
its existing inline path when `MUTATION_SHARD` is empty; a worker validates its
zero-based `K/N` and the downloaded early manifest before passing `--shard K/N
--sharding round-robin` to cargo-mutants. `rust-gate mutants-aggregate` treats
downloaded artifacts as data, validates receipts, assigned identities, raw
outcomes and counters, and writes the merged canonical reports only after every
planned worker is complete. Local CI skips planning and still runs the full
mutation command unsharded.

Featureless engine controls use a clean default test-target build to establish compile
membership from Cargo JSON and rustc dep-info. Non-members receive the explicit
`NotCompiledWithoutFeatures` outcome rather than redundant default test runs. Compiled
mutants still run and must be caught by default tests. Aggregation verifies the retained
source-bound membership and exact caught engine twins before accepting non-members;
raw tested outcomes remain separate. Compilation-changing cargo-mutants configuration
falls back to testing all assigned mutants. See [mutation testing](ci.md#mutation-testing).

Coverage, API comparison, feature checks and mutation share the policy-aware
child-command seam in `checks/native_cache.rs`. Every selected child revalidates
its root; the first inventory is retained for the job's final save decision.
Engine workers verify their plan before root verification and cargo-mutants
injection. Featureless, default and Windows mutation execution remove an
inherited configured variable; so do API and feature-check children whenever
policy is present, even on an unsupported platform. Without policy, commands and
environments stay unchanged. Pre-validation mutation metadata is unchanged. Every engine shard
restores; only shard zero inventories and saves after success. Native policy
opt-in on a selected Unix platform also selects no-wrapper coverage for both
passes. See the
[consumer native-cache contract](ci.md#optional-native-cache).

The compiler keeps the layers apart: a step is private to `gate/src/steps/`, so
neither the checks nor the runner can reach it. ARC-004, declared in
`maestro-quality.toml`, keeps the imports one way, and
`a_step_offers_only_its_declaration_and_reaches_no_sibling` the rest: no step
imports another step, and a step exposes nothing but `STEPS`.

The contract tests never read the gate's source. A test runs a step in a
fixture and reads three things: what the step wrote, what it declares
(`rust-gate describe`), and the trace of every command it ran. The trace is
the file `RUST_GATE_TRACE` names, one line per command, the environment the
gate set first, written just before each command starts. Environment values
are redacted by default; only the reviewed compiler, documentation, Miri and
build-directory settings remain visible. The child still receives the original
values. The fixture asks for a trace on every run; the hosted native cache
fixture also enables it to prove child environment isolation. Arguments are not a secret channel and must never carry credentials.
A proof that the upload never passes `--clobber`, or that the release tests
run before the auditable build, is read from that trace, never from a
function body.

## What the migration removed

The two composite actions that used to hold the shared Bash, the three copies
of the path guard (the test that compared them now checks that every validate
step goes through the one guard), and the harness code that ran action bodies.
ShellCheck still covers the one action body left, the build step of the gate
action, and the Just recipes, from the contract test
`every_bash_line_left_in_the_repository_passes_shellcheck`. What stayed is the
black-box contract: every test
runs `rust-gate <command>` against a fixture, exactly as it ran the Bash
before, and passes the same assertions.

## Native local boundaries

The binary compiles for Windows GNU without adding production dependencies or
unsafe Rust. Private directories use Unix mode 0700 or a fixed Windows PowerShell
adapter calling `CreateDirectoryW` with a protected owner-only inheritable DACL.
Creation is atomic and exclusive; existing directories and reparse destinations
are not reused, and the security descriptor is freed on every native return.
Paths are environment data, never interpolated into script source. The registry
step declares PowerShell only in a Windows build. Configuration files retain
exclusive creation and inherit the private directory grant on Windows.
This is security-critical native interop, not a claim that all runtime code is
memory-safe. No post-create ACL exposure or readonly substitute is used.

The native suite exercises real Windows units, registry boundaries, ACLs and
Cargo examples. Linux-only tool installation and nightly guards remain closed,
and Linux ELF hardening is not reclassified as PE verification. The complete
Linux workflow replay stays in `just check`. See [native Windows checks](../CONTRIBUTING.md#native-windows-checks)
for the exact separate scopes and the unverified native-execution boundary.

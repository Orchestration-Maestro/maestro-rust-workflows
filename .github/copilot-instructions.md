# Copilot instructions for maestro-rust-workflows

## Start here

A Rust repository of the organization adds no workflow: the organization's
ruleset runs `ci.yml` on every pull request, and the same run uploads Clippy and
secret-scan findings to the repository's Security tab and coverage and test
results to Codecov; see
[uploads](../docs/ci.md#uploads-to-code-scanning-and-codecov). Codecov needs its
GitHub App installed on the organization; the upload logs in through OIDC, so
there is no Codecov token to store. Every merge goes through the repository's
merge queue, and the run on the merge group's commit, the one the default branch
moves to, files both on the default branch: that is the baseline each pull
request compares against, with no push run and no per-repository workflow.

Paths below are relative to this repository. Before editing, read
[AGENTS.md](../AGENTS.md) for the rules that bind every change,
[CONTEXT.md](../CONTEXT.md) for the words it uses and
[CONTRIBUTING.md](../CONTRIBUTING.md) for how a change is proposed. The
organization's [golden
rules](https://github.com/Orchestration-Maestro/.github/blob/main/golden-rules/engineering.md)
come first: nothing in a specification, a plan or this repository weakens them.

For quality, engineering or security changes, read
[northstar.md](../docs/standards/northstar.md),
[engineering.md](../docs/standards/engineering.md) and
[security.md](../docs/standards/security.md): this repository's map of the
organization's golden rules.

Keep changes scoped to the request, and read historical plans and specifications
as records, not as instructions to start new work.

## Repository tree

Every tracked file, with what it is for. `rust-gate guide` writes this tree at
every commit and keeps each explanation already here, so improve an explanation
in place.

```text
.                                                                # Repository root
├── .config/                                                     # Tool settings that live in a directory
│   └── nextest.toml                                             # TST-004: the nextest profile, retries = 0, for this repository's own runs
├── .github/                                                     # GitHub metadata, templates and workflows
│   ├── ISSUE_TEMPLATE/                                          # Structured issue forms and the chooser
│   │   ├── bug_report.yml                                       # Bug form: runner, arch, regression, release impact
│   │   ├── config.yml                                           # Chooser links for security, support and contracts
│   │   └── feature_request.yml                                  # Feature form, including breaking-change intent
│   ├── actions/                                                 # The one action every workflow calls by commit SHA
│   │   └── gate/                                                # Builds rust-gate from this commit and puts it on the PATH
│   │       └── action.yml                                       # Cache per commit, then cargo build with the crate's own compiler
│   ├── assets/                                                  # Repository artwork referenced by the README
│   │   ├── how-it-works.svg                                     # Diagram in the README: what calls what, and what publication must pass
│   │   └── rust-workflows.webp                                  # README banner
│   ├── release-please/                                          # Release-please configuration and version manifest
│   │   ├── config.json                                          # Release strategy and changelog sections
│   │   └── manifest.json                                        # Current released version per package
│   ├── workflows/                                               # Callable workflows and this repository own CI
│   │   ├── attest-binaries.yml                                  # Isolated signing job; re-verifies before it signs
│   │   ├── ci-internal.yml                                      # Repository quality, the consumer matrix and both dry-run publishers on every pull request
│   │   ├── ci.yml                                               # The Rust CI a ruleset runs, uploads included, or a workflow calls
│   │   ├── dependabot-auto-merge.yml                            # Queues Dependabot patch and minor updates to merge on the organization's bot token
│   │   ├── docs-sync.yml                                        # On a pull request from this repository, the bot commits the tables just docs regenerated
│   │   ├── fuzz.yml                                             # Bounded fuzz regression on a nightly toolchain
│   │   ├── gate-mutation.yml                                    # Manual and weekly gate mutation shards, full-suite replay and complete evidence
│   │   ├── hygiene.yml                                          # The reusable CI of a repository without Rust: secrets, hygiene, managed files, hooks
│   │   ├── native-cache-fixture.yml                             # Native cache fixture
│   │   ├── publish-binaries.yml                                 # Protected binary release, dry-run by default
│   │   ├── publish-crate.yml                                    # Protected crate publication explicitly to public crates.io
│   │   ├── publish-evidence.yml                                 # Verifies release reports, dry-run first, then uploads GitHub Release assets
│   │   ├── release-please.yml                                   # Release pull request and tag on a GitHub App token, skipped until the app is set up
│   │   ├── scorecard.yml                                        # Weekly OpenSSF Scorecard of this repository, published for the badge and shown in code scanning
│   │   ├── tool-updates.yml                                     # Weekly pull request moving every pinned tool to its latest release, on the bot token
│   │   ├── toolbelt-platforms.yml                               # rust-gate setup from nothing and a consumer's hooks, on every platform the toolbelt is pinned for
│   │   └── unsafe-audit.yml                                     # Undefined-behaviour audit under Miri
│   ├── CODEOWNERS                                               # Required reviewers for every change
│   ├── PULL_REQUEST_TEMPLATE.md                                 # Review checklist and release-impact prompt
│   ├── actionlint.yml                                           # Uses the built-in GitHub-hosted runner labels
│   ├── copilot-instructions.md                                  # This file: the maintained-file map
│   ├── dependabot.yml                                           # Weekly action and Cargo updates, patch and minor grouped per ecosystem
│   └── zizmor.yml                                               # Workflow audit exceptions, each with its reason
├── docs/                                                        # Contracts, standards and platform boundaries
│   ├── generators/                                              # The jaq filters just docs renders the generated tables with
│   │   ├── gates.jq                                             # The README's three gate tables from gates.toml
│   │   ├── inputs.jq                                            # A workflow's inputs as a Markdown table
│   │   ├── outputs.jq                                           # A workflow's outputs as a Markdown table
│   │   ├── own-inputs.jq                                        # The inputs one publisher has and the other lacks
│   │   └── shared-inputs.jq                                     # The inputs both publishers share, the forwarded ones in one row
│   ├── standards/                                               # The bars this repository holds itself to
│   │   ├── controls.md                                          # The four axes and their proofs, consumer defaults, evidence, extended security controls
│   │   ├── engineering.md                                       # Rule map of the engineering rules, then this repository's stricter rules
│   │   ├── northstar.md                                         # The point and one KPI per pillar, written by rust-gate rules
│   │   └── security.md                                          # Rule map of the security rules, and what this repository protects
│   ├── superpowers/                                             # Designs written and approved before a change is built
│   │   ├── plans/                                               # One implementation plan per approved design, task by task
│   │   │   ├── 2026-09-24-guide-and-rule-map-1-rules.md         # Plan 1 of the rule map in rust-gate: rust-gate rules and rules --check
│   │   │   ├── 2026-09-24-guide-and-rule-map-2-guide.md         # Plan 2 of the rule map in rust-gate: rust-gate guide, as it ran, and its parity
│   │   │   ├── 2026-09-24-guide-and-rule-map-3-hooks.md         # Plan 3 of the rule map in rust-gate: the commit hooks, init and the skipped CI hooks
│   │   │   ├── 2026-09-24-guide-and-rule-map-5-own-rule-map.md  # Plan 5 of the rule map in rust-gate: this repository's own standards as its rule map
│   │   │   ├── 2026-09-24-org-quality-gate-1-architecture.md    # Plan 1 of 2: the module structure rules, and this repository held to them
│   │   │   └── 2026-09-24-org-quality-gate-2-complete.md        # Plan 2 of 2: every remaining rule, the generated files, the release and the repositories
│   │   └── specs/                                               # One approved design per change, named by date and topic
│   │       ├── 2026-09-24-guide-and-rule-map-design.md          # The Copilot guide and rule map, moved into rust-gate and written by a commit hook
│   │       └── 2026-09-24-org-quality-gate-design.md            # The quality gate every organization repository inherits, and how
│   ├── README.md                                                # Complete workflow contracts and usage examples
│   ├── ci.md                                                    # Every CI input, output, gate and report
│   ├── gates.toml                                               # Every gate the README lists: what fails it, its switch, its standard and proof
│   ├── platform-requirements.md                                 # Runners, registries, identity and their unknowns
│   ├── provisioned-host.md                                      # Provisioned-host ownership
│   ├── publishing.md                                            # Dry-run and protected publication procedures
│   ├── rust-gate.md                                             # The gate: why one binary holds the step bodies, its invariants and layout
│   └── steps.md                                                 # Generated by rust-gate describe: every step, what it reads, runs and writes
├── examples/                                                    # Real consumer shapes the gate runs against
│   ├── binary/                                                  # Single binary package fixture
│   │   ├── src/                                                 # Workspace library sources
│   │   │   ├── lib.rs                                           # Workspace library surface with doc comments
│   │   │   └── main.rs                                          # Binary entry point
│   │   ├── supply-chain/                                        # cargo-vet ledger: the six imports VET-001 requires, and the exemptions
│   │   │   ├── audits.toml                                      # The audits recorded here
│   │   │   ├── config.toml                                      # The imports and the reviewed exemptions
│   │   │   └── imports.lock                                     # The imported audits, pinned for cargo vet --locked
│   │   ├── tests/                                               # Workflow contract validation
│   │   │   └── cli.rs                                           # CLI integration test
│   │   ├── Cargo.lock                                           # Locked resolution for the test crate
│   │   ├── Cargo.toml                                           # Isolated workflow-contract test target
│   │   ├── LICENSE                                              # MIT notice included in the Cargo package
│   │   ├── README.md                                            # What the fixture is, for crates.io
│   │   └── rust-toolchain.toml                                  # Exact stable compiler pin for tests
│   ├── library/                                                 # Library-only package fixture
│   │   ├── src/                                                 # Workspace library sources
│   │   │   └── lib.rs                                           # Workspace library surface with doc comments
│   │   ├── supply-chain/                                        # cargo-vet ledger: the six imports VET-001 requires, and the exemptions
│   │   │   ├── audits.toml                                      # The audits recorded here
│   │   │   ├── config.toml                                      # The imports and the reviewed exemptions
│   │   │   └── imports.lock                                     # The imported audits, pinned for cargo vet --locked
│   │   ├── Cargo.lock                                           # Locked resolution for the test crate
│   │   ├── Cargo.toml                                           # Isolated workflow-contract test target
│   │   ├── LICENSE                                              # MIT notice included in the Cargo package
│   │   ├── README.md                                            # What the fixture is, for crates.io
│   │   └── rust-toolchain.toml                                  # Exact stable compiler pin for tests
│   └── workspace/                                               # Multi-member Cargo workspace fixture
│       ├── app/                                                 # Workspace binary package
│       │   ├── src/                                             # Workspace library sources
│       │   │   └── main.rs                                      # Binary entry point
│       │   ├── tests/                                           # Workflow contract validation
│       │   │   └── cli.rs                                       # CLI integration test
│       │   ├── Cargo.toml                                       # Isolated workflow-contract test target
│       │   └── LICENSE                                          # MIT notice included in the Cargo package
│       ├── core/                                                # Workspace library package
│       │   ├── src/                                             # Workspace library sources
│       │   │   ├── arithmetic.rs                                # Checked arithmetic exposed by the workspace consumer fixture
│       │   │   ├── lib.rs                                       # Workspace library surface with doc comments
│       │   │   └── windows.rs                                   # Small Windows-only behavior owned by the native mutation job
│       │   ├── Cargo.toml                                       # Isolated workflow-contract test target
│       │   ├── LICENSE                                          # MIT notice included in the Cargo package
│       │   └── README.md                                        # What the member is, for crates.io
│       ├── supply-chain/                                        # cargo-vet ledger: the six imports VET-001 requires, and the exemptions
│       │   ├── audits.toml                                      # The audits recorded here
│       │   ├── config.toml                                      # The imports and the reviewed exemptions
│       │   └── imports.lock                                     # The imported audits, pinned for cargo vet --locked
│       ├── Cargo.lock                                           # Locked resolution for the test crate
│       ├── Cargo.toml                                           # Isolated workflow-contract test target
│       └── rust-toolchain.toml                                  # Exact stable compiler pin for tests
├── gate/                                                        # The gate: one binary the workflows build at the pinned commit
│   ├── golden-rules/                                            # The golden rules this release carries, copied from .github
│   │   ├── commit.txt                                           # The .github commit the golden rules were copied from
│   │   ├── engineering.md                                       # Copy of the engineering rules; never edited here
│   │   ├── glossary.md                                          # Copy of the organization's glossary, whose _Never_ words HYG-007 refuses; never edited here
│   │   ├── northstar.md                                         # Copy of the Northstar; never edited here
│   │   └── security.md                                          # Copy of the security rules; never edited here
│   ├── src/                                                     # The three layers: runner, checks, steps
│   │   ├── checks/                                              # What the steps share, built on the runner and never on a step
│   │   │   ├── cargo_metadata.rs                                # The jaq programs several steps read over Cargo's records
│   │   │   ├── checkout_paths.rs                                # Canonical forms, containment in the checkout, symlinks, Rust sources
│   │   │   ├── coverage_features.rs                             # Qualified workspace coverage features validated through Cargo metadata
│   │   │   ├── digests.rs                                       # SHA-256, for mise's own download on every platform, where sha256sum is Linux's alone
│   │   │   ├── findings.rs                                      # A rule's finding as one report line, and the exceptions that excuse some
│   │   │   ├── gate_rules.rs                                    # The gate's one list of rules, and the exceptions it allows
│   │   │   ├── gate_rules.tsv                                   # Every rule: ID, short name, exception or none, what it holds
│   │   │   ├── inputs.rs                                        # The ci.yml inputs with a shape of their own: policies, threshold and key, typed
│   │   │   ├── lint_policy.rs                                   # LNT-001: the organization's lints and clippy.toml, written and compared
│   │   │   ├── manifests.rs                                     # What Cargo says beyond module trees: packages, the workspace, what members inherit
│   │   │   ├── mod.rs                                           # The registry of every step, run and describe, the two doors main.rs calls
│   │   │   ├── module_tree.rs                                   # Every Cargo target's module tree: files, items, named paths and re-exports
│   │   │   ├── mutation_engine.rs                               # Validate package-local features and exact engine mutation ownership
│   │   │   ├── mutation_host.rs                                 # Tested-head policy for exact provisioned-host mutation ownership
│   │   │   ├── mutation_windows.rs                              # Validate the configured Windows-owned mutation paths before use
│   │   │   ├── native_cache.rs                                  # Strict policy-file-only native cache contract, independent of engine ownership
│   │   │   ├── native_cache_inventory.rs                        # Published root-child inventories exclude publication staging and build targets
│   │   │   ├── native_cache_roots.rs                            # Owner-only restore roots
│   │   │   ├── organization_config.rs                           # The tools' configuration passed at run time, and the header sync writes
│   │   │   ├── private_directories.rs                           # Private temporary directories under the runner's own
│   │   │   ├── pull_request.rs                                  # A pull request against its base: added and touched lines, the title's type
│   │   │   ├── quality_config.rs                                # maestro-quality.toml read through jaq: declared layers and reasoned exceptions
│   │   │   ├── quality_config_cases.rs                          # Quality configuration cases kept beside the check to preserve its size limit
│   │   │   ├── release_boundary.rs                              # What both publishers ask of a release before anything is published
│   │   │   ├── rust_code.rs                                     # Rust source with comments and literals blanked, and its top-level items
│   │   │   ├── rust_code_cases.rs                               # Edge cases for Rust source scanning and function boundaries
│   │   │   ├── rust_paths.rs                                    # Every path a Rust file names: use trees expanded, a::b chains, visibilities left out
│   │   │   ├── rust_test_cases.rs                               # Edge cases for Rust source scanning and function boundaries
│   │   │   ├── rust_tests.rs                                    # The tests inside Rust source: test functions, test-only code, waits on time
│   │   │   ├── rust_versions.rs                                 # Rust version strings compared the way sort -V compared them
│   │   │   ├── simple_names.rs                                  # One validator for every simple-name rule, and hex strings
│   │   │   ├── toolbelt.rs                                      # The organization's toolbelt from the pins the gate embeds, installed and linked per user
│   │   │   └── workflow_home.rs                                 # The home of the reusable workflows, whose ci.yml, Dependabot and hooks are its own
│   │   ├── runner/                                              # The runner as the gate sees it: inputs, GITHUB_* files, tools
│   │   │   ├── commands.rs                                      # Running a pinned tool: streamed, captured into a report, or both, and the trace
│   │   │   ├── github_actions.rs                                # Inputs from env, the four GITHUB_* writers, masking, the job's directories
│   │   │   ├── mod.rs                                           # The registry of every step, run and describe, the two doors main.rs calls
│   │   │   ├── outcome.rs                                       # How a step ends: complete, or failed with the tool's own status or with one message
│   │   │   └── step_declaration.rs                              # A step as data: what it declares, and the refusal of anything undeclared
│   │   ├── steps/                                               # One module per step, private to the directory; mod.rs is the one door
│   │   │   ├── architecture/                                    # rust-gate architecture: the step and one module per group of source rules
│   │   │   │   ├── cycles.rs                                    # ARC-001: no import cycle between the files of a crate
│   │   │   │   ├── doors.rs                                     # ARC-002 and ARC-003: doors only declare, and paths go through them
│   │   │   │   ├── layers.rs                                    # ARC-004: imports run only to the layers on the right
│   │   │   │   ├── lints.rs                                     # LNT-001: the lints denied in the root manifest, a committed clippy.toml no looser
│   │   │   │   ├── mod.rs                                       # The step's door: its modules and its declaration
│   │   │   │   ├── names.rs                                     # NAME-001, NAME-002 and NAME-003: package, test and feature names
│   │   │   │   ├── packages.rs                                  # LIB-002, TST-003, WSP-001 and WSP-002, read from the manifests
│   │   │   │   ├── roots.rs                                     # ARC-006 and ARC-007: thin binary roots, and the module tree is the file tree
│   │   │   │   ├── seams.rs                                     # ARC-005: a seam a door offers serves two callers
│   │   │   │   ├── sizes.rs                                     # SIZE-002 and SIZE-003: lines of code per file and columns per line
│   │   │   │   ├── sources.rs                                   # DOC-001, LIB-001 and TST-001: module comments, library prints, waits in tests
│   │   │   │   ├── step.rs                                      # The step: module trees, the rules, the exceptions and the report
│   │   │   │   └── variables.rs                                 # NAME-004: a maestro- package reads MAESTRO_ variables and the platform's
│   │   │   ├── changed_coverage/                                # Ordinary changed coverage and the versioned pending host obligation
│   │   │   │   ├── mod.rs                                       # Ordinary changed coverage and the versioned pending host obligation
│   │   │   │   ├── pending.rs                                   # Versioned pending coverage keeps ordinary and full denominators unchanged
│   │   │   │   └── step.rs                                      # rust-gate changed-coverage: COV-002, the lines a pull request adds held
│   │   │   ├── copilot_guide/                                   # rust-gate guide: a repository's Copilot guide, written from its tracked files
│   │   │   │   ├── describe.rs                                  # What a file is for, from what it says of itself
│   │   │   │   ├── mod.rs                                       # The step's door: its five modules and its declarations
│   │   │   │   ├── render.rs                                    # The guide: where to start, the tree, how to change and verify
│   │   │   │   ├── step.rs                                      # rust-gate guide and guide --check: written, or refused when stale
│   │   │   │   ├── text.rs                                      # Sentences, textwrap's wrapping, blocks, comments and key lines
│   │   │   │   ├── tree.rs                                      # The annotated tree: kept, README-table and image explanations
│   │   │   │   └── tree_cases.rs                                # Tree width, directory defaults and README annotations at parser boundaries
│   │   │   ├── hygiene/                                         # rust-gate hygiene: the step and one module per group of rules over tracked files
│   │   │   │   ├── comments.rs                                  # HYG-001: work left for later names its issue
│   │   │   │   ├── files.rs                                     # HYG-002 to HYG-005: snapshots, large files, modes, case, symlinks, required files
│   │   │   │   ├── mod.rs                                       # The step's door: its modules and its declaration
│   │   │   │   ├── names.rs                                     # HYG-006: every file named the way its kind is named across the organization
│   │   │   │   ├── step.rs                                      # The step: tracked files, the rules, the exceptions and the report
│   │   │   │   ├── widths.rs                                    # SIZE-003 for shell scripts and justfiles
│   │   │   │   └── words.rs                                     # HYG-007: no word a glossary marks _Never_, the organization's or CONTEXT.md's
│   │   │   ├── local_ci/                                        # rust-gate ci --local: the checks job of ci.yml on a developer's machine, before a push
│   │   │   │   ├── cache.rs                                     # What CI's cache keeps between runs, moved into the job and back out
│   │   │   │   ├── checkout.rs                                  # The commit a run checks: a branch as its pull request's merge, the default branch as a push
│   │   │   │   ├── environment.rs                               # The one environment every step starts with: a runner's, each step's exports taken
│   │   │   │   ├── job.rs                                       # ci.yml's jobs as a local run takes them: each step run, done here, or not applied and why
│   │   │   │   ├── mod.rs                                       # The step's door: its five modules and its declarations
│   │   │   │   └── step.rs                                      # rust-gate ci --local: every step in order, fail fast or keep going, and the summary
│   │   │   ├── managed_files/                                   # rust-gate sync, sync --check, init and managed-files: the files every repository holds
│   │   │   │   ├── hooks.rs                                     # The commit hooks rendered: prek's checks, each tool through mise with the organization's options, the gate at the release, CI before a push
│   │   │   │   ├── mod.rs                                       # The steps' door: their modules and their declaration
│   │   │   │   ├── pin.rs                                       # The release a caller pins: a commit and its version
│   │   │   │   ├── render.rs                                    # Every managed file rendered: what a tool or GitHub reads from the repository itself
│   │   │   │   └── step.rs                                      # The steps: write, compare, and refuse by name what differs
│   │   │   ├── mutation_testing/                                # rust-gate's mutation planning, scoped execution and shard aggregation
│   │   │   │   ├── aggregate/                                   # Validate every worker receipt and raw result before merging its counters
│   │   │   │   │   ├── artifacts.rs                             # Keep downloaded mutation evidence inside the runner's temporary directory
│   │   │   │   │   ├── engine_evidence.rs                       # Harvest both required engine modes, preserving exact policy and shard identity
│   │   │   │   │   ├── evidence.rs                              # Cross-check one shard's receipt, identity assignment and raw outcomes
│   │   │   │   │   ├── host_evidence.rs                         # Join typed host outcomes with pending coverage, without editing LCOV
│   │   │   │   │   ├── merge.rs                                 # Merge validated outcomes and derive the final mutation verdict
│   │   │   │   │   ├── mod.rs                                   # Validate every worker receipt and raw result before merging its counters
│   │   │   │   │   ├── outcomes.rs                              # Validate cargo-mutants outcome paths and produce one canonical JSON document
│   │   │   │   │   ├── run.rs                                   # Run the aggregate job and validate the full plan before accepting worker evidence
│   │   │   │   │   ├── viability.rs                             # Compare mode-aware identities against their same-source featureless control
│   │   │   │   │   └── windows_evidence.rs                      # Join the native Windows owner without ever applying engine features to it
│   │   │   │   ├── engine_control/                              # Verify featureless compile membership before classifying inactive control mutants
│   │   │   │   │   ├── dep_info.rs                              # Verify raw compiler records and translate rustc dependency paths
│   │   │   │   │   ├── membership.rs                            # Retain and verify the default compiler's dep-info, bound to the worker receipt
│   │   │   │   │   ├── mod.rs                                   # Verify featureless compile membership before classifying inactive control mutants
│   │   │   │   │   ├── outcomes.rs                              # Preserve raw tested results and account explicitly for every verified non-member mutant
│   │   │   │   │   ├── package_build.rs                         # Retain and verify one owning package's independent compiler evidence
│   │   │   │   │   ├── run.rs                                   # Run tests only for compiled assigned mutants, preserving the exact complete control plan
│   │   │   │   │   └── source.rs                                # Bind the compiler coordinate frame and package selection to retained Cargo metadata
│   │   │   │   ├── engine_plan.rs                               # Discover both modes and preserve every featureless obligation before routing workers
│   │   │   │   ├── engine_run.rs                                # Execute exact shard obligations independently in featureless and engine modes
│   │   │   │   ├── host_plan.rs                                 # Full-file host discovery, bound to the ordinary plan and tested source
│   │   │   │   ├── mod.rs                                       # rust-gate's mutation planning, scoped execution and shard aggregation
│   │   │   │   ├── plan.rs                                      # Decide whether the current mutation run stays inline or needs every shard
│   │   │   │   ├── plan_identity.rs                             # Validate immutable mutation discovery, worker identity and mode-independent shard bounds
│   │   │   │   ├── reports.rs                                   # Preserve standard mutation summaries and reject incomplete outcomes
│   │   │   │   ├── scope.rs                                     # The existing first-parent scope, named relative to the checkout and hashed
│   │   │   │   ├── selftest.rs                                  # Prepare the repository-owned mutation-shard fixture before planning or execution
│   │   │   │   ├── step.rs                                      # rust-gate mutants: cargo-mutants over the checked source scope, failing on
│   │   │   │   └── windows.rs                                   # Select and run mutations for Windows-owned files only
│   │   │   ├── quality_scorecard/                               # rust-gate scorecard: the step and the value it renders
│   │   │   │   ├── mod.rs                                       # The step's door: its two modules and its declaration
│   │   │   │   ├── scorecard.rs                                 # A run's scorecard as a value: its controls, and the JSON, Markdown and badge of them
│   │   │   │   └── step.rs                                      # rust-gate scorecard: what ran, as JSON, Markdown and a self-contained badge
│   │   │   ├── rule_map/                                        # rust-gate rules: a repository's rule map, the golden rules adapted to it
│   │   │   │   ├── golden.rs                                    # The embedded golden rules: their rules, pillars and motto
│   │   │   │   ├── mod.rs                                       # The step's door: its four modules and its declarations
│   │   │   │   ├── page.rs                                      # What a repository wrote, read back; paragraphs wrapped at 80
│   │   │   │   ├── render.rs                                    # The three pages: kept rows, organization defaults, not mapped
│   │   │   │   └── step.rs                                      # rust-gate rules: the pages written where they differ
│   │   │   ├── secret_scan/                                     # Source secret scanning with gate-owned, reviewed exact-content exceptions
│   │   │   │   ├── archive.rs                                   # Neutralize Gitleaks' automatic root ignore-file loading without dropping its content
│   │   │   │   ├── mod.rs                                       # Source secret scanning with gate-owned, reviewed exact-content exceptions
│   │   │   │   ├── policy.rs                                    # The exception authority is compiled into the gate, never read from a consumer
│   │   │   │   ├── reports.rs                                   # Decode raw findings in memory and emit only locations, hashes and review metadata
│   │   │   │   └── step.rs                                      # rust-gate secrets: built-in Gitleaks rules, no consumer allowlist, and
│   │   │   ├── api_compatibility.rs                             # rust-gate api: cargo-semver-checks against the base branch unless the title declares a break
│   │   │   ├── attest_binaries.rs                               # rust-gate attest-binaries: validate, extract the SBOM, verify, record the outcome
│   │   │   ├── binary_hardening.rs                              # rust-gate hardening: reproducible, PIE, RELRO, no executable stack, auditable
│   │   │   ├── commit_hooks.rs                                  # rust-gate hooks: the repository's commit hooks over every file, through the pinned prek
│   │   │   ├── configure_cargo_registry.rs                      # rust-gate registry: private job-local Cargo home for direct crates.io
│   │   │   ├── declared_msrv.rs                                 # rust-gate msrv: every member declares a rust-version the compiler under test reaches
│   │   │   ├── dependency_policy.rs                             # rust-gate licenses: DEP-001 rendered at run time; a committed deny.toml refused
│   │   │   ├── feature_combinations.rs                          # rust-gate features: cargo hack builds each declared feature, not only the default set
│   │   │   ├── format_lint_test.rs                              # rust-gate quality: fmt, Clippy, tests, doc tests, strict rustdoc, with the organization's configuration
│   │   │   ├── fuzz_regression.rs                               # rust-gate fuzz: inputs, nightly toolchain with cargo-fuzz, corpus replay and exploration
│   │   │   ├── gate_rules.rs                                    # rust-gate gate-rules: the list of rules, one a line
│   │   │   ├── hygiene_workflow.rs                              # rust-gate hygiene prepare: the checkout and reports directory of hygiene.yml
│   │   │   ├── install_toolchain.rs                             # rust-gate install-tools: what it refuses, honours, and ci.yml installs
│   │   │   ├── install_tools.rs                                 # rust-gate install-tools: official release assets, digests verified before extraction
│   │   │   ├── line_coverage.rs                                 # rust-gate coverage: LCOV line coverage, failing below the threshold
│   │   │   ├── local_runs.rs                                    # rust-gate architecture, hygiene and clippy --local: a step as a hook or a developer runs it
│   │   │   ├── mod.rs                                           # One module per step, the registry among them; run and describe are its doors
│   │   │   ├── native_cache_transport.rs                        # Guarded restore preparation and successful-job publication inventory
│   │   │   ├── performance.rs                                   # rust-gate performance: PRF-001, declared benchmarks base against head under gungraun
│   │   │   ├── publish_binaries.rs                              # rust-gate publish-binaries: the publication boundary of the binary publisher
│   │   │   ├── publish_crate.rs                                 # rust-gate publish-crate: boundary, toolchain, package, semver, publish
│   │   │   ├── publish_evidence.rs                              # rust-gate publish-evidence: validate reports and upload release assets
│   │   │   ├── pull_request_rules.rs                            # rust-gate pull-request: PRL-001, a feature with its test, and PRL-002, its size
│   │   │   ├── recorded_audits.rs                               # rust-gate vet: cargo-vet against the committed ledger
│   │   │   ├── registry.rs                                      # Every step's declaration in workflow order, and the two doors main.rs calls
│   │   │   ├── release_build.rs                                 # rust-gate build: release tests, auditable build, packages, per-member SBOMs
│   │   │   ├── report_duplicates.rs                             # rust-gate duplication: pairs of alike functions reported, three alike refused (DUP-001)
│   │   │   ├── report_sizes.rs                                  # rust-gate complexity: function and file sizes, reported and never enforced
│   │   │   ├── require_every_check.rs                           # rust-gate required: the one status a branch protection can require
│   │   │   ├── stage_payload.rs                                 # rust-gate stage: the immutable payload, its provenance and checksums
│   │   │   ├── toolbelt_setup.rs                                # rust-gate setup: the pinned toolbelt for this user, its PATH line, the commit hooks
│   │   │   ├── unsafe_audit.rs                                  # rust-gate unsafe-audit: inputs, nightly toolchain with Miri, the run and its reach
│   │   │   ├── unused_dependencies.rs                           # rust-gate unused: cargo-machete on declared-but-unused dependencies
│   │   │   ├── validate_inputs.rs                               # rust-gate validate: every ci.yml input checked before any side effect
│   │   │   ├── verify_payload.rs                                # rust-gate verify-payload: manifest, checksums, symlinks, revision and provenance
│   │   │   ├── vulnerability_audit.rs                           # rust-gate audit: the lockfile against RustSec, yanked and unsound denied
│   │   │   └── write_lints.rs                                   # rust-gate lints --write: the organization's lints into the root manifest
│   │   └── main.rs                                              # Argument parsing only; every step runs through steps::run, describe prints the steps
│   ├── Cargo.lock                                               # Locked resolution for the test crate
│   ├── Cargo.toml                                               # Isolated workflow-contract test target
│   └── LICENSE                                                  # MIT notice included in the Cargo package
├── policy/                                                      # Policy
│   └── secret-scan-exceptions.json                              # JSON data: secret scan exceptions
├── scripts/                                                     # Provisioning that has to run before the toolbelt exists
│   └── bootstrap.sh                                             # This repository's own verified toolbelt and hooks, on Linux x64
├── supply-chain/                                                # The audits the organization publishes for every repository to import
│   └── audits.toml                                              # cargo-vet audits recorded by the organization, VET-001's first import
├── tests/                                                       # Workflow contract validation
│   ├── ci/                                                      # ci.yml, one module per gate it runs: what each step accepts, refuses, builds and reports
│   │   ├── mutation_shards/                                     # ci.yml: mutation selection, full shard matrices and fail-closed planning
│   │   │   ├── aggregation_evidence.rs                          # Mutation evidence validation and aggregation tests
│   │   │   ├── aggregation_rejections.rs                        # Mutation aggregation refusal cases for incomplete or inconsistent evidence
│   │   │   ├── engine_aggregation.rs                            # Every engine mode and its featureless control are required mutation evidence
│   │   │   ├── engine_compile_regression.rs                     # Real package compilation and configured exact-selection regressions
│   │   │   ├── engine_execution.rs                              # Engine worker selection, bound modes and complete execution
│   │   │   ├── engine_input_ownership.rs                        # Input-only ownership follows immutable checks outputs through every mutation worker
│   │   │   ├── engine_membership.rs                             # Compile membership, not redundant default test runs, proves featureless inactivity
│   │   │   ├── engine_membership_binding.rs                     # Membership requires source metadata and a retained equivalent compile command
│   │   │   ├── engine_membership_records.rs                     # Independent compiler record shape and failed-process stimuli
│   │   │   ├── engine_package_controls.rs                       # Independent package builds, evidence binding and package-local fallback
│   │   │   ├── engine_planning.rs                               # Mode-aware engine ownership plans at the executable boundary
│   │   │   ├── engine_regression.rs                             # Real, offline three-package regression of the complete required mutation gate
│   │   │   ├── engine_rejections.rs                             # Plan, artifact and outcome corruption never becomes engine gate success
│   │   │   ├── engine_windows.rs                                # Windows ownership remains native, featureless and part of engine aggregation
│   │   │   ├── inline_execution.rs                              # The serial mutation run preserves reports and cargo-mutants' exit status
│   │   │   ├── mod.rs                                           # ci.yml: mutation selection, full shard matrices and fail-closed planning
│   │   │   ├── pinned_mutants.rs                                # Real pinned cargo-mutants listing and two-worker execution tests
│   │   │   ├── shard_execution.rs                               # Mutation worker plan validation and execution tests
│   │   │   ├── shard_inputs.rs                                  # Mutation shard input validation before environment exports
│   │   │   ├── shard_planning.rs                                # Mutation input forwarding and deterministic shard selection tests
│   │   │   ├── shard_workflow.rs                                # Workflow routing, required status and final scorecard contract tests
│   │   │   ├── windows_execution.rs                             # The Windows mutation step guards configured files and runs exact diff scopes
│   │   │   └── workflow_contracts.rs                            # Static workflow wiring contracts for mutation planning and shards
│   │   ├── api_compatibility.rs                                 # ci.yml: an undeclared API break fails a pull request; what has no API is not applicable
│   │   ├── architecture_rules.rs                                # ci.yml: ARC-001 to ARC-007, each refused by name, and the exceptions maestro-quality.toml takes
│   │   ├── central_uploads.rs                                   # ci.yml: SARIF and Codecov uploads from the ruleset's run, on a merge group to the default branch
│   │   ├── commit_hooks.rs                                      # hooks, the local runs a hook makes, and hygiene.yml's first step
│   │   ├── complexity_report.rs                                 # ci.yml: function and file sizes, reported and never held against the run
│   │   ├── copilot_guide.rs                                     # guide and guide --check: written, kept, refused when a file makes it stale
│   │   ├── coverage_features.rs                                 # Default compatibility, input transport and real merged coverage regression
│   │   ├── duplication_report.rs                                # ci.yml: pairs reported, three functions of one shape refused unless excused
│   │   ├── feature_combinations.rs                              # ci.yml: real per-feature and combined compilation, plus replay coverage
│   │   ├── host_coverage_join.rs                                # Host evidence is distinct from LLVM hits and cannot enlarge ordinary allowances
│   │   ├── host_ownership.rs                                    # Provisioned-host ownership is a transfer, never a coverage exemption
│   │   ├── host_policy_validation.rs                            # Strict tested-head host policy, overlaps and all discovery modes
│   │   ├── host_required_status.rs                              # Every default mode requires host execution, aggregation and coverage in the same attempt
│   │   ├── input_validation.rs                                  # unsafe-audit.yml and fuzz.yml: every malformed input refused before a toolchain is touched
│   │   ├── install_tools.rs                                     # rust-gate install-tools: what it refuses, honours, and ci.yml installs
│   │   ├── internal_shard_selftest.rs                           # Repository-only synthetic mutation-shard input validation
│   │   ├── local_ci_run.rs                                      # rust-gate ci --local: every ci.yml step run or said not applied, a branch as its pull request
│   │   ├── managed_files.rs                                     # init, sync, sync --check and managed-files: written, refused by name, written back
│   │   ├── mod.rs                                               # The repository modules, listed and nothing else
│   │   ├── native_cache.rs                                      # Native cache policy, private restore transport and coverage-only injection
│   │   ├── native_cache_checks.rs                               # API and feature children share only a policy-opted-in, reverified native root
│   │   ├── native_cache_fixture.rs                              # Real fresh-target builds prove optional consumer cache reuse, not a command stub
│   │   ├── native_cache_hosted_mutation.rs                      # Hosted mutation proof shares production transport rather than a copied contract
│   │   ├── native_cache_mutation.rs                             # Native mutation transport routing, child-only injection and legacy compatibility
│   │   ├── native_cache_policy.rs                               # Native policy parser isolation and coverage child environment regressions
│   │   ├── organization_lints.rs                                # LNT-001: written, refused when missing or looser, and read by real Clippy through the gate
│   │   ├── performance_budget.rs                                # PRF-001: a rise past 5 % refused unless excused, and when nothing is measured
│   │   ├── platform_portability.rs                              # ci.yml: named platforms become pinned runners that the required status holds
│   │   ├── pull_request_rules.rs                                # COV-002, PRL-001 and PRL-002 over a real change against a base commit
│   │   ├── quality_gates.rs                                     # ci.yml: lint, documentation, coverage and analysis gates, each proven to fail
│   │   ├── quality_reports.rs                                   # ci.yml: diagnostics survive failing tools without changing their verdict
│   │   ├── release_payload.rs                                   # ci.yml: release build, payload, bills of materials, and the example gate
│   │   ├── release_payload_refusals.rs                          # The release payload's refusals: lockfile drift, unhardened or irreproducible binaries, malformed staging
│   │   ├── release_rebuild.rs                                   # Clean same-path release rebuilds preserve generated code, flags and cached objects
│   │   ├── repository_hygiene.rs                                # ci.yml: HYG-001 to HYG-005 and shell width, each refused by name
│   │   ├── required_status.rs                                   # Contract tests for the sole branch-protection status
│   │   ├── rule_map.rs                                          # rules and rules --check: written, kept, refused when stale or unmapped
│   │   ├── ruleset_settings.rs                                  # ci.yml run by a ruleset: its inputs from the base commit's [ci], macOS and Windows kept
│   │   ├── scorecard_and_required_status.rs                     # ci.yml: the scorecard, the required status and mutation testing
│   │   ├── scorecard_states.rs                                  # ci.yml: selection, applicability and execution reported separately
│   │   ├── secret_exceptions.rs                                 # Reviewed secret exceptions are gate-owned and require all four exact keys
│   │   ├── source_rules.rs                                      # ci.yml: SIZE, NAME, DOC, LIB, TST and WSP, each refused by name, and the limits a repository tightens
│   │   ├── supply_chain.rs                                      # ci.yml: dependency policy, direct crates.io reads and the scanners
│   │   ├── toolbelt_setup.rs                                    # rust-gate setup: the locked toolbelt linked and its PATH printed, a bad mise digest refused
│   │   ├── workspace_boundary.rs                                # ci.yml: a workspace whose manifests or sources reach outside the checkout is refused before any lint
│   │   └── workspace_packaging.rs                               # ci.yml: the build packages only members that may be published, earlier archives removed
│   ├── fixtures/                                                # Test fixtures
│   │   ├── native-consumer/                                     # Native consumer
│   │   │   └── build.rs.in                                      # Build-script template for private, verified, atomic native entry publication
│   │   └── native-consumer-project/                             # Committed hosted consumer, byte-checked against its generator
│   │       ├── crates/                                          # Three feature-partition workspace members
│   │       │   ├── a/                                           # Optional native consumer
│   │       │   │   ├── src/                                     # Default and engine-only sources
│   │       │   │   │   ├── engine.rs                            # Engine-only coverage assertion
│   │       │   │   │   └── lib.rs                               # Default workspace coverage assertion
│   │       │   │   └── Cargo.toml                               # Optional native dependency and engine feature
│   │       │   ├── b/                                           # Forwarded engine feature
│   │       │   │   ├── src/                                     # Forwarding member source
│   │       │   │   │   └── lib.rs                               # Default workspace coverage assertion
│   │       │   │   └── Cargo.toml                               # Forwards crate-a's engine feature
│   │       │   └── c/                                           # Featureless member
│   │       │       ├── src/                                     # Featureless member source
│   │       │       │   └── lib.rs                               # Default workspace coverage assertion
│   │       │       └── Cargo.toml                               # Unrelated workspace member
│   │       ├── native/                                          # Verified native publication fixture
│   │       │   ├── src/                                         # Generated native payload consumer
│   │       │   │   └── lib.rs                                   # Includes the build-script payload
│   │       │   ├── Cargo.toml                                   # Excluded optional native dependency
│   │       │   └── build.rs                                     # Generated copy of the native publication template
│   │       ├── .gitignore                                       # Keeps consumer build output out of the tested commit
│   │       ├── Cargo.lock                                       # Offline, pinned consumer dependency graph
│   │       ├── Cargo.toml                                       # Three-package hosted fixture workspace
│   │       └── maestro-quality.toml                             # Optional engine and Unix native cache policy
│   ├── gate/                                                    # The gate and the tests as structures: layers, no import cycle, the step registry, what holds every step and refusal
│   │   ├── layer_boundaries.rs                                  # This crate's own step shape, the checks door, seam unit tests and no whole-harness import
│   │   ├── mod.rs                                               # The repository modules, listed and nothing else
│   │   ├── step_and_refusal_coverage.rs                         # Every declared step is run by a contract test; every refusal the binary composes is asserted by a test
│   │   └── step_registry.rs                                     # The step registry: declarations, the generated document, every body registered
│   ├── harness/                                                 # The one door of the tests: the repository, YAML readers, gate declarations and the fixture
│   │   ├── engine_compile.rs                                    # Shared real package build and aggregate fixtures for exact compile membership
│   │   ├── engine_mutations.rs                                  # Shared mode-aware engine planner, worker and aggregate fixtures
│   │   ├── engine_workspace.rs                                  # Real Git setup for mode and input-only ownership regressions
│   │   ├── fixture.rs                                           # One temporary checkout, one environment table, a step run against stand-ins, every command traced
│   │   ├── gate_declarations.rs                                 # The gate built once per test process, and what rust-gate describe declares about its steps
│   │   ├── host_fixture.rs                                      # A synthetic consumer with a tracked host owner and full deterministic listing
│   │   ├── mod.rs                                               # The repository modules, listed and nothing else
│   │   ├── mutation_shards.rs                                   # Shared test fixtures for mutation planning and evidence aggregation
│   │   ├── native_cache.rs                                      # Shared opted-in native cache and observed coverage child fixtures
│   │   ├── replay_processes.rs                                  # Memory-limited replay processes and their preserved evidence
│   │   ├── repository.rs                                        # The repository root, the toolbelt, commands run to completion, temporary directories, stand-in executables, every test file
│   │   └── workflow_yaml.rs                                     # Readers of workflow and action YAML: whole documents, one step's body, tool rows, jaq queries
│   ├── hosted_cache/                                            # Hosted cache
│   │   ├── harness/                                             # Native, cross-platform fixture preparation and observed coverage evidence
│   │   │   ├── mod.rs                                           # Native, cross-platform fixture preparation and observed coverage evidence
│   │   │   ├── mutations.rs                                     # Real engine execution evidence from the production planner and worker commands
│   │   │   └── runtime.rs                                       # Real hosted fixture setup and evidence, without Bash or command stand-ins
│   │   └── main.rs                                              # Hosted native cache transport: real consumer preparation and coverage execution evidence
│   ├── nightly/                                                 # The nightly workflows, unsafe-audit.yml and fuzz.yml, outside the stable policy
│   │   ├── fuzz_regression.rs                                   # fuzz.yml: the nightly with rust-src and cargo-fuzz, every committed target replayed with the corpus first
│   │   ├── input_validation.rs                                  # unsafe-audit.yml and fuzz.yml: every malformed input refused before a toolchain is touched
│   │   ├── mod.rs                                               # The repository modules, listed and nothing else
│   │   └── unsafe_audit.rs                                      # unsafe-audit.yml: Miri on the selected nightly, and no pass without a test executed under it
│   ├── publishers/                                              # The publishing workflows: dry run first, a live path only a protected release takes, what each verifies before handing a release on
│   │   ├── binary_attestation.rs                                # attest-binaries.yml: signing only what the job verified
│   │   ├── crate_and_binaries.rs                                # Publishers: dry-run first, and a live path only a protected release takes
│   │   ├── crate_toolchains.rs                                  # publish-crate.yml: the dry run installs the pinned toolchain, the publication the validated one
│   │   ├── evidence_publication.rs                              # publish-evidence.yml: source-bound release reports and explicit dry-runs
│   │   ├── github_releases.rs                                   # Live approval, release identity and no-overwrite upload contracts
│   │   ├── mod.rs                                               # The repository modules, listed and nothing else
│   │   ├── payload_verification.rs                              # The shared payload verification: revision, checksum manifest and provenance, every flaw refused by name
│   │   └── recorded_attestation.rs                              # attest-binaries.yml: the recorded attestation verified through gh, refused unless it covers the digest
│   ├── repository/                                              # The repository itself: files, documents, pins, sizes, policies, hooks, scans and the names of its tests
│   │   ├── gate_mutation_replay/                                # Exact hosted mutation replay and its bounded child processes
│   │   │   ├── execution.rs                                     # Child command execution and allocation evidence for this replay only
│   │   │   ├── mod.rs                                           # Exact hosted mutation replay and its bounded child processes
│   │   │   └── replay_worker.rs                                 # Hosted replay of exact stage-one diffs against gate units and the contract harness
│   │   ├── commit_message_hooks.rs                              # The commit-msg hooks: a conventional header first, 80 columns, refused by prek in a fresh repository
│   │   ├── documentation_coverage.rs                            # Every report, input and secret documented; links resolve; cited tests exist
│   │   ├── evidence_receipt.rs                                  # The evidence receipt: produced only when every upstream result succeeded
│   │   ├── executable_stubs.rs                                  # Stand-in executables written outside the test process, so none is refused as Text file busy
│   │   ├── gate_action.rs                                       # The gate action: built from the workflow's own commit in every job
│   │   ├── gate_mutation.rs                                     # Report-only gate mutation workflow and full-scope execution contracts
│   │   ├── gate_rules.rs                                        # Every rule the gate names is listed, and has its row in docs/ci.md
│   │   ├── generated_documents.rs                               # Every generated table and the diagram's count are what just docs writes
│   │   ├── metadata_and_inventory.rs                            # Repository files, hook, editor and release policies, the Copilot inventory
│   │   ├── mod.rs                                               # The repository modules, listed and nothing else
│   │   ├── north_star.rs                                        # Promised controls run, every gate names its proof, no lint silenced
│   │   ├── pinned_tool_usage.rs                                 # Every job installs every pinned tool it invokes before a step reads it
│   │   ├── rendered_hooks_live.rs                               # CHECK_NETWORK=1: the rendered hooks in a fresh clone on the toolbelt setup installs
│   │   ├── replay_memory.rs                                     # The replay cap refuses setup failure and records its effective value
│   │   ├── secret_and_advisory_scans.rs                         # Gitleaks over the tree; RustSec audits under CHECK_NETWORK=1
│   │   ├── tool_updates.rs                                      # Every install row is what mise locked; update-tools moves a pin everywhere at once
│   │   ├── toolbelt_and_shellcheck.rs                           # Toolbelt links to the locked builds; ShellCheck over every Bash line left
│   │   ├── toolbelt_platforms.rs                                # Every pin locked with a checksum on every platform, or its declared gap
│   │   ├── version_pins.rs                                      # Tool versions, the toolchain pin and the speed target, one copy each
│   │   ├── workflow_environment.rs                              # Refuses reserved GitHub and runner environment keys at every workflow scope
│   │   └── workflow_policy.rs                                   # Permissions, timeouts, runners, trust boundaries, shell policy and the local calls
│   ├── Cargo.lock                                               # Locked resolution for the test crate
│   ├── Cargo.toml                                               # Isolated workflow-contract test target
│   ├── LICENSE                                                  # MIT notice included in the Cargo package
│   ├── fixture_sources.rs                                       # Shared three-package workspace with an optional native publication consumer
│   ├── native_runtime.rs                                        # Real native Windows processes, registry boundary and temporary trees, the native suite's one door
│   ├── native_windows.rs                                        # Native gate units, ACLs and real Cargo examples, not Linux ELF replay
│   └── workflows.rs                                             # Test crate root: one directory per what the tests prove, and the harness they share
├── .editorconfig                                                # UTF-8, LF, final newlines, space indentation
├── .gitattributes                                               # Text normalization, Rust-aware diff, binary images
├── .gitignore                                                   # Local tools/caches, Cargo build output, Windows markers
├── .pre-commit-config.yaml                                      # This repository's own hooks: the organization's set on the pinned toolbelt, and its generated tables
├── .rumdl.toml                                                  # Markdown structure: lines wrap where their writer wraps them; this repository's own
├── .taplo.toml                                                  # TOML formatting: arrays keep the shape they were written in; this repository's own
├── .yamlfmt.yml                                                 # YAML formatting for workflows and metadata
├── AGENTS.md                                                    # Authoritative workflow objectives and constraints
├── CHANGELOG.md                                                 # Written by release-please from conventional commit titles
├── CONTEXT.md                                                   # Domain glossary for workflows, runners and publication
├── CONTRIBUTING.md                                              # Pinned tools, setup, checks and review procedure
├── LICENSE                                                      # MIT licence for the repository and its Cargo packages
├── README.md                                                    # Complete workflow contracts and usage examples
├── SECURITY.md                                                  # Runner trust, token handling, publication boundaries
├── SUPPORT.md                                                   # Troubleshooting and safe diagnostic steps
├── clippy.toml                                                  # The organization's thresholds and test allowances, for this repository's own Clippy
├── deny.toml                                                    # DEP-001 and the reviewed licences, for this repository's own cargo-deny
├── justfile                                                     # Development commands: setup and check
├── maestro-quality.toml                                         # The layers this repository's crates declare, its reasoned exceptions and its words
├── mise.lock                                                    # Resolved URL and checksum of every toolbelt download, on every platform
├── mise.toml                                                    # The organization's toolbelt: each tool at the version CI pins, and its gaps
├── rust-toolchain.toml                                          # The one compiler pin: the gate, the tests and the action build with it
├── rustfmt.toml                                                 # The 2024 formatting style, for this repository's own rustfmt
├── typos.toml                                                   # The words this repository means, from maestro-quality.toml; rendered by rust-gate sync
└── version.txt                                                  # Simple-release version, not a compiler pin
```

## Change and verification procedure

1. Read the rules in AGENTS.md that cover the files you change, and keep every
   gate intact: never weaken one to pass.
2. Add an executable regression check for a change in behaviour.
3. The commit hook `rust-gate guide` rewrites this guide when a file is added,
   moved or removed; commit it with the change. The organization's daily drift
   check reports a guide left stale.
4. Run `scripts/bootstrap.sh` once, then `just check`, and report the commands
   you actually ran.
5. Commits are signed, with a conventional title; the default branch takes only
   squash-merged pull requests.

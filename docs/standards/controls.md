# Controls beyond the rule map

The rule map, [engineering.md](engineering.md), [security.md](security.md) and
[northstar.md](northstar.md), says what holds each of the organization's golden
rules here; `rust-gate rules` writes those pages and keeps what each row says.
This page is written by hand and holds what they have no place for: the bar
behind each North Star pillar and the test that proves it, the defaults a
consumer inherits, what counts as evidence for a green claim, the notes behind
the mandates, and the extended security controls. The identifiers SDL, SST, SCH
and VR are this repository's own; the golden rules have no such families.

A deterministic control produces a repeatable machine result. A judgement
control needs a person, and review is not proof. Both appear below, always
labelled. Missing enforcement is unsupported, not compliant, and nothing here
grants a permission, a token or an exemption.

## 📐 Four axes

Each pillar has one bar, the gate that holds it, and the test that fails when it
slips. These are acceptance targets, not claims of a measured green run.

| Axis | Bar | Held by | Proof |
| --- | --- | --- | --- |
| Quality | All 15 consumer and compiler cases pass; at least 90% line coverage on every owned fixture; zero surviving mutants (mutation testing) in the owned fixtures; zero formatting, Clippy or rustdoc warnings, doctests included, Clippy's pedantic group among them. | `just check` replays the steps of `ci.yml` against every fixture, coverage floor at 90 and every offline gate on; the hosted matrix runs the 15 cases. | `example_gate_replays_ci_step_bodies_against_every_fixture`, `north_star_promises_are_enforced_by_the_local_gate` |
| Speed | `just check` at or under 40 seconds with tools and dependencies cached. | `just check` prints its own duration against the target on every run. | `the_speed_target_is_the_one_the_gate_prints` |
| Security | Zero unreviewed secret findings, known vulnerabilities, disallowed licences or unexpected package sources; every action pinned to a commit and every download to a checksum; no silenced lint in the gate or the fixtures. | Gitleaks with gate-owned reviewed exceptions bound to exact repository, path, rule and Match hash, the mandatory hosted RustSec lookup, the cargo-deny licence and source policy, the CycloneDX SBOM, and the pin and download tests. | `scanners_propagate_findings_execution_errors_and_missing_tools`, `permissions_timeouts_and_shell_policy_hold_in_every_workflow`, `tool_installation_verifies_every_download_and_fetches_nothing_unasked`, `no_lint_is_silenced_in_the_gate_or_the_fixtures` |
| Maintainability | Every file explained in the Copilot inventory, and no explanation naming a code artefact its file does not hold; every item of the gate documented, private ones included; zero stale links; every test a standard or a gate cites exists; every gate names the test that proves its failure case; no function above 15 in cognitive complexity, 100 lines or 5 parameters; no name a layer door offers used by fewer than two modules without its reason written down, and no name the harness door offers that no test outside it uses; no Rust file above 500 lines of code, with the files over 300 reported; no line of Rust or shell above 100 columns. | The inventory, link, documentation and citation tests; Clippy's private-item documentation lint on the gate crate; strict rustdoc; the organization's lints in each crate's manifest and its thresholds in `clippy.toml`, held by LNT-001, and the limits tests. | `metadata_forms_and_copilot_inventory_are_complete`, `every_code_token_a_tree_comment_names_lives_in_its_file`, `every_relative_link_in_the_repository_resolves`, `every_input_output_and_secret_is_documented`, `every_test_the_standards_cite_exists`, `every_gate_names_the_test_that_proves_it`, `a_seam_serving_one_outside_caller_is_refused_unless_excused`, `no_test_module_imports_the_whole_harness`, `a_clippy_toml_looser_than_the_organization_s_is_refused`, `clippy_knows_every_organization_lint_and_applies_it`, `oversized_files_and_lines_are_refused_and_three_hundred_is_reported`, `badly_named_tests_and_one_word_test_modules_are_refused`, `wide_shell_lines_and_justfiles_are_refused` |

The KPI behind each pillar is in [northstar.md](northstar.md). Speed is an
improvement target, not a merge gate: `just check` prints its duration against
the target and never fails a run for time. Measure from this repository root
with tool installation, the live advisory fetch and the hosted matrix excluded.
A missed target requires a recorded cause and a focused improvement, not fewer
checks. Hosted cold start and queue time are a different measurement.

## 🛡️ How we hold ourselves to it

- Shift left, automated. A rule that matters is a gate in the prek hooks and
  in `just check`, the same gate CI runs. A rule that cannot be automated is
  written down as a check someone runs, and the standards say which is which.
- Guardrails over gatekeepers. A test, a lint or a pinned tool beats a review
  comment that will be forgotten.
- A gate that cannot fail is not a gate. Every gate in the README names the
  test that proves its failure case, and a test that no longer exists turns
  the gate red.
- A decision is recorded where it is enforced. The test that holds a rule
  carries the reason in its own comment, and the standards name that test.
- A change names the axis it moves. The pull request template asks which axis
  a change moves and what it costs the others; a trade-off nobody stated is
  not merged quietly.

## ⚖️ Defaults a consumer inherits

The coverage floor is 90%, the owned fixtures' and every caller's: the public
`ci.yml` default is 90 and a lower `coverage-threshold` is refused (COV-001). Six
gates are on by default as deliberate exceptions to adoption safety, because a
golden workflow enforces the standard: mutation testing, the unused-dependency
check, the `unsafe` ban, SARIF reports, public API compatibility and recorded
dependency audits (VET-001), each with one input to switch it off. The
scaffolding lints and the dependency policy have no off switch:
`license-policy` accepts `auto` and `enforce`, both DEP-001, the organization's
policy the gate renders at run time, with the organization allowlist's licences
added when provided, and refuses `off`. A committed `deny.toml` is refused.
Every other default may not change to something that fails a consumer on
upgrade. The README lists every gate against these axes. Coverage measures Rust
fixture lines, not how completely the gate's own refusals are tested; preserve
executable negative cases.

## 🧾 Evidence before green claims

Local checks establish local behavior only. `CHECK_NETWORK=1 just check` adds a
fresh advisory lookup; it still does not establish GitHub runner, authentication
or live-publication readiness. This candidate has no published hosted-run
baseline yet.

1. Select completed successful **Repository quality** and **Consumer workflow
   tests** runs for the intended commit. Preserve the run URL and attempt, not a
   screenshot alone.
2. Download their `repository-evidence-*` and `consumer-evidence-*` artifacts
   plus the relevant `*-reports` artifacts. Receipts identify revision, run,
   scope and outcomes.
3. Download logs through GitHub's **Download log archive** operation. GitHub
   applies its masking there; raw process output and environment dumps are not
   copied into receipts.
4. Attach the receipts, reports and checked log archive to the approved internal
   review record, with both run links. Check for confidential information before
   sharing.

Artifacts request seven-day retention, subject to organization limits. Archive
needed evidence before expiry. A consumer receipt is generated only after all
required matrix and dry-run outcomes succeed; failed, cancelled, skipped or
missing outcomes cannot produce a success receipt. The completed GitHub run
remains the source of truth, including an upload failure or rerun. Per-job
reports alone are not proof that the whole workflow passed. Live publishing
needs separate evidence.

## ✂️ Keep the bar small

Use fast prek hooks for commit feedback and full checks in CI. Put what holds
each golden rule in the rule map, this repository's stricter rules in
[engineering.md](engineering.md)'s "Stricter here", terminology in
[CONTEXT.md](../../CONTEXT.md), and the short scorecard in
[README.md](../../README.md). Update the README when a target changes. Add a KPI
only when it changes a review or delivery decision. A hand-read KPI carries its
date in [northstar.md](northstar.md); every other piece of evidence stays out of
committed source files.

## Notes on the mandates

**ENF-002, every claimed platform: hosted gates remain Linux x64.** The separate
native Windows Cargo suite requires its own execution evidence, distinct from
Linux acceptance. The gate claims no Windows or macOS run of its own; the
`platforms` input runs a consumer's `cargo test` on pinned macOS, Windows and
Linux arm64 runners.

**ENF-004, conventional commits.** `release-please` derives the next version
from the merged commit message. On a squash merge that message is the pull
request title, which is why the pull request template states the prefixes and
their version effect, and why a test keeps that section present.

**ENF-006, never weaken a gate.** `|| true` is the usual way this rule gets
broken quietly, so a test rejects it anywhere in a workflow. That check exists
because a silenced SBOM step stays green without producing an SBOM.

**ENF-008, tiered checks.** `ci-internal.yml` runs `just check`, the same entry
point a contributor runs locally. There is no second copy of the gate to drift.

**ENF-009, allowlists that cannot rot.** The tested-version list is exactly five
pins. A test compares them against the consumer matrix and the declared default,
so a sixth or a drifted entry fails immediately. Acceptance is not an allowlist:
any exact stable version from the MSRV up is installed.

## Secure development (SDL)

- **SDL-001: Threat models at boundaries.** A feature crossing a trust boundary
  needs a threat model beside its decision record, revisited when the boundary
  changes.
- **SDL-002: Secure defaults.** Deny by default, ship safe defaults, no implicit
  activation of an unsafe configuration.
- **SDL-003: Parse at the trust boundary.** Reject early (P-011), parse into a
  checked representation (P-013), covering syntax, semantics, size bounds and
  authorized scope.
- **SDL-004: Injection and output contexts.** Untrusted input must never become
  executable syntax.
- **SDL-005: Enforced authentication and authorization.** Decided host-side
  under least privilege. Client claims grant nothing.
- **SDL-006: Secret lifecycle.** Secrets must never enter the repository,
  including history, examples and fixtures. Supplied through the approved store,
  rotated, redacted from logs.
- **SDL-007: Safe errors and logging.** Fail safely without revealing sensitive
  detail; logs omit or redact secrets.
- **SDL-008: Dependency hygiene.** Pinned with lockfiles, licensed under policy,
  audited. An unresolved licence or vulnerability decision is not clearance.

| ID | Status |
| --- | --- |
| SDL-001 | Judgement. The trust boundary is a consumer-supplied input reaching a GitHub-hosted runner; each workflow's `validate` step is the mitigation. |
| SDL-002 | **Deterministic.** Every optional gate but five defaults to off or permissive; mutation testing, unused dependencies, the `unsafe` ban, SARIF reports and public API compatibility are on by default, as a golden workflow enforces the standard, and each is one input to switch off. The dependency source policy and the scaffolding lints hold for every project and have no input. No unsafe behaviour activates implicitly. |
| SDL-003 | **Deterministic.** The `validate` step of every workflow. This is the single most-tested behaviour in the repository. |
| SDL-004 | **Deterministic.** Consumer input reaches the gate only through environment variables, never through `${{ }}` interpolation inside a `run:` body, which is the script-injection path in GitHub Actions; every `run:` body is one fixed `rust-gate` command. |
| SDL-005 | **Deterministic.** Publication requires a protected environment; the workflow cannot self-authorize. |
| SDL-006 | **Deterministic.** Gitleaks on source; the Cargo configuration is written to an isolated `CARGO_HOME`, atomically owner-only through mode 0700 on Unix or a protected inheritable owner DACL on Windows, and carries no credential. Only the protected live Cargo step receives the crates.io token. |
| SDL-007 | **Deterministic in part.** Reports are redacted before upload; log masking is GitHub's. |
| SDL-008 | **Deterministic.** `Cargo.lock` committed, `--locked` everywhere, `cargo audit` denying yanked, unsound and unmaintained crates, and `cargo deny` against DEP-001, the organization's policy the gate renders at run time (licences, bans, sources, advisories). |

## Security testing (SST)

Static analysis (SST-001), composition analysis (SST-002), secret scanning
(SST-003), infrastructure and configuration scanning (SST-004), workflow
security (SST-005), fuzzing and property testing (SST-006), findings reporting
(SST-007), expiring suppressions (SST-008) and fail-closed gates (SST-009).
Secret-scan exceptions use [the reviewed central list](../ci.md#reviewed-secret-scan-exceptions),
not consumer suppressions. Excepted locations, hashes and approval metadata remain
in redacted JSON and SARIF; stale approvals warn without failing. New flagged
content, another repository, path or rule cannot reuse an approval. Changes require
a pull request here, organization review and a gate release.

| ID | Status |
| --- | --- |
| SST-001 | **Deterministic.** Clippy with `-D warnings` across all targets *is* the Rust static analyser. `clippy-level` widens it to `pedantic` or `nursery`. |
| SST-002 | **Deterministic.** `cargo audit` plus `cargo deny check advisories`, blocking. |
| SST-003 | **Deterministic.** Gitleaks, blocking, with an optional redacted SARIF report. |
| SST-004 | **Deterministic.** `actionlint` on every workflow and `zizmor`, pedantic persona, offline, on every workflow, the gate action and the Dependabot configuration, its one documented exception in `.github/zizmor.yml`; every step body is one `rust-gate` command, and the gate crate is held to rustfmt, Clippy `-D warnings` and strict rustdoc; `every_bash_line_left_in_the_repository_passes_shellcheck` over the Just recipes, `bootstrap.sh` and the gate action's build step. |
| SST-005 | **Deterministic.** `permissions_timeouts_and_shell_policy_hold_in_every_workflow`: explicit permissions, timeouts, and `\|\| true` rejected; every action pinned to a commit, GitHub's own included, held by zizmor `unpinned-uses` under the `hash-pin` policy in `.github/zizmor.yml`. |
| SST-006 | **Available, opt-in.** `fuzz.yml` replays the committed corpus then explores for a bounded budget. `unsafe-audit.yml` runs Miri. Both are nightly-only, so both sit outside `ci.yml`. |
| SST-007 | **Deterministic.** Every gate writes a report into the run's reports artifact, and the scorecard summarises which were active. |
| SST-008 | **Deterministic.** No version allowlist exists: the MSRV is the floor and the five tested pins are one list with one test that fails when it drifts. No finding is suppressed anywhere. |
| SST-009 | **Deterministic.** This is what the ban on `\|\| true` protects. A tool that fails to run fails the job. |

## Supply chain (SCH)

This is where the repository does most of its work.

| ID | Requirement | Status |
| --- | --- | --- |
| SCH-001 | Build provenance | **Available, unproven on the hosted platform.** `attest-binaries.yml` requests signed provenance. The default `on-unavailable: skip` tolerates only Enterprise Server identified by successful metadata lookup; unknown availability and signing errors block. |
| SCH-002 | Signing and verification | **Deterministic where available.** The workflow verifies its own attestation with `gh attestation verify --signer-workflow` and compares the verified subject digest against the one it hashed itself. Successful command execution is not accepted as verification. |
| SCH-003 | Both SPDX **and** CycloneDX SBOMs | **Met.** CycloneDX per member plus a hierarchical merge into `payload.cdx.json` (the attestation subject), and SPDX 2.3 per member from `cargo-sbom`. A check fails the build when the two formats describe different members. |
| SCH-004 | Release checksums | **Deterministic.** `SHA256SUMS` published and re-verified inside the attesting job before signing. |
| SCH-005 | Immutable dependency inputs | **Deterministic.** Every external action pinned to a full commit SHA; every tool downloaded as a prebuilt release and checked against a pinned sha256; `Cargo.lock` committed; `--locked` everywhere. A test rejects a mutable tag. |
| SCH-006 | Least-privilege build identities | **Deterministic.** Tested. Untrusted pull requests reach no runner and no secret. |
| SCH-007 | Reviewed dependency updates | **Deterministic.** Dependabot proposes, one grouped pull request per ecosystem for patch and minor updates; the organization's bot queues those to merge only behind the full gate. A major update waits for a person. |
| SCH-008 | Embedded dependency metadata | **Met.** Both release builds run through `cargo auditable`, embedding the resolved dependency list in a `.dep-v0` ELF section, and the hardening step fails when that section is missing rather than assuming the tool ran. |
| SCH-009 | Reproducibility | **Met, and claimed only because it is measured.** The release build runs a second time into a different target directory and the digests must match. The claim rests on that comparison, not on a successful build. |
| SCH-010 | Third-party and vendored policy | **Deterministic.** DEP-001, rendered by the gate at run time, binds licences, bans, sources and advisories in every repository, with the organization allowlist's licences added when set; a committed `deny.toml` is refused. Nothing is vendored. |
| SCH-011 | Published artifact verification | **Met, and exercised.** [SECURITY.md](../../SECURITY.md) documents checksums, attestation identity and subject, both SBOM formats and the embedded dependency list. The procedure was run, every step passing, on the assets of release-canary v0.1.0. |

### What is left

**SCH-001** and **SCH-002** are written and self-verifying but unproven until
administrators confirm Cloud entitlement and permissions, and a hosted run
successfully signs and verifies the artifact. Use `on-unavailable: fail` when
provenance is required. A private Cloud entitlement cannot be inferred from an
API error; permission validation may reject a workflow before its runtime
checks.

One implementation note worth keeping, because the obvious approach is wrong:
SPDX is generated natively rather than converted from the CycloneDX document.
`cyclonedx convert` loses nested components from a hierarchical merge and
duplicates shared ones from a flat merge, and it drops the SPDX relationship
graph entirely. A silently incomplete SBOM is worse than none, because it looks
like an answer.

## Vulnerability response (VR)

- **VR-001: Private reporting.** A private intake path must exist and be
  reachable before a reporter's first instinct is a public issue.
- **VR-002: Honest acknowledgement.** Acknowledge without overstating what is
  known.
- **VR-003: Severity and weakness triage.**
- **VR-004: Gated security fixes.** A security fix passes the same gates.
- **VR-005: Regression test first.** The failing test precedes the fix (ENF-005).
- **VR-006: Security advisories.**
- **VR-007: Coordinated disclosure.**
- **VR-008: Root cause and weakness sweep.** Fix the class, not only the
  instance.
- **VR-009: Dependency vulnerability response.**
- **VR-010: Retained truthful response evidence.**

| ID | Status |
| --- | --- |
| VR-001 | **Deterministic.** The issue chooser offers a private security route before any form, and a test asserts it stays there. |
| VR-004, VR-005 | **Deterministic.** The same `just check` gate applies, and the pull request checklist requires a regression test *and its failure case*. |
| VR-009 | **Deterministic.** `cargo audit` and `cargo deny check advisories` block on a known advisory. |
| VR-002, VR-003, VR-006, VR-007, VR-008, VR-010 | Judgement, executed through the process in [SECURITY.md](../../SECURITY.md). |

## Framework alignment

Alignment is recorded, not certified. No assessment, attestation or maturity
score is claimed.

| Framework | Scope here |
| --- | --- |
| OWASP Top 10:2025 | Reviewed at boundaries; injection is the applicable class, addressed by SDL-004. |
| OWASP Top 10 for LLM Applications 2025 | Applicability reviewed; this repository ships no model or agent surface. |
| SLSA 1.2 Build Track | SCH-001 and SCH-002 produce and verify provenance. **No level is claimed**, because the platform capability is unconfirmed. |
| CWE Top 25 | Considered in review under SDL-004. |

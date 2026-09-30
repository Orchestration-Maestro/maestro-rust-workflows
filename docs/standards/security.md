# Security rules in `maestro-rust-workflows`

`maestro-rust-workflows` follows the organization's [security
rules](https://github.com/Orchestration-Maestro/.github/blob/864d85597a833864cd8506c3925830503b3c2163/golden-rules/security.md).
This page is its rule map (C-001): for every rule, what holds it here, or why it
does not apply. A row may name a stricter local rule; none weakens one.

`rust-gate rules` writes the rows from the golden rules of `.github@864d855` at
every commit and keeps what each row says here. A rule added there arrives as
"Not mapped yet", and the daily drift check reports it until it is mapped.

## What this repository protects

The reusable workflows and the `rust-gate` binary every organization
repository runs in its CI, and the publishers that release crates and binaries.
A compromised workflow or gate runs in every caller's CI; untrusted input enters
as caller inputs, pull request contexts and downloaded tools, and the crates.io
token and the signing identity are what an attacker would want. Two
requirements are written but unproven, build provenance and its verification on
the hosted platform, and [controls.md](controls.md) says so along with the
extended controls, SDL, SST, SCH and VR.

## Rule map

| Rule | Held here by |
| --- | --- |
| SEC-001 Minimise sensitive data | **Deterministic.** Gitleaks scans source; reports and logs expose only locations, hashes and review metadata, never matched text. Gate-owned [reviewed exceptions](../ci.md#reviewed-secret-scan-exceptions) require exact repository, path, rule and Match hash; consumer allowlists grant nothing. `an_exact_exception_passes_with_redacted_reports` and `consumer_allowlists_cannot_grant_a_secret_exception` prove redaction and authority. |
| SEC-002 Treat input as data | **Deterministic in part.** Consumer inputs are data: every workflow validates them and none is interpolated into a shell command unquoted. A ruleset run takes its `[ci]` inputs from the base commit, so a pull request cannot loosen its own gate, and `mutation-windows` alone from the head, since it moves mutants to the Windows job and never skips one: `a_run_no_workflow_called_takes_the_ci_table_of_the_base_commit` and `a_head_listing_a_missing_windows_file_is_refused`. |
| SEC-003 Validate boundaries | **Deterministic.** `ci_rejects_unsafe_paths_and_symlinks` and `every_live_publisher_requires_reviewers_and_only_release_tag_deployments` prove traversal, symlink and out-of-scope rejection. |
| SEC-004 Use real authority | **Deterministic.** Each job declares `permissions:`; a test asserts least privilege. Elevated scopes sit in separate callable workflows. |
| SEC-005 Scope sensitive approvals | **Deterministic.** Secret-scan approvals are reviewed in this repository and released with the gate; `a_mismatch_in_each_exception_key_still_fails` binds all four exact keys. All publishers are dry-run by default; live writes require an explicit input, a protected tag and API-verified reviewer/tag rules on the `release` environment, which GitHub Team offers because the organization's repositories are public. |
| SEC-006 Inspect code safely | **Deterministic.** `ci_entry_jobs_accept_fork_pull_requests` and `publisher_entry_jobs_reject_untrusted_pull_request_contexts`: CI runs a fork pull request only because it takes no secret and a read-only token, publishers refuse fork pull requests, and `pull_request_target` is refused everywhere. |
| SEC-007 Stop and escalate incidents | Judgement. The incident path is [SECURITY.md](../../SECURITY.md). |
| SEC-008 Keep truthful evidence | **Deterministic in part.** `ci_evidence_rejects_failed_skipped_and_missing_provenance` refuses to emit a success receipt for a failed, skipped or missing outcome. The scorecard reports what is *not* active as well as what is. |
| SEC-009 Preserve safe progress | Judgement. |
| SEC-010 Report vulnerabilities privately | Organization: private vulnerability reporting is on (`maestrolabs-baseline`), and [SECURITY.md](../../SECURITY.md) routes reports to it. **Deterministic.** The issue chooser offers the private route before any form, and a test asserts it stays there. |
| SEC-011 Sign every release | **Deterministic where available.** `SHA256SUMS` published and re-verified before signing; CycloneDX and SPDX SBOMs for every member; `attest-binaries.yml` signs and verifies build provenance, unproven on the hosted platform (SCH-001 to SCH-004 in [controls.md](controls.md)); [SECURITY.md](../../SECURITY.md) documents how to verify a release. |

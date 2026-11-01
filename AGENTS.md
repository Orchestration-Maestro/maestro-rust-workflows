# maestro-rust-workflows

The shared engineering rules are in [Maestro's AGENTS.md](https://github.com/Orchestration-Maestro/maestro/blob/main/AGENTS.md).
This file holds only instructions specific to the shared CI. Where the two
disagree, Maestro's file wins.

- Accept only the optional `.github/ci.toml` declaration documented in
  `docs/caller-checks.md`, including its single standalone `test_prepare` command;
  no other caller settings or command hooks. The
  directory input is for the self-test, proof, and advisory and fuzz scans; organization
  rules use the root.
  The caller's `.config/nextest.toml` is test-suite runner metadata (such as
  resource groups), not shared-CI settings. Shared-CI partitioning and coverage
  policy stay in the workflow; default test filters are ignored and CI allows
  two retries, naming flaky passes in the summary.
- Test changes at the workflow seam in `.github/workflows/ci-internal.yml`; its
  seam steps run the scripts in `.github/ci-internal/`.
  Keep the broken fixtures broken, and outside any root workspace.
- Pin actions by full commit SHA. Do not add a gate program or checks beyond
  [the shared CI specification](https://github.com/Orchestration-Maestro/.github/blob/main/docs/specs/2026-10-04-shared-rust-ci.md).
- Release with release-please; organization rules own callers' release pins.

## Agent skills

### Issue tracker

Issues live in this repository's GitHub Issues. See `docs/agents/issue-tracker.md`.

### Triage labels

Use Matt Pocock's five default triage labels. See `docs/agents/triage-labels.md`.

### Domain docs

Use [CONTEXT.md](CONTEXT.md) for this repository's terms.

Single-context layout. See `docs/agents/domain.md`.

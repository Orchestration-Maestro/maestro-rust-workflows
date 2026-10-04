# maestro-rust-workflows

The shared CI checks only formatting, Clippy warnings and tests. Its spec is
[One shared Rust CI](https://github.com/Orchestration-Maestro/.github/blob/main/docs/specs/2026-10-04-shared-rust-ci.md).

- Keep consumer settings out of the workflow. The directory input is for its
  self-test; organization rules use the repository root.
- Test changes at the workflow seam in `.github/workflows/ci-internal.yml`.
  Keep the broken fixtures broken, and outside any root workspace.
- Pin actions by full commit SHA. Do not add a gate program or extra checks.
- Release with release-please; organization rules own consumer version pins.

## Agent skills

### Issue tracker

Issues live in this repository's GitHub Issues. See `docs/agents/issue-tracker.md`.

### Triage labels

Use Matt Pocock's five default triage labels. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context layout. See `docs/agents/domain.md`.

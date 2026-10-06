# maestro-rust-workflows

The shared CI always checks formatting, Clippy warnings, documentation and
tests. The optional caller declaration adds isolated tests, real search tools,
wasm builds, native foreign-target checks and the fixed browser recipe hook. Its spec is
[One shared Rust CI](https://github.com/Orchestration-Maestro/.github/blob/main/docs/specs/2026-10-04-shared-rust-ci.md).

- Accept only the optional `.github/ci.toml` declaration documented in
  `docs/caller-checks.md`; no other consumer settings or command hooks. The
  directory input is for the self-test; organization rules use the root.
- Test changes at the workflow seam in `.github/workflows/ci-internal.yml`.
  Keep the broken fixtures broken, and outside any root workspace.
- Pin actions by full commit SHA. Do not add a gate program or checks beyond the specification.
- Release with release-please; organization rules own consumer version pins.

## Agent skills

### Issue tracker

Issues live in this repository's GitHub Issues. See `docs/agents/issue-tracker.md`.

### Triage labels

Use Matt Pocock's five default triage labels. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context layout. See `docs/agents/domain.md`.

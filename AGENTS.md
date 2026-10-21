# maestro-rust-workflows

The shared CI always checks formatting, Clippy warnings, documentation and
tests. One additional required job checks licenses, dependency use, spelling,
secrets, workflows and at least 80% line coverage, with embedded defaults and
checksum-verified tool releases. Only the caller declaration may extend those
defaults or raise coverage. The optional caller declaration adds isolated tests, real search tools,
caller-pinned build and test tools, additional locked manifest provisioning, wasm builds, native foreign-target checks, the
fixed browser build recipe hook, a warm test filter for compile-starting tests, and a [nightly fuzz job](docs/caller-checks.md#nightly-fuzz-job) for declared targets. Optional [Chromium browser tests](docs/caller-checks.md#chromium-browser-tests)
use shared verified runtime pins, not caller-selected browser versions. Its spec is
[One shared Rust CI](https://github.com/Orchestration-Maestro/.github/blob/main/docs/specs/2026-10-04-shared-rust-ci.md).

- Accept only the optional `.github/ci.toml` declaration documented in
  `docs/caller-checks.md`; no other consumer settings or command hooks. The
  directory input is for the self-test, proof, and advisory and fuzz scans; organization
  rules use the root.
  The caller's `.config/nextest.toml` is test-suite runner metadata (such as
  resource groups), not shared-CI settings. Shared-CI partitioning and coverage
  policy stay in the workflow; default test filters are ignored and CI allows
  two retries, naming flaky passes in the summary. PRs select changed crates
  and reverse dependencies; merge groups/main pushes run full coverage.
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

Use [CONTEXT.md](CONTEXT.md) for this repository's terms.

Single-context layout. See `docs/agents/domain.md`.

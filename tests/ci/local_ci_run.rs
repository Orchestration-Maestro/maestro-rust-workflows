//! `rust-gate ci --local`: the `checks` job of `ci.yml` before a push, every
//! step in its order and environment, over the commit CI would check. Most
//! tests use a stand-in that records the environment; the unset-input regression
//! delegates `mutants` to the real gate.

use crate::harness::{Fixture, gate_bin, refused, succeeds, workflow, write_executable};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::process::Output;

/// What the stand-in records of each step's environment.
const RECORDED: &str = "GITHUB_BASE_REF|GITHUB_HEAD_REF|GITHUB_EVENT_NAME|GITHUB_SHA|\
                        GITHUB_ACTIONS|GITHUB_REPOSITORY|\
                        PULL_REQUEST_TITLE|CALLED|RUSTFLAGS|PROJECT|CI|OUT_[A-Z_]*|[A-Z_]*_APPLIED";

/// A fixture whose project is a git repository on `main` with an `origin`,
/// the toolbelt cached, and a stand-in gate for every step: it records its
/// step, the variables [`RECORDED`] names and its PATH's first entry,
/// exports what `validate` and `registry` export, and fails the step a
/// `fail-<id>` file names.
fn repository() -> Fixture {
    let mut fixture = Fixture::new();
    fixture.cached_mise("");
    let root = fixture.root.display().to_string();
    write_executable(
        &fixture.root.join("bin/rust-gate"),
        &format!(
            r#"#!/bin/bash
set -euo pipefail
recorded=$(env | grep -E '^({RECORDED})=' | sort | tr '\n' ' ')
first=${{PATH%%:*}}
built=$([[ -e "$RUNNER_TEMP/rust-target/built" ]] && echo built || echo fresh)
printf '%s %s| %s| %s| %s\n' "$1" "$(pwd)" "$recorded" "$first" "$built" >> {root}/steps
case $1 in
  validate)
    printf 'PROJECT=%s\nREPORTS=%s\nCARGO_TARGET_DIR=%s\nCARGO_BUILD_TARGET=%s\n' \
      "$GITHUB_WORKSPACE" "$RUNNER_TEMP/rust-reports" "$RUNNER_TEMP/rust-target" \
      x86_64-unknown-linux-gnu >> "$GITHUB_ENV"
    {{ git rev-parse HEAD 'HEAD^{{tree}}'; git rev-parse --verify -q HEAD^1 || echo none; }} \
      > {root}/validated ;;
  registry)
    mkdir -p "$RUNNER_TEMP/cargo-home"
    printf 'CARGO_HOME=%s\n' "$RUNNER_TEMP/cargo-home" >> "$GITHUB_ENV" ;;
  quality)
    kept="$RUNNER_TEMP/rust-target/built"
    {{ [[ Cargo.toml -nt "$kept" ]] && echo newer || echo older; }} >> {root}/sources
    mkdir -p "$RUNNER_TEMP/rust-target" && touch "$kept" ;;
  mutants)
    touch "$RUNNER_TEMP/rust-target/built"
    printf 'applied=false\n' >> "$GITHUB_OUTPUT" ;;
  features) printf 'applied=false\n' >> "$GITHUB_OUTPUT" ;;
esac
if [[ -e {root}/fail-$1 ]]; then echo "the $1 stand-in failed" >&2; exit 3; fi
"#
        ),
    );
    let git = "git -c user.name=t -c user.email=t@t -c init.defaultBranch=main";
    succeeds(&fixture.run_body(&format!(
        "cd project && {git} init -q && {git} add -A && {git} commit -qm 'feat: start' && \
         {git} init -q --bare ../origin.git && {git} remote add origin ../origin.git && \
         {git} push -q origin main && {git} fetch -q origin"
    )));
    // What the developer's shell sets never reaches a step.
    fixture.set("RUSTFLAGS", "-D warnings");
    fixture
}

/// Commit `file` on a new branch `branch` with `subject`.
fn commit_on(fixture: &Fixture, branch: &str, subject: &str, file: &str) {
    succeeds(&fixture.run_body(&format!(
        "cd project && git checkout -q -B {branch} && echo {file} > {file} && git add {file} && \
         git -c user.name=t -c user.email=t@t commit -qm '{subject}'"
    )));
}

/// Run `command`, a `rust-gate` command line, in the project with the gate
/// itself, not the stand-in the steps start.
fn run_locally(fixture: &Fixture, command: &str) -> Output {
    let gate = gate_bin().join("rust-gate");
    fixture.run_body(&format!(
        "cd project && {}{}",
        gate.display(),
        command.strip_prefix("rust-gate").unwrap()
    ))
}

/// Each step the stand-in ran: its id and the line it recorded.
fn steps(fixture: &Fixture) -> Vec<(String, String)> {
    fs::read_to_string(fixture.root.join("steps"))
        .unwrap_or_default()
        .lines()
        .map(|line| {
            let (id, rest) = line.split_once(' ').unwrap();
            (id.to_owned(), rest.to_owned())
        })
        .collect()
}

/// The line the stand-in recorded for `id`.
fn step_line(fixture: &Fixture, id: &str) -> String {
    steps(fixture)
        .into_iter()
        .find(|(step, _)| step == id)
        .map_or_else(|| panic!("{id} never ran"), |(_, line)| line)
}

fn run_real_mutation_step(fixture: &Fixture) {
    let path = fixture.root.join("bin/rust-gate");
    let script = fs::read_to_string(&path).unwrap();
    let prefix = format!(
        r#"#!/bin/bash
set -euo pipefail
if [[ "$1" == mutants ]]; then exec "{}" "$@"; fi
if [[ "$1" == validate ]]; then
  printf 'MUTATION_TEST=true\n' >> "$GITHUB_ENV"
  mkdir -p "$RUNNER_TEMP/rust-reports"
fi
"#,
        gate_bin().join("rust-gate").display()
    );
    let script = script.replacen("#!/bin/bash\nset -euo pipefail\n", &prefix, 1);
    write_executable(&path, &script);
}

/// The steps of the `checks` job of `ci.yml`, in order.
fn checks_job() -> Vec<Value> {
    workflow("ci")["jobs"]["checks"]["steps"]
        .as_array()
        .unwrap()
        .clone()
}

/// The gate steps a local run must start, in order, read from `stdout`
/// against `ci.yml`: every step of the job is announced in its order, and one
/// not started is declared not applied with its reason.
fn announced_steps(stdout: &str) -> Vec<String> {
    let blocks: Vec<&str> = stdout.split("\n==> ").skip(1).collect();
    let job = checks_job();
    assert_eq!(blocks.len(), job.len(), "{stdout}");
    let mut expected = Vec::new();
    for (step, block) in job.iter().zip(&blocks) {
        let name = step["name"].as_str().unwrap();
        assert!(block.starts_with(&format!("{name}\n")), "{name}: {block}");
        let id = step["run"]
            .as_str()
            .and_then(|run| run.trim().strip_prefix("rust-gate "))
            .filter(|id| *id != "install-tools");
        let not_applied = block
            .lines()
            .find_map(|line| line.strip_prefix("not applied locally: "));
        match (id, not_applied) {
            (Some(id), None) => expected.push(id.to_owned()),
            (_, Some(reason)) => assert!(reason.len() > 20, "{name}: {reason}"),
            (None, None) => assert!(
                ["Checkout consumer revision", "Install the settings reader"].contains(&name)
                    || name.starts_with("Restore Cargo registry"),
                "{name} is neither run nor declared not applied: {block}"
            ),
        }
    }
    expected
}

#[test]
fn every_step_of_the_ci_job_runs_locally_or_says_why_not() {
    // The one list of what a local run does is held to ci.yml: a step CI
    // gains and the local run does not know fails this test.
    let fixture = repository();
    fs::write(
        fixture.root.join("project/maestro-quality.toml"),
        "[ci]\nmutation-shards = 0\n",
    )
    .unwrap();
    succeeds(&fixture.run_body(
        "cd project && git add maestro-quality.toml && \
         git -c user.name=t -c user.email=t@t commit -qm 'chore: enable mutation sharding'",
    ));
    let output = run_locally(&fixture, "rust-gate ci --local");
    succeeds(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let ran: Vec<String> = steps(&fixture).into_iter().map(|(id, _)| id).collect();
    assert_eq!(ran, announced_steps(&stdout));
    assert!(ran.iter().any(|id| id == "mutants"));
    assert!(!ran.iter().any(|id| id == "mutants-plan"));
    assert!(stdout.contains(concat!(
        "job mutation-plan: local CI always runs the full mutation suite inline; ",
        "shard planning is remote-only"
    )));
    // Just's formatter runs among the hooks and the private documentation in
    // the quality step, so a local run holds a repository to both.
    assert!(ran.iter().any(|id| id == "hooks") && ran.iter().any(|id| id == "quality"));
    for id in workflow("ci")["jobs"].as_object().unwrap().keys() {
        if id != "checks" {
            assert!(stdout.contains(&format!("job {id}: ")), "{id}: {stdout}");
        }
    }
    assert!(stdout.contains("ci --local: every step CI runs here passed\n"));
    // Each step starts in the checkout, on the toolbelt, with what validate
    // exported, CI's variables and nothing of the developer's shell.
    let checkout = fixture.root.join("cache/maestro/ci").display().to_string();
    let toolbelt = fixture.root.join("cache/maestro/tools/bin");
    let first = format!("| {}|", toolbelt.display());
    for (id, line) in steps(&fixture) {
        assert!(
            line.starts_with(&checkout) && line.contains(&first),
            "{id}: {line}"
        );
        assert!(
            line.contains(" CI=true ")
                && line.contains(" GITHUB_ACTIONS=false ")
                && !line.contains("GITHUB_REPOSITORY=")
                && !line.contains("RUSTFLAGS"),
            "{id}: {line}"
        );
        assert!(
            id == "validate" || line.contains(" PROJECT="),
            "{id}: {line}"
        );
    }
}

#[test]
fn local_ci_runs_when_internal_shard_selftest_is_unset() {
    let mut fixture = repository();
    run_real_mutation_step(&fixture);
    write_executable(
        &fixture.root.join("bin/cargo"),
        concat!(
            "#!/bin/bash\n",
            "set -euo pipefail\n",
            "printf 'WARN No mutants found under the active filters\\n' >&2\n",
        ),
    );
    fixture.env.remove("INTERNAL_SHARD_SELFTEST");
    let output = run_locally(&fixture, "rust-gate ci --local");
    succeeds(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("ci --local: every step CI runs here passed\n"),
        "{stdout}"
    );
    assert!(stdout.contains("scope: full workspace, a first commit"));
}

#[test]
fn a_failing_step_ends_the_local_run_and_the_scorecard_still_says_so() {
    let fixture = repository();
    fs::write(fixture.root.join("fail-quality"), "").unwrap();
    let output = run_locally(&fixture, "rust-gate ci --local");
    refused(&output, "the quality stand-in failed\n");
    refused(
        &output,
        "ci --local: one step failed: Formatting, Clippy and tests",
    );
    let ran: Vec<String> = steps(&fixture).into_iter().map(|(id, _)| id).collect();
    assert_eq!(ran.last().map(String::as_str), Some("scorecard"));
    let after: Vec<&String> = ran.iter().skip_while(|id| *id != "quality").collect();
    assert_eq!(after, ["quality", "scorecard"]);
    // The scorecard is handed every outcome as ci.yml hands it.
    let card = step_line(&fixture, "scorecard");
    let job = checks_job();
    let scorecard = job.iter().find(|step| step["id"] == "scorecard").unwrap();
    for (variable, value) in scorecard["env"].as_object().unwrap() {
        let source = value.as_str().unwrap();
        let Some(id) = source
            .strip_prefix("${{ steps.")
            .and_then(|rest| rest.strip_suffix(".outcome }}"))
        else {
            continue;
        };
        let outcome = match id {
            "quality" => "failure",
            _ if ran.contains(&id.to_owned()) => "success",
            _ => "skipped",
        };
        assert!(
            card.contains(&format!(" {variable}={outcome} ")),
            "{variable}: {card}"
        );
    }
    // --keep-going runs every step whatever failed before it.
    let again = run_locally(&fixture, "rust-gate ci --local --keep-going");
    refused(
        &again,
        "ci --local: one step failed: Formatting, Clippy and tests",
    );
    assert!(step_line(&fixture, "stage").contains('|'));
}

#[test]
fn a_branch_runs_as_its_pull_request_and_the_default_branch_as_a_push() {
    let fixture = repository();
    // The default branch, even without origin/HEAD, runs as a push.
    succeeds(&run_locally(&fixture, "rust-gate ci --local"));
    let head = fs::read_to_string(fixture.root.join("validated")).unwrap();
    let line = step_line(&fixture, "validate");
    assert!(line.contains(" GITHUB_EVENT_NAME=push "), "{line}");
    assert!(line.contains(" CALLED=false "), "{line}");
    assert!(!line.contains("GITHUB_BASE_REF=main"), "{line}");
    // A branch runs as the merge of its commits into where it left main.
    fs::remove_file(fixture.root.join("steps")).unwrap();
    commit_on(&fixture, "feat/probe", "feat: add the probe", "probe");
    commit_on(&fixture, "feat/probe", "test: prove the probe", "proof");
    succeeds(&fixture.run_body("cd project && git remote set-head origin main"));
    succeeds(&run_locally(&fixture, "rust-gate ci --local"));
    let merged = fs::read_to_string(fixture.root.join("validated")).unwrap();
    let merged: Vec<&str> = merged.lines().collect();
    let tree = fixture.run_body("cd project && git rev-parse 'HEAD^{tree}'");
    assert_eq!(merged[2], head.lines().next().unwrap(), "HEAD^1 is main");
    assert_eq!(merged[1], String::from_utf8(tree.stdout).unwrap().trim());
    let line = step_line(&fixture, "validate");
    for set in [
        " GITHUB_EVENT_NAME=pull_request ",
        " GITHUB_BASE_REF=main ",
        " GITHUB_HEAD_REF=feat/probe ",
        &format!(" GITHUB_SHA={} ", merged[0]),
        " PULL_REQUEST_TITLE=feat: add the probe ",
    ] {
        assert!(line.contains(set), "{set}: {line}");
    }
    // The title the pull request will have, when the developer names it.
    let mut titled = fixture;
    fs::remove_file(titled.root.join("steps")).unwrap();
    titled.set("PULL_REQUEST_TITLE", "feat: probe everything");
    succeeds(&run_locally(&titled, "rust-gate ci --local"));
    assert!(step_line(&titled, "validate").contains(" PULL_REQUEST_TITLE=feat: probe everything "));
}

#[test]
fn the_build_directory_of_one_run_serves_the_next() {
    // What CI's cache step restores, a local run keeps: the second run's
    // steps find what the first one built.
    let fixture = repository();
    succeeds(&run_locally(&fixture, "rust-gate ci --local"));
    assert!(step_line(&fixture, "quality").ends_with("| fresh"));
    fs::remove_file(fixture.root.join("steps")).unwrap();
    succeeds(&run_locally(&fixture, "rust-gate ci --local"));
    assert!(step_line(&fixture, "quality").ends_with("| built"));
    let kept: PathBuf = fs::read_dir(fixture.root.join("cache/maestro/ci"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(kept.join("cache/RUNNER_TEMP/rust-target/built").is_file());
    // The checkout is written anew, every file newer than the build a mutant
    // of the last run left, so that build is never taken for these sources.
    let sources = fs::read_to_string(fixture.root.join("sources")).unwrap();
    assert!(!sources.contains("older"), "{sources}");
    // A run stopped midway leaves the build directory in the job; the next
    // run keeps it before it starts afresh.
    fs::rename(
        kept.join("cache/RUNNER_TEMP/rust-target"),
        kept.join("temp/rust-target"),
    )
    .unwrap();
    fs::remove_file(fixture.root.join("steps")).unwrap();
    succeeds(&run_locally(&fixture, "rust-gate ci --local"));
    assert!(step_line(&fixture, "quality").ends_with("| built"));
}

#[test]
fn a_repository_without_a_default_branch_or_of_the_workflows_is_refused() {
    let fixture = repository();
    succeeds(&fixture.run_body("cd project && git remote remove origin"));
    refused(
        &run_locally(&fixture, "rust-gate ci --local"),
        "ci --local: origin names no default branch; name it with git remote set-head origin \
         --auto",
    );
    fs::create_dir_all(fixture.root.join("project/.github/workflows")).unwrap();
    fs::write(
        fixture.root.join("project/.github/workflows/ci.yml"),
        "on:\n  workflow_call:\n",
    )
    .unwrap();
    refused(
        &run_locally(&fixture, "rust-gate ci --local"),
        "ci --local: maestro-rust-workflows runs its checks in just check, not through ci.yml",
    );
    assert!(steps(&fixture).is_empty());
}

//! Qualified coverage selection, default compatibility and same-job profile merging.

use crate::harness::{Fixture, engine_workspace, fixture_git, output, refused, succeeds, workflow};
use serde_json::json;
use std::fs;

/// A real three-package workspace using E04a's source fixture.
fn coverage_workspace() -> Fixture {
    let mut fixture = Fixture::new();
    let project = fixture.root.join("project");
    engine_workspace(&project, true);
    fs::write(project.join("maestro-quality.toml"), "").unwrap();
    fs::write(
        project.join("crates/a/src/lib.rs"),
        concat!(
            "//! Coverage profile union.\n#[cfg(feature=\"engine\")] pub mod engine;\n",
            "pub fn selected() -> u8 {\n    let value = 8;\n    value + 1\n}\n",
            "#[cfg(not(feature=\"engine\"))]\npub fn absent() -> u8 { 3 }\n",
            "#[cfg(test)] mod tests {\n",
            "#[cfg(feature=\"engine\")] #[test] fn selected_is_nine() {\n",
            "assert_eq!(super::selected(), 9); }\n",
            "#[cfg(not(feature=\"engine\"))] #[test] fn absent_is_three() {\n",
            "assert_eq!(super::absent(), 3); }\n}\n"
        ),
    )
    .unwrap();
    fixture_git(&project, &["init", "--quiet"]);
    fixture_git(&project, &["config", "user.name", "Fixture"]);
    fixture_git(
        &project,
        &["config", "user.email", "fixture@example.invalid"],
    );
    // Commit an empty library first, so the diff covers every meaningful new line.
    let source = fs::read_to_string(project.join("crates/a/src/lib.rs")).unwrap();
    fs::write(project.join("crates/a/src/lib.rs"), "//! Base.\n").unwrap();
    succeeds(&fixture.run_body("cd project && cargo generate-lockfile --offline"));
    fixture_git(&project, &["add", "."]);
    fixture_git(
        &project,
        &["-c", "commit.gpgsign=false", "commit", "-qm", "base"],
    );
    fs::write(project.join("crates/a/src/lib.rs"), source).unwrap();
    fixture_git(&project, &["add", "."]);
    fixture_git(
        &project,
        &["-c", "commit.gpgsign=false", "commit", "-qm", "head"],
    );
    let sha = fixture_git(&project, &["rev-parse", "HEAD"]);
    fixture.set("GITHUB_SHA", &sha);
    fixture.set("GITHUB_WORKSPACE", &project.display().to_string());
    fixture.set("DIRECTORY", ".");
    fixture.set(
        "CARGO_TARGET_DIR",
        &fixture.root.join("target").display().to_string(),
    );
    fixture.set("GITHUB_BASE_REF", "base");
    fixture.set("PULL_REQUEST_TITLE", "feat: feature coverage");
    fixture_git(
        &project,
        &["update-ref", "refs/remotes/origin/base", "HEAD^1"],
    );
    succeeds(&fixture.run_body("cd project && cargo generate-lockfile --offline"));
    fixture
}

#[test]
#[ignore = "real coverage replay: just check runs example_gate with nocapture"]
fn example_gate_merges_profiles_and_preserves_feature_absent_lines() {
    let mut fixture = coverage_workspace();
    // Default execution has uncovered selected() lines; threshold enforcement is
    // tested below, independently of the unchanged 90% workspace line floor.
    let default = fixture.run("ci", "coverage");
    assert!(!default.status.success());
    refused(&fixture.run("ci", "changed-coverage"), "changed-coverage:");
    println!("RED: default changed-line coverage refuses unexecuted selected() lines");
    fixture.set("COVERAGE_FEATURES", "[\"crate-a/engine\"]");
    succeeds(&fixture.run("ci", "coverage"));
    succeeds(&fixture.run("ci", "changed-coverage"));
    let lcov = fs::read_to_string(fixture.root.join("reports/coverage.lcov")).unwrap();
    assert!(lcov.contains("engine_answer"), "{lcov}");
    assert!(lcov.contains("FNDA:1,_RN"), "{lcov}");
    assert!(
        lcov.contains("DA:8,1"),
        "default-only body was lost: {lcov}"
    );
    assert!(lcov.contains("DA:4,1") && lcov.contains("DA:5,1"), "{lcov}");
    assert!(
        lcov.contains("absent"),
        "default-only function was lost: {lcov}"
    );
    let binding = fs::read_to_string(fixture.root.join("reports/coverage-binding.txt")).unwrap();
    assert!(binding.contains(&fixture.env["GITHUB_SHA"]));
    assert!(binding.contains("crate-a/engine"));
    let summary = fs::read_to_string(fixture.root.join("summary")).unwrap();
    assert!(summary.contains(&binding));
    println!("GREEN: {binding}");
}

/// Stub Cargo metadata for selection validation, with a nonmember dependency.
fn selection_fixture() -> Fixture {
    let mut fixture = Fixture::new();
    fixture.set(
        "METADATA",
        &json!({"workspace_members":["member"], "packages":[
            {"id":"member", "name":"fixture", "features":{"engine":[], "other":[]}},
            {"id":"dependency", "name":"dependency", "features":{"engine":[]}}
        ]})
        .to_string(),
    );
    fixture.stub(
        "cargo",
        "[[ $1 == metadata ]] && printf '%s' \"$METADATA\"; exit 0",
    );
    fixture
}

#[test]
fn invalid_coverage_selections_are_refused_before_any_tests_run() {
    for (value, message) in [
        ("[]", "coverage-features must not be empty"),
        ("null", "coverage-features must be a JSON array of strings"),
        ("[1]", "coverage-features must be a JSON array of strings"),
        (
            "[\"fixture/engine\\n\"]",
            "coverage-features must be a JSON array of strings",
        ),
        (
            "[\"fixture/engine\\r\"]",
            "coverage-features must be a JSON array of strings",
        ),
        (
            "[\"engine\"]",
            "coverage-features entry `engine` must be package/feature",
        ),
        (
            "[\"fixture/engine/other\"]",
            "coverage-features entry `fixture/engine/other` must be package/feature",
        ),
        (
            "[\"fixture/engine\",\"fixture/engine\"]",
            "coverage-features entry `fixture/engine` is duplicated",
        ),
        (
            "[\"dependency/engine\"]",
            "coverage-features package `dependency` is not a workspace member",
        ),
        (
            "[\"fixture/missing\"]",
            "coverage-features package `fixture` does not declare feature `missing`",
        ),
    ] {
        let mut fixture = selection_fixture();
        fixture.set("COVERAGE_FEATURES", value);
        refused(&fixture.run("ci", "validate"), message);
        assert!(!fixture.calls().contains("llvm-cov"));
    }
}

#[test]
fn absent_selection_keeps_the_exact_original_coverage_command() {
    let fixture = Fixture::new();
    fixture.stub(
        "cargo",
        r#"while [[ $# -gt 0 ]]; do
  if [[ $1 == --output-path ]]; then printf 'TN:\nend_of_record\n' > "$2"; fi
  shift
done"#,
    );
    succeeds(&fixture.run("ci", "coverage"));
    assert_eq!(
        fixture.trace().trim(),
        format!(
            concat!(
                "cargo llvm-cov --workspace --locked --lcov --output-path ",
                "{}/reports/coverage.lcov --fail-under-lines 90"
            ),
            fixture.root.display()
        )
    );
    assert!(!fixture.root.join("reports/coverage-binding.txt").exists());
}

#[test]
fn explicit_selection_overrides_head_policy_and_exports_the_resolved_list() {
    let mut fixture = selection_fixture();
    fs::write(
        fixture.root.join("project/maestro-quality.toml"),
        "[ci]\ncoverage-features=['fixture/other']\n",
    )
    .unwrap();
    succeeds(&fixture.run("ci", "validate"));
    assert_eq!(output(&fixture, "coverage-features"), "[\"fixture/other\"]");
    fixture.set("COVERAGE_FEATURES", "[\"fixture/engine\"]");
    succeeds(&fixture.run("ci", "validate"));
    assert!(
        fs::read_to_string(fixture.root.join("environment"))
            .unwrap()
            .lines()
            .any(|line| line == "COVERAGE_FEATURES=[\"fixture/engine\"]")
    );
    // The resolved job environment, not a later raw caller value, reaches coverage.
    fixture.set("COVERAGE_FEATURES", "[\"unknown/engine\"]");
    fixture.stub(
        "git",
        "[[ $1 != rev-parse ]] || printf '%s' \"$GITHUB_SHA\"; exit 0",
    );
    fixture.stub(
        "cargo",
        r#"if [[ $1 == metadata ]]; then printf '%s' "$METADATA"; fi
while [[ $# -gt 0 ]]; do
  if [[ $1 == --output-path ]]; then printf 'TN:\nend_of_record\n' > "$2"; fi
  shift
done"#,
    );
    fs::create_dir_all(fixture.root.join("rust-reports")).unwrap();
    for line in fs::read_to_string(fixture.root.join("environment"))
        .unwrap()
        .lines()
    {
        let (key, value) = line.split_once('=').unwrap();
        fixture.set(key, value);
    }
    succeeds(&fixture.run("ci", "coverage"));
    assert!(fixture.calls().contains("--features fixture/engine"));
}

#[test]
fn every_coverage_caller_transports_the_validated_selection() {
    let ci = workflow("ci");
    let steps = ci["jobs"]["checks"]["steps"].as_array().unwrap();
    let validate = steps.iter().find(|step| step["id"] == "validate").unwrap();
    assert_eq!(
        validate["env"]["COVERAGE_FEATURES"],
        "${{ inputs.coverage-features }}"
    );
    assert_eq!(
        ci["on"]["workflow_call"]["inputs"]["coverage-features"]["default"],
        ""
    );
    // Coverage shares validate's job and receives its GITHUB_ENV export, not
    // the caller's raw value. Both publication workflows must forward the input.
    let coverage = steps.iter().find(|step| step["id"] == "coverage").unwrap();
    assert_eq!(coverage["run"], "rust-gate coverage");
    assert!(coverage["env"]["COVERAGE_FEATURES"].is_null());
    for name in ["publish-binaries", "publish-crate"] {
        let publisher = workflow(name);
        assert_eq!(
            publisher["jobs"]["ci"]["with"]["coverage-features"],
            "${{ inputs.coverage-features }}"
        );
    }
}

#[test]
fn explicit_coverage_override_survives_every_mutation_worker_validation() {
    let mut checks = selection_fixture();
    let policy = "[ci]\ncoverage-features=['fixture/missing']\n";
    fs::write(checks.root.join("project/maestro-quality.toml"), policy).unwrap();
    checks.set("COVERAGE_FEATURES", "[\"fixture/engine\"]");
    succeeds(&checks.run("ci", "validate"));
    let resolved = output(&checks, "coverage-features");
    assert_eq!(resolved, "[\"fixture/engine\"]");

    let ci = workflow("ci");
    for name in ["mutations", "mutation-engine", "mutation-engine-default"] {
        let job = &ci["jobs"][name];
        let validate = job["steps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|step| step["run"] == "rust-gate validate")
            .unwrap();
        let selection = validate["env"]
            .get("COVERAGE_FEATURES")
            .unwrap_or(&job["env"]["COVERAGE_FEATURES"]);
        let mut worker = selection_fixture();
        fs::write(worker.root.join("project/maestro-quality.toml"), policy).unwrap();
        worker.set(
            "COVERAGE_FEATURES",
            if selection == "${{ needs.mutation-plan.outputs.coverage-features }}" {
                &resolved
            } else {
                ""
            },
        );
        let validated = worker.run_body(validate["run"].as_str().unwrap());
        println!("{name} validation: {}", validated.status);
        succeeds(&validated);
        assert_eq!(output(&worker, "coverage-features"), resolved);
        assert_eq!(
            selection,
            "${{ needs.mutation-plan.outputs.coverage-features }}"
        );
    }
    assert_eq!(
        ci["jobs"]["checks"]["outputs"]["coverage-features"],
        "${{ steps.validate.outputs.coverage-features }}"
    );
}

#[test]
fn unbound_or_changed_checkout_cannot_produce_merged_coverage_success() {
    for state in ["foreign-sha", "dirty-source", "drift-during-tests"] {
        let mut fixture = selection_fixture();
        fixture.set("COVERAGE_FEATURES", "[\"fixture/engine\"]");
        fixture.set("BINDING_STATE", state);
        fixture.stub(
            "git",
            r#"if [[ $1 == rev-parse ]]; then
  if [[ $BINDING_STATE == foreign-sha || -f "$RUNNER_TEMP/drift" ]]; then
    printf '%040d\n' 0
  else printf '%s\n' "$GITHUB_SHA"; fi
else
  [[ $BINDING_STATE != dirty-source ]] || printf ' M src/lib.rs\n'
fi
exit 0"#,
        );
        fixture.stub(
            "cargo",
            r#"if [[ $1 == metadata ]]; then printf '%s' "$METADATA"; fi
if [[ $* == *--no-report* && $BINDING_STATE == drift-during-tests ]]; then
  touch "$RUNNER_TEMP/drift"
fi
while [[ $# -gt 0 ]]; do
  if [[ $1 == --output-path ]]; then printf 'TN:\nend_of_record\n' > "$2"; fi
  shift
done"#,
        );
        refused(
            &fixture.run("ci", "coverage"),
            "coverage: checkout does not match the job source SHA",
        );
        assert!(!fixture.root.join("reports/coverage-binding.txt").exists());
        assert!(!fixture.calls().contains("llvm-cov report"));
    }
}

#[test]
fn merged_execution_cleans_profiles_and_evaluates_the_line_floor_once() {
    let mut fixture = selection_fixture();
    fixture.set(
        "COVERAGE_FEATURES",
        "[\"fixture/engine\",\"fixture/other\"]",
    );
    fixture.stub(
        "git",
        "[[ $1 != rev-parse ]] || printf '%s' \"$GITHUB_SHA\"; exit 0",
    );
    fixture.stub(
        "cargo",
        r#"if [[ $1 == metadata ]]; then printf '%s' "$METADATA"; fi
while [[ $# -gt 0 ]]; do
  if [[ $1 == --output-path ]]; then printf 'TN:\nend_of_record\n' > "$2"; fi
  shift
done"#,
    );
    succeeds(&fixture.run("ci", "coverage"));
    let calls = fixture.calls();
    let coverage: Vec<_> = calls
        .lines()
        .filter(|line| line.starts_with("llvm-cov"))
        .collect();
    assert_eq!(
        coverage,
        [
            "llvm-cov clean --workspace".to_owned(),
            "llvm-cov --workspace --locked --no-report".to_owned(),
            "llvm-cov --workspace --locked --no-report --features fixture/engine,fixture/other"
                .to_owned(),
            format!(
                concat!(
                    "llvm-cov report --lcov --output-path ",
                    "{}/reports/coverage.lcov --fail-under-lines 90"
                ),
                fixture.root.display()
            ),
        ]
    );
    assert_eq!(calls.matches("--fail-under-lines").count(), 1);
    // Both tests and the merged report can fail the coverage step. A previous
    // report, even nonempty, never changes their exit status to success.
    fs::write(fixture.root.join("reports/coverage.lcov"), "old report").unwrap();
    fixture.stub(
        "cargo",
        "[[ $1 != metadata ]] || printf '%s' \"$METADATA\"; exit 0",
    );
    assert!(
        !fixture.run("ci", "coverage").status.success(),
        "old report was accepted"
    );
    for failure in ["--no-report", "--features", "report"] {
        fixture.set("COVERAGE_FAILURE", failure);
        fixture.stub(
            "cargo",
            r#"[[ $1 != metadata ]] || { printf '%s' "$METADATA"; exit 0; }
[[ $* != *"$COVERAGE_FAILURE"* ]] || exit 17
exit 0"#,
        );
        assert_eq!(fixture.run("ci", "coverage").status.code(), Some(17));
    }
}

#[test]
fn a_policy_with_nonarray_coverage_selection_is_refused() {
    let fixture = selection_fixture();
    fs::write(
        fixture.root.join("project/maestro-quality.toml"),
        "[ci]\ncoverage-features='fixture/engine'\n",
    )
    .unwrap();
    refused(
        &fixture.run("ci", "validate"),
        "maestro-quality.toml: [ci] coverage-features must be an array of package/feature strings",
    );
}

#[test]
fn uncommitted_source_is_refused_even_with_the_correct_head_sha() {
    let mut fixture = coverage_workspace();
    fixture.set("COVERAGE_FEATURES", "[\"crate-a/engine\"]");
    fs::write(
        fixture.root.join("project/crates/a/src/uncommitted.rs"),
        "pub fn uncommitted() {}\n",
    )
    .unwrap();
    refused(
        &fixture.run("ci", "coverage"),
        "coverage: checkout does not match the job source SHA",
    );
}

#[test]
#[ignore = "real coverage replay: just check runs example_gate with nocapture"]
fn example_gate_native_no_wrapper_keeps_the_default_feature_coverage_union() {
    let mut fixture = coverage_workspace();
    fs::write(
        fixture.root.join("project/maestro-quality.toml"),
        concat!(
            "[native-cache]\nenvironment='FIXTURE_NATIVE_CACHE_DIR'\n",
            "platforms=['linux']\nkey-files=['Cargo.lock']\npublished=['entry-*']\n"
        ),
    )
    .unwrap();
    let project = fixture.root.join("project");
    fixture_git(&project, &["add", "."]);
    fixture_git(
        &project,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "native-policy",
        ],
    );
    fixture.set("GITHUB_SHA", &fixture_git(&project, &["rev-parse", "HEAD"]));
    fixture.set("COVERAGE_FEATURES", "[\"crate-a/engine\"]");
    succeeds(&fixture.run("ci", "coverage"));
    let lcov = fs::read_to_string(fixture.root.join("reports/coverage.lcov")).unwrap();
    assert!(
        lcov.contains("engine_answer") && lcov.contains("absent"),
        "{lcov}"
    );
    for line in ["DA:8,1", "DA:4,1", "DA:5,1"] {
        assert!(lcov.contains(line), "{line}: {lcov}");
    }
    let binding = fs::read_to_string(fixture.root.join("reports/coverage-binding.txt")).unwrap();
    assert!(binding.contains(&fixture.env["GITHUB_SHA"]));
    assert_eq!(binding.matches("--no-rustc-wrapper").count(), 2);
    assert_eq!(fixture.trace().matches("--fail-under-lines 90").count(), 1);
    println!("GREEN native no-wrapper: same-source default/feature union passes the 90% floor");
}

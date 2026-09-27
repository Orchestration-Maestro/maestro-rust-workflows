//! Portability: a caller names extra platforms, the gate turns them into a
//! matrix of pinned runners, and the required status holds the run to them.

use crate::harness::{Fixture, refused, succeeds, workflow};
use serde_json::json;
use std::fs;

fn outputs(fixture: &Fixture) -> String {
    fs::read_to_string(fixture.root.join("output")).unwrap_or_default()
}

#[test]
fn named_platforms_become_a_matrix_of_pinned_runners() {
    let mut fixture = Fixture::new();
    fixture.set("PLATFORMS", "macos windows linux-arm");
    succeeds(&fixture.run("ci", "validate"));
    let written = outputs(&fixture);
    assert!(
        written
            .lines()
            .any(|line| line == r#"platforms=["macos-15","windows-2025","ubuntu-24.04-arm"]"#),
        "{written}"
    );
    assert!(written.lines().any(|line| line == "toolchain=1.98.1"));

    // Nothing named, nothing to run: the portability job is skipped.
    let none = Fixture::new();
    succeeds(&none.run("ci", "validate"));
    assert!(outputs(&none).lines().any(|line| line == "platforms="));

    // Refused before any output, like every other input.
    for (value, message) in [
        (
            "freebsd",
            "platforms may name only macos, windows and linux-arm",
        ),
        ("macos macos", "platforms names macos twice"),
        ("macos\nINJECT=1", "platforms may name only"),
    ] {
        let mut bad = Fixture::new();
        bad.set("PLATFORMS", value);
        refused(&bad.run("ci", "validate"), message);
        assert!(!bad.root.join("output").exists(), "{value:?}");
    }
}

#[test]
fn requested_platforms_must_pass_for_the_required_status() {
    let mut fixture = Fixture::new();
    for (key, value) in [
        ("MUTATION_TEST", "false"),
        ("MUTATION_MODE", "disabled"),
        ("MUTATION_COUNT", "0"),
        ("MUTATION_SHARDS", "0"),
        ("MUTATION_MATRIX", "[]"),
        ("MUTATIONS_RESULT", "skipped"),
        ("MUTATION_SUMMARY_RESULT", "skipped"),
        ("MUTATION_ATTEMPT", "1"),
    ] {
        fixture.set(key, value);
    }
    fixture.set("RESULT", "success");
    fixture.set("RUNNERS", r#"["macos-15"]"#);
    for status in ["failure", "cancelled", "skipped", ""] {
        fixture.set("PORTABILITY", status);
        refused(
            &fixture.run("ci", "required"),
            "Portability checks failed or were skipped",
        );
    }
    fixture.set("PORTABILITY", "success");
    succeeds(&fixture.run("ci", "required"));
    // No platform named: the skipped job is what was asked for.
    fixture.set("RUNNERS", "");
    fixture.set("PORTABILITY", "skipped");
    succeeds(&fixture.run("ci", "required"));
}

#[test]
fn the_portability_job_tests_the_validated_project_on_each_runner() {
    let ci = workflow("ci");
    let job = &ci["jobs"]["portability"];
    // After checks, so the directory and toolchain it uses were validated.
    assert_eq!(job["needs"], json!(["checks"]));
    assert_eq!(job["if"], "${{ needs.checks.outputs.platforms != '' }}");
    assert_eq!(
        job["strategy"]["matrix"]["runner"],
        "${{ fromJSON(needs.checks.outputs.platforms) }}"
    );
    assert_eq!(job["strategy"]["fail-fast"], false);
    assert_eq!(job["runs-on"], "${{ matrix.runner }}");
    assert_eq!(job["permissions"], json!({"contents": "read"}));
    assert_eq!(
        job["env"]["RUSTUP_TOOLCHAIN"],
        "${{ needs.checks.outputs.toolchain }}"
    );
    // A job's own run defaults replace the workflow's, so the job names Bash
    // again: Windows would otherwise run the bodies in PowerShell, where
    // "$RUSTUP_TOOLCHAIN" is not the environment variable.
    assert_eq!(
        job["defaults"]["run"],
        json!({"shell": "bash", "working-directory": "${{ needs.checks.outputs.directory }}"})
    );
    let steps = job["steps"].as_array().unwrap();
    assert!(
        steps[0]["uses"]
            .as_str()
            .unwrap()
            .starts_with("actions/checkout@")
    );
    assert_eq!(steps[0]["with"]["persist-credentials"], false);
    let bodies: Vec<&str> = steps[1..]
        .iter()
        .map(|step| step["run"].as_str().unwrap().trim())
        .collect();
    assert_eq!(
        bodies,
        [
            r#"rustup toolchain install "$RUSTUP_TOOLCHAIN" --profile minimal"#,
            "cargo test --locked --workspace"
        ]
    );
}

#[test]
fn checks_hand_the_runners_to_portability_and_the_result_to_the_required_status() {
    let ci = workflow("ci");
    let checks = &ci["jobs"]["checks"]["outputs"];
    assert_eq!(
        checks["platforms"],
        "${{ steps.validate.outputs.platforms }}"
    );
    assert_eq!(
        checks["toolchain"],
        "${{ steps.validate.outputs.toolchain }}"
    );
    assert_eq!(
        checks["directory"],
        "${{ steps.validate.outputs.directory }}"
    );
    let gate = &ci["jobs"]["gate"];
    assert_eq!(
        gate["needs"],
        json!(["checks", "portability", "mutations", "mutation-summary"])
    );
    let required = gate["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|step| step["id"] == "required")
        .unwrap();
    assert_eq!(
        required["env"]["PORTABILITY"],
        "${{ needs.portability.result }}"
    );
    assert_eq!(
        required["env"]["RUNNERS"],
        "${{ needs.checks.outputs.platforms }}"
    );
    assert_eq!(
        required["env"]["MUTATIONS_RESULT"],
        "${{ needs.mutations.result }}"
    );
    assert_eq!(
        required["env"]["MUTATION_SUMMARY_RESULT"],
        "${{ needs.mutation-summary.result }}"
    );
}

#[test]
fn the_consumer_matrix_proves_every_platform_and_requires_it() {
    // One consumer case names every platform, so each pinned runner builds
    // and tests a real fixture on every pull request here, and the required
    // consumer status waits for it.
    let internal = workflow("ci-internal");
    let case = &internal["jobs"]["portability-ci"];
    assert_eq!(case["uses"], "./.github/workflows/ci.yml");
    assert_eq!(case["with"]["platforms"], "macos windows linux-arm");
    let required = &internal["jobs"]["required"];
    assert!(
        required["needs"]
            .as_array()
            .unwrap()
            .contains(&json!("portability-ci"))
    );
    assert_eq!(
        required["steps"][0]["env"]["PORTABILITY_RESULT"],
        "${{ needs.portability-ci.result }}"
    );
}

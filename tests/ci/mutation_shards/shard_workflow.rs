//! Workflow routing, required status and final scorecard contract tests.

use crate::harness::{Fixture, SCORECARD_OUTCOMES, describe_text, refused, succeeds, workflow};
use serde_json::{Value, json};
use std::fs;

fn scorecard_fixture(scorecard: &Value, state: &str) -> Fixture {
    let mut fixture = Fixture::new();
    fixture.set("MUTATION_STATE", state);
    fs::write(
        fixture.root.join("reports/scorecard.json"),
        serde_json::to_vec(&scorecard).unwrap(),
    )
    .unwrap();
    fixture
}

fn assert_required_gate_wiring(ci: &Value) {
    let jobs = &ci["jobs"];
    for name in ["checks", "portability", "mutations", "mutation-summary"] {
        assert!(
            jobs["gate"]["needs"]
                .as_array()
                .unwrap()
                .contains(&json!(name))
        );
    }
    for output in ["artifact-id", "artifact-name", "revision"] {
        assert_eq!(
            jobs["gate"]["outputs"][output],
            format!(
                concat!(
                    "${{{{ steps.required.outcome == 'success' && ",
                    "needs.checks.outputs.{} || '' }}}}"
                ),
                output
            )
        );
    }
}

fn assert_matrix_and_summary_contract(ci: &Value) {
    let jobs = &ci["jobs"];
    let workers = &jobs["mutations"];
    assert_eq!(workers["strategy"]["fail-fast"], false);
    assert_eq!(workers["strategy"]["max-parallel"], 8);
    assert!(
        workers["if"]
            .as_str()
            .unwrap()
            .contains("mutation-mode == 'sharded'")
    );
    assert_eq!(
        workers["strategy"]["matrix"]["shard"],
        "${{ fromJSON(needs.checks.outputs.mutation-matrix) }}"
    );
    let worker_steps = workers["steps"].as_array().unwrap();
    let execution = worker_steps
        .iter()
        .find(|step| step["id"] == "mutation-worker-run")
        .unwrap();
    assert_eq!(execution["timeout-minutes"], 35);
    assert_eq!(execution["run"], "rust-gate mutants");
    assert!(worker_steps.iter().any(|step| {
        step["uses"]
            .as_str()
            .unwrap_or_default()
            .starts_with("actions/upload-artifact@")
            && step["if"] == "${{ always() }}"
    }));
    let summary = &jobs["mutation-summary"];
    for name in ["checks", "mutations"] {
        assert!(summary["needs"].as_array().unwrap().contains(&json!(name)));
    }
    assert!(summary["if"].as_str().unwrap().contains("always()"));
    let summary_steps = summary["steps"].as_array().unwrap();
    let aggregate = summary_steps
        .iter()
        .position(|step| step["run"] == "rust-gate mutants-aggregate")
        .unwrap();
    let finalize = summary_steps
        .iter()
        .position(|step| step["run"] == "rust-gate scorecard-finalize")
        .unwrap();
    assert!(aggregate < finalize);
    for name in ["upload", "coverage"] {
        assert!(
            jobs[name]["needs"]
                .as_array()
                .unwrap()
                .contains(&json!("mutation-summary"))
        );
        assert!(
            jobs[name]["if"]
                .as_str()
                .unwrap()
                .contains("needs.mutation-summary.result == 'success'")
        );
    }
}

fn assert_planner_and_consumer_contract(ci: &Value) {
    let mutation_steps = ci["jobs"]["checks"]["steps"].as_array().unwrap();
    let planner = mutation_steps
        .iter()
        .position(|step| step["id"] == "mutants-plan")
        .unwrap();
    let runner = mutation_steps
        .iter()
        .position(|step| step["id"] == "mutants")
        .unwrap();
    assert!(planner < runner);
    assert_eq!(mutation_steps[planner]["run"], "rust-gate mutants-plan");

    let internal = workflow("ci-internal");
    let sharded = &internal["jobs"]["sharded-consumer"];
    assert_eq!(sharded["uses"], "./.github/workflows/ci.yml");
    assert_eq!(sharded["with"]["working-directory"], "examples/workspace");
    assert_eq!(sharded["with"]["mutation-shards"], 2);
    assert_eq!(sharded["with"]["artifact-key"], "sharded-consumer");
    assert!(
        internal["jobs"]["required"]["needs"]
            .as_array()
            .unwrap()
            .contains(&json!("sharded-consumer"))
    );
    let required_steps = internal["jobs"]["required"]["steps"].as_array().unwrap();
    assert!(
        required_steps
            .iter()
            .any(|step| step["env"]["SHARDED_RESULT"] == "${{ needs.sharded-consumer.result }}")
    );
}

#[test]
fn shard_workflow_contract_names_matrix_and_summary_jobs() {
    let ci = workflow("ci");
    assert_required_gate_wiring(&ci);
    assert_matrix_and_summary_contract(&ci);
    assert_planner_and_consumer_contract(&ci);
}

#[test]
fn shard_aggregation_finalizes_the_skipped_scorecard_control() {
    for (state, active, color) in [
        ("passed", true, "#3f7d3f"),
        ("failed", false, "#8c3a2b"),
        ("not-run", false, "#3f7d3f"),
    ] {
        let mut fixture = Fixture::new();
        fixture.set("RUSTUP_TOOLCHAIN", "1.98.1");
        fixture.set("MUTATION_TEST", "true");
        fixture.set("OUT_MUTANTS", "skipped");
        for key in SCORECARD_OUTCOMES {
            fixture.set(key, "success");
        }
        for key in [
            "FEATURES_APPLIED",
            "API_APPLIED",
            "HOOKS_APPLIED",
            "CHANGED_COVERAGE_APPLIED",
            "PULL_REQUEST_APPLIED",
            "PERFORMANCE_APPLIED",
        ] {
            fixture.set(key, "true");
        }
        succeeds(&fixture.run("ci", "scorecard"));
        let before: Value =
            serde_json::from_slice(&fs::read(fixture.root.join("reports/scorecard.json")).unwrap())
                .unwrap();
        assert_eq!(before["controls"][8]["state"], "not-run");
        fixture.set("MUTATION_STATE", state);
        succeeds(&fixture.run("ci", "scorecard-finalize"));
        let after: Value =
            serde_json::from_slice(&fs::read(fixture.root.join("reports/scorecard.json")).unwrap())
                .unwrap();
        assert_eq!(after["controls"][8]["state"], state);
        assert_eq!(after["revision"], before["revision"]);
        assert_eq!(
            after["controls"][7]["state"],
            before["controls"][7]["state"]
        );
        assert_eq!(
            after["active"].as_i64(),
            before["active"].as_i64().map(|n| n + i64::from(active))
        );
        assert!(
            fs::read_to_string(fixture.root.join("reports/scorecard.md"))
                .unwrap()
                .contains(&format!("| mutation testing | optional | {state} |"))
        );
        assert!(
            fs::read_to_string(fixture.root.join("reports/scorecard.svg"))
                .unwrap()
                .contains(color)
        );
    }
}

#[test]
fn scorecard_finalizer_refuses_invalid_mutation_states_and_reports() {
    let mutation_control = json!({
        "control": "mutation testing",
        "kind": "optional",
        "state": "not-run"
    });
    for (controls, revision, expected) in [
        (
            json!([mutation_control.clone()]),
            "sha",
            "MUTATION_STATE must be passed, failed or not-run",
        ),
        (
            json!([{"control":"mutation testing","kind":"optional","state":"unknown"}]),
            "sha",
            "scorecard contains an unknown control state",
        ),
        (
            json!([{"control":"mutation testing","kind":"","state":"not-run"}]),
            "sha",
            "scorecard controls have an invalid shape",
        ),
        (
            json!([]),
            "sha",
            "scorecard has no mutation testing control",
        ),
        (
            json!([mutation_control.clone(), mutation_control.clone()]),
            "sha",
            "scorecard mutation control is duplicated or invalid",
        ),
        (
            json!([mutation_control]),
            "",
            "scorecard revision or toolchain is missing",
        ),
    ] {
        let scorecard = json!({"revision": revision, "toolchain": "1.98.1", "controls": controls});
        let fixture = scorecard_fixture(
            &scorecard,
            if expected.starts_with("MUTATION_STATE") {
                "invalid"
            } else {
                "passed"
            },
        );
        refused(&fixture.run("ci", "scorecard-finalize"), expected);
    }
}

#[test]
fn mutation_steps_and_required_status_refuse_missing_evidence() {
    let description = describe_text();
    for name in ["mutants-plan", "mutants", "mutants-aggregate"] {
        assert!(description.contains(&format!("`{name}`")), "{name}");
    }
    let mut fixture = Fixture::new();
    for (name, value) in [
        ("RESULT", "success"),
        ("PORTABILITY", "skipped"),
        ("RUNNERS", ""),
        ("MUTATION_TEST", "true"),
        ("MUTATION_MODE", "sharded"),
        ("MUTATION_COUNT", "3"),
        ("MUTATIONS_RESULT", "failure"),
        ("MUTATION_SUMMARY_RESULT", "success"),
        ("MUTATION_SHARDS", "2"),
        ("MUTATION_MATRIX", "[0,1]"),
        ("MUTATION_ATTEMPT", "1"),
        ("GITHUB_RUN_ATTEMPT", "1"),
    ] {
        fixture.set(name, value);
    }
    refused(
        &fixture.run("ci", "required"),
        "Mutation shards failed or were incomplete",
    );
    fixture.set("MUTATIONS_RESULT", "success");
    fixture.set("MUTATION_SUMMARY_RESULT", "success");
    fixture.set("MUTATION_MATRIX", "[1,0]");
    refused(
        &fixture.run("ci", "required"),
        "Mutation shard plan is missing or invalid",
    );
    fixture.set("MUTATION_MATRIX", "[0,1]");
    fixture.set("MUTATION_ATTEMPT", "2");
    refused(
        &fixture.run("ci", "required"),
        "Mutation plan belongs to a different run attempt",
    );
    fixture.set("MUTATION_MODE", "inline");
    fixture.set("MUTATION_COUNT", "");
    fixture.set("MUTATION_SHARDS", "1");
    fixture.set("MUTATION_MATRIX", "[]");
    refused(
        &fixture.run("ci", "required"),
        "Mutation jobs were not intentionally skipped",
    );
}

#[test]
fn internal_required_status_rejects_any_skipped_consumer() {
    let mut fixture = Fixture::new();
    let consumers = [
        "CI_RESULT",
        "SHARDED_RESULT",
        "BINARY_RESULT",
        "CRATE_RESULT",
        "PORTABILITY_RESULT",
    ];
    for key in consumers {
        fixture.set(key, "success");
    }
    succeeds(&fixture.run("ci-internal", "required"));
    for key in consumers {
        fixture.set(key, "skipped");
        assert!(!fixture.run("ci-internal", "required").status.success());
        fixture.set(key, "success");
    }
}

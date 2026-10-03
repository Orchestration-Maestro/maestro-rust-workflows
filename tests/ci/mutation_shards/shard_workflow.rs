//! Required mutation status and final scorecard contract tests.

use crate::harness::{Fixture, SCORECARD_OUTCOMES, describe_text, refused, succeeds, workflow};
use serde_json::{Value, json};
use std::fs;

#[test]
fn early_planning_uploads_the_complete_immutable_reports_before_execution() {
    let ci = workflow("ci");
    let jobs = &ci["jobs"];
    let plan = &jobs["mutation-plan"];
    assert!(plan.get("needs").is_none());
    assert_eq!(plan["if"], jobs["checks"]["if"]);
    assert_eq!(plan["permissions"], json!({"contents":"read"}));
    let steps = plan["steps"].as_array().unwrap();
    let positions: Vec<_> = [
        "validate",
        "registry",
        "tools",
        "mutants-plan",
        "plan-upload",
    ]
    .map(|id| steps.iter().position(|step| step["id"] == id).unwrap())
    .into();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(steps[positions[3]]["run"], "rust-gate mutants-plan");
    let checks = jobs["checks"]["steps"].as_array().unwrap();
    assert_eq!(
        steps[positions[0]],
        *checks.iter().find(|step| step["id"] == "validate").unwrap()
    );
    assert_eq!(
        steps[2]["with"],
        json!({"ref":"${{ github.sha }}", "persist-credentials":false, "fetch-depth":2})
    );
    let upload = &steps[positions[4]];
    assert_eq!(
        upload["with"]["name"],
        "${{ steps.validate.outputs.artifact-name }}-plan"
    );
    assert_eq!(upload["with"]["path"], "${{ runner.temp }}/rust-reports/");
    assert_eq!(upload["with"]["if-no-files-found"], "error");
}

#[test]
fn early_planning_releases_every_mutation_worker_before_checks_finish() {
    let ci = workflow("ci");
    let jobs = &ci["jobs"];
    for name in [
        "mutations",
        "mutation-engine",
        "mutation-engine-default",
        "mutation-windows",
    ] {
        let worker = &jobs[name];
        assert_eq!(worker["needs"], json!(["mutation-plan"]), "{name}");
        assert!(
            worker["if"]
                .as_str()
                .unwrap()
                .contains("needs.mutation-plan.result == 'success'")
        );
        assert!(!worker.to_string().contains("needs.checks"), "{name}");
    }
    assert_eq!(jobs["checks"]["needs"], json!(["mutation-plan"]));
    let checks = jobs["checks"]["steps"].as_array().unwrap();
    assert!(
        !checks
            .iter()
            .any(|step| step["run"] == "rust-gate mutants-plan")
    );
    let download = checks
        .iter()
        .find(|step| step["id"] == "checks-plan-download")
        .unwrap();
    assert_eq!(
        download["with"]["name"],
        "${{ needs.mutation-plan.outputs.artifact-name }}-plan"
    );
    assert_eq!(download["with"]["path"], "${{ runner.temp }}/rust-reports");
    for name in ["gate", "mutation-summary"] {
        assert!(
            jobs[name]["needs"]
                .as_array()
                .unwrap()
                .contains(&json!("mutation-plan"))
        );
    }
}

#[test]
fn failed_planning_cannot_skip_the_required_status() {
    let ci = workflow("ci");
    let jobs = &ci["jobs"];
    assert_eq!(
        jobs["gate"]["if"],
        "${{ always() && (inputs.artifact-key != '' || github.repository_id != '1382744803') }}"
    );
    assert_eq!(jobs["checks"]["needs"], json!(["mutation-plan"]));
    let required = jobs["gate"]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|step| step["id"] == "required")
        .unwrap();
    assert_eq!(required["env"]["RESULT"], "${{ needs.checks.result }}");
    // Checks depend on successful planning, so each unsuccessful plan skips them.
    for planner_result in ["failure", "cancelled", "skipped"] {
        let needs = json!({
            "mutation-plan": {"result": planner_result},
            "checks": {"result": "skipped"}
        });
        let mut fixture = Fixture::new();
        fixture.set("RESULT", needs["checks"]["result"].as_str().unwrap());
        refused(
            &fixture.run("ci", "required"),
            "Required Rust checks failed or were skipped",
        );
    }
}

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
        "Mutation plan belongs to a different run attempt; Re-run all jobs",
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
fn internal_shard_selftest_requires_exactly_two_shards() {
    let mut fixture = Fixture::new();
    for (name, value) in [
        ("RESULT", "success"),
        ("PORTABILITY", "skipped"),
        ("RUNNERS", ""),
        ("MUTATION_TEST", "true"),
        ("MUTATION_MODE", "sharded"),
        ("MUTATION_COUNT", "2"),
        ("MUTATIONS_RESULT", "success"),
        ("MUTATION_SUMMARY_RESULT", "success"),
        ("MUTATION_SHARDS", "2"),
        ("MUTATION_MATRIX", "[0,1]"),
        ("MUTATION_ATTEMPT", "1"),
        ("GITHUB_RUN_ATTEMPT", "1"),
        ("INTERNAL_SHARD_SELFTEST", "true"),
    ] {
        fixture.set(name, value);
    }
    succeeds(&fixture.run("ci", "required"));
    for (mode, count, shards, matrix) in [("empty", "0", "0", "[]"), ("sharded", "2", "1", "[0]")] {
        fixture.set("MUTATION_MODE", mode);
        fixture.set("MUTATION_COUNT", count);
        fixture.set("MUTATION_SHARDS", shards);
        fixture.set("MUTATION_MATRIX", matrix);
        refused(
            &fixture.run("ci", "required"),
            "internal shard self-test did not run exactly two shards",
        );
    }
}

#[test]
fn one_unplanned_mutation_job_must_still_be_skipped() {
    for (mutations, summary) in [("success", "skipped"), ("skipped", "success")] {
        let mut fixture = Fixture::new();
        for (name, value) in [
            ("RESULT", "success"),
            ("RUNNERS", ""),
            ("MUTATION_TEST", "true"),
            ("MUTATION_MODE", "inline"),
            ("MUTATION_COUNT", ""),
            ("MUTATIONS_RESULT", mutations),
            ("MUTATION_SUMMARY_RESULT", summary),
            ("MUTATION_SHARDS", "1"),
            ("MUTATION_MATRIX", "[]"),
        ] {
            fixture.set(name, value);
        }
        refused(
            &fixture.run("ci", "required"),
            "Mutation jobs were not intentionally skipped",
        );
    }
}

#[test]
fn required_status_accepts_a_256_entry_shard_matrix() {
    let mut fixture = Fixture::new();
    let shards = 256;
    let matrix = format!(
        "[{}]",
        (0..shards)
            .map(|index| index.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );
    for (name, value) in [
        ("RESULT", "success".to_owned()),
        ("PORTABILITY", "skipped".to_owned()),
        ("RUNNERS", String::new()),
        ("MUTATION_TEST", "true".to_owned()),
        ("MUTATION_MODE", "sharded".to_owned()),
        ("MUTATION_COUNT", shards.to_string()),
        ("MUTATIONS_RESULT", "success".to_owned()),
        ("MUTATION_SUMMARY_RESULT", "success".to_owned()),
        ("MUTATION_SHARDS", shards.to_string()),
        ("MUTATION_MATRIX", matrix),
        ("MUTATION_ATTEMPT", "1".to_owned()),
        ("GITHUB_RUN_ATTEMPT", "1".to_owned()),
    ] {
        fixture.set(name, &value);
    }
    succeeds(&fixture.run("ci", "required"));
}

#[test]
fn required_status_rejects_shard_counts_outside_two_to_two_hundred_fifty_six() {
    for shards in [1, 257] {
        let mut fixture = Fixture::new();
        let matrix = format!(
            "[{}]",
            (0..shards)
                .map(|index| index.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
        for (name, value) in [
            ("RESULT", "success".to_owned()),
            ("PORTABILITY", "skipped".to_owned()),
            ("RUNNERS", String::new()),
            ("MUTATION_TEST", "true".to_owned()),
            ("MUTATION_MODE", "sharded".to_owned()),
            ("MUTATION_COUNT", shards.to_string()),
            ("MUTATIONS_RESULT", "success".to_owned()),
            ("MUTATION_SUMMARY_RESULT", "success".to_owned()),
            ("MUTATION_SHARDS", shards.to_string()),
            ("MUTATION_MATRIX", matrix),
            ("MUTATION_ATTEMPT", "1".to_owned()),
            ("GITHUB_RUN_ATTEMPT", "1".to_owned()),
        ] {
            fixture.set(name, &value);
        }
        refused(
            &fixture.run("ci", "required"),
            "Mutation shard plan is missing or invalid",
        );
    }
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

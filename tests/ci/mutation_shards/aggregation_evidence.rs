//! Mutation evidence validation and aggregation tests.

use crate::harness::{
    aggregation_fixture, incomplete_reason, output, refused, shard_outcomes, succeeds,
};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

#[test]
fn aggregation_merges_only_complete_identity_and_outcome_evidence() {
    let complete = aggregation_fixture(true);
    succeeds(&complete.run_body("rust-gate mutants-aggregate"));
    assert_eq!(output(&complete, "mutation-state"), "passed");
    let aggregate_report = fs::read(complete.root.join("reports/mutants.json")).unwrap();
    let outcomes: Value = serde_json::from_slice(&aggregate_report)
        .unwrap_or_else(|error| panic!("{error}: {}", String::from_utf8_lossy(&aggregate_report)));
    assert_eq!(outcomes["total_mutants"], 5);
    assert_eq!(outcomes["caught"], 5);
    assert_eq!(outcomes["missed"], 0);
    assert!(
        fs::read_to_string(complete.root.join("reports/mutants.txt"))
            .unwrap()
            .contains("Aggregate complete: 5 mutants; 2 shards")
    );

    let incomplete = aggregation_fixture(false);
    refused(
        &incomplete.run_body("rust-gate mutants-aggregate"),
        &incomplete_reason(2, 1),
    );
    assert_eq!(output(&incomplete, "mutation-state"), "failed");
    assert!(!incomplete.root.join("reports/mutants.json").exists());
    assert!(
        fs::read_to_string(incomplete.root.join("reports/mutants.txt"))
            .unwrap()
            .contains("Aggregate total unavailable; partial shard diagnostics are retained.")
    );
}

#[test]
fn aggregate_reports_mutants_missing_from_an_incomplete_shard() {
    let fixture = aggregation_fixture(true);
    let path = shard_outcomes(&fixture, 0);
    let mut document: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    document["outcomes"].as_array_mut().unwrap().pop();
    document["end_time"] = serde_json::json!("");
    fs::write(path, serde_json::to_vec(&document).unwrap()).unwrap();

    let result = fixture.run_body("rust-gate mutants-aggregate");
    refused(&result, &incomplete_reason(1, 1));
    assert_eq!(output(&fixture, "mutation-state"), "failed");
    assert!(
        fs::read_to_string(fixture.root.join("reports/mutants.txt"))
            .unwrap()
            .contains("1 mutants untested in 1 shards")
    );
}

#[test]
fn aggregate_creates_its_reports_directory_before_writing() {
    let fixture = aggregation_fixture(true);
    let reports = Path::new(&fixture.env["REPORTS"]);
    fs::remove_dir_all(reports).unwrap();
    succeeds(&fixture.run_body("rust-gate mutants-aggregate"));
    assert!(reports.join("mutants.txt").is_file());
    assert!(reports.join("mutants.json").is_file());
}

#[test]
fn aggregation_rejects_discovery_outside_its_round_robin_slice() {
    let fixture = aggregation_fixture(true);
    let path = Path::new(&fixture.env["MUTATION_ARTIFACTS"])
        .join("fixture-mutants-0-of-2/mutants/mutants.out/mutants.json");
    let mut discovery: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let plan_dir = Path::new(&fixture.env["MUTATION_PLAN_DIR"]);
    let planned: Value =
        serde_json::from_slice(&fs::read(plan_dir.join("mutants-list.json")).unwrap()).unwrap();
    discovery[0] = planned[1].clone();
    fs::write(&path, serde_json::to_vec(&discovery).unwrap()).unwrap();

    let result = fixture.run_body("rust-gate mutants-aggregate");
    refused(&result, &incomplete_reason(0, 1));
    assert!(
        String::from_utf8_lossy(&result.stdout)
            .contains("shard discovery differs from round-robin assignment")
    );
    assert_eq!(output(&fixture, "mutation-state"), "failed");
}

#[test]
fn timeout_and_failed_parent_jobs_never_pass_complete_evidence() {
    let timeout = aggregation_fixture(true);
    let path = shard_outcomes(&timeout, 0);
    let mut document: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    document["outcomes"][1]["summary"] = json!("Timeout");
    document["outcomes"][1]["phase_results"][1]["process_status"] = json!("Timeout");
    document["caught"] = json!(2);
    document["timeout"] = json!(1);
    fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
    let result = timeout.run_body("rust-gate mutants-aggregate");
    refused(
        &result,
        "mutation shard evidence is complete, but a required result failed",
    );
    assert_eq!(output(&timeout, "mutation-state"), "failed");
    let aggregate: Value =
        serde_json::from_slice(&fs::read(timeout.root.join("reports/mutants.json")).unwrap())
            .unwrap();
    assert_eq!(aggregate["timeout"], 1);

    for (name, value) in [
        ("MUTATIONS_RESULT", "failure"),
        ("CHECKS_RESULT", "failure"),
    ] {
        let mut fixture = aggregation_fixture(true);
        fixture.set(name, value);
        refused(
            &fixture.run_body("rust-gate mutants-aggregate"),
            "mutation shard evidence is complete, but a required result failed",
        );
        assert_eq!(output(&fixture, "mutation-state"), "failed", "{name}");
        assert!(
            fixture.root.join("reports/mutants.json").is_file(),
            "{name}"
        );
    }
}

#[test]
fn a_surviving_mutant_fails_even_when_the_worker_returns_success() {
    let fixture = aggregation_fixture(true);
    let path = shard_outcomes(&fixture, 0);
    let mut document: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    document["outcomes"][1]["summary"] = json!("MissedMutant");
    document["outcomes"][1]["phase_results"][1]["process_status"] = json!("Success");
    document["caught"] = json!(2);
    document["missed"] = json!(1);
    fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
    let result = fixture.run_body("rust-gate mutants-aggregate");
    refused(
        &result,
        "mutation shard evidence is complete, but a required result failed",
    );
    assert_eq!(output(&fixture, "mutation-state"), "failed");
    let aggregate: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("reports/mutants.json")).unwrap())
            .unwrap();
    assert_eq!(aggregate["total_mutants"], 5);
    assert_eq!(aggregate["missed"], 1);
}

#[test]
fn a_failed_baseline_never_passes_a_complete_mutation_aggregate() {
    let fixture = aggregation_fixture(true);
    let path = shard_outcomes(&fixture, 0);
    let mut document: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    document["outcomes"][0]["summary"] = json!("Failure");
    document["outcomes"][0]["phase_results"][1]["process_status"] = json!({"Failure": 101});
    fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
    let result = fixture.run_body("rust-gate mutants-aggregate");
    refused(
        &result,
        "mutation shard evidence is complete, but a required result failed",
    );
    assert_eq!(output(&fixture, "mutation-state"), "failed");
    assert!(fs::read(fixture.root.join("reports/mutants.json")).is_ok());
}

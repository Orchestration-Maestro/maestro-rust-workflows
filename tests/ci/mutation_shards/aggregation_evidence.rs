//! Mutation evidence validation and aggregation tests.

use super::common::{aggregation_fixture, output, shard_outcomes};
use crate::harness::{refused, succeeds};
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
        "one or more expected mutation shards are missing or incomplete",
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

#[test]
fn aggregation_refuses_missing_duplicate_foreign_or_partial_results() {
    let duplicate = aggregation_fixture(true);
    let path = shard_outcomes(&duplicate, 0);
    let mut document: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    document["outcomes"][2]["scenario"]["Mutant"] =
        document["outcomes"][1]["scenario"]["Mutant"].clone();
    fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
    let result = duplicate.run_body("rust-gate mutants-aggregate");
    refused(
        &result,
        "one or more expected mutation shards are missing or incomplete",
    );
    assert!(
        String::from_utf8_lossy(&result.stdout)
            .contains("completed outcomes omit or duplicate a planned mutant")
    );

    let inconsistent = aggregation_fixture(true);
    let path = shard_outcomes(&inconsistent, 0);
    let mut document: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    document["caught"] = json!(2);
    fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
    let result = inconsistent.run_body("rust-gate mutants-aggregate");
    refused(
        &result,
        "one or more expected mutation shards are missing or incomplete",
    );
    assert!(
        String::from_utf8_lossy(&result.stdout)
            .contains("outcome counters disagree with completed mutant scenarios")
    );

    let escaping = aggregation_fixture(true);
    let path = shard_outcomes(&escaping, 0);
    let mut document: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    document["outcomes"][1]["log_path"] = json!("../../outside.log");
    fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
    let result = escaping.run_body("rust-gate mutants-aggregate");
    refused(
        &result,
        "one or more expected mutation shards are missing or incomplete",
    );
    assert!(
        String::from_utf8_lossy(&result.stdout)
            .contains("outcomes contain an unsafe log or diff path")
    );

    let foreign = aggregation_fixture(true);
    let foreign_dir = Path::new(&foreign.env["MUTATION_ARTIFACTS"]).join("foreign");
    fs::create_dir_all(&foreign_dir).unwrap();
    fs::write(foreign_dir.join("unexpected.txt"), "not a worker").unwrap();
    refused(
        &foreign.run_body("rust-gate mutants-aggregate"),
        "download contains foreign mutation shard artifacts",
    );

    let partial = aggregation_fixture(true);
    fs::remove_file(shard_outcomes(&partial, 1)).unwrap();
    let result = partial.run_body("rust-gate mutants-aggregate");
    refused(
        &result,
        "one or more expected mutation shards are missing or incomplete",
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("missing mutation evidence"));
}

#[test]
fn aggregate_rejects_outcome_summaries_that_disagree_with_phases() {
    let fixture = aggregation_fixture(true);
    let artifacts = Path::new(&fixture.env["MUTATION_ARTIFACTS"]);
    let path = artifacts.join("fixture-mutants-0-of-2/mutants/mutants.out/outcomes.json");
    let mut document: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    document["outcomes"][1]["phase_results"][1]["process_status"] = json!("Success");
    fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
    let result = fixture.run_body("rust-gate mutants-aggregate");
    refused(
        &result,
        "one or more expected mutation shards are missing or incomplete",
    );
    assert!(
        String::from_utf8_lossy(&result.stdout)
            .contains("outcome summary does not match its phase results")
    );
}

#[test]
fn malformed_outcomes_are_rejected_before_path_scanning() {
    let fixture = aggregation_fixture(true);
    fs::write(shard_outcomes(&fixture, 0), b"{").unwrap();
    let result = fixture.run_body("rust-gate mutants-aggregate");
    refused(
        &result,
        "one or more expected mutation shards are missing or incomplete",
    );
    assert!(
        String::from_utf8_lossy(&result.stdout)
            .contains("outcomes JSON is malformed or has invalid counters")
    );
}

#[test]
fn aggregate_refuses_invalid_plan_metadata_before_accepting_shards() {
    for (name, value, message) in [
        (
            "MUTATION_SHARDS",
            "two",
            "MUTATION_SHARDS is not an integer",
        ),
        (
            "MUTATION_SHARDS",
            "1",
            "sharded aggregation requires 2 through 32 planned shards",
        ),
        (
            "MUTATION_MODE",
            "inline",
            "mutation mode or matrix differs from the complete plan",
        ),
        (
            "MUTATION_MATRIX",
            "[0]",
            "mutation mode or matrix differs from the complete plan",
        ),
        (
            "MUTATION_ARTIFACT_NAME",
            "../fixture",
            "mutation artifact root is not a safe name",
        ),
        (
            "GITHUB_RUN_ATTEMPT",
            "2",
            "sharded plan attempt differs from this run; use Re-run all jobs",
        ),
        (
            "GITHUB_SHA",
            "cccccccccccccccccccccccccccccccccccccccc",
            "mutation plan identity differs from this workflow run",
        ),
    ] {
        let mut fixture = aggregation_fixture(true);
        fixture.set(name, value);
        refused(&fixture.run_body("rust-gate mutants-aggregate"), message);
    }

    let fixture = aggregation_fixture(true);
    let plan_dir = Path::new(&fixture.env["MUTATION_PLAN_DIR"]);
    let path = plan_dir.join("mutation-plan.json");
    let mut plan: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    plan["mutant_count"] = json!(4);
    fs::write(path, serde_json::to_vec(&plan).unwrap()).unwrap();
    refused(
        &fixture.run_body("rust-gate mutants-aggregate"),
        "mutation plan count differs from its filtered listing",
    );
}

#[test]
fn aggregate_rejects_incomplete_shard_documents_and_receipts() {
    for (field, value, message) in [
        (
            "total_mutants",
            json!(4),
            "outcome counters do not cover every assigned mutant",
        ),
        (
            "cargo_mutants_version",
            json!("26.0.0"),
            "outcomes cargo-mutants version differs from the plan",
        ),
        ("end_time", json!(""), "outcomes document is incomplete"),
    ] {
        let fixture = aggregation_fixture(true);
        let path = shard_outcomes(&fixture, 0);
        let mut document: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        document[field] = value;
        fs::write(path, serde_json::to_vec(&document).unwrap()).unwrap();
        let result = fixture.run_body("rust-gate mutants-aggregate");
        refused(
            &result,
            "one or more expected mutation shards are missing or incomplete",
        );
        assert!(String::from_utf8_lossy(&result.stdout).contains(message));
    }

    let fixture = aggregation_fixture(true);
    let receipt = Path::new(&fixture.env["MUTATION_ARTIFACTS"])
        .join("fixture-mutants-0-of-2/rust-reports/mutants-shard.json");
    let mut identity: Value = serde_json::from_slice(&fs::read(&receipt).unwrap()).unwrap();
    identity["shard_index"] = json!(1);
    fs::write(receipt, serde_json::to_vec(&identity).unwrap()).unwrap();
    let result = fixture.run_body("rust-gate mutants-aggregate");
    refused(
        &result,
        "one or more expected mutation shards are missing or incomplete",
    );
    assert!(
        String::from_utf8_lossy(&result.stdout)
            .contains("shard receipt identity or denominator differs")
    );
}

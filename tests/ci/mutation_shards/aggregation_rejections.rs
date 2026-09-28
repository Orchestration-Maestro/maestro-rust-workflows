//! Mutation aggregation refusal cases for incomplete or inconsistent evidence.

use crate::harness::{aggregation_fixture, copy_tree, refused, shard_outcomes};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

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
    let artifacts = Path::new(&foreign.env["MUTATION_ARTIFACTS"]);
    copy_tree(
        &artifacts.join("fixture-mutants-0-of-2"),
        &artifacts.join("foreign"),
    );
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
    let output = String::from_utf8_lossy(&result.stdout);
    assert!(output.contains("missing mutation evidence"));
    assert!(output.contains("Incomplete shard indices: 1"));
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
            "sharded aggregation requires 2 through 64 planned shards",
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

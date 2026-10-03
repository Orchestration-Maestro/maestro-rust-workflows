//! Every default mode requires host execution, aggregation and coverage in the same attempt.

use serde_json::Value;
use std::fs;

use crate::harness::{Fixture, SCORECARD_OUTCOMES, refused, succeeds};

/// A valid default status without a host partition, preserving all existing owners.
fn required_fixture(mode: &str) -> Fixture {
    let mut fixture = Fixture::new();
    for (name, value) in [
        ("RESULT", "success"),
        ("RUNNERS", ""),
        ("MUTATION_TEST", "true"),
        ("MUTATION_MODE", mode),
        ("MUTATION_SUMMARY_RESULT", "skipped"),
        ("MUTATIONS_RESULT", "skipped"),
        ("MUTATION_ATTEMPT", "1"),
        ("MUTATION_HOST_COUNT", "0"),
        ("MUTATION_HOST_FILES", "[]"),
    ] {
        fixture.set(name, value);
    }
    let (count, shards, matrix) = match mode {
        "empty" => ("0", "0", "[]"),
        "inline" => ("1", "1", "[]"),
        _ => ("2", "2", "[0,1]"),
    };
    fixture.set("MUTATION_COUNT", count);
    fixture.set("MUTATION_SHARDS", shards);
    fixture.set("MUTATION_MATRIX", matrix);
    if mode == "sharded" {
        fixture.set("MUTATIONS_RESULT", "success");
        fixture.set("MUTATION_SUMMARY_RESULT", "success");
    }
    fixture
}

#[test]
fn host_status_is_required_for_empty_inline_sharded_and_mixed_owners() {
    for mode in ["empty", "inline", "sharded"] {
        for owners in ["none", "windows", "engine", "both"] {
            let mut fixture = required_fixture(mode);
            if matches!(owners, "windows" | "both") {
                fixture.set("MUTATION_WINDOWS", "[\"src/windows.rs\"]");
                fixture.set("WINDOWS_MUTATIONS_RESULT", "success");
            }
            if matches!(owners, "engine" | "both") {
                fixture.set("MUTATION_ENGINE_FILES", "[\"src/engine.rs\"]");
                fixture.set("MUTATION_ENGINE_COUNT", "1");
                fixture.set("ENGINE_MUTATIONS_RESULT", "success");
                fixture.set("MUTATION_SUMMARY_RESULT", "success");
            }
            succeeds(&fixture.run_body("rust-gate required"));
            fixture.set("MUTATION_HOST_COUNT", "1");
            fixture.set("MUTATION_HOST_FILES", "[\"src/host.rs\"]");
            refused(
                &fixture.run_body("rust-gate required"),
                "provisioned-host mutation result is missing; no host executor ran",
            );
            fixture.set("HOST_MUTATIONS_RESULT", "success");
            fixture.set("MUTATION_SUMMARY_RESULT", "success");
            fixture.set("CHANGED_COVERAGE_STATE", "passed");
            succeeds(&fixture.run_body("rust-gate required"));
            for (name, value) in [
                ("HOST_MUTATIONS_RESULT", "skipped"),
                ("HOST_MUTATIONS_RESULT", "failure"),
                ("MUTATION_SUMMARY_RESULT", "skipped"),
                ("CHANGED_COVERAGE_STATE", "not-run"),
                ("MUTATION_ATTEMPT", "2"),
            ] {
                let original = fixture.env[name].clone();
                fixture.set(name, value);
                refused(
                    &fixture.run_body("rust-gate required"),
                    "provisioned-host execution, aggregation and coverage join must \
                    pass in this attempt",
                );
                fixture.set(name, &original);
            }
        }
    }
}

#[test]
fn configured_host_files_cannot_satisfy_status_with_a_zero_plan() {
    let mut fixture = required_fixture("empty");
    fixture.set("MUTATION_HOST_FILES", "[\"src/host.rs\"]");
    refused(
        &fixture.run_body("rust-gate required"),
        "provisioned-host ownership requires an enabled nonempty mutation plan",
    );
}

#[test]
fn pending_host_credit_is_inactive_until_the_final_join_succeeds() {
    let mut fixture = Fixture::new();
    for outcome in SCORECARD_OUTCOMES {
        fixture.set(outcome, "success");
    }
    fixture.set("MUTATION_TEST", "true");
    fixture.set("MUTANTS_APPLIED", "true");
    fixture.set("MUTATION_HOST_COUNT", "1");
    fixture.set("CHANGED_COVERAGE_APPLIED", "pending-host");
    succeeds(&fixture.run_body("rust-gate scorecard"));
    let card_path = fixture.root.join("reports/scorecard.json");
    let control = |name: &str| -> String {
        let card: Value = serde_json::from_slice(&fs::read(&card_path).unwrap()).unwrap();
        card["controls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["control"] == name)
            .unwrap()["state"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(control("changed-line coverage"), "not-run");
    assert_eq!(control("mutation testing"), "not-run");
    fixture.set("MUTATION_STATE", "passed");
    fixture.set("CHANGED_COVERAGE_STATE", "passed");
    succeeds(&fixture.run_body("rust-gate scorecard-finalize"));
    assert_eq!(control("changed-line coverage"), "passed");
    fixture.set("CHANGED_COVERAGE_STATE", "not-run");
    succeeds(&fixture.run_body("rust-gate scorecard-finalize"));
    assert_eq!(control("changed-line coverage"), "not-run");
}

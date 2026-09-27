//! Mutation worker plan validation and execution tests.

use crate::harness::{Fixture, planning_fixture, refused, succeeds};
use serde_json::{Value, json};
use std::fs;

#[test]
fn workers_reject_wrong_count_scope_shard_count_and_empty_assignments() {
    fn worker() -> Fixture {
        let mut fixture = planning_fixture(5, 2, 1);
        succeeds(&fixture.run_body("rust-gate mutants-plan"));
        fixture.set(
            "MUTATION_PLAN",
            &fixture
                .root
                .join("reports/mutation-plan.json")
                .display()
                .to_string(),
        );
        fixture.set(
            "MUTATION_LIST",
            &fixture
                .root
                .join("reports/mutants-list.json")
                .display()
                .to_string(),
        );
        fixture.set("MUTATION_SHARD", "0/2");
        fixture
    }

    let count_mismatch = worker();
    let manifest = count_mismatch.root.join("reports/mutation-plan.json");
    let mut plan: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    plan["mutant_count"] = json!(4);
    fs::write(manifest, serde_json::to_vec(&plan).unwrap()).unwrap();
    refused(
        &count_mismatch.run_body("rust-gate mutants"),
        "mutation plan count does not match its complete listing",
    );

    let mut shards_mismatch = worker();
    shards_mismatch.set("MUTATION_SHARDS", "3");
    refused(
        &shards_mismatch.run_body("rust-gate mutants"),
        "mutation worker shard count differs from its planned matrix",
    );

    let mut identity_mismatch = worker();
    identity_mismatch.set("GITHUB_SHA", "cccccccccccccccccccccccccccccccccccccccc");
    refused(
        &identity_mismatch.run_body("rust-gate mutants"),
        "mutation worker identity or scope differs from its plan; Re-run all jobs",
    );

    let mut empty_shard = worker();
    let manifest = empty_shard.root.join("reports/mutation-plan.json");
    let mut plan: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    plan["shard_count"] = json!(6);
    fs::write(manifest, serde_json::to_vec(&plan).unwrap()).unwrap();
    empty_shard.set("MUTATION_SHARDS", "6");
    empty_shard.set("MUTATION_SHARD", "5/6");
    refused(
        &empty_shard.run_body("rust-gate mutants"),
        "mutation plan assigns an empty shard",
    );
}

#[test]
fn a_worker_refuses_a_no_work_skip_for_a_nonempty_assignment() {
    let mut fixture = planning_fixture(5, 2, 1);
    succeeds(&fixture.run_body("rust-gate mutants-plan"));
    fixture.set(
        "MUTATION_PLAN",
        &fixture
            .root
            .join("reports/mutation-plan.json")
            .display()
            .to_string(),
    );
    fixture.set(
        "MUTATION_LIST",
        &fixture
            .root
            .join("reports/mutants-list.json")
            .display()
            .to_string(),
    );
    fixture.set("MUTATION_SHARD", "0/2");
    fixture.stub(
        "cargo",
        "echo 'WARN No mutants found under the active filters'; exit 0",
    );
    refused(
        &fixture.run_body("rust-gate mutants"),
        "cargo-mutants reported no work for a shard with planned mutants",
    );
}

#[test]
fn workers_preserve_cargo_mutants_exit_statuses_and_outcomes() {
    for code in [2, 3, 4] {
        let mut fixture = planning_fixture(5, 2, 1);
        succeeds(&fixture.run_body("rust-gate mutants-plan"));
        fixture.set(
            "MUTATION_PLAN",
            &fixture
                .root
                .join("reports/mutation-plan.json")
                .display()
                .to_string(),
        );
        fixture.set(
            "MUTATION_LIST",
            &fixture
                .root
                .join("reports/mutants-list.json")
                .display()
                .to_string(),
        );
        fixture.set("MUTATION_SHARD", "0/2");
        fixture.stub(
            "cargo",
            &format!(
                concat!(
                    "mkdir -p \"$RUNNER_TEMP/mutants/mutants.out\"\n",
                    "printf '{{\"outcomes\":[]}}' > ",
                    "\"$RUNNER_TEMP/mutants/mutants.out/outcomes.json\"\n",
                    "exit {}"
                ),
                code
            ),
        );
        let result = fixture.run_body("rust-gate mutants");
        assert_eq!(result.status.code(), Some(code));
        assert!(fixture.root.join("reports/mutants.json").is_file());
        assert!(
            fixture
                .root
                .join("mutants/mutants.out/outcomes.json")
                .is_file()
        );
    }
}

#[test]
fn shard_workers_require_both_plan_inputs() {
    for name in ["MUTATION_PLAN", "MUTATION_LIST"] {
        let mut fixture = planning_fixture(2, 2, 50);
        succeeds(&fixture.run_body("rust-gate mutants-plan"));
        fixture.set("MUTATION_SHARD", "0/2");
        if name == "MUTATION_LIST" {
            fixture.set(
                "MUTATION_PLAN",
                &fixture
                    .root
                    .join("reports/mutation-plan.json")
                    .display()
                    .to_string(),
            );
        }
        fixture.set(name, "");
        refused(
            &fixture.run_body("rust-gate mutants"),
            &format!("{name} is required for a mutation shard"),
        );
    }
}

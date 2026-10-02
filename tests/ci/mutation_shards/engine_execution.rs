//! Engine worker selection, bound modes and complete execution.

use crate::harness::{engine_fixture, refused, succeeds};
use serde_json::{Value, json};
use std::fs;

#[test]
fn engine_workers_keep_default_obligations_and_refuse_stale_policy_or_incomplete_work() {
    let mut fixture = engine_fixture(true);
    succeeds(&fixture.run_body("rust-gate mutants-engine"));
    succeeds(&fixture.run_body("rust-gate mutants-engine-default"));
    let commands = fixture.trace();
    assert!(
        commands
            .lines()
            .any(|line| line.contains("--features engine --package fixture"))
    );
    assert!(
        commands
            .lines()
            .any(|line| line.contains("--file src/engine.rs") && !line.contains("--features"))
    );
    fixture.set("MUTATION_ENGINE_FEATURES", "[\"other\"]");
    refused(
        &fixture.run_body("rust-gate mutants-engine"),
        "engine worker policy or mode listings differ from its plan",
    );
    fixture.set("MUTATION_ENGINE_FEATURES", "[\"engine\"]");
    let path = fixture.root.join("engine-outcomes.json");
    let mut outcomes: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    outcomes["outcomes"].as_array_mut().unwrap().pop();
    fs::write(path, outcomes.to_string()).unwrap();
    refused(
        &fixture.run_body("rust-gate mutants-engine"),
        "mutation outcomes do not equal their complete mode-aware plan",
    );
    fixture.set("MUTATION_TEST", "false");
    refused(
        &fixture.run_body("rust-gate mutants-engine"),
        "engine mutation evidence cannot be skipped",
    );
    fixture.set("MUTATION_TEST", "true");
    fixture.set("MUTATION_SHARDS", "2");
    refused(
        &fixture.run_body("rust-gate mutants-engine"),
        "engine mode worker differs from its planned shard matrix",
    );
    fixture.set("MUTATION_SHARDS", "1");
    let manifest_path = fixture.root.join("reports/mutation-engine-plan.json");
    let saved = fs::read(&manifest_path).unwrap();
    let mut manifest: Value = serde_json::from_slice(&saved).unwrap();
    manifest["shard_count"] = Value::Null;
    fs::write(&manifest_path, manifest.to_string()).unwrap();
    refused(
        &fixture.run_body("rust-gate mutants-engine"),
        "engine manifest shard count is invalid",
    );
    manifest["shard_count"] = json!(2);
    manifest["engine_shards"] = json!(2);
    fixture.set("MUTATION_SHARDS", "2");
    fixture.set("MUTATION_SHARD", "0/2");
    fs::write(&manifest_path, manifest.to_string()).unwrap();
    refused(
        &fixture.run_body("rust-gate mutants-engine"),
        "engine mode plan assigns an empty shard",
    );
    fs::write(&manifest_path, saved).unwrap();
}

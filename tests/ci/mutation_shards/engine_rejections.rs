//! Plan, artifact and outcome corruption never becomes engine gate success.

use crate::harness::{engine_aggregation_fixture, refused, succeeds, summarize_engine, tool};
use serde_json::{Value, json};
use std::fs;

#[test]
fn mode_aggregation_refuses_plan_drift_missing_shards_and_failed_baselines() {
    let mut fixture = engine_aggregation_fixture();
    for (variable, value, message) in [
        (
            "CHECKS_RESULT",
            "failure",
            "default mutation checks failed or were skipped",
        ),
        (
            "MUTATION_MODE",
            "disabled",
            "engine aggregation requires a valid default plan mode",
        ),
        (
            "MUTATION_ENGINE_FEATURES",
            "[\"other\"]",
            "engine plan policy, source identity or mode listing digest differs",
        ),
        (
            "MUTATION_ENGINE_MATRIX",
            "[]",
            "engine mode matrix differs from the complete planned obligations",
        ),
        (
            "WINDOWS_MUTATIONS_RESULT",
            "success",
            "unconfigured Windows evidence was not skipped",
        ),
    ] {
        let saved = fixture.env[variable].clone();
        fixture.set(variable, value);
        refused(&summarize_engine(&fixture), message);
        fixture.set(variable, &saved);
    }
    let manifest = fixture.root.join("checks/mutation-plan.json");
    let saved = fs::read(&manifest).unwrap();
    let mut changed: Value = serde_json::from_slice(&saved).unwrap();
    changed["mutant_count"] = json!(2);
    fs::write(&manifest, changed.to_string()).unwrap();
    refused(
        &summarize_engine(&fixture),
        "default partition plan count differs from its complete listing",
    );
    fs::write(&manifest, saved).unwrap();
    let manifest = fixture.root.join("checks/mutation-engine-plan.json");
    let saved = fs::read(&manifest).unwrap();
    let mut changed: Value = serde_json::from_slice(&saved).unwrap();
    changed["packages"] = json!([]);
    fs::write(&manifest, changed.to_string()).unwrap();
    refused(
        &summarize_engine(&fixture),
        "engine plan contains mutants outside its exact owners",
    );
    fs::write(&manifest, saved).unwrap();
    let artifact = fixture.root.join("engine/fixture-engine-mutants-0");
    fs::rename(&artifact, fixture.root.join("engine/extra")).unwrap();
    refused(
        &summarize_engine(&fixture),
        "engine mode has missing or unexpected shard artifacts",
    );
    fs::rename(fixture.root.join("engine/extra"), &artifact).unwrap();
    let receipt = artifact.join("rust-reports/mutants-engine-shard.json");
    let saved = fs::read(&receipt).unwrap();
    let mut changed: Value = serde_json::from_slice(&saved).unwrap();
    changed["attempt"] = json!("stale");
    fs::write(&receipt, changed.to_string()).unwrap();
    refused(
        &summarize_engine(&fixture),
        "engine mode receipt differs from its complete policy and plan",
    );
    fs::write(&receipt, saved).unwrap();
    succeeds(&summarize_engine(&fixture));
}

#[test]
fn mode_aggregation_refuses_failed_baselines_survivors_and_untested_default_mutants() {
    let fixture = engine_aggregation_fixture();
    let artifact = fixture.root.join("engine/fixture-engine-mutants-0");
    let outcomes = artifact.join("mutants-engine/mutants.out/outcomes.json");
    let saved = fs::read(&outcomes).unwrap();
    let mut changed: Value = serde_json::from_slice(&saved).unwrap();
    changed["outcomes"][0]["summary"] = json!("Failure");
    changed["outcomes"][0]["phase_results"][0]["process_status"] = json!({"Failure":101});
    fs::write(&outcomes, changed.to_string()).unwrap();
    refused(
        &summarize_engine(&fixture),
        "partition baseline failed or its counters disagree with outcomes",
    );
    changed = serde_json::from_slice(&saved).unwrap();
    changed["outcomes"][1]["summary"] = json!("MissedMutant");
    changed["outcomes"][1]["phase_results"][0]["process_status"] = json!("Success");
    changed["caught"] = json!(0);
    changed["missed"] = json!(1);
    fs::write(&outcomes, changed.to_string()).unwrap();
    refused(
        &summarize_engine(&fixture),
        "partition contains a survivor, timeout or untested mutant",
    );
    fs::write(&outcomes, saved).unwrap();
    let outcomes = fixture
        .root
        .join("checks/mutants/mutants.out/outcomes.json");
    let saved = fs::read(&outcomes).unwrap();
    let mut changed: Value = serde_json::from_slice(&saved).unwrap();
    changed["outcomes"].as_array_mut().unwrap().pop();
    fs::write(&outcomes, changed.to_string()).unwrap();
    refused(
        &summarize_engine(&fixture),
        "partition evidence differs from its complete planned mutant identities",
    );
    fs::write(&outcomes, saved).unwrap();
    succeeds(&summarize_engine(&fixture));
}

#[test]
fn an_empty_engine_mode_is_intentionally_skipped_without_losing_default_obligations() {
    let mut fixture = engine_aggregation_fixture();
    let listing = fixture.root.join("checks/mutation-engine-list.json");
    fs::write(&listing, "[]").unwrap();
    let hash = tool("sha256sum").arg(&listing).output().unwrap();
    succeeds(&hash);
    let digest = String::from_utf8(hash.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned();
    let path = fixture.root.join("checks/mutation-engine-plan.json");
    let mut plan: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    plan["engine_count"] = json!(0);
    plan["engine_shards"] = json!(0);
    plan["engine_sha256"] = json!(digest);
    plan["mutant_count"] = json!(1);
    fs::write(&path, plan.to_string()).unwrap();
    plan["mode"] = json!("default");
    plan["shard_index"] = json!(0);
    plan["shard_count"] = json!(1);
    plan["expected_mutants"] = json!(1);
    fs::write(
        fixture
            .root
            .join("engine-default/fixture-engine-default-mutants-0")
            .join("rust-reports/mutants-engine-default-shard.json"),
        plan.to_string(),
    )
    .unwrap();
    fixture.set("MUTATION_ENGINE_COUNT", "0");
    fixture.set("MUTATION_ENGINE_SHARDS", "0");
    fixture.set("MUTATION_ENGINE_MATRIX", "[]");
    refused(
        &summarize_engine(&fixture),
        "empty engine mode was not intentionally skipped",
    );
    fixture.set("ENGINE_MUTATIONS_RESULT", "skipped");
    succeeds(&summarize_engine(&fixture));
}

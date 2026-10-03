//! Every engine mode and its featureless control are required mutation evidence.

use crate::harness::{
    engine_aggregation_fixture, engine_inactive_fixture, refused, succeeds, summarize_engine,
};
use serde_json::{Value, json};
use std::fs;

#[test]
fn aggregation_requires_both_modes_and_rejects_new_unviable_or_untested_mutants() {
    let mut fixture = engine_aggregation_fixture();
    succeeds(&summarize_engine(&fixture));
    let merged: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("reports/mutants.json")).unwrap())
            .unwrap();
    assert_eq!(merged["total_mutants"], 3);
    for (variable, message) in [
        (
            "ENGINE_MUTATIONS_RESULT",
            "engine mutation mode is missing, skipped or failed",
        ),
        (
            "ENGINE_DEFAULT_MUTATIONS_RESULT",
            "engine default mode is missing, skipped or failed",
        ),
    ] {
        for state in ["failure", "skipped", "cancelled"] {
            fixture.set(variable, state);
            refused(&summarize_engine(&fixture), message);
        }
        fixture.set(variable, "success");
    }
    let path = fixture
        .root
        .join("engine/fixture-engine-mutants-0/mutants-engine/mutants.out/outcomes.json");
    let original = fs::read(&path).unwrap();
    let mut outcomes: Value = serde_json::from_slice(&original).unwrap();
    outcomes["caught"] = json!(0);
    outcomes["unviable"] = json!(1);
    outcomes["outcomes"][1]["summary"] = json!("Unviable");
    outcomes["outcomes"][1]["phase_results"][0]["phase"] = json!("Build");
    fs::write(&path, outcomes.to_string()).unwrap();
    refused(
        &summarize_engine(&fixture),
        "feature-only mutant became unviable in the engine mode",
    );
    outcomes["outcomes"].as_array_mut().unwrap().pop();
    fs::write(&path, outcomes.to_string()).unwrap();
    refused(
        &summarize_engine(&fixture),
        "mutation outcomes do not equal their complete mode-aware plan",
    );
    fs::write(&path, original).unwrap();
    succeeds(&summarize_engine(&fixture));
}

#[test]
fn aggregate_counters_include_every_verified_non_compiled_control_obligation() {
    let fixture = engine_inactive_fixture();
    succeeds(&summarize_engine(&fixture));
    let merged: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("reports/mutants.json")).unwrap())
            .unwrap();
    assert_eq!(merged["engine_control_schema"], 1);
    assert_eq!(merged["total_mutants"], 3);
    assert_eq!(merged["caught"], 2);
    assert_eq!(merged["missed"], 0);
    assert_eq!(merged["not_compiled_without_features"], 1);
    assert_eq!(
        merged["inactive_without_features_caught_with_engine"]["total"],
        1
    );
    assert_eq!(
        merged["outcomes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|result| result["scenario"].is_object())
            .count(),
        3
    );
}

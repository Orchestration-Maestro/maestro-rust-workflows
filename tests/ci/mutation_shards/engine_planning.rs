//! Mode-aware engine ownership plans at the executable boundary.

use crate::harness::{engine_fixture, output, refused, succeeds};
use serde_json::{Value, json};
use std::fs;

#[test]
fn engine_planning_preserves_default_mode_and_rejects_unowned_feature_mutants() {
    let mut fixture = engine_fixture(false);
    succeeds(&fixture.run_body("rust-gate mutants-plan"));
    assert_eq!(output(&fixture, "mutation-engine-count"), "1");
    assert_eq!(output(&fixture, "mutation-count"), "1");
    assert_eq!(output(&fixture, "mutation-engine-shards"), "1");
    for (name, expected) in [
        ("mutants-list.json", 1),
        ("mutation-engine-list.json", 1),
        ("mutation-engine-default-list.json", 1),
    ] {
        let listing: Value =
            serde_json::from_slice(&fs::read(fixture.root.join("reports").join(name)).unwrap())
                .unwrap();
        assert_eq!(listing.as_array().unwrap().len(), expected);
    }
    let mut engine: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("engine.json")).unwrap()).unwrap();
    engine[1]["replacement"] = json!("unowned feature replacement");
    fs::write(fixture.root.join("engine.json"), engine.to_string()).unwrap();
    refused(
        &fixture.run_body("rust-gate mutants-plan"),
        "feature-enabled discovery contains an unowned mode-specific mutant",
    );
    fixture.set("METADATA", "{\"packages\":[]}");
    refused(
        &fixture.run_body("rust-gate mutants-plan"),
        "engine mutation file `src/engine.rs` has no Cargo package owner",
    );
    fixture.set("MUTATION_ENGINE_FILES", "null");
    refused(
        &fixture.run_body("rust-gate mutants-plan"),
        "MUTATION_ENGINE_FILES must be a JSON array of strings",
    );
}

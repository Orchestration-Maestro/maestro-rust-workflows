//! Mode-aware engine ownership plans at the executable boundary.

use crate::harness::{output, planning_fixture, refused, succeeds};
use serde_json::{Value, json};
use std::fs;

#[test]
fn engine_planning_preserves_default_mode_and_rejects_unowned_feature_mutants() {
    let mut fixture = planning_fixture(2, 0, 1);
    fixture.set("MUTATION_ENGINE_FEATURES", "[\"engine\"]");
    fixture.set("MUTATION_ENGINE_FILES", "[\"src/engine.rs\"]");
    fs::write(fixture.root.join("project/src/engine.rs"), "").unwrap();
    let path = fixture.root.join("listing.json");
    let mut default: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    default[0]["file"] = json!("src/engine.rs");
    fs::write(&path, default.to_string()).unwrap();
    let mut engine = default.clone();
    engine[0]["replacement"] = json!("engine replacement");
    fs::write(fixture.root.join("engine.json"), engine.to_string()).unwrap();
    fixture.set(
        "METADATA",
        &json!({"packages":[{
            "name":"fixture", "manifest_path":fixture.root.join("project/Cargo.toml")
        }]})
        .to_string(),
    );
    fixture.stub(
        "cargo",
        r#"if [[ $1 == metadata ]]; then printf '%s' "$METADATA"; exit; fi
[[ $1 == mutants && $2 == --list ]] || exit 88
if [[ " $* " == *" --features "* ]]; then cat "$RUNNER_TEMP/engine.json";
else cat "$RUNNER_TEMP/listing.json"; fi"#,
    );
    succeeds(&fixture.run_body("rust-gate mutants-plan"));
    assert_eq!(output(&fixture, "mutation-engine-count"), "2");
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

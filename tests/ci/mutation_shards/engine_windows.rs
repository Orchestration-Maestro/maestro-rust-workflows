//! Windows ownership remains native, featureless and part of engine aggregation.

use crate::harness::{Fixture, engine_aggregation_fixture, refused, succeeds, summarize_engine};
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;

/// Construct native-shaped Windows evidence sharing the planner's source/config identity.
fn windows_evidence(fixture: &mut Fixture) -> PathBuf {
    let root = fixture.root.join("windows");
    fs::create_dir_all(root.join("rust-reports")).unwrap();
    fs::create_dir_all(root.join("mutants/mutants.out")).unwrap();
    let mut plan: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("checks/mutation-plan.json")).unwrap())
            .unwrap();
    plan["partition"] = json!("windows");
    plan["os"] = json!("windows");
    plan["files"] = json!(["src/windows.rs"]);
    fs::write(
        root.join("rust-reports/mutation-windows-plan.json"),
        plan.to_string(),
    )
    .unwrap();
    let mut listing: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("checks/mutants-list.json")).unwrap())
            .unwrap();
    listing[0]["file"] = json!("src/windows.rs");
    fs::write(
        root.join("rust-reports/mutation-windows-list.json"),
        listing.to_string(),
    )
    .unwrap();
    let mut outcomes: Value = serde_json::from_slice(
        &fs::read(
            fixture
                .root
                .join("checks/mutants/mutants.out/outcomes.json"),
        )
        .unwrap(),
    )
    .unwrap();
    outcomes["outcomes"][1]["scenario"]["Mutant"] = listing[0].clone();
    fs::write(
        root.join("mutants/mutants.out/outcomes.json"),
        outcomes.to_string(),
    )
    .unwrap();
    fixture.set("MUTATION_WINDOWS", "[\"src/windows.rs\"]");
    fixture.set("WINDOWS_MUTATIONS_RESULT", "success");
    fixture.set("MUTATION_WINDOWS_ARTIFACTS", &root.display().to_string());
    root
}

#[test]
fn engine_aggregation_requires_windows_evidence_bound_to_its_own_exact_files() {
    let mut fixture = engine_aggregation_fixture();
    let root = windows_evidence(&mut fixture);
    succeeds(&summarize_engine(&fixture));
    for state in ["skipped", "failure", "cancelled"] {
        fixture.set("WINDOWS_MUTATIONS_RESULT", state);
        refused(
            &summarize_engine(&fixture),
            "Windows mutation evidence is missing, skipped or failed",
        );
    }
    fixture.set("WINDOWS_MUTATIONS_RESULT", "success");
    let manifest = root.join("rust-reports/mutation-windows-plan.json");
    let saved = fs::read(&manifest).unwrap();
    let mut plan: Value = serde_json::from_slice(&saved).unwrap();
    plan["mutant_count"] = json!(2);
    fs::write(&manifest, plan.to_string()).unwrap();
    refused(
        &summarize_engine(&fixture),
        "Windows plan count differs from its complete owned listing",
    );
    fs::write(&manifest, saved).unwrap();
    let listing = root.join("rust-reports/mutation-windows-list.json");
    let saved = fs::read(&listing).unwrap();
    let mut changed: Value = serde_json::from_slice(&saved).unwrap();
    changed[0]["file"] = json!("src/unowned.rs");
    fs::write(&listing, changed.to_string()).unwrap();
    refused(
        &summarize_engine(&fixture),
        "Windows listing contains a mutant outside its exact owned files",
    );
    fs::write(&listing, saved).unwrap();
    succeeds(&summarize_engine(&fixture));
}

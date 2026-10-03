//! Independent compiler record shape and failed-process stimuli.

use crate::harness::{
    engine_aggregation_fixture, engine_fixture, evidence_hash, refused, summarize_engine,
};
use serde_json::{Value, json};
use std::fs;

#[test]
fn failed_compiler_status_cannot_supply_successful_membership_records() {
    let mut fixture = engine_fixture(true);
    fixture.set("CONTROL_BUILD_STATUS", "7");
    let result = fixture.run_body("rust-gate mutants-engine-default");
    assert_eq!(result.status.code(), Some(7));
}

#[test]
fn failed_mutants_status_cannot_be_excused_by_complete_looking_outcomes() {
    let mut fixture = engine_fixture(true);
    fixture.set("CONTROL_MUTANT_STATUS", "3");
    let result = fixture.run_body("rust-gate mutants-engine-default");
    assert_eq!(result.status.code(), Some(3));
}

#[test]
fn malformed_compiler_records_are_refused_with_refreshed_digests() {
    let fixture = engine_aggregation_fixture();
    let root = fixture
        .root
        .join("engine-default/fixture-engine-default-mutants-0")
        .join("mutants-engine-default/mutants.out");
    let manifest = root.join("compile-membership.json");
    let binding: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    let cargo = root.join("cargo-build.json");
    let rows: Vec<Value> = fs::read_to_string(&cargo)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let mut probes = vec![
        vec![rows[0].clone()],
        vec![rows[0].clone(), rows[1].clone(), rows[1].clone()],
        vec![rows[1].clone()],
    ];
    for (pointer, value) in [
        ("/target/src_path", json!("relative")),
        ("/target/src_path", json!(1)),
        ("/filenames", json!([])),
        ("/filenames", json!("invalid")),
        ("/filenames", json!([1])),
    ] {
        let mut changed = rows.clone();
        *changed[0].pointer_mut(pointer).unwrap() = value;
        probes.push(changed);
    }
    for probe in probes {
        fs::write(
            &cargo,
            probe
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let mut changed = binding.clone();
        changed["cargo_sha256"] = json!(evidence_hash(&cargo));
        fs::write(&manifest, changed.to_string()).unwrap();
        refused(
            &summarize_engine(&fixture),
            "featureless build did not verify fresh compile membership",
        );
    }
}

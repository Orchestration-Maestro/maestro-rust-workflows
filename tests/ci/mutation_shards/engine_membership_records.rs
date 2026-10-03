//! Independent compiler record shape and failed-process stimuli.

use crate::harness::{
    engine_aggregation_fixture, engine_fallback_fixture, engine_fixture, evidence_hash, refused,
    summarize_engine,
};
use serde_json::{Value, json};
use std::fs;

#[test]
fn unverified_control_survivors_fail_without_their_exact_engine_twins() {
    let fixture = engine_fallback_fixture(false);
    refused(
        &summarize_engine(&fixture),
        "featureless control survivor is not caught by its exact engine twin",
    );
}

#[test]
fn aggregation_rejects_verified_compiled_survivors_despite_a_caught_engine_twin() {
    let fixture = engine_aggregation_fixture();
    let root = fixture
        .root
        .join("engine-default/fixture-engine-default-mutants-0")
        .join("mutants-engine-default/mutants.out");
    for name in ["outcomes.json", "tested-outcomes.json"] {
        let path = root.join(name);
        let mut outcomes: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        outcomes["caught"] = json!(0);
        outcomes["missed"] = json!(1);
        outcomes["outcomes"][1]["summary"] = json!("MissedMutant");
        outcomes["outcomes"][1]["phase_results"][0]["process_status"] = json!("Success");
        fs::write(path, outcomes.to_string()).unwrap();
    }
    refused(
        &summarize_engine(&fixture),
        "partition contains a survivor, timeout or untested mutant",
    );
}

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
    let cargo = root.join("builds/0/cargo-build.json");
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
        changed["packages"][0]["cargo_sha256"] = json!(evidence_hash(&cargo));
        fs::write(&manifest, changed.to_string()).unwrap();
        refused(
            &summarize_engine(&fixture),
            "featureless build did not verify fresh compile membership",
        );
    }
}

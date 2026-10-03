//! Versioned host envelopes and nested records refuse schema drift.

use crate::harness::{
    Fixture, host_coverage_fixture, host_evidence_digest, host_evidence_fixture, refused,
};
use serde_json::{Value, json};
use std::fs;
use std::iter::once;

/// Strict schema failures use the identity refusal before behavioural checks.
const HOST_IDENTITY_REFUSAL: &str = "host plan, pending coverage or outcomes belong to \
    another source, policy or run";

/// The same three independent artifacts used by the real join.
const PATHS: [&str; 3] = [
    "checks/mutation-host-plan.json",
    "checks/changed-coverage-pending.json",
    "host-evidence/host-outcomes.json",
];

/// Rebind altered plans so schema rejection cannot be masked by a stale digest.
fn write_bound_documents(fixture: &Fixture, values: &mut [Value]) {
    fs::write(fixture.root.join(PATHS[0]), values[0].to_string()).unwrap();
    if values[0]["identity"].get("foreign").is_some() {
        fs::write(
            fixture.root.join("checks/mutation-plan.json"),
            values[0]["identity"].to_string(),
        )
        .unwrap();
    }
    let binding = host_evidence_digest(fixture, &fixture.root.join(PATHS[0]).display().to_string());
    for index in [1, 2] {
        if values[index].get("plan_sha256").is_some() {
            values[index]["plan_sha256"] = json!(binding);
        }
        fs::write(fixture.root.join(PATHS[index]), values[index].to_string()).unwrap();
    }
}

/// Keep line-record tampering partition-consistent to isolate strict record validation.
fn mirror_line_record(pending: &mut Value, original: &Value, pointer: &str) {
    let changed = pending.pointer(pointer).unwrap().clone();
    let file = original.pointer(pointer).unwrap()["file"].clone();
    let line = original.pointer(pointer).unwrap()["line"].clone();
    for partition in ["host", "ordinary", "coverable", "ordinary_uncovered"] {
        for record in pending[partition].as_array_mut().unwrap() {
            if record["file"] == file && record["line"] == line {
                *record = changed.clone();
            }
        }
    }
}

#[test]
fn host_join_refuses_unknown_plan_pending_and_outcome_fields() {
    let fixture = host_evidence_fixture();
    let originals: Vec<Value> = PATHS
        .iter()
        .map(|path| serde_json::from_slice(&fs::read(fixture.root.join(path)).unwrap()).unwrap())
        .collect();
    for (document, pointer) in [
        (0, ""),
        (1, ""),
        (2, ""),
        (1, "/host/0"),
        (1, "/ordinary/0"),
        (1, "/coverable/0"),
        (1, "/ordinary_uncovered/0"),
        (0, "/identity"),
        (2, "/baseline_before"),
        (2, "/baseline_after"),
        (2, "/outcomes/0"),
        (2, "/outcomes/0/receipt"),
    ] {
        let required: Vec<String> = originals[document]
            .pointer(pointer)
            .unwrap()
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        for key in once("foreign").chain(required.iter().map(String::as_str)) {
            fs::write(
                fixture.root.join("checks/mutation-plan.json"),
                originals[0]["identity"].to_string(),
            )
            .unwrap();
            let mut values = originals.clone();
            let record = values[document]
                .pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap();
            if key == "foreign" {
                record.insert(key.to_owned(), json!(true));
            } else {
                record.remove(key);
            }
            if document == 1 && !pointer.is_empty() {
                mirror_line_record(&mut values[1], &originals[1], pointer);
            }
            write_bound_documents(&fixture, &mut values);
            refused(
                &fixture.run_body("rust-gate mutants-aggregate"),
                HOST_IDENTITY_REFUSAL,
            );
        }
    }
    fs::write(
        fixture.root.join("checks/mutation-plan.json"),
        originals[0]["identity"].to_string(),
    )
    .unwrap();
    for document in 0..3 {
        let mut values = originals.clone();
        values[document]["schema"] = json!(2);
        write_bound_documents(&fixture, &mut values);
        refused(
            &fixture.run_body("rust-gate mutants-aggregate"),
            HOST_IDENTITY_REFUSAL,
        );
    }
}

#[test]
fn pending_coverage_requires_a_downloaded_immutable_host_plan() {
    let fixture = host_coverage_fixture(1);
    fs::remove_file(fixture.root.join("reports/mutation-host-plan.json")).unwrap();
    refused(
        &fixture.run_body("rust-gate changed-coverage"),
        "host coverage requires mutation-host-plan.json",
    );
}

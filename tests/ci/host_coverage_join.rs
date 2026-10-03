//! Host evidence is distinct from LLVM hits and cannot enlarge ordinary allowances.

use crate::harness::{
    host_coverage_fixture, host_evidence_digest, host_evidence_fixture, output, refused, succeeds,
};
use serde_json::{Value, json};
use std::fs;

/// Shared immutable identity refusal for each independently tampered binding.
const HOST_IDENTITY_REFUSAL: &str = "host plan, pending coverage or outcomes belong to \
    another source, policy or run";

/// Shared refusal for infrastructure failures and invalid behavioural phase evidence.
const HOST_PHASE_REFUSAL: &str = "host evidence requires caught mutants, successful baselines, \
    build, provisioning and cleanup";

#[test]
fn pending_host_coverage_preserves_raw_hits_and_both_denominators() {
    let fixture = host_coverage_fixture(1);
    succeeds(&fixture.run_body("rust-gate changed-coverage"));
    let pending: Value = serde_json::from_slice(
        &fs::read(fixture.root.join("reports/changed-coverage-pending.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(pending["coverable"].as_array().unwrap().len(), 4);
    assert_eq!(pending["ordinary"].as_array().unwrap().len(), 3);
    assert_eq!(pending["host"].as_array().unwrap().len(), 1);
    assert_eq!(pending["host"][0]["hits"], 0);
    assert_eq!(pending["ordinary_allowed"], 1);
    assert_eq!(pending["full_allowed"], 1);
    assert!(
        fs::read_to_string(fixture.root.join("reports/changed-coverage.txt"))
            .unwrap()
            .contains("pending-host")
    );
    let failing = host_coverage_fixture(2);
    refused(
        &failing.run_body("rust-gate changed-coverage"),
        "changed-coverage: ordinary uncovered lines exceed their unchanged allowance",
    );
}

#[test]
fn complete_host_proof_joins_without_forging_llvm_execution() {
    let fixture = host_evidence_fixture();
    succeeds(&fixture.run_body("rust-gate mutants-aggregate"));
    assert_eq!(output(&fixture, "changed-coverage-state"), "passed");
    let report = fs::read_to_string(fixture.root.join("reports/changed-coverage.txt")).unwrap();
    assert!(report.starts_with("4 coverable new lines;"), "{report}");
    assert!(report.contains("1 HOST_BEHAVIOUR_VERIFIED"), "{report}");
    assert!(report.contains("0 raw host LLVM_EXECUTED"), "{report}");
    assert!(
        report.contains("1 ordinary uncovered, 1 ordinary allowed"),
        "{report}"
    );
    assert_eq!(
        fs::read(fixture.root.join("checks/coverage.lcov")).unwrap(),
        fs::read(fixture.root.join("reports/coverage.lcov")).unwrap()
    );
}

#[test]
fn host_join_rejects_stale_duplicate_unviable_timed_out_and_partial_outcomes() {
    let fixture = host_evidence_fixture();
    let path = fixture.root.join("host-evidence/host-outcomes.json");
    let original: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for (pointer, value, message) in [
        ("/sha", json!("0".repeat(40)), HOST_IDENTITY_REFUSAL),
        ("/schema", json!(2), HOST_IDENTITY_REFUSAL),
        ("/run_id", json!("999"), HOST_IDENTITY_REFUSAL),
        (
            "/policy_sha256",
            json!("0".repeat(64)),
            HOST_IDENTITY_REFUSAL,
        ),
        ("/attempt", json!("2"), HOST_IDENTITY_REFUSAL),
        (
            "/source_sha256/src~1host.rs",
            json!("0".repeat(64)),
            HOST_IDENTITY_REFUSAL,
        ),
        (
            "/outcomes/0/mutant/name",
            json!("foreign"),
            "host outcomes do not equal the complete nonempty planned mutant population",
        ),
        ("/plan_sha256", json!("0".repeat(64)), HOST_IDENTITY_REFUSAL),
        (
            "/provisioner_sha256",
            json!("0".repeat(64)),
            HOST_IDENTITY_REFUSAL,
        ),
        (
            "/outcomes",
            json!([]),
            "host outcomes do not equal the complete nonempty planned mutant population",
        ),
        (
            "/outcomes",
            json!([original["outcomes"][0], original["outcomes"][0]]),
            "host outcomes do not equal the complete nonempty planned mutant population",
        ),
        (
            "/outcomes/0/outcome",
            json!("unviable"),
            "host evidence requires caught mutants, successful baselines, build, \
            provisioning and cleanup",
        ),
        (
            "/outcomes/0/outcome",
            json!("timeout"),
            "host evidence requires caught mutants, successful baselines, build, \
            provisioning and cleanup",
        ),
        (
            "/outcomes/0/receipt/cleanup",
            json!("failed"),
            "host evidence requires caught mutants, successful baselines, build, \
            provisioning and cleanup",
        ),
        (
            "/outcomes/0/receipt/provision",
            json!("failed"),
            "host evidence requires caught mutants, successful baselines, build, \
            provisioning and cleanup",
        ),
        (
            "/outcomes/0/receipt/build",
            json!("failed"),
            HOST_PHASE_REFUSAL,
        ),
        (
            "/outcomes/0/receipt/test_failure",
            json!("foreign_assertion"),
            HOST_PHASE_REFUSAL,
        ),
        (
            "/outcomes/0/receipt/selected_tests",
            json!([]),
            HOST_PHASE_REFUSAL,
        ),
        (
            "/baseline_after/test",
            json!("not-run"),
            "host evidence requires caught mutants, successful baselines, build, \
            provisioning and cleanup",
        ),
    ] {
        let mut changed = original.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        fs::write(&path, changed.to_string()).unwrap();
        refused(&fixture.run_body("rust-gate mutants-aggregate"), message);
    }
    fs::write(&path, original.to_string()).unwrap();
    succeeds(&fixture.run_body("rust-gate mutants-aggregate"));
}

#[test]
fn host_join_refuses_unmapped_functions_and_ordinary_allowance_inflation() {
    let fixture = host_evidence_fixture();
    let pending = fixture.root.join("checks/changed-coverage-pending.json");
    let original: Value = serde_json::from_slice(&fs::read(&pending).unwrap()).unwrap();
    for change in ["line", "ordinary", "allowance", "full-allowance", "target"] {
        let mut value = original.clone();
        match change {
            "line" => {
                value["host"][0]["line"] = json!(2);
                value["coverable"][0]["line"] = json!(2);
            }
            "ordinary" => {
                value["ordinary"][1]["hits"] = json!(0);
                value["coverable"][2]["hits"] = json!(0);
                value["ordinary_uncovered"] = json!([value["ordinary"][0], value["ordinary"][1]]);
            }
            "full-allowance" => value["full_allowed"] = json!(2),
            "target" => value["target"] = json!(85),
            _ => value["ordinary_allowed"] = json!(2),
        }
        fs::write(&pending, value.to_string()).unwrap();
        refused(
            &fixture.run_body("rust-gate mutants-aggregate"),
            "host coverage partition, ordinary allowance or enclosing-function \
            evidence is incomplete",
        );
    }
    fs::write(&pending, original.to_string()).unwrap();
    succeeds(&fixture.run_body("rust-gate mutants-aggregate"));
}

#[test]
fn host_pending_coverage_refuses_absent_owned_lcov_records() {
    let fixture = host_coverage_fixture(0);
    fs::write(fixture.root.join("reports/coverage.lcov"), "TN:\n").unwrap();
    refused(
        &fixture.run_body("rust-gate changed-coverage"),
        "host-owned file `src/host.rs` has no LCOV line records",
    );
}

#[test]
fn host_raw_phase_logs_require_safe_paths_and_exact_digests() {
    let mut fixture = host_evidence_fixture();
    let path = fixture.root.join("host-evidence/host-outcomes.json");
    let original: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for (logs, message) in [
        (json!({}), "host evidence is missing raw phase logs"),
        (
            json!({"../outside.log":"a".repeat(64)}),
            "host evidence log path escapes its artifact",
        ),
        (
            json!({"phases.log":"a".repeat(64)}),
            "host evidence raw log digest is missing or mismatched",
        ),
    ] {
        let mut changed = original.clone();
        changed["baseline_before"]["logs"] = logs;
        fs::write(&path, changed.to_string()).unwrap();
        refused(&fixture.run_body("rust-gate mutants-aggregate"), message);
    }
    fixture.set("MUTATION_HOST_COUNT", "bad");
    refused(
        &fixture.run_body("rust-gate mutants-aggregate"),
        "provisioned-host mutation count is invalid",
    );
    fixture.set("MUTATION_HOST_COUNT", "1");
    fixture.set("HOST_MUTATIONS_RESULT", "skipped");
    refused(
        &fixture.run_body("rust-gate mutants-aggregate"),
        "provisioned-host mutation job failed or was skipped",
    );
    fs::write(&path, original.to_string()).unwrap();
    fixture.set("HOST_MUTATIONS_RESULT", "success");
    succeeds(&fixture.run_body("rust-gate mutants-aggregate"));
}

#[test]
fn named_phase_only_and_multiple_phase_failures_are_legitimate_host_kills() {
    let fixture = host_evidence_fixture();
    let path = fixture.root.join("host-evidence/host-outcomes.json");
    let original: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for names in [vec!["abandon"], vec!["abandon", "recover_abandon"]] {
        let mut changed = original.clone();
        let receipt = &mut changed["outcomes"][0]["receipt"];
        receipt["passed"] = json!(1);
        receipt["failed"] = json!(0);
        receipt["test_failure"] = json!(names);
        for name in names {
            receipt["phases"][name] = json!("failed");
        }
        fs::write(&path, changed.to_string()).unwrap();
        succeeds(&fixture.run_body("rust-gate mutants-aggregate"));
    }
}

#[test]
fn unnamed_unrun_and_baseline_phase_failures_never_count_as_host_kills() {
    let fixture = host_evidence_fixture();
    let path = fixture.root.join("host-evidence/host-outcomes.json");
    let original: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for (pointer, value) in [
        ("/outcomes/0/receipt/phases/abandon", json!("failed")),
        ("/outcomes/0/receipt/phases/normal", json!("not-run")),
        ("/baseline_before/phases/normal", json!("failed")),
        ("/baseline_after/phases/recover_preparing", json!("failed")),
        ("/outcomes/0/receipt/test_failure", json!([])),
        (
            "/outcomes/0/receipt/test_failure",
            json!(["host_assertion", "host_assertion"]),
        ),
        (
            "/outcomes/0/receipt/selected_tests",
            json!(["host_assertion", "host_assertion"]),
        ),
        ("/outcomes/0/receipt/passed", json!(0.5)),
        ("/outcomes/0/receipt/failed", json!(0.5)),
        ("/outcomes/0/receipt/passed", json!(1)),
        ("/outcomes/0/receipt/ignored", json!(1)),
        ("/outcomes/0/receipt/test_sha256", json!("bad")),
        ("/outcomes/0/receipt/bootstrap_sha256", json!("bad")),
        ("/outcomes/0/patched_source_sha256", json!("bad")),
    ] {
        let mut changed = original.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        if pointer.ends_with("selected_tests") {
            changed["outcomes"][0]["receipt"]["passed"] = json!(1);
        } else if pointer.ends_with("test_failure")
            && changed
                .pointer(pointer)
                .unwrap()
                .as_array()
                .is_some_and(|names| names.len() == 2)
        {
            changed["outcomes"][0]["receipt"]["selected_tests"] =
                json!(["host_assertion", "other"]);
            changed["outcomes"][0]["receipt"]["failed"] = json!(2);
        }
        fs::write(&path, changed.to_string()).unwrap();
        refused(
            &fixture.run_body("rust-gate mutants-aggregate"),
            HOST_PHASE_REFUSAL,
        );
    }
    fs::write(&path, original.to_string()).unwrap();
    succeeds(&fixture.run_body("rust-gate mutants-aggregate"));
}

#[test]
fn host_receipt_refuses_unnamed_failed_rust_tests_beside_phase_failures() {
    let fixture = host_evidence_fixture();
    let path = fixture.root.join("host-evidence/host-outcomes.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["outcomes"][0]["receipt"]["phases"]["abandon"] = json!("failed");
    value["outcomes"][0]["receipt"]["test_failure"] = json!(["abandon"]);
    fs::write(path, value.to_string()).unwrap();
    refused(
        &fixture.run_body("rust-gate mutants-aggregate"),
        HOST_PHASE_REFUSAL,
    );
}

#[test]
fn host_toolchain_and_tool_version_drift_refuse_current_evidence() {
    let mut fixture = host_evidence_fixture();
    for (key, value, original) in [
        ("RUSTUP_TOOLCHAIN", "1.98.0", "1.98.1"),
        ("CARGO_MUTANTS_VERSION", "27.0.0", "27.1.0"),
    ] {
        fixture.set(key, value);
        refused(
            &fixture.run_body("rust-gate mutants-aggregate"),
            HOST_IDENTITY_REFUSAL,
        );
        fixture.set(key, original);
    }
}

#[test]
fn host_lines_before_function_and_large_partition_misses_are_refused() {
    let fixture = host_evidence_fixture();
    let plan_path = fixture.root.join("checks/mutation-host-plan.json");
    let pending_path = fixture.root.join("checks/changed-coverage-pending.json");
    let outcomes_path = fixture.root.join("host-evidence/host-outcomes.json");
    let mut plan: Value = serde_json::from_slice(&fs::read(&plan_path).unwrap()).unwrap();
    let mut pending: Value = serde_json::from_slice(&fs::read(&pending_path).unwrap()).unwrap();
    let mut outcomes: Value = serde_json::from_slice(&fs::read(&outcomes_path).unwrap()).unwrap();
    for (start, end, misses) in [(2, 40, 1), (1, 40, 2)] {
        plan["mutants"][0]["function"]["span"]["start"]["line"] = json!(start);
        plan["mutants"][0]["function"]["span"]["end"]["line"] = json!(end);
        fs::write(&plan_path, plan.to_string()).unwrap();
        let binding = host_evidence_digest(&fixture, &plan_path.display().to_string());
        pending["plan_sha256"] = json!(binding);
        outcomes["plan_sha256"] = json!(binding);
        outcomes["outcomes"][0]["mutant"] = plan["mutants"][0].clone();
        let host: Vec<Value> = (1..=40)
            .map(|line| json!({"file":"src/host.rs","line":line,"hits":0}))
            .collect();
        let ordinary: Vec<Value> = (1..=3)
            .map(|line| json!({"file":"src/lib.rs","line":line,"hits":u8::from(line > misses)}))
            .collect();
        pending["host"] = json!(host);
        pending["ordinary"] = json!(ordinary);
        pending["ordinary_uncovered"] = json!(&ordinary[..misses]);
        pending["coverable"] = json!(host.into_iter().chain(ordinary).collect::<Vec<_>>());
        pending["full_allowed"] = json!(2);
        fs::write(&pending_path, pending.to_string()).unwrap();
        fs::write(&outcomes_path, outcomes.to_string()).unwrap();
        refused(
            &fixture.run_body("rust-gate mutants-aggregate"),
            "host coverage partition, ordinary allowance or enclosing-function \
            evidence is incomplete",
        );
    }
}

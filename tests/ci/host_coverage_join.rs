//! Host evidence is distinct from LLVM hits and cannot enlarge ordinary allowances.

use crate::harness::{Fixture, copy_tree, fixture_git, host_fixture, output, refused, succeeds};
use serde_json::{Value, json};
use std::fmt::Write as _;
use std::fs;

/// Shared immutable identity refusal for each independently tampered binding.
const HOST_IDENTITY_REFUSAL: &str = "host plan, pending coverage or outcomes belong to \
    another source, policy or run";

/// Shared refusal for infrastructure failures and invalid behavioural phase evidence.
const HOST_PHASE_REFUSAL: &str = "host evidence requires caught mutants, successful baselines, \
    build, provisioning and cleanup";

/// Create real changed identities and the pending report through the gate.
fn coverage_fixture(misses: usize) -> Fixture {
    let mut fixture = host_fixture();
    let project = fixture.root.join("project");
    fs::write(
        project.join("src/host.rs"),
        "pub fn host() -> bool { false }\n",
    )
    .unwrap();
    fs::write(
        project.join("src/lib.rs"),
        "pub fn ordinary() {}\n".repeat(3),
    )
    .unwrap();
    fixture_git(&project, &["add", "."]);
    fixture_git(
        &project,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "sources",
        ],
    );
    fixture.set("GITHUB_SHA", &fixture_git(&project, &["rev-parse", "HEAD"]));
    fixture.set("GITHUB_BASE_REF", "main");
    fixture.set("PULL_REQUEST_TITLE", "feat(ci): host owner");
    succeeds(&fixture.run_body("rust-gate mutants-plan"));
    let lcov = format!(
        "SF:{}/src/host.rs\nDA:1,0\nend_of_record\nSF:{}/src/lib.rs\n{}end_of_record\n",
        project.display(),
        project.display(),
        (1..=3).fold(String::new(), |mut text, line| {
            writeln!(text, "DA:{line},{}", u8::from(line > misses)).unwrap();
            text
        })
    );
    fs::write(fixture.root.join("reports/coverage.lcov"), lcov).unwrap();
    fixture
}

/// Hash raw fixture evidence with the same standard SHA utility, outside gate trust.
fn digest(fixture: &Fixture, path: &str) -> String {
    let result = fixture.run_body(&format!("sha256sum '{path}'"));
    succeeds(&result);
    String::from_utf8(result.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned()
}

/// Synthetic schema-1 receipt consumed by the permanent aggregation path, not production.
fn evidence_fixture() -> Fixture {
    let mut fixture = coverage_fixture(1);
    succeeds(&fixture.run_body("rust-gate changed-coverage"));
    let checks = fixture.root.join("checks");
    copy_tree(&fixture.root.join("reports"), &checks);
    let root = fixture.root.join("host-evidence");
    fs::create_dir(&root).unwrap();
    fs::write(
        root.join("phases.log"),
        "selected host assertion failed; cleanup completed\n",
    )
    .unwrap();
    let plan: Value =
        serde_json::from_slice(&fs::read(checks.join("mutation-host-plan.json")).unwrap()).unwrap();
    let receipt = json!({
        "build":"passed", "provision":"passed", "test":"passed", "cleanup":"passed",
        "selected_tests":["host_assertion"], "passed":1, "failed":0, "ignored":0,
        "phases":{"normal":"passed","abandon":"passed","preparing":"passed",
            "recover_abandon":"passed","recover_preparing":"passed"},
        "test_sha256":"c".repeat(64), "bootstrap_sha256":"d".repeat(64),
        "logs":{"phases.log":digest(&fixture, &root.join("phases.log").display().to_string())}
    });
    let mut caught = receipt.clone();
    caught["test"] = json!("failed");
    caught["passed"] = json!(0);
    caught["failed"] = json!(1);
    caught["test_failure"] = json!(["host_assertion"]);
    let plan_digest = digest(
        &fixture,
        &checks.join("mutation-host-plan.json").display().to_string(),
    );
    let outcomes = json!({
        "schema":1,"sha":fixture.env["GITHUB_SHA"],"run_id":"123","attempt":"1",
        "plan_sha256":plan_digest,
        "policy_sha256":plan["policy_sha256"],"provisioner_sha256":plan["provisioner_sha256"],
        "source_sha256":plan["source_sha256"],"baseline_before":receipt,"baseline_after":receipt,
        "outcomes":[{"mutant":plan["mutants"][0],"outcome":"caught",
            "patched_source_sha256":"e".repeat(64), "receipt":caught}]
    });
    fs::write(root.join("host-outcomes.json"), outcomes.to_string()).unwrap();
    for (key, value) in [
        ("MUTATION_HOST_COUNT", "1"),
        ("HOST_MUTATIONS_RESULT", "success"),
        ("CHECKS_RESULT", "success"),
        ("MUTATION_MODE", "empty"),
    ] {
        fixture.set(key, value);
    }
    fixture.set("MUTATION_PLAN_DIR", &checks.display().to_string());
    fixture.set("MUTATION_HOST_ARTIFACTS", &root.display().to_string());
    fixture
}

#[test]
fn pending_host_coverage_preserves_raw_hits_and_both_denominators() {
    let fixture = coverage_fixture(1);
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
    let failing = coverage_fixture(2);
    refused(
        &failing.run_body("rust-gate changed-coverage"),
        "changed-coverage: ordinary uncovered lines exceed their unchanged allowance",
    );
}

#[test]
fn complete_host_proof_joins_without_forging_llvm_execution() {
    let fixture = evidence_fixture();
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
    let fixture = evidence_fixture();
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
    let fixture = evidence_fixture();
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
    let fixture = coverage_fixture(0);
    fs::write(fixture.root.join("reports/coverage.lcov"), "TN:\n").unwrap();
    refused(
        &fixture.run_body("rust-gate changed-coverage"),
        "host-owned file `src/host.rs` has no LCOV line records",
    );
}

#[test]
fn host_raw_phase_logs_require_safe_paths_and_exact_digests() {
    let mut fixture = evidence_fixture();
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
    let fixture = evidence_fixture();
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
    let fixture = evidence_fixture();
    let path = fixture.root.join("host-evidence/host-outcomes.json");
    let original: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for (pointer, value) in [
        ("/outcomes/0/receipt/phases/abandon", json!("failed")),
        ("/outcomes/0/receipt/phases/normal", json!("not-run")),
        ("/baseline_before/phases/normal", json!("failed")),
        ("/baseline_after/phases/recover_preparing", json!("failed")),
        ("/outcomes/0/receipt/test_failure", json!([])),
    ] {
        let mut changed = original.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        fs::write(&path, changed.to_string()).unwrap();
        refused(
            &fixture.run_body("rust-gate mutants-aggregate"),
            HOST_PHASE_REFUSAL,
        );
    }
    fs::write(&path, original.to_string()).unwrap();
    succeeds(&fixture.run_body("rust-gate mutants-aggregate"));
}

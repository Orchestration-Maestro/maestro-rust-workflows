//! Provisioned-host ownership is a transfer, never a coverage exemption.

use crate::harness::{host_fixture, output, refused, succeeds, workflow};
use serde_json::Value;
use std::fs;

#[test]
fn host_ownership_requires_enabled_mutation_even_for_called_workflows() {
    let mut fixture = host_fixture();
    fixture.set("MUTATION_TEST", "false");
    refused(
        &fixture.run_body("rust-gate validate"),
        "[ci.mutation-provisioned-host] requires mutation-test=true",
    );
    fixture.set("MUTATION_TEST", "true");
    succeeds(&fixture.run_body("rust-gate validate"));
    assert_eq!(output(&fixture, "mutation-host-files"), "[\"src/host.rs\"]");
}

#[test]
fn host_only_plan_discovers_every_file_without_default_filters() {
    let fixture = host_fixture();
    succeeds(&fixture.run_body("rust-gate mutants-plan"));
    assert_eq!(output(&fixture, "mutation-host-count"), "1");
    assert_eq!(output(&fixture, "mutation-mode"), "empty");
    let calls = fixture.calls();
    assert!(calls.contains("--no-config"), "{calls}");
    assert!(calls.contains("--features fixture/host-tests"), "{calls}");
    assert!(calls.contains("--exclude src/host.rs"), "{calls}");
    let plan: Value = serde_json::from_slice(
        &fs::read(fixture.root.join("reports/mutation-host-plan.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(plan["schema"], 1);
    assert_eq!(plan["identity"]["sha"], fixture.env["GITHUB_SHA"]);
}

#[test]
fn host_only_and_inline_plans_never_accept_absent_host_execution() {
    let mut fixture = host_fixture();
    succeeds(&fixture.run_body("rust-gate mutants-plan"));
    fixture.set("MUTATION_HOST_COUNT", "1");
    fixture.set(
        "MUTATION_PLAN_DIR",
        &fixture.root.join("reports").display().to_string(),
    );
    fixture.set("CHECKS_RESULT", "success");
    for mode in ["empty", "inline"] {
        fixture.set("MUTATION_MODE", mode);
        refused(
            &fixture.run_body("rust-gate mutants-aggregate"),
            "provisioned-host mutation result is missing; no host executor ran",
        );
    }
}

#[test]
fn host_aggregation_is_scheduled_from_the_early_plan_not_checks() {
    let ci = workflow("ci");
    let condition = ci["jobs"]["mutation-summary"]["if"].as_str().unwrap();
    assert!(condition.contains("needs.mutation-plan.outputs.mutation-host-count != '0'"));
    assert!(!ci["jobs"]["mutation-plan"]["needs"].is_array());
    for job in [
        "mutation-engine",
        "mutation-engine-default",
        "mutation-windows",
    ] {
        assert!(
            !ci["jobs"][job]["if"]
                .as_str()
                .unwrap()
                .contains("mutation-host-count")
        );
    }
    let required = &ci["jobs"]["gate"]["steps"];
    assert!(
        required
            .as_array()
            .unwrap()
            .iter()
            .any(|step| step["env"]["MUTATION_HOST_COUNT"]
                == "${{ needs.mutation-plan.outputs.mutation-host-count }}")
    );
}

#[test]
fn host_plan_download_precedes_changed_coverage_in_checks() {
    let ci = workflow("ci");
    let steps = ci["jobs"]["checks"]["steps"].as_array().unwrap();
    let position = |id| steps.iter().position(|step| step["id"] == id).unwrap();
    assert!(position("checks-plan-download") < position("changed-coverage"));
}

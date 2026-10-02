//! Report-only mutation of the gate reuses the consumer planner, workers and aggregation.

use crate::harness::{output, planning_fixture, refused, succeeds, tool_rows, workflow};
use serde_json::json;
use std::fs;

#[test]
fn full_scope_planning_ignores_the_first_parent_diff() {
    let mut fixture = planning_fixture(4, 2, 50);
    fixture.set("MUTATION_FULL_SCOPE", "true");
    fs::remove_dir(fixture.root.join("reports")).unwrap();
    succeeds(&fixture.run("ci", "mutants-plan"));
    assert_eq!(output(&fixture, "mutation-count"), "4");
    assert!(!fixture.calls().contains("--in-diff"));
    assert!(!fixture.root.join("reports/mutants.diff").exists());
    let plan: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.root.join("reports/mutation-plan.json")).unwrap())
            .unwrap();
    assert_eq!(plan["first_parent"], "b".repeat(40));
}

#[test]
fn malformed_full_scope_is_refused_before_discovery() {
    let mut fixture = planning_fixture(4, 2, 50);
    fixture.set("MUTATION_FULL_SCOPE", "yes");
    refused(
        &fixture.run("ci", "mutants-plan"),
        "mutation-full-scope must be true or false",
    );
    assert!(!fixture.calls().contains("mutants --list"));
}

#[test]
fn gate_mutation_is_report_only_with_complete_shard_evidence() {
    let data = workflow("gate-mutation");
    assert_eq!(data["jobs"]["plan"]["env"]["MUTATION_FULL_SCOPE"], "true");
    assert_eq!(
        data["jobs"]["mutations"]["env"]["MUTATION_FULL_SCOPE"],
        "true"
    );
    assert_eq!(
        data["jobs"]["mutations"]["env"]["MUTATION_IN_PLACE"],
        "true"
    );
    assert!(data["name"].as_str().unwrap().contains("report-only"));
    assert_eq!(data["on"]["push"]["branches"], json!(["main"]));
    assert!(data["on"].get("workflow_dispatch").is_some());
    assert!(
        data["on"]["pull_request"]["paths"]
            .as_array()
            .unwrap()
            .contains(&json!("gate/**"))
    );
}

#[test]
fn gate_mutation_workers_preserve_every_planned_shard() {
    let data = workflow("gate-mutation");
    for (id, command) in [
        ("plan", "rust-gate mutants-plan"),
        ("mutations", "rust-gate mutants"),
        ("summary", "rust-gate mutants-aggregate"),
    ] {
        let job = &data["jobs"][id];
        assert!(job["name"].as_str().unwrap().contains("report-only"));
        assert_eq!(job["runs-on"], "ubuntu-24.04");
        assert!(job["timeout-minutes"].is_number());
        let steps = job["steps"].as_array().unwrap();
        assert!(steps.iter().any(|step| step["run"] == command));
        assert!(steps.iter().any(|step| {
            step["if"] == "${{ always() }}"
                && step["uses"]
                    .as_str()
                    .unwrap_or_default()
                    .starts_with("actions/upload-artifact@")
        }));
    }
    let workers = &data["jobs"]["mutations"];
    assert_eq!(workers["strategy"]["fail-fast"], false);
    assert_eq!(workers["strategy"]["max-parallel"], 32);
    assert_eq!(
        workers["strategy"]["matrix"]["shard"],
        "${{ fromJSON(needs.plan.outputs.mutation-matrix) }}"
    );
    assert_eq!(
        data["jobs"]["summary"]["if"],
        "${{ always() && needs.plan.outputs.mutation-mode == 'sharded' }}"
    );
    let required = workflow("ci-internal")["jobs"]["required"].clone();
    assert!(
        !required["needs"]
            .as_array()
            .unwrap()
            .contains(&json!("gate-mutation"))
    );
    assert!(
        workflow("ci")["on"]["workflow_call"]["inputs"]
            .get("mutation-full-scope")
            .is_none()
    );
}

#[test]
fn full_scope_workers_and_inline_execution_use_the_same_selection() {
    for (shard, in_place) in [("", false), ("0/2", false), ("0/2", true)] {
        let mut fixture = planning_fixture(4, 2, 50);
        fixture.set("MUTATION_FULL_SCOPE", "true");
        succeeds(&fixture.run_body("rust-gate mutants-plan"));
        for (name, file) in [
            ("MUTATION_PLAN", "mutation-plan.json"),
            ("MUTATION_LIST", "mutants-list.json"),
        ] {
            fixture.set(
                name,
                &fixture
                    .root
                    .join("reports")
                    .join(file)
                    .display()
                    .to_string(),
            );
        }
        fixture.set("MUTATION_SHARD", shard);
        if in_place {
            fixture.set("MUTATION_IN_PLACE", "true");
        }
        if !shard.is_empty() {
            fixture.set(
                "REPORTS",
                &fixture.root.join("worker-reports").display().to_string(),
            );
        }
        fixture.stub(
            "cargo",
            r#"mkdir -p "$RUNNER_TEMP/mutants/mutants.out"
printf '{"caught":2,"missed":0,"timeout":0,"unviable":0}' > \
"$RUNNER_TEMP/mutants/mutants.out/outcomes.json""#,
        );
        succeeds(&fixture.run_body("rust-gate mutants"));
        assert!(!fixture.calls().contains("--in-diff"));
        assert_eq!(fixture.calls().contains("--in-place"), in_place);
        fixture.set("MUTATION_FULL_SCOPE", "false");
        fixture.set(
            "REPORTS",
            &fixture.root.join("reports").display().to_string(),
        );
        let message = if shard.is_empty() {
            "mutation plan full scope differs from its execution scope"
        } else {
            "mutation worker identity or scope differs from its plan; Re-run all jobs"
        };
        refused(&fixture.run_body("rust-gate mutants"), message);
    }
}

#[test]
fn malformed_in_place_execution_is_refused_before_running_mutants() {
    let mut fixture = planning_fixture(4, 1, 50);
    fixture.set("MUTATION_IN_PLACE", "yes");
    refused(
        &fixture.run_body("rust-gate mutants"),
        "mutation-in-place must be true or false",
    );
    assert!(!fixture.calls().contains("mutants --no-shuffle"));
}

#[test]
fn gate_unit_workers_install_the_tools_their_tests_invoke() {
    let data = workflow("gate-mutation");
    let installed: Vec<String> = data["jobs"]["mutations"]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(tool_rows)
        .map(|row| row.name)
        .collect();
    for tool in ["jaq", "cargo-mutants", "gitleaks"] {
        assert!(installed.iter().any(|name| name == tool), "{tool}");
    }
}

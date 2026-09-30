//! Mutation input forwarding and deterministic shard selection tests.

use crate::harness::{Fixture, output, planning_fixture, refused, succeeds, workflow};
use serde_json::{Value, json};
use std::fs;

#[test]
fn mutation_inputs_keep_serial_defaults_and_publisher_forwarding() {
    let ci = workflow("ci");
    let inputs = &ci["on"]["workflow_call"]["inputs"];
    assert_eq!(inputs["mutation-shards"]["type"], "number");
    assert_eq!(inputs["mutation-shards"]["default"], 1);
    assert_eq!(inputs["mutation-mutants-per-shard"]["type"], "number");
    assert_eq!(inputs["mutation-mutants-per-shard"]["default"], 50);
    assert!(
        inputs["mutation-shards"]["description"]
            .as_str()
            .unwrap()
            .contains("automatic")
    );
    for publisher in ["publish-binaries", "publish-crate"] {
        let caller = workflow(publisher);
        for name in ["mutation-shards", "mutation-mutants-per-shard"] {
            assert_eq!(
                caller["on"]["workflow_call"]["inputs"][name]["type"],
                "number"
            );
            assert_eq!(
                caller["on"]["workflow_call"]["inputs"][name]["default"],
                inputs[name]["default"]
            );
            assert_eq!(
                caller["jobs"]["ci"]["with"][name],
                format!("${{{{ inputs.{name} }}}}")
            );
        }
    }
}

#[test]
fn disabled_and_default_plans_never_invoke_cargo_mutants() {
    for (enabled, shards, mode, count) in
        [("false", "0", "disabled", "0"), ("true", "1", "inline", "")]
    {
        let mut fixture = Fixture::new();
        fixture.set("MUTATION_TEST", enabled);
        fixture.set("MUTATION_SHARDS", shards);
        succeeds(&fixture.run_body("rust-gate mutants-plan"));
        assert_eq!(output(&fixture, "mutation-mode"), mode);
        assert_eq!(output(&fixture, "mutation-count"), count);
        assert_eq!(output(&fixture, "mutation-matrix"), "[]");
        assert!(fixture.calls().is_empty(), "{}", fixture.calls());
    }
}

#[test]
fn automatic_counts_choose_only_nonempty_complete_shards() {
    for (count, target, mode, shards, matrix) in [
        (0, 50, "empty", "0", "[]"),
        (1, 50, "inline", "1", "[]"),
        (50, 50, "inline", "1", "[]"),
        (51, 50, "sharded", "2", "[0,1]"),
        (100, 50, "sharded", "2", "[0,1]"),
        (6895, 50, "sharded", "138", ""),
    ] {
        let fixture = planning_fixture(count, 0, target);
        succeeds(&fixture.run_body("rust-gate mutants-plan"));
        assert_eq!(output(&fixture, "mutation-mode"), mode, "M={count}");
        assert_eq!(output(&fixture, "mutation-count"), count.to_string());
        assert_eq!(output(&fixture, "mutation-shards"), shards);
        if count == 6895 {
            let matrix: Vec<usize> =
                serde_json::from_str(&output(&fixture, "mutation-matrix")).unwrap();
            assert_eq!(matrix, (0..138).collect::<Vec<_>>());
        } else {
            assert_eq!(output(&fixture, "mutation-matrix"), matrix);
        }
    }
}

#[test]
fn automatic_planning_refuses_when_the_matrix_cannot_meet_its_target() {
    let mut fixture = planning_fixture(12801, 0, 50);
    fixture.set("MUTATION_WINDOWS", "[]");
    let result = fixture.run_body("rust-gate mutants-plan");
    refused(
        &result,
        concat!(
            "automatic mutation plan needs 257 shards for 12801 mutants at target 50; ",
            "GitHub's matrix limit is 256"
        ),
    );
    assert!(String::from_utf8_lossy(&result.stderr).contains("; GitHub's matrix limit is 256"));
}

#[test]
fn fixed_counts_reduce_to_the_number_of_available_mutants() {
    for (requested, expected, mode, matrix) in [
        (2, 2, "sharded", "[0,1]"),
        (5, 5, "sharded", "[0,1,2,3,4]"),
        (8, 5, "sharded", "[0,1,2,3,4]"),
        (64, 5, "sharded", "[0,1,2,3,4]"),
        (200, 5, "sharded", "[0,1,2,3,4]"),
    ] {
        let fixture = planning_fixture(5, requested, 1);
        succeeds(&fixture.run_body("rust-gate mutants-plan"));
        assert_eq!(output(&fixture, "mutation-mode"), mode);
        assert_eq!(output(&fixture, "mutation-shards"), expected.to_string());
        assert_eq!(output(&fixture, "mutation-matrix"), matrix);
    }
}

#[test]
fn planning_preserves_scope_and_refuses_unexplained_empty_output() {
    let fixture = planning_fixture(2, 2, 50);
    succeeds(&fixture.run_body("rust-gate mutants-plan"));
    let calls = fixture.calls();
    assert!(calls.contains("--list --json"), "{calls}");
    assert!(calls.contains("--no-shuffle"), "{calls}");
    assert!(calls.contains("--cargo-arg=--locked"), "{calls}");
    assert!(calls.contains("--colors=never"), "{calls}");
    assert!(calls.contains("--level=info"), "{calls}");
    assert!(calls.contains("--in-diff"), "{calls}");
    assert!(fixture.root.join("reports/mutants-list.json").is_file());
    assert!(fixture.root.join("reports/mutation-plan.json").is_file());

    let unexplained = planning_fixture(1, 2, 50);
    unexplained.stub("cargo", "exit 0");
    refused(
        &unexplained.run_body("rust-gate mutants-plan"),
        "cargo-mutants listing was empty without a recognized no-work result",
    );
    let malformed = planning_fixture(1, 2, 50);
    fs::write(malformed.root.join("listing.json"), "{").unwrap();
    refused(
        &malformed.run_body("rust-gate mutants-plan"),
        "cargo-mutants listing is malformed JSON",
    );
    let failed = planning_fixture(1, 2, 50);
    failed.stub("cargo", "exit 12");
    assert_eq!(
        failed.run_body("rust-gate mutants-plan").status.code(),
        Some(12)
    );
}

#[test]
fn linux_mutation_listing_excludes_windows_owned_files() {
    let mut fixture = planning_fixture(2, 2, 50);
    fs::write(fixture.root.join("project/src/windows.rs"), "").unwrap();
    fixture.set("MUTATION_WINDOWS", "[\"src/windows.rs\"]");
    succeeds(&fixture.run_body("rust-gate mutants-plan"));
    assert!(fixture.calls().contains("--exclude src/windows.rs"));
}

#[test]
fn plan_rejects_duplicate_listing_identity_and_invalid_first_parent() {
    let duplicate = planning_fixture(2, 2, 50);
    let path = duplicate.root.join("listing.json");
    let mut listing: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    listing[1]["name"] = listing[0]["name"].clone();
    fs::write(path, serde_json::to_vec(&listing).unwrap()).unwrap();
    refused(
        &duplicate.run_body("rust-gate mutants-plan"),
        "cargo-mutants listing contains duplicate mutant identities",
    );

    let invalid_parent = planning_fixture(2, 2, 50);
    invalid_parent.stub("git", "printf 'not-a-sha\\n'");
    refused(
        &invalid_parent.run_body("rust-gate mutants-plan"),
        "git returned an invalid first-parent SHA",
    );
}

#[test]
fn inline_mutation_uses_only_a_verified_plan_scope() {
    for (parent, digest, diff, message) in [
        (
            "",
            "invalid",
            None,
            "mutation plan has an invalid full-workspace digest",
        ),
        (
            "",
            "unused",
            Some("diff"),
            "parentless mutation plan unexpectedly contains a diff",
        ),
        (
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "invalid",
            Some("different diff"),
            "mutation plan diff digest does not match its report",
        ),
    ] {
        let mut fixture = Fixture::new();
        let report = fixture.root.join("reports");
        let plan = json!({"first_parent": parent, "diff_sha256": digest});
        fs::write(
            report.join("mutation-plan.json"),
            serde_json::to_vec(&plan).unwrap(),
        )
        .unwrap();
        if let Some(diff) = diff {
            fs::write(report.join("mutants.diff"), diff).unwrap();
        }
        fixture.set("MUTATION_TEST", "true");
        refused(&fixture.run_body("rust-gate mutants"), message);
    }
}

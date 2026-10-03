//! Real package compilation and configured exact-selection regressions.

use crate::harness::{
    compile_aggregate_fixture, compile_fixture, evidence_hash, refused, succeeds, summarize_engine,
};
use serde_json::{Value, json};
use std::fs;

#[test]
fn integration_binary_uplift_retains_only_exact_compiler_dep_info() {
    for probe in ["uplifted-bin", "uplifted-bin-nextest"] {
        let mut fixture = compile_fixture(probe);
        fixture.set("MUTATION_SHARD", "0/1");
        fixture.set("CARGO_BUILD_TARGET", "x86_64-unknown-linux-gnu");
        succeeds(&fixture.run_body("rust-gate mutants-engine-default"));
        let root = fixture
            .root
            .join("mutants-engine-default/mutants.out/builds/0");
        let records = fs::read_to_string(root.join("cargo-build.json")).unwrap();
        assert!(records.contains("/x86_64-unknown-linux-gnu/debug/crate-a\""));
        assert!(
            !fixture
                .root
                .join("engine-control-target/0/x86_64-unknown-linux-gnu/debug/crate-a.d")
                .exists()
        );
        assert!(fs::read_dir(root.join("dep-info")).unwrap().any(|entry| {
            fs::read_to_string(entry.unwrap().path())
                .unwrap()
                .contains("src/main.rs")
        }));
    }
}

#[test]
fn nextest_cargo_verbosity_preserves_artifact_hashes_and_dep_info_bytes() {
    let mut fixture = compile_fixture("uplifted-bin-nextest");
    fixture.set("CARGO_BUILD_TARGET", "x86_64-unknown-linux-gnu");
    let mut previous = None;
    for instrumentation in ["", "--cargo-verbose"] {
        succeeds(&fixture.run_body(&format!(
            "cd project && cargo nextest run --no-run --verbose {instrumentation} \
             --package=crate-a@0.1.0 --locked --cargo-message-format=json \
             --target-dir ../verbosity-target > ../verbosity-records.json"
        )));
        succeeds(&fixture.run_body(
            "find verbosity-target -name '*.d' -print0 | sort -z | \
             xargs -0 sha256sum > verbosity-deps.txt",
        ));
        let mut artifacts: Vec<_> = fs::read_to_string(fixture.root.join("verbosity-records.json"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .filter(|record| record["reason"] == "compiler-artifact")
            .map(|record| record["filenames"].to_string())
            .collect();
        artifacts.sort();
        assert!(!artifacts.is_empty());
        let digests = fs::read(fixture.root.join("verbosity-deps.txt")).unwrap();
        assert!(!digests.is_empty());
        if let Some(expected) = &previous {
            assert_eq!(&(artifacts, digests), expected);
        } else {
            previous = Some((artifacts, digests));
        }
        fs::remove_dir_all(fixture.root.join("verbosity-target")).unwrap();
    }
}

#[test]
fn workspace_feature_unification_cannot_hide_package_compiled_survivors() {
    let mut fixture = compile_fixture("workspace");
    fixture.set("MUTATION_SHARD", "0/1");
    refused(
        &fixture.run_body("rust-gate mutants-engine-default"),
        "partition contains a survivor, timeout or untested mutant",
    );
}

#[test]
fn default_cap_lints_preserves_compilation_affecting_cargo_configuration() {
    let mut fixture = compile_fixture("rustflags");
    fixture.set("MUTATION_SHARD", "0/1");
    refused(
        &fixture.run_body("rust-gate mutants-engine-default"),
        "partition contains a survivor, timeout or untested mutant",
    );
}

#[test]
fn configured_inclusion_regex_cannot_broaden_either_compiled_shard() {
    let mut fixture = compile_fixture("regex");
    for shard in ["0/2", "1/2"] {
        fixture.set("MUTATION_SHARD", shard);
        succeeds(&fixture.run_body("rust-gate mutants-engine-default"));
        let retained = fs::read_to_string(
            fixture
                .root
                .join("mutants-engine-default/mutants.out/selection-config.toml"),
        )
        .unwrap();
        assert!(!retained.contains("examine_re"));
        assert!(retained.contains("cap_lints = false"));
    }
}

#[test]
fn nextest_non_members_have_no_test_phase_in_a_mixed_control_assignment() {
    let mut fixture = compile_fixture("nextest");
    fixture.set("MUTATION_SHARD", "0/1");
    succeeds(&fixture.run_body("rust-gate mutants-engine-default"));
    let root = fixture.root.join("mutants-engine-default/mutants.out");
    let outcomes: Value =
        serde_json::from_slice(&fs::read(root.join("outcomes.json")).unwrap()).unwrap();
    assert_eq!(outcomes["not_compiled_without_features"], 2);
    assert_eq!(outcomes["caught"], 2);
    for outcome in outcomes["outcomes"].as_array().unwrap() {
        if outcome["summary"] == "NotCompiledWithoutFeatures" {
            assert_eq!(outcome["phase_results"], json!([]));
        }
    }
    let build: Value =
        serde_json::from_slice(&fs::read(root.join("builds/0/build-record.json")).unwrap())
            .unwrap();
    assert_eq!(
        build["argv"],
        json!([
            "cargo",
            "nextest",
            "run",
            "--no-run",
            "--verbose",
            "--cargo-verbose",
            "--package=crate-a@0.1.0",
            "--locked",
            "--cargo-message-format=json",
            "--target-dir",
            fixture.root.join("engine-control-target/0")
        ])
    );
}

#[test]
fn nextest_compiled_survivors_are_rejected_after_a_verified_membership_build() {
    let mut fixture = compile_fixture("nextest-survivor");
    fixture.set("MUTATION_SHARD", "0/1");
    refused(
        &fixture.run_body("rust-gate mutants-engine-default"),
        "partition contains a survivor, timeout or untested mutant",
    );
    assert!(
        fixture
            .trace()
            .contains("cargo nextest run --no-run --verbose")
    );
}

#[test]
fn nextest_aggregation_rejects_bound_argv_drift_even_with_refreshed_digests() {
    let fixture = compile_aggregate_fixture("nextest");
    let root = fixture
        .root
        .join("engine-default/fixture-engine-default-mutants-0")
        .join("mutants-engine-default/mutants.out");
    let build = root.join("builds/0/build-record.json");
    let original = fs::read(&build).unwrap();
    succeeds(&summarize_engine(&fixture));
    for (index, argument) in [
        (1, "test"),
        (6, "--workspace"),
        (8, "--message-format=json"),
    ] {
        let mut record: Value = serde_json::from_slice(&original).unwrap();
        record["argv"][index] = json!(argument);
        fs::write(&build, record.to_string()).unwrap();
        let manifest = root.join("compile-membership.json");
        let mut binding: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        binding["packages"][0]["build_sha256"] = json!(evidence_hash(&build));
        fs::write(manifest, binding.to_string()).unwrap();
        refused(
            &summarize_engine(&fixture),
            "featureless compile membership command is not equivalent",
        );
    }
}

#[test]
fn root_directory_spelling_preserves_verified_single_package_membership() {
    let mut fixture = compile_fixture("nextest");
    fixture.set("MUTATION_SHARD", "0/1");
    succeeds(&fixture.run_body("rust-gate mutants-engine-default"));
    let root = fixture.root.join("mutants-engine-default/mutants.out");
    let source: Value =
        serde_json::from_slice(&fs::read(root.join("source-metadata.json")).unwrap()).unwrap();
    assert_eq!(source["project"], source["checkout"]);
    let outcomes: Value =
        serde_json::from_slice(&fs::read(root.join("outcomes.json")).unwrap()).unwrap();
    assert_eq!(outcomes["not_compiled_without_features"], 2);
}

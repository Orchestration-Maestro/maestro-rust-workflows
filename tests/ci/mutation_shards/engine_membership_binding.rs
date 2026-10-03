//! Membership requires source metadata and a retained equivalent compile command.

use crate::harness::{
    engine_aggregation_fixture, engine_fallback_fixture, engine_fixture, evidence_hash, refused,
    succeeds, summarize_engine,
};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

#[test]
fn unverified_controls_accept_engine_only_survivors_with_exact_caught_twins() {
    let fixture = engine_fallback_fixture(true);
    let worker =
        fs::read_to_string(fixture.root.join("reports/mutants-engine-default.txt")).unwrap();
    assert!(worker.contains("compiled-survivor rejection inactive"));
    succeeds(&summarize_engine(&fixture));
    let report = fs::read_to_string(fixture.root.join("reports/mutants.txt")).unwrap();
    assert!(report.contains("inactive without features, caught with engine"));
    let merged: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("reports/mutants.json")).unwrap())
            .unwrap();
    assert_eq!(
        merged["inactive_without_features_caught_with_engine"]["total"],
        1
    );
    assert_eq!(merged["not_compiled_without_features"], 0);
}

/// Retain a semantic change's hash so digest checks do not mask the guard under test.
fn refresh_manifest(manifest: &Path, key: &str, evidence: &Path) {
    let mut binding: Value = serde_json::from_slice(&fs::read(manifest).unwrap()).unwrap();
    binding[key] = json!(evidence_hash(evidence));
    fs::write(manifest, binding.to_string()).unwrap();
}

#[test]
fn retained_source_metadata_requires_workspace_shape_directory_and_planned_owners() {
    let fixture = engine_aggregation_fixture();
    let root = fixture
        .root
        .join("engine-default/fixture-engine-default-mutants-0")
        .join("mutants-engine-default/mutants.out");
    let source = root.join("source-metadata.json");
    let manifest = root.join("compile-membership.json");
    let original: Value = serde_json::from_slice(&fs::read(&source).unwrap()).unwrap();
    let binding: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    for (pointer, value) in [
        ("/metadata/workspace_root", json!("relative")),
        ("/metadata/workspace_root", json!(1)),
        ("/checkout", json!("/foreign")),
        ("/checkout", json!(1)),
        ("/metadata/packages/0/name", json!("foreign")),
        (
            "/metadata/packages/0/manifest_path",
            json!("/foreign/Cargo.toml"),
        ),
    ] {
        let mut changed = original.clone();
        *changed.pointer_mut(pointer).unwrap() = value.clone();
        fs::write(&source, changed.to_string()).unwrap();
        let mut changed_binding = binding.clone();
        if pointer == "/metadata/workspace_root" {
            changed_binding["workspace"] = value;
        }
        fs::write(&manifest, changed_binding.to_string()).unwrap();
        refresh_manifest(&manifest, "source_sha256", &source);
        refused(
            &summarize_engine(&fixture),
            "featureless compile membership binding differs from its plan",
        );
    }
}

#[test]
fn retained_compilation_settings_and_arguments_require_exact_equivalence() {
    let fixture = engine_aggregation_fixture();
    let root = fixture
        .root
        .join("engine-default/fixture-engine-default-mutants-0")
        .join("mutants-engine-default/mutants.out");
    let manifest = root.join("compile-membership.json");
    let config = root.join("compile-config.toml");
    fs::write(&config, "cap_lints = true\n").unwrap();
    refresh_manifest(&manifest, "config_sha256", &config);
    refused(
        &summarize_engine(&fixture),
        "featureless compile membership command is not equivalent",
    );
    fs::write(&config, "").unwrap();
    refresh_manifest(&manifest, "config_sha256", &config);
    let build = root.join("build-record.json");
    let mut record: Value = serde_json::from_slice(&fs::read(&build).unwrap()).unwrap();
    record["argv"][4] = json!("--workspace");
    fs::write(&build, record.to_string()).unwrap();
    refresh_manifest(&manifest, "build_sha256", &build);
    refused(
        &summarize_engine(&fixture),
        "featureless compile membership command is not equivalent",
    );
}

#[test]
fn unverified_package_version_and_compilation_configuration_test_all_assigned_mutants() {
    let mut fixture = engine_fixture(true);
    fixture.set("CONTROL_COMPILED", "false");
    let mut metadata: Value = serde_json::from_str(&fixture.env["METADATA"]).unwrap();
    metadata["packages"][0]["version"] = json!(null);
    fixture.set("METADATA", &metadata.to_string());
    succeeds(&fixture.run_body("rust-gate mutants-engine-default"));
    assert!(!fixture.trace().contains("cargo test --no-run"));
    let root = fixture.root.join("mutants-engine-default/mutants.out");
    let outcomes: Value =
        serde_json::from_slice(&fs::read(root.join("outcomes.json")).unwrap()).unwrap();
    assert_eq!(outcomes["not_compiled_without_features"], 0);
    fs::create_dir_all(fixture.root.join(".cargo")).unwrap();
    metadata["workspace_root"] = json!(fixture.root);
    for config in [
        "cap_lints = true\n",
        "additional_cargo_args = ['--release']\n",
    ] {
        fs::write(fixture.root.join(".cargo/mutants.toml"), config).unwrap();
        metadata["packages"][0]["version"] = json!("0.1.0");
        fixture.set("METADATA", &metadata.to_string());
        fs::write(fixture.root.join("trace"), "").unwrap();
        succeeds(&fixture.run_body("rust-gate mutants-engine-default"));
        assert!(!fixture.trace().contains("cargo test --no-run"));
        let selected: Value =
            serde_json::from_slice(&fs::read(root.join("tested-mutants.json")).unwrap()).unwrap();
        assert_eq!(selected.as_array().unwrap().len(), 1);
        let retained = fs::read_to_string(root.join("selection-config.toml")).unwrap();
        assert!(retained.contains(config.split('=').next().unwrap().trim()));
    }
}

#[test]
fn nested_control_source_frames_conservatively_test_every_assigned_mutant() {
    let mut fixture = engine_fixture(true);
    fixture.set("CONTROL_COMPILED", "false");
    let mut metadata: Value = serde_json::from_str(&fixture.env["METADATA"]).unwrap();
    metadata["workspace_root"] = json!(fixture.root);
    fixture.set("METADATA", &metadata.to_string());
    succeeds(&fixture.run_body("rust-gate mutants-engine-default"));
    assert!(!fixture.trace().contains("cargo test --no-run"));
    let outcomes: Value = serde_json::from_slice(
        &fs::read(
            fixture
                .root
                .join("mutants-engine-default/mutants.out/outcomes.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(outcomes["not_compiled_without_features"], 0);
    assert_eq!(outcomes["caught"], 1);
}

#[test]
fn retained_artifact_paths_cannot_escape_the_verified_clean_target() {
    let fixture = engine_aggregation_fixture();
    let root = fixture
        .root
        .join("engine-default/fixture-engine-default-mutants-0")
        .join("mutants-engine-default/mutants.out");
    let cargo = root.join("cargo-build.json");
    let mut records: Vec<Value> = fs::read_to_string(&cargo)
        .unwrap()
        .lines()
        .map(|row| serde_json::from_str(row).unwrap())
        .collect();
    records[0]["filenames"] = json!(["/foreign-target/debug/deps/libfixture.rlib"]);
    fs::write(
        &cargo,
        records
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    refresh_manifest(
        &root.join("compile-membership.json"),
        "cargo_sha256",
        &cargo,
    );
    refused(
        &summarize_engine(&fixture),
        "featureless build dep-info escapes its clean target",
    );
}

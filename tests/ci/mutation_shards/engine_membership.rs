//! Compile membership, not redundant default test runs, proves featureless inactivity.

use crate::harness::{
    engine_aggregation_fixture, engine_fixture, engine_inactive_fixture, refused, succeeds,
    summarize_engine, tool,
};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

#[test]
fn feature_only_mutants_are_classified_without_running_default_tests() {
    let mut fixture = engine_fixture(true);
    fixture.set("CONTROL_COMPILED", "false");
    fs::write(fixture.root.join("trace"), "").unwrap();
    succeeds(&fixture.run_body("rust-gate mutants-engine-default"));
    let trace = fixture.trace();
    assert!(trace.contains("cargo test --workspace --all-targets --no-run --locked"));
    assert!(!trace.lines().any(|line| line.contains("cargo mutants")));
    let outcomes: Value = serde_json::from_slice(
        &fs::read(
            fixture
                .root
                .join("mutants-engine-default/mutants.out/outcomes.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(outcomes["not_compiled_without_features"], 1);
    assert_eq!(
        outcomes["outcomes"][1]["summary"],
        "NotCompiledWithoutFeatures"
    );
    assert_eq!(outcomes["missed"], 0);
}

#[test]
fn default_compiled_control_survivors_fail_even_with_a_caught_engine_twin() {
    let fixture = engine_fixture(true);
    let path = fixture.root.join("default-outcomes.json");
    let mut outcomes: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    outcomes["caught"] = json!(0);
    outcomes["missed"] = json!(1);
    outcomes["outcomes"][1]["summary"] = json!("MissedMutant");
    outcomes["outcomes"][1]["phase_results"][0]["process_status"] = json!("Success");
    fs::write(path, outcomes.to_string()).unwrap();
    refused(
        &fixture.run_body("rust-gate mutants-engine-default"),
        "partition contains a survivor, timeout or untested mutant",
    );
}

#[test]
fn aggregate_refuses_unverified_or_source_mismatched_membership_classifications() {
    let fixture = engine_aggregation_fixture();
    let root = fixture
        .root
        .join("engine-default/fixture-engine-default-mutants-0/mutants-engine-default/mutants.out");
    let path = root.join("outcomes.json");
    let mut outcomes: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    outcomes["caught"] = json!(0);
    outcomes["not_compiled_without_features"] = json!(1);
    outcomes["outcomes"][1]["summary"] = json!("NotCompiledWithoutFeatures");
    outcomes["outcomes"][1]["phase_results"] = json!([]);
    fs::write(&path, outcomes.to_string()).unwrap();
    refused(
        &summarize_engine(&fixture),
        "featureless compile membership differs from its outcomes",
    );
    fs::remove_file(root.join("compile-membership.json")).unwrap();
    refused(
        &summarize_engine(&fixture),
        "featureless compile membership evidence is missing",
    );
}

#[test]
fn retained_membership_rejects_binding_drift_digest_drift_and_incomplete_units() {
    let fixture = engine_aggregation_fixture();
    let root = fixture
        .root
        .join("engine-default/fixture-engine-default-mutants-0/mutants-engine-default/mutants.out");
    let manifest = root.join("compile-membership.json");
    let saved = fs::read(&manifest).unwrap();
    for (pointer, value, message) in [
        (
            "/schema",
            json!(2),
            "featureless compile membership binding differs from its plan",
        ),
        (
            "/binding/sha",
            json!("wrong"),
            "featureless compile membership binding differs from its plan",
        ),
        (
            "/binding/attempt",
            json!("2"),
            "featureless compile membership binding differs from its plan",
        ),
        (
            "/cargo_sha256",
            json!("wrong"),
            "featureless compile membership cargo digest differs",
        ),
        (
            "/build_sha256",
            json!("wrong"),
            "featureless compile membership build record digest differs",
        ),
        (
            "/dep_sha256",
            json!([]),
            "featureless compile membership omits dep-info units",
        ),
        (
            "/dep_sha256/0",
            json!("wrong"),
            "featureless compile membership dep-info digest differs",
        ),
        (
            "/target",
            json!("/wrong"),
            "featureless build dep-info escapes its clean target",
        ),
        (
            "/target",
            json!(""),
            "featureless fallback contains unexpected compile evidence",
        ),
    ] {
        let mut changed: Value = serde_json::from_slice(&saved).unwrap();
        *changed.pointer_mut(pointer).unwrap() = value;
        fs::write(&manifest, changed.to_string()).unwrap();
        refused(&summarize_engine(&fixture), message);
    }
    fs::write(&manifest, saved).unwrap();
    succeeds(&summarize_engine(&fixture));
}

#[test]
fn aggregation_rechecks_successful_fresh_cargo_output_and_raw_dependency_rules() {
    let fixture = engine_aggregation_fixture();
    let root = fixture
        .root
        .join("engine-default/fixture-engine-default-mutants-0/mutants-engine-default/mutants.out");
    let manifest = root.join("compile-membership.json");
    let saved = fs::read(&manifest).unwrap();
    let cargo = root.join("cargo-build.json");
    let original = fs::read_to_string(&cargo).unwrap();
    for (key, value) in [("fresh", true), ("success", false)] {
        let mut rows: Vec<Value> = original
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let row = if key == "fresh" {
            &mut rows[0]
        } else {
            &mut rows[1]
        };
        row[key] = json!(value);
        fs::write(
            &cargo,
            rows.iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let mut binding: Value = serde_json::from_slice(&saved).unwrap();
        binding["cargo_sha256"] = json!(evidence_hash(&cargo));
        fs::write(&manifest, binding.to_string()).unwrap();
        refused(
            &summarize_engine(&fixture),
            "featureless build did not verify fresh compile membership",
        );
    }
    for (filename, message) in [
        ("", "featureless artifact filename is invalid"),
        (
            "/target/nohash/build-script-build",
            "featureless build-script artifact filename is invalid",
        ),
    ] {
        let mut rows: Vec<Value> = original
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        rows[0]["filenames"] = json!([filename]);
        fs::write(
            &cargo,
            rows.iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let mut binding: Value = serde_json::from_slice(&saved).unwrap();
        binding["cargo_sha256"] = json!(evidence_hash(&cargo));
        fs::write(&manifest, binding.to_string()).unwrap();
        refused(&summarize_engine(&fixture), message);
    }
    fs::write(&cargo, original).unwrap();
    let dep = root.join("dep-info/0.d");
    let original = fs::read(&dep).unwrap();
    for (contents, message) in [
        ("", "featureless dep-info is empty"),
        ("malformed", "featureless dep-info has no dependency rule"),
        (
            "a: src/unfinished\\",
            "featureless dep-info has an incomplete escape",
        ),
        ("a: ", "featureless dep-info has no source dependencies"),
        (
            "a: src/engine.rs",
            "featureless compile membership omits an artifact source",
        ),
    ] {
        fs::write(&dep, contents).unwrap();
        let mut binding: Value = serde_json::from_slice(&saved).unwrap();
        binding["dep_sha256"][0] = json!(evidence_hash(&dep));
        fs::write(&manifest, binding.to_string()).unwrap();
        refused(&summarize_engine(&fixture), message);
    }
    fs::write(&dep, original).unwrap();
    fs::write(&manifest, saved).unwrap();
    succeeds(&summarize_engine(&fixture));
}

#[test]
fn compilation_changing_config_falls_back_to_testing_every_assigned_mutant() {
    let mut fixture = engine_fixture(true);
    let mut metadata: Value = serde_json::from_str(&fixture.env["METADATA"]).unwrap();
    // Exercise a package working directory with configuration at its workspace root.
    metadata["workspace_root"] = json!(fixture.root);
    fixture.set("METADATA", &metadata.to_string());
    fs::create_dir_all(fixture.root.join(".cargo")).unwrap();
    fs::write(
        fixture.root.join(".cargo/mutants.toml"),
        "additional_cargo_args = ['--release']\n",
    )
    .unwrap();
    fs::write(fixture.root.join("trace"), "").unwrap();
    succeeds(&fixture.run_body("rust-gate mutants-engine-default"));
    let trace = fixture.trace();
    assert!(trace.contains("cargo mutants"));
    assert!(!trace.contains("cargo test --workspace --all-targets --no-run"));
    let root = fixture.root.join("mutants-engine-default/mutants.out");
    let outcomes: Value =
        serde_json::from_slice(&fs::read(root.join("outcomes.json")).unwrap()).unwrap();
    assert_eq!(outcomes["not_compiled_without_features"], 0);
    assert_eq!(outcomes["caught"], 1);
    assert!(
        fs::read_to_string(fixture.root.join("reports/mutants-engine-default.txt"))
            .unwrap()
            .contains(
                "cargo-mutants configuration changes compilation; testing every assigned mutant"
            )
    );
}

#[test]
fn membership_uses_pinned_mutants_encoded_flag_precedence_not_cargo_config_flags() {
    let mut fixture = engine_fixture(true);
    fixture.set("CONTROL_COMPILED", "false");
    fixture.set("RUSTFLAGS", "-D warnings --cfg fixture_flag");
    for (encoded, expected) in [
        (
            None,
            "-D\u{1f}warnings\u{1f}--cfg\u{1f}fixture_flag\u{1f}--cap-lints=warn",
        ),
        (Some(""), "--cap-lints=warn"),
        (
            Some("--cfg\u{1f}other"),
            "--cfg\u{1f}other\u{1f}--cap-lints=warn",
        ),
    ] {
        if let Some(encoded) = encoded {
            fixture.set("CARGO_ENCODED_RUSTFLAGS", encoded);
        }
        succeeds(&fixture.run_body("rust-gate mutants-engine-default"));
        let record: Value = serde_json::from_slice(
            &fs::read(
                fixture
                    .root
                    .join("mutants-engine-default/mutants.out/build-record.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(record["encoded_rustflags"], expected);
    }
}

/// Refresh a digest to exercise semantic refusal rather than only byte-integrity refusal.
fn evidence_hash(path: &Path) -> String {
    let result = tool("sha256sum").arg(path).output().unwrap();
    succeeds(&result);
    String::from_utf8(result.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned()
}

#[test]
fn aggregation_accepts_verified_non_members_only_with_exact_caught_engine_twins() {
    let fixture = engine_inactive_fixture();
    succeeds(&summarize_engine(&fixture));
    let engine = fixture
        .root
        .join("engine/fixture-engine-mutants-0/mutants-engine/mutants.out/outcomes.json");
    let original = fs::read(&engine).unwrap();
    let mut changed: Value = serde_json::from_slice(&original).unwrap();
    changed["caught"] = json!(0);
    changed["unviable"] = json!(1);
    changed["outcomes"][1]["summary"] = json!("Unviable");
    changed["outcomes"][1]["phase_results"][0]["phase"] = json!("Build");
    fs::write(&engine, changed.to_string()).unwrap();
    refused(
        &summarize_engine(&fixture),
        "non-compiled featureless control is not caught by its exact engine twin",
    );
    fs::write(&engine, original).unwrap();
    let dep = fixture
        .root
        .join("engine-default/fixture-engine-default-mutants-0")
        .join("mutants-engine-default/mutants.out/dep-info/0.d");
    let original = fs::read(&dep).unwrap();
    fs::write(&dep, b"a: src/lib.rs src/engine.rs\n").unwrap();
    let manifest = dep
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("compile-membership.json");
    let saved = fs::read(&manifest).unwrap();
    let mut binding: Value = serde_json::from_slice(&saved).unwrap();
    binding["dep_sha256"][0] = json!(evidence_hash(&dep));
    fs::write(&manifest, binding.to_string()).unwrap();
    refused(
        &summarize_engine(&fixture),
        "featureless compile membership differs from its outcomes",
    );
    fs::write(&dep, original).unwrap();
    fs::write(&manifest, saved).unwrap();
    succeeds(&summarize_engine(&fixture));
}

#[test]
fn aggregation_refuses_a_tested_subset_that_does_not_equal_compile_membership() {
    let fixture = engine_aggregation_fixture();
    let root = fixture
        .root
        .join("engine-default/fixture-engine-default-mutants-0")
        .join("mutants-engine-default/mutants.out");
    let dep = root.join("dep-info/0.d");
    fs::write(&dep, "a: src/lib.rs src/nested/../engine.rs\n").unwrap();
    let manifest = root.join("compile-membership.json");
    let mut binding: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    binding["dep_sha256"][0] = json!(evidence_hash(&dep));
    fs::write(manifest, binding.to_string()).unwrap();
    succeeds(&summarize_engine(&fixture));
    let discovery = fixture
        .root
        .join("engine-default/fixture-engine-default-mutants-0")
        .join("mutants-engine-default/mutants.out/tested-mutants.json");
    fs::write(discovery, "[]").unwrap();
    refused(
        &summarize_engine(&fixture),
        "featureless tested discovery differs from compile membership",
    );
}

//! Real package compilation and configured exact-selection regressions.

use crate::harness::{
    Fixture, copy_tree, engine_workspace, evidence_hash, fixture_git, output, refused, succeeds,
    summarize_engine,
};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

/// Bind the real workspace and planner before selecting a control shard.
fn planned_fixture(probe: &str) -> Fixture {
    let mut fixture = Fixture::new();
    let project = fixture.root.join("project");
    configure_source(&project, probe);
    for (key, value) in [
        ("DIRECTORY", "."),
        ("MUTATION_TEST", "true"),
        ("MUTATION_SHARDS", if probe == "regex" { "2" } else { "1" }),
        ("CARGO_BUILD_JOBS", "3"),
        ("CARGO_NET_OFFLINE", "true"),
        ("CARGO_INCREMENTAL", "0"),
        ("CARGO_MUTANTS_VERSION", "27.1.0"),
        ("MUTATION_ENGINE_FEATURES", "[\"engine\"]"),
        ("MUTATION_ENGINE_FILES", "[\"crates/a/src/engine.rs\"]"),
    ] {
        fixture.set(key, value);
    }
    if probe == "nextest" {
        fixture.set(
            "MUTATION_ENGINE_FILES",
            "[\"crates/a/src/engine.rs\",\"crates/a/src/engine_only.rs\"]",
        );
    }
    fixture.set("GITHUB_WORKSPACE", project.to_str().unwrap());
    succeeds(&fixture.run_body("cd project && cargo generate-lockfile --offline"));
    fixture_git(&project, &["init", "--quiet", "-b", "main"]);
    fixture_git(&project, &["config", "user.name", "Fixture"]);
    fixture_git(
        &project,
        &["config", "user.email", "fixture@example.invalid"],
    );
    fixture_git(&project, &["add", "--all"]);
    fixture_git(
        &project,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ],
    );
    fixture.set("GITHUB_SHA", &fixture_git(&project, &["rev-parse", "HEAD"]));
    succeeds(&fixture.run_body("rust-gate mutants-plan"));
    for (key, name) in [
        ("MUTATION_ENGINE_PLAN", "mutation-engine-plan.json"),
        ("MUTATION_ENGINE_LIST", "mutation-engine-list.json"),
        (
            "MUTATION_ENGINE_DEFAULT_LIST",
            "mutation-engine-default-list.json",
        ),
    ] {
        fixture.set(
            key,
            fixture.root.join("reports").join(name).to_str().unwrap(),
        );
    }
    fixture.set(
        "MUTATION_ENGINE_DEFAULT_COUNT",
        &output(&fixture, "mutation-engine-default-count"),
    );
    fixture
}

/// Each probe changes only the compilation or discovery condition it names.
fn configure_source(project: &Path, probe: &str) {
    engine_workspace(project, true);
    let lib = project.join("crates/a/src/lib.rs");
    let original = fs::read_to_string(&lib).unwrap();
    let replacement = match probe {
        "workspace" => "#[cfg(not(feature = \"other\"))]",
        "rustflags" => "#[cfg(default_active)]",
        _ => "",
    };
    fs::write(
        &lib,
        original.replace("#[cfg(feature = \"engine\")]", replacement),
    )
    .unwrap();
    if probe == "workspace" {
        for (path, from, to) in [
            (
                "crates/a/Cargo.toml",
                "[features]\n",
                "[features]\nother = []\n",
            ),
            (
                "crates/b/Cargo.toml",
                "crate-a = { path = '../a' }",
                "crate-a = { path = '../a', features = ['other'] }",
            ),
        ] {
            let path = project.join(path);
            fs::write(&path, fs::read_to_string(&path).unwrap().replace(from, to)).unwrap();
        }
    }
    fs::create_dir_all(project.join(".cargo")).unwrap();
    if probe == "rustflags" {
        fs::write(
            project.join(".cargo/config.toml"),
            "[build]\nrustflags = ['--cfg', 'default_active']\n",
        )
        .unwrap();
    }
    if probe == "regex" {
        fs::write(
            project.join(".cargo/mutants.toml"),
            "examine_re = ['engine_answer']\ncap_lints = false\n",
        )
        .unwrap();
    } else if probe == "nextest" {
        fs::write(
            project.join(".cargo/mutants.toml"),
            "test_tool = 'nextest'\n",
        )
        .unwrap();
        let engine = project.join("crates/a/src/engine.rs");
        let original = fs::read_to_string(&engine).unwrap();
        let lib_source = fs::read_to_string(&lib).unwrap();
        fs::write(
            &lib,
            format!("{lib_source}\n#[cfg(feature = \"engine\")]\npub mod engine_only;\n"),
        )
        .unwrap();
        fs::write(project.join("crates/a/src/engine_only.rs"), original).unwrap();
        let policy = project.join("maestro-quality.toml");
        fs::write(
            &policy,
            fs::read_to_string(&policy).unwrap().replace(
                "files = ['crates/a/src/engine.rs']",
                "files = ['crates/a/src/engine.rs', 'crates/a/src/engine_only.rs']",
            ),
        )
        .unwrap();
    } else {
        if probe == "nextest-survivor" {
            fs::write(
                project.join(".cargo/mutants.toml"),
                "test_tool = 'nextest'\n",
            )
            .unwrap();
        }
        let engine = project.join("crates/a/src/engine.rs");
        fs::write(
            &engine,
            fs::read_to_string(&engine)
                .unwrap()
                .replace("#[cfg(test)]", "#[cfg(all(test, feature = \"engine\"))]"),
        )
        .unwrap();
    }
}

#[test]
fn workspace_feature_unification_cannot_hide_package_compiled_survivors() {
    let mut fixture = planned_fixture("workspace");
    fixture.set("MUTATION_SHARD", "0/1");
    refused(
        &fixture.run_body("rust-gate mutants-engine-default"),
        "partition contains a survivor, timeout or untested mutant",
    );
}

#[test]
fn default_cap_lints_preserves_compilation_affecting_cargo_configuration() {
    let mut fixture = planned_fixture("rustflags");
    fixture.set("MUTATION_SHARD", "0/1");
    refused(
        &fixture.run_body("rust-gate mutants-engine-default"),
        "partition contains a survivor, timeout or untested mutant",
    );
}

#[test]
fn configured_inclusion_regex_cannot_broaden_either_compiled_shard() {
    let mut fixture = planned_fixture("regex");
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
    let mut fixture = planned_fixture("nextest");
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
        serde_json::from_slice(&fs::read(root.join("build-record.json")).unwrap()).unwrap();
    assert_eq!(
        build["argv"],
        json!([
            "cargo",
            "nextest",
            "run",
            "--no-run",
            "--verbose",
            "--package=crate-a@0.1.0",
            "--locked",
            "--cargo-message-format=json",
            "--target-dir",
            fixture.root.join("engine-control-target")
        ])
    );
}

#[test]
fn nextest_compiled_survivors_are_rejected_after_a_verified_membership_build() {
    let mut fixture = planned_fixture("nextest-survivor");
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

/// Transport real worker outputs with the same immutable plan and mode matrices as GitHub.
fn nextest_aggregate_fixture() -> Fixture {
    let mut fixture = planned_fixture("nextest");
    succeeds(&fixture.run_body("rust-gate mutants"));
    fixture.set("MUTATION_SHARD", "0/1");
    for step in ["mutants-engine", "mutants-engine-default"] {
        succeeds(&fixture.run_body(&format!("rust-gate {step}")));
    }
    let checks = fixture.root.join("checks");
    copy_tree(&fixture.root.join("reports"), &checks);
    copy_tree(&fixture.root.join("mutants"), &checks.join("mutants"));
    for (prefix, mode) in [
        ("engine", "mutants-engine"),
        ("engine-default", "mutants-engine-default"),
    ] {
        let worker = fixture
            .root
            .join(prefix)
            .join(format!("fixture-{prefix}-mutants-0"));
        copy_tree(&fixture.root.join(mode), &worker.join(mode));
        copy_tree(&fixture.root.join("reports"), &worker.join("rust-reports"));
        fixture.set(
            if prefix == "engine" {
                "MUTATION_ENGINE_ARTIFACTS"
            } else {
                "MUTATION_ENGINE_DEFAULT_ARTIFACTS"
            },
            fixture.root.join(prefix).to_str().unwrap(),
        );
    }
    for (key, value) in [
        ("MUTATION_MODE", "inline"),
        ("MUTATION_MATRIX", "[]"),
        ("MUTATION_ARTIFACT_NAME", "fixture"),
        ("MUTATION_ENGINE_COUNT", "4"),
        ("MUTATION_ENGINE_DEFAULT_COUNT", "4"),
        ("MUTATION_ENGINE_SHARDS", "1"),
        ("MUTATION_ENGINE_DEFAULT_SHARDS", "1"),
        ("MUTATION_ENGINE_MATRIX", "[0]"),
        ("MUTATION_ENGINE_DEFAULT_MATRIX", "[0]"),
        ("ENGINE_MUTATIONS_RESULT", "success"),
        ("ENGINE_DEFAULT_MUTATIONS_RESULT", "success"),
        ("CHECKS_RESULT", "success"),
        ("MUTATIONS_RESULT", "skipped"),
    ] {
        fixture.set(key, value);
    }
    for key in ["MUTATION_PLAN_DIR", "MUTATION_ENGINE_PLAN_DIR"] {
        fixture.set(key, checks.to_str().unwrap());
    }
    fixture
}

#[test]
fn nextest_aggregation_rejects_bound_argv_drift_even_with_refreshed_digests() {
    let fixture = nextest_aggregate_fixture();
    let root = fixture
        .root
        .join("engine-default/fixture-engine-default-mutants-0")
        .join("mutants-engine-default/mutants.out");
    let build = root.join("build-record.json");
    let original = fs::read(&build).unwrap();
    succeeds(&summarize_engine(&fixture));
    for (index, argument) in [
        (1, "test"),
        (5, "--workspace"),
        (7, "--message-format=json"),
    ] {
        let mut record: Value = serde_json::from_slice(&original).unwrap();
        record["argv"][index] = json!(argument);
        fs::write(&build, record.to_string()).unwrap();
        let manifest = root.join("compile-membership.json");
        let mut binding: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        binding["build_sha256"] = json!(evidence_hash(&build));
        fs::write(manifest, binding.to_string()).unwrap();
        refused(
            &summarize_engine(&fixture),
            "featureless compile membership command is not equivalent",
        );
    }
}

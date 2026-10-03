//! Real package compilation and configured exact-selection regressions.

use crate::harness::{Fixture, engine_workspace, fixture_git, output, refused, succeeds};
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
    } else {
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

//! Shared real package build and aggregate fixtures for exact compile membership.

use super::engine_workspace::{engine_workspace, fixture_git};
use super::fixture::{Fixture, succeeds};
use super::mutation_shards::{copy_tree, output};
use serde_json::json;
use std::fs;
use std::path::Path;

/// Bind the real workspace and planner before selecting a control shard.
pub(crate) fn compile_fixture(probe: &str) -> Fixture {
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
    if probe == "nextest" || probe.starts_with("packages") {
        fixture.set(
            "MUTATION_ENGINE_FILES",
            &if probe.starts_with("packages") {
                json!([
                    "crates/a/src/engine.rs",
                    "crates/a/src/engine_only.rs",
                    "crates/b/src/engine.rs",
                    "crates/b/src/engine_only.rs"
                ])
                .to_string()
            } else {
                json!(["crates/a/src/engine.rs", "crates/a/src/engine_only.rs"]).to_string()
            },
        );
    }
    fixture.set("GITHUB_WORKSPACE", project.to_str().unwrap());
    let spelling = if probe.starts_with("packages") {
        project.clone()
    } else {
        project.join(".")
    };
    fixture.set("PROJECT", spelling.to_str().unwrap());
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
    if probe.starts_with("uplifted-bin") {
        configure_binary(project);
    }
    if probe == "workspace" {
        for (path, from, to) in [
            (
                "crates/a/Cargo.toml",
                "[features]\n",
                "[features]\nother = []\nleak = []\n",
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
    if probe == "uplifted-bin-nextest" {
        fs::write(
            project.join(".cargo/mutants.toml"),
            "test_tool = 'nextest'\n",
        )
        .unwrap();
    }
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
    } else if probe == "nextest" || probe.starts_with("packages") {
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
        fs::write(project.join("crates/a/src/engine_only.rs"), &original).unwrap();
        let policy = project.join("maestro-quality.toml");
        fs::write(
            &policy,
            fs::read_to_string(&policy).unwrap().replace(
                "files = ['crates/a/src/engine.rs']",
                "files = ['crates/a/src/engine.rs', 'crates/a/src/engine_only.rs']",
            ),
        )
        .unwrap();
        if probe.starts_with("packages") {
            configure_packages(project, probe, &original);
        }
    } else if !probe.starts_with("uplifted-bin") {
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

/// Require a binary uplift for an integration test's Cargo-provided executable path.
fn configure_binary(project: &Path) {
    fs::write(project.join("crates/a/src/main.rs"), "fn main() {}\n").unwrap();
    fs::create_dir_all(project.join("crates/a/tests")).unwrap();
    fs::write(
        project.join("crates/a/tests/harness_free.rs"),
        "fn main() {}\n",
    )
    .unwrap();
    let manifest = project.join("crates/a/Cargo.toml");
    fs::write(
        &manifest,
        format!(
            "{}\n[[test]]\nname = 'harness_free'\nharness = false\n",
            fs::read_to_string(&manifest).unwrap()
        ),
    )
    .unwrap();
    fs::write(
        project.join("crates/a/tests/binary_output.rs"),
        concat!(
            "#[test]\nfn integration_uses_the_uplifted_binary() {\n",
            " assert!(std::process::Command::new(env!(\"CARGO_BIN_EXE_crate-a\"))",
            ".status().unwrap().success());\n}\n"
        ),
    )
    .unwrap();
}

/// B's dependency features both hide and expose A's modules only in B's own build.
fn configure_packages(project: &Path, probe: &str, engine: &str) {
    let policy = project.join("maestro-quality.toml");
    fs::write(
        &policy,
        fs::read_to_string(&policy).unwrap().replace(
            "'crates/a/src/engine_only.rs']",
            concat!(
                "'crates/a/src/engine_only.rs', 'crates/b/src/engine.rs', ",
                "'crates/b/src/engine_only.rs']"
            ),
        ),
    )
    .unwrap();
    let sources = project.join("crates/b/src");
    fs::write(sources.join("engine.rs"), engine).unwrap();
    fs::write(sources.join("engine_only.rs"), engine).unwrap();
    let lib = sources.join("lib.rs");
    fs::write(
        &lib,
        format!(
            "{}\npub mod engine;\n#[cfg(feature = \"engine\")]\npub mod engine_only;\n",
            fs::read_to_string(&lib).unwrap()
        ),
    )
    .unwrap();
    let lib = project.join("crates/a/src/lib.rs");
    fs::write(
        &lib,
        fs::read_to_string(&lib)
            .unwrap()
            .replace(
                "pub mod engine;",
                "#[cfg(any(feature = \"engine\", not(feature = \"other\")))]\npub mod engine;",
            )
            .replace(
                "#[cfg(feature = \"engine\")]\npub mod engine_only;",
                "#[cfg(any(feature = \"engine\", feature = \"leak\"))]\npub mod engine_only;",
            ),
    )
    .unwrap();
    let manifest = project.join("crates/a/Cargo.toml");
    fs::write(
        &manifest,
        fs::read_to_string(&manifest)
            .unwrap()
            .replace("[features]\n", "[features]\nother = []\nleak = []\n"),
    )
    .unwrap();
    let manifest = project.join("crates/b/Cargo.toml");
    fs::write(
        &manifest,
        fs::read_to_string(&manifest).unwrap().replace(
            "crate-a = { path = '../a' }",
            "crate-a = { path = '../a', features = ['other', 'leak'] }",
        ),
    )
    .unwrap();
    if probe == "packages-inactive" {
        for lib in [project.join("crates/a/src/lib.rs"), sources.join("lib.rs")] {
            fs::write(
                &lib,
                concat!(
                    "//! Featureless inactive fixture.\n",
                    "#[cfg(feature = \"engine\")]\npub mod engine;\n",
                    "#[cfg(feature = \"engine\")]\npub mod engine_only;\n",
                ),
            )
            .unwrap();
        }
    }
    if probe == "packages-fallback-survivor" {
        let source = project.join("crates/a/src/engine.rs");
        fs::write(
            &source,
            engine.replace("#[cfg(test)]", "#[cfg(all(test, feature = \"engine\"))]"),
        )
        .unwrap();
    }
    if probe == "packages-survivor" {
        fs::write(
            sources.join("engine.rs"),
            engine.replace("#[cfg(test)]", "#[cfg(all(test, feature = \"engine\"))]"),
        )
        .unwrap();
    }
}

/// Transport real worker outputs with the same immutable plan and mode matrices as GitHub.
pub(crate) fn compile_aggregate_fixture(probe: &str) -> Fixture {
    let mut fixture = compile_fixture(probe);
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
        (
            "MUTATION_ENGINE_COUNT",
            if probe.starts_with("packages") {
                "8"
            } else {
                "4"
            },
        ),
        (
            "MUTATION_ENGINE_DEFAULT_COUNT",
            if probe.starts_with("packages") {
                "8"
            } else {
                "4"
            },
        ),
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

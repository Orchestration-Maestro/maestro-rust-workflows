//! Real, offline three-package regression of the complete required mutation gate.

use crate::harness::{Fixture, copy_tree, output, refused, succeeds, tool};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Output;

/// Run real Git, never a fixture stand-in, and return its trimmed output.
fn git(project: &Path, args: &[&str]) -> String {
    let result = tool("git")
        .args(args)
        .current_dir(project)
        .output()
        .unwrap();
    succeeds(&result);
    String::from_utf8(result.stdout).unwrap().trim().to_owned()
}

/// A and B declare/forward engine; C is unrelated and declares no features.
fn workspace(project: &Path, killing: bool) {
    fs::write(
        project.join("Cargo.toml"),
        "[workspace]\nmembers=['crates/a','crates/b','crates/c']\nresolver='3'\n",
    )
    .unwrap();
    for (package, extras) in [
        ("a", "[features]\nengine=[]\n"),
        (
            "b",
            "[features]\nengine=['crate-a/engine']\n[dependencies]\ncrate-a={path='../a'}\n",
        ),
        ("c", ""),
    ] {
        let root = project.join("crates").join(package);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            format!("[package]\nname='crate-{package}'\nversion='0.1.0'\nedition='2024'\n{extras}"),
        )
        .unwrap();
        let module = if package == "a" {
            "#[cfg(feature=\"engine\")] pub mod engine;\n"
        } else {
            ""
        };
        fs::write(
            root.join("src/lib.rs"),
            format!(
                concat!(
                    "//! Feature-partition regression.\n{}pub fn answer() -> u8 {{ 7 }}\n",
                    "#[cfg(test)] mod tests {{ #[test] fn answer_is_seven() {{ ",
                    "assert_eq!(super::answer(), 7); }} }}\n"
                ),
                module
            ),
        )
        .unwrap();
    }
    let assertion = if killing {
        "assert_eq!(super::engine_answer(), 9);"
    } else {
        ""
    };
    fs::write(
        project.join("crates/a/src/engine.rs"),
        format!(
            concat!(
                "//! Engine-only regression.\npub fn engine_answer() -> u8 {{ 9 }}\n",
                "#[cfg(test)] mod tests {{ #[test] fn engine_answer_is_nine() {{ {} }} }}\n"
            ),
            assertion
        ),
    )
    .unwrap();
    fs::write(
        project.join("maestro-quality.toml"),
        "[ci.mutation-engine]\nfeatures=['engine']\nfiles=['crates/a/src/engine.rs']\n",
    )
    .unwrap();
}

/// Default builds/catches `Default::default()`; the engine type deliberately lacks `Default`.
fn rising_workspace(project: &Path) {
    workspace(project, true);
    fs::write(
        project.join("crates/a/src/lib.rs"),
        concat!(
            "//! Viability regression.\n",
            "#[cfg(not(feature=\"engine\"))] pub type EngineValue = u8;\n",
            "macro_rules! engine_value { () => {\n",
            "#[cfg(feature=\"engine\")] pub struct EngineValue(u8);\n",
            "#[cfg(feature=\"engine\")] impl From<u8> for EngineValue {\n",
            "fn from(value:u8) -> Self { Self(value) } }\n",
            "#[cfg(feature=\"engine\")] impl From<EngineValue> for u8 {\n",
            "fn from(value:EngineValue) -> Self { value.0 } } }; }\nengine_value!();\n",
            "pub mod engine;\npub fn answer() -> u8 { 7 }\n",
            "#[cfg(test)] mod tests { #[test] fn answer_is_seven() {\n",
            "assert_eq!(super::answer(), 7); } }\n"
        ),
    )
    .unwrap();
    fs::write(project.join("crates/a/src/engine.rs"), concat!(
        "//! Viability regression.\npub fn engine_answer() -> super::EngineValue { 9.into() }\n",
        "#[cfg(test)] mod tests { #[test] fn engine_answer_is_nine() {\n",
        "let actual:u8 = super::engine_answer().into(); assert_eq!(actual,9); } }\n")).unwrap();
}

/// Every real fixture command is bounded and uses only the installed registry/toolchain cache.
fn run(fixture: &Fixture, step: &str) -> Output {
    fixture.run_body(&format!("timeout --kill-after=5s 120s rust-gate {step}"))
}

/// Only a matrix worker receives a shard selector; the inline default worker does not.
fn engine_worker(fixture: &mut Fixture, step: &str) -> Output {
    fixture.set("MUTATION_SHARD", "0/1");
    let result = run(fixture, step);
    fixture.set("MUTATION_SHARD", "");
    result
}

/// Copy exactly the artifact layouts GitHub uploads and downloads before aggregation.
fn harvest(fixture: &mut Fixture) {
    let checks = fixture.root.join("checks");
    if checks.exists() {
        fs::remove_dir_all(&checks).unwrap();
    }
    copy_tree(&fixture.root.join("reports"), &checks);
    copy_tree(
        &fixture.root.join("mutants/mutants.out"),
        &checks.join("mutants/mutants.out"),
    );
    for (mode, variable) in [
        ("engine", "MUTATION_ENGINE_ARTIFACTS"),
        ("engine-default", "MUTATION_ENGINE_DEFAULT_ARTIFACTS"),
    ] {
        let artifacts = fixture.root.join(mode);
        if artifacts.exists() {
            fs::remove_dir_all(&artifacts).unwrap();
        }
        let worker = artifacts.join(format!("fixture-{mode}-mutants-0"));
        copy_tree(
            &fixture.root.join(format!("mutants-{mode}")),
            &worker.join(format!("mutants-{mode}")),
        );
        fs::create_dir_all(worker.join("rust-reports")).unwrap();
        fs::copy(
            fixture
                .root
                .join(format!("reports/mutants-{mode}-shard.json")),
            worker.join(format!("rust-reports/mutants-{mode}-shard.json")),
        )
        .unwrap();
        fixture.set(variable, &artifacts.display().to_string());
    }
    fixture.set("MUTATION_PLAN_DIR", &checks.display().to_string());
    fixture.set("MUTATION_ENGINE_PLAN_DIR", &checks.display().to_string());
    fs::remove_dir_all(fixture.root.join("reports")).unwrap();
    fs::create_dir_all(fixture.root.join("reports")).unwrap();
}

/// Independent planner/worker invocations never reuse a previous phase's report/output trees.
fn restart(fixture: &Fixture) {
    for directory in [
        "reports",
        "mutants",
        "mutants-engine",
        "mutants-engine-default",
    ] {
        let path = fixture.root.join(directory);
        if path.exists() {
            fs::remove_dir_all(path).unwrap();
        }
    }
    fs::create_dir_all(fixture.root.join("reports")).unwrap();
    fs::write(fixture.root.join("output"), "").unwrap();
}

/// Commit a parentless snapshot to exercise every package rather than a changed-line sample.
fn snapshot(fixture: &mut Fixture, amended: bool) {
    let project = fixture.root.join("project");
    git(&project, &["add", "--all"]);
    let mut args = vec![
        "-c",
        "commit.gpgsign=false",
        "commit",
        "--quiet",
        "-m",
        "engine fixture",
    ];
    if amended {
        args.push("--amend");
    }
    git(&project, &args);
    fixture.set("GITHUB_SHA", &git(&project, &["rev-parse", "HEAD"]));
}

/// Export planner outputs exactly as the reusable workflow transports them to workers and gate.
fn routing(fixture: &mut Fixture) {
    for (key, value) in [
        ("MUTATION_MODE", output(fixture, "mutation-mode")),
        ("MUTATION_COUNT", output(fixture, "mutation-count")),
        ("MUTATION_MATRIX", output(fixture, "mutation-matrix")),
        (
            "MUTATION_ENGINE_COUNT",
            output(fixture, "mutation-engine-count"),
        ),
        (
            "MUTATION_ENGINE_SHARDS",
            output(fixture, "mutation-engine-shards"),
        ),
        (
            "MUTATION_ENGINE_MATRIX",
            output(fixture, "mutation-engine-matrix"),
        ),
        (
            "MUTATION_ENGINE_DEFAULT_COUNT",
            output(fixture, "mutation-engine-default-count"),
        ),
        (
            "MUTATION_ENGINE_DEFAULT_SHARDS",
            output(fixture, "mutation-engine-default-shards"),
        ),
        (
            "MUTATION_ENGINE_DEFAULT_MATRIX",
            output(fixture, "mutation-engine-default-matrix"),
        ),
    ] {
        fixture.set(key, &value);
    }
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
            &fixture
                .root
                .join("reports")
                .join(name)
                .display()
                .to_string(),
        );
    }
}

#[test]
fn three_package_gate_refuses_engine_survivors_then_catches_every_default_and_engine_mutant() {
    let mut fixture = Fixture::new();
    let project = fixture.root.join("project");
    workspace(&project, false);
    for (key, value) in [
        ("DIRECTORY", "."),
        ("MUTATION_TEST", "true"),
        ("MUTATION_SHARDS", "1"),
        ("CARGO_BUILD_JOBS", "3"),
        ("CARGO_NET_OFFLINE", "true"),
        ("CARGO_INCREMENTAL", "0"),
        ("CARGO_MUTANTS_VERSION", "27.1.0"),
        ("MUTATION_ARTIFACT_NAME", "fixture"),
        ("CHECKS_RESULT", "success"),
        ("RESULT", "success"),
        ("RUNNERS", ""),
        ("MUTATIONS_RESULT", "skipped"),
    ] {
        fixture.set(key, value);
    }
    fixture.set("GITHUB_WORKSPACE", project.to_str().unwrap());
    succeeds(&fixture.run_body("cd project && cargo generate-lockfile --offline"));
    git(&project, &["init", "--quiet", "-b", "main"]);
    git(&project, &["config", "user.name", "Fixture"]);
    git(
        &project,
        &["config", "user.email", "fixture@example.invalid"],
    );
    snapshot(&mut fixture, false);
    succeeds(&run(&fixture, "validate"));
    // Worker inputs normally come from validate outputs and GITHUB_ENV.
    fixture.set("MUTATION_ENGINE_FEATURES", "[\"engine\"]");
    fixture.set("MUTATION_ENGINE_FILES", "[\"crates/a/src/engine.rs\"]");
    survivor_phase(&mut fixture);
    restart(&fixture);
    workspace(&project, true);
    snapshot(&mut fixture, true);
    caught_phase(&mut fixture);
    restart(&fixture);
    rising_workspace(&project);
    snapshot(&mut fixture, true);
    unviable_phase(&mut fixture);
}

/// Removing only the engine assertion must fail the ordinary branch-protection gate.
fn survivor_phase(fixture: &mut Fixture) {
    succeeds(&run(fixture, "mutants-plan"));
    routing(fixture);
    succeeds(&run(fixture, "mutants"));
    succeeds(&engine_worker(fixture, "mutants-engine-default"));
    fixture.set("ENGINE_DEFAULT_MUTATIONS_RESULT", "success");
    let red = engine_worker(fixture, "mutants-engine");
    assert!(!red.status.success());
    let outcomes: Value = serde_json::from_slice(
        &fs::read(
            fixture
                .root
                .join("mutants-engine/mutants.out/outcomes.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(outcomes["missed"], 2);
    eprintln!(
        "C2 RED: engine-only assertion removed; 2 survivors, ordinary required gate refuses."
    );
    fixture.set("ENGINE_MUTATIONS_RESULT", "failure");
    harvest(fixture);
    refused(
        &run(fixture, "mutants-aggregate"),
        "engine mutation mode is missing, skipped or failed",
    );
    fixture.set("MUTATION_SUMMARY_RESULT", "failure");
    refused(
        &run(fixture, "required"),
        "Mutation partition aggregation failed or was skipped",
    );
}

/// The paired controls remain MISSED, visibly counted separately from eight CAUGHT mutants.
fn caught_phase(fixture: &mut Fixture) {
    succeeds(&run(fixture, "mutants-plan"));
    routing(fixture);
    succeeds(&run(fixture, "mutants"));
    succeeds(&engine_worker(fixture, "mutants-engine-default"));
    succeeds(&engine_worker(fixture, "mutants-engine"));
    fixture.set("ENGINE_MUTATIONS_RESULT", "success");
    harvest(fixture);
    succeeds(&run(fixture, "mutants-aggregate"));
    fixture.set("MUTATION_SUMMARY_RESULT", "success");
    succeeds(&run(fixture, "required"));
    let outcomes: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("reports/mutants.json")).unwrap())
            .unwrap();
    assert_eq!(outcomes["total_mutants"], 10);
    assert_eq!(outcomes["caught"], 8);
    assert_eq!(outcomes["unviable"], 0);
    assert_eq!(outcomes["missed"], 2);
    let inactive = &outcomes["inactive_without_features_caught_with_engine"];
    assert_eq!(inactive["total"], 2);
    assert_eq!(inactive["by_package"]["crate-a"], 2);
    assert_eq!(inactive["by_partition"]["engine-default"], 2);
    assert_eq!(outcomes["timeout"], 0);
    let mutants = outcomes["outcomes"].as_array().unwrap();
    assert_eq!(
        mutants
            .iter()
            .filter(
                |outcome| outcome["scenario"]["Mutant"]["package"] == "crate-c"
                    && outcome["summary"] == "CaughtMutant"
            )
            .count(),
        2
    );
    assert!(
        mutants
            .iter()
            .all(|outcome| outcome["mutation_mode"].is_string())
    );
    retained_paths(fixture, mutants);
    eprintln!(concat!(
        "C2 GREEN: default 6/6, C 2/2, engine 2/2; mode-aware equality; ",
        "2 inactive controls explicitly counted; zero added unviable."
    ));
}

/// Retained paths remain usable; assertion failures, not memory kills, caught each mutant.
fn retained_paths(fixture: &Fixture, mutants: &[Value]) {
    for outcome in mutants
        .iter()
        .filter(|outcome| outcome["summary"] == "CaughtMutant")
    {
        let path = outcome["log_path"].as_str().unwrap();
        let log = fs::read_to_string(fixture.root.join("reports").join(path)).unwrap();
        assert!(log.contains("assertion `left == right` failed"), "{log}");
    }
    for path in mutants
        .iter()
        .flat_map(|outcome| ["log_path", "diff_path"].map(|key| &outcome[key]))
    {
        if let Some(path) = path.as_str() {
            assert!(fixture.root.join("reports").join(path).is_file());
        }
    }
}

/// A real shared default-caught mutant cannot become engine-unviable behind a green tool exit.
fn unviable_phase(fixture: &mut Fixture) {
    succeeds(&run(fixture, "mutants-plan"));
    routing(fixture);
    succeeds(&run(fixture, "mutants"));
    succeeds(&engine_worker(fixture, "mutants-engine-default"));
    succeeds(&engine_worker(fixture, "mutants-engine"));
    let outcomes: Value = serde_json::from_slice(
        &fs::read(
            fixture
                .root
                .join("mutants-engine/mutants.out/outcomes.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(outcomes["unviable"], 1);
    let mutant = outcomes["outcomes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|outcome| outcome["summary"] == "Unviable")
        .unwrap();
    let log = fs::read_to_string(
        fixture
            .root
            .join("mutants-engine/mutants.out")
            .join(mutant["log_path"].as_str().unwrap()),
    )
    .unwrap();
    assert!(log.contains("E0277") && log.contains("Default"), "{log}");
    harvest(fixture);
    refused(
        &run(fixture, "mutants-aggregate"),
        "default-caught mutant became unviable in the engine mode",
    );
    fixture.set("MUTATION_SUMMARY_RESULT", "failure");
    refused(
        &run(fixture, "required"),
        "Mutation partition aggregation failed or was skipped",
    );
    eprintln!("C2 UNVIABLE RED: default 1 caught -> engine 1 unviable; aggregate and gate refuse.");
}

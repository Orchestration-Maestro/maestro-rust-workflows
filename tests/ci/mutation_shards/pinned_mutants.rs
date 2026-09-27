//! Real pinned cargo-mutants listing and two-worker execution tests.

use super::common::{copy_tree, output};
use crate::harness::{Fixture, succeeds};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Command;

#[test]
fn real_pinned_listings_partition_round_robin_by_mutant_identity() {
    let mut fixture = Fixture::new();
    fixture.set("MUTATION_TEST", "true");
    fixture.set("MUTATION_SHARDS", "0");
    fixture.set("MUTATION_MUTANTS_PER_SHARD", "1");
    fixture.set("CARGO_MUTANTS_VERSION", "27.1.0");
    fixture.set("CARGO_BUILD_JOBS", "3");
    let fixture_project = fixture.root.join("project");
    fs::write(
        fixture_project.join("src/lib.rs"),
        r"pub fn first(left: i32, right: i32) -> i32 { left + right }
pub fn second(left: i32, right: i32) -> i32 { left * right }
",
    )
    .unwrap();
    let change = r"--- a/src/lib.rs
+++ b/src/lib.rs
@@ -0,0 +1,2 @@
+pub fn first(left: i32, right: i32) -> i32 { left + right }
+pub fn second(left: i32, right: i32) -> i32 { left * right }
";
    fs::write(fixture.root.join("changes.diff"), change).unwrap();
    fixture.stub(
        "git",
        concat!(
            r#"case "$1" in rev-parse) printf '%s\n' "#,
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb ;; diff) cat ",
            r#""$RUNNER_TEMP/changes.diff" ;; esac"#
        ),
    );
    let fixture_report = fixture.root.join("reports").display().to_string();
    fixture.set("REPORTS", &fixture_report);
    fixture.set("PROJECT", &fixture_project.display().to_string());
    succeeds(&fixture.run_body("rust-gate mutants-plan"));
    let listing: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("reports/mutants-list.json")).unwrap())
            .unwrap();
    let mutants = listing.as_array().unwrap();
    assert!(!mutants.is_empty());
    let shards = output(&fixture, "mutation-shards")
        .parse::<usize>()
        .unwrap();
    assert!(shards > 1);
    assert!(fixture.root.join("reports/mutation-plan.json").is_file());

    for index in 0..shards {
        let result = Command::new("cargo")
            .args([
                "mutants",
                "--list",
                "--json",
                "--no-shuffle",
                "--cargo-arg=--locked",
                "--colors=never",
                "--level=info",
                "--shard",
                &format!("{index}/{shards}"),
                "--sharding",
                "round-robin",
                "--in-diff",
                fixture.root.join("mutants.diff").to_str().unwrap(),
            ])
            .envs(&fixture.env)
            .current_dir(&fixture_project)
            .output()
            .unwrap();
        succeeds(&result);
        let assigned: Vec<Value> = mutants
            .iter()
            .enumerate()
            .filter(|(position, _)| position % shards == index)
            .map(|(_, mutant)| mutant.clone())
            .collect();
        let actual: Vec<Value> = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(actual, assigned, "shard {index}/{shards}");
    }
}

fn real_sharded_worker() -> Fixture {
    let mut fixture = Fixture::new();
    for (key, value) in [
        ("MUTATION_TEST", "true"),
        ("MUTATION_SHARDS", "2"),
        ("MUTATION_MUTANTS_PER_SHARD", "50"),
        ("CARGO_MUTANTS_VERSION", "27.1.0"),
        ("CARGO_BUILD_JOBS", "3"),
        ("GITHUB_BASE_REF", ""),
    ] {
        fixture.set(key, value);
    }
    let project = fixture.root.join("project");
    fs::write(
        project.join("Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\npublish = false\n",
    )
    .unwrap();
    fs::write(
        project.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        project.join("src/lib.rs"),
        "pub fn add(left: u8, right: u8) -> u8 { left + right }\n\
         #[cfg(test)] mod tests {\n\
             use super::add;\n\
             #[test] fn addition_is_exact() {\n\
                 assert_eq!(add(1, 2), 3);\n\
                 assert_eq!(add(8, 5), 13);\n\
             }\n\
         }\n",
    )
    .unwrap();
    fixture.stub(
        "git",
        "[[ \"$*\" == \"rev-parse --verify -q HEAD^1\" ]] && exit 1; exit 88",
    );
    succeeds(&fixture.run_body("rust-gate mutants-plan"));
    assert_eq!(output(&fixture, "mutation-mode"), "sharded");
    assert_eq!(output(&fixture, "mutation-shards"), "2");
    assert!(output(&fixture, "mutation-count").parse::<usize>().unwrap() >= 2);
    fixture
}

fn run_real_shard(fixture: &mut Fixture, artifacts: &Path, index: usize) {
    fixture.set(
        "MUTATION_PLAN",
        &fixture
            .root
            .join("reports/mutation-plan.json")
            .display()
            .to_string(),
    );
    fixture.set(
        "MUTATION_LIST",
        &fixture
            .root
            .join("reports/mutants-list.json")
            .display()
            .to_string(),
    );
    fixture.set("MUTATION_SHARD", &format!("{index}/2"));
    if index > 0 {
        fs::remove_dir_all(fixture.root.join("mutants")).unwrap();
    }
    succeeds(&fixture.run_body("rust-gate mutants"));
    let artifact = artifacts.join(format!("fixture-mutants-{index}-of-2"));
    copy_tree(
        &fixture.root.join("mutants/mutants.out"),
        &artifact.join("mutants/mutants.out"),
    );
    for file in ["mutants.json", "mutants.txt", "mutants-shard.json"] {
        let source = fixture.root.join("reports").join(file);
        let destination = artifact.join("rust-reports").join(file);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::copy(source, destination).unwrap();
    }
}

#[test]
fn real_two_shard_execution_aggregates_pinned_outcomes() {
    let mut fixture = real_sharded_worker();
    let count = output(&fixture, "mutation-count").parse::<usize>().unwrap();
    let checks = fixture.root.join("checks");
    let artifacts = fixture.root.join("artifacts");
    fs::create_dir_all(&checks).unwrap();
    fs::create_dir_all(&artifacts).unwrap();
    for file in ["mutation-plan.json", "mutants-list.json"] {
        fs::copy(fixture.root.join("reports").join(file), checks.join(file)).unwrap();
    }
    for index in 0..2 {
        run_real_shard(&mut fixture, &artifacts, index);
    }
    for (key, value) in [
        ("CARGO_MUTANTS_VERSION", "27.1.0"),
        ("CHECKS_RESULT", "success"),
        ("MUTATIONS_RESULT", "success"),
        ("MUTATION_MODE", "sharded"),
        ("MUTATION_SHARDS", "2"),
        ("MUTATION_MATRIX", "[0,1]"),
        ("MUTATION_ARTIFACT_NAME", "fixture"),
    ] {
        fixture.set(key, value);
    }
    fixture.set("MUTATION_PLAN_DIR", &checks.display().to_string());
    fixture.set("MUTATION_ARTIFACTS", &artifacts.display().to_string());

    succeeds(&fixture.run_body("rust-gate mutants-aggregate"));
    assert_eq!(output(&fixture, "mutation-state"), "passed");
    let outcomes: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("reports/mutants.json")).unwrap())
            .unwrap();
    assert_eq!(outcomes["total_mutants"], count);
    assert_eq!(outcomes["caught"], count);
    assert_eq!(outcomes["outcomes"].as_array().unwrap().len(), count + 2);
    assert!(
        fs::read_to_string(fixture.root.join("reports/mutants.txt"))
            .unwrap()
            .contains(&format!("Aggregate complete: {count} mutants; 2 shards"))
    );
}

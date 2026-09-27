//! Real pinned cargo-mutants listing and two-worker execution tests.

use crate::harness::{Fixture, copy_tree, output, root, succeeds, tool};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Command;

fn git(project: &Path, arguments: &[&str]) -> String {
    let output = tool("git")
        .args(arguments)
        .current_dir(project)
        .output()
        .unwrap();
    succeeds(&output);
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn commit(project: &Path, message: &str) {
    git(project, &["add", "--all"]);
    git(
        project,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--quiet",
            "-m",
            message,
        ],
    );
}

#[test]
fn internal_shard_selftest_has_a_behavior_equivalent_two_shard_diff() {
    let mut fixture = Fixture::new();
    let project = fixture.root.join("project");
    copy_tree(&root().join("examples/workspace"), &project);
    git(&project, &["init", "--quiet", "-b", "main"]);
    commit(&project, "base");
    let base_sha = git(&project, &["rev-parse", "HEAD"]);
    fixture.set("PROJECT", &project.display().to_string());
    fixture.set("GITHUB_WORKSPACE", &project.display().to_string());
    fixture.set("MUTATION_TEST", "true");
    fixture.set("INTERNAL_SHARD_SELFTEST", "true");
    fixture.set("MUTATION_SHARDS", "2");
    fixture.set("MUTATION_MUTANTS_PER_SHARD", "50");
    fixture.set("CARGO_MUTANTS_VERSION", "27.1.0");
    fixture.set("CARGO_BUILD_JOBS", "3");
    fixture.set("GITHUB_BASE_REF", "main");
    fixture.set("GITHUB_SHA", &base_sha);
    succeeds(&fixture.run_body("rust-gate mutants-plan"));

    let baseline = Command::new("cargo")
        .args(["test", "--workspace", "--locked"])
        .envs(&fixture.env)
        .current_dir(&project)
        .output()
        .unwrap();
    succeeds(&baseline);
    assert_eq!(output(&fixture, "mutation-mode"), "sharded");
    assert_eq!(output(&fixture, "mutation-shards"), "2");
    assert!(output(&fixture, "mutation-count").parse::<usize>().unwrap() >= 2);
    assert_eq!(
        git(&project, &["diff", "--name-only", "HEAD^1", "HEAD"]),
        "core/src/lib.rs"
    );
    assert!(
        fs::read_to_string(fixture.root.join("mutants.diff"))
            .unwrap()
            .contains("core/src/lib.rs")
    );

    let mut worker = Fixture::new();
    let worker_project = worker.root.join("project");
    fs::remove_dir_all(&worker_project).unwrap();
    let clone = tool("git")
        .args([
            "clone",
            "--quiet",
            project.to_str().unwrap(),
            worker_project.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    succeeds(&clone);
    git(
        &worker_project,
        &["checkout", "--quiet", "--detach", &base_sha],
    );
    worker.set("PROJECT", &worker_project.display().to_string());
    worker.set("GITHUB_WORKSPACE", &worker_project.display().to_string());
    worker.set("GITHUB_SHA", &base_sha);
    worker.set("GITHUB_BASE_REF", "main");
    worker.set("MUTATION_TEST", "true");
    worker.set("INTERNAL_SHARD_SELFTEST", "true");
    worker.set("MUTATION_SHARDS", "2");
    worker.set("MUTATION_SHARD", "0/2");
    worker.set("MUTATION_MUTANTS_PER_SHARD", "50");
    worker.set("CARGO_MUTANTS_VERSION", "27.1.0");
    worker.set("CARGO_BUILD_JOBS", "3");
    let plan = worker.root.join("mutation-plan.json");
    let listing = worker.root.join("mutants-list.json");
    fs::copy(fixture.root.join("reports/mutation-plan.json"), &plan).unwrap();
    fs::copy(fixture.root.join("reports/mutants-list.json"), &listing).unwrap();
    worker.set("MUTATION_PLAN", &plan.display().to_string());
    worker.set("MUTATION_LIST", &listing.display().to_string());
    worker.stub(
        "cargo",
        r#"[[ "$1" == mutants ]] || exit 88
out=""
while [[ $# -gt 0 ]]; do
  if [[ "$1" == --output ]]; then out=$2; shift 2; else shift; fi
done
[[ -n "$out" ]] || exit 89
mkdir -p "$out/mutants.out"
printf '{"caught":1,"missed":0,"timeout":0,"unviable":0}' > "$out/mutants.out/outcomes.json""#,
    );
    succeeds(&worker.run_body("rust-gate mutants"));
    assert_eq!(
        git(&worker_project, &["diff", "--name-only", "HEAD^1", "HEAD"]),
        "core/src/lib.rs"
    );
}

#[test]
fn real_pinned_listings_partition_nested_push_diff_with_config_exclusions() {
    let mut fixture = Fixture::new();
    for (key, value) in [
        ("MUTATION_TEST", "true"),
        ("MUTATION_SHARDS", "0"),
        ("MUTATION_MUTANTS_PER_SHARD", "1"),
        ("CARGO_MUTANTS_VERSION", "27.1.0"),
        ("CARGO_BUILD_JOBS", "3"),
        ("GITHUB_BASE_REF", ""),
    ] {
        fixture.set(key, value);
    }
    let workspace = fixture.root.join("project");
    let project = workspace.join("crates/service");
    fs::create_dir_all(project.join("src")).unwrap();
    fs::create_dir_all(project.join(".cargo")).unwrap();
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
        project.join(".cargo/mutants.toml"),
        "exclude_globs = [\"src/excluded.rs\"]\n",
    )
    .unwrap();
    fs::write(
        project.join("src/lib.rs"),
        "//! Nested fixture crate.\npub mod excluded;\n",
    )
    .unwrap();
    fs::write(
        project.join("src/excluded.rs"),
        "pub fn excluded(left: i32, right: i32) -> i32 { left + right }\n",
    )
    .unwrap();
    git(&workspace, &["init", "--quiet"]);
    git(&workspace, &["config", "user.name", "Fixture"]);
    git(
        &workspace,
        &["config", "user.email", "fixture@example.invalid"],
    );
    commit(&workspace, "base");

    fs::write(
        project.join("src/lib.rs"),
        concat!(
            "//! Nested fixture crate.\n",
            "pub mod excluded;\n",
            "pub fn first(left: i32, right: i32) -> i32 { left + right }\n",
            "pub fn second(left: i32, right: i32) -> i32 { left - right }\n",
            "pub fn third(left: i32, right: i32) -> i32 { left * right }\n",
        ),
    )
    .unwrap();
    fs::write(
        project.join("src/excluded.rs"),
        "pub fn excluded(left: i32, right: i32) -> i32 { left - right }\n",
    )
    .unwrap();
    commit(&workspace, "change nested fixture sources");
    fixture.set("GITHUB_WORKSPACE", &workspace.display().to_string());
    fixture.set("PROJECT", &project.display().to_string());
    fixture.set("GITHUB_SHA", &git(&workspace, &["rev-parse", "HEAD"]));
    succeeds(&fixture.run_body("rust-gate mutants-plan"));

    let listing: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("reports/mutants-list.json")).unwrap())
            .unwrap();
    let mutants = listing.as_array().unwrap();
    assert!(mutants.len() > 1);
    assert!(mutants.iter().all(|mutant| mutant["file"] == "src/lib.rs"));
    let diff = fs::read_to_string(fixture.root.join("mutants.diff")).unwrap();
    assert!(diff.contains("src/lib.rs") && diff.contains("src/excluded.rs"));
    let shards = output(&fixture, "mutation-shards")
        .parse::<usize>()
        .unwrap();
    assert!(shards > 1);
    assert!(fixture.root.join("reports/mutation-plan.json").is_file());

    assert_shard_assignments(&fixture, &project, mutants, shards);
}

fn assert_shard_assignments(fixture: &Fixture, project: &Path, mutants: &[Value], shards: usize) {
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
            .current_dir(project)
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
    git(&project, &["init", "--quiet"]);
    git(&project, &["config", "user.name", "Fixture"]);
    git(
        &project,
        &["config", "user.email", "fixture@example.invalid"],
    );
    commit(&project, "root");
    fixture.set("GITHUB_SHA", &git(&project, &["rev-parse", "HEAD"]));
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

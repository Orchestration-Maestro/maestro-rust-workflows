//! Shared test fixtures for mutation planning and evidence aggregation.

use crate::harness::{Fixture, succeeds};
use serde_json::{Value, json};
use std::fs;
use std::iter;
use std::path::{Path, PathBuf};

/// A listing with `count` distinct pinned-tool mutant records and a changed
/// Rust diff, run against the real `mutants-plan` executable boundary.
pub(super) fn planning_fixture(count: usize, shards: usize, target: usize) -> Fixture {
    let mut fixture = Fixture::new();
    fixture.set("MUTATION_TEST", "true");
    fixture.set("MUTATION_SHARDS", &shards.to_string());
    fixture.set("MUTATION_MUTANTS_PER_SHARD", &target.to_string());
    fixture.set("CARGO_MUTANTS_VERSION", "27.1.0");
    fixture.set("GITHUB_BASE_REF", "main");
    fixture.stub(
        "git",
        r#"case "$1" in
  rev-parse) printf '%s\n' bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb ;;
  diff) printf 'diff --git a/src/lib.rs b/src/lib.rs\n' ;;
  *) exit 88 ;;
esac"#,
    );
    let mutants: Vec<Value> = (0..count)
        .map(|index| {
            json!({
                "package": "fixture",
                "name": format!("src/lib.rs:1:1: mutation {index}"),
                "file": "src/lib.rs",
                "replacement": format!("replacement {index}"),
                "span": {
                    "start": {"line": 1, "column": index + 1},
                    "end": {"line": 1, "column": index + 2}
                }
            })
        })
        .collect();
    fs::write(
        fixture.root.join("listing.json"),
        serde_json::to_vec(&mutants).unwrap(),
    )
    .unwrap();
    fixture.stub(
        "cargo",
        r#"[[ "$1" == mutants && "$2" == --list && "$3" == --json ]] || exit 88
cat "$RUNNER_TEMP/listing.json""#,
    );
    fixture
}

/// Read one small routing output written by the planning step.
pub(super) fn output(fixture: &Fixture, name: &str) -> String {
    fs::read_to_string(fixture.root.join("output"))
        .unwrap_or_default()
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{name}=")))
        .unwrap_or_default()
        .to_owned()
}

pub(super) fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = destination.join(entry.file_name());
        if from.is_dir() {
            copy_tree(&from, &to);
        } else {
            fs::copy(from, to).unwrap();
        }
    }
}

pub(super) fn shard_outcomes(fixture: &Fixture, index: usize) -> PathBuf {
    Path::new(&fixture.env["MUTATION_ARTIFACTS"]).join(format!(
        "fixture-mutants-{index}-of-2/mutants/mutants.out/outcomes.json"
    ))
}

fn write_fixture_shard(fixture: &mut Fixture, listing: &[Value], artifacts: &Path, index: usize) {
    let assigned: Vec<Value> = listing
        .iter()
        .enumerate()
        .filter(|(position, _)| position % 2 == index)
        .map(|(_, mutant)| mutant.clone())
        .collect();
    let outcomes = json!({
        "outcomes": iter::once(json!({
            "scenario": "Baseline",
            "summary": "Success",
            "phase_results": [
                {
                    "phase": "Build",
                    "duration": 1.0,
                    "process_status": "Success",
                    "argv": ["cargo", "build"]
                },
                {
                    "phase": "Test",
                    "duration": 1.0,
                    "process_status": "Success",
                    "argv": ["cargo", "test"]
                }
            ]
        }))
        .chain(assigned.iter().map(|mutant| json!({
            "scenario": {"Mutant": mutant},
            "summary": "CaughtMutant",
            "phase_results": [
                {
                    "phase": "Build",
                    "duration": 1.0,
                    "process_status": "Success",
                    "argv": ["cargo", "build"]
                },
                {
                    "phase": "Test",
                    "duration": 1.0,
                    "process_status": {"Failure": 101},
                    "argv": ["cargo", "test"]
                }
            ]
        })))
        .collect::<Vec<_>>(),
        "total_mutants": assigned.len(),
        "caught": assigned.len(),
        "missed": 0,
        "timeout": 0,
        "unviable": 0,
        "success": 0,
        "start_time": "2026-01-01T00:00:00Z",
        "end_time": "2026-01-01T00:01:00Z",
        "cargo_mutants_version": "27.1.0"
    });
    let assigned_path = fixture.root.join("assigned.json");
    let outcomes_path = fixture.root.join("outcomes.json");
    fs::write(&assigned_path, serde_json::to_vec(&assigned).unwrap()).unwrap();
    fs::write(&outcomes_path, serde_json::to_vec(&outcomes).unwrap()).unwrap();
    fixture.set("ASSIGNED_MUTANTS", &assigned_path.display().to_string());
    fixture.set("TEST_OUTCOMES", &outcomes_path.display().to_string());
    fixture.set("MUTATION_SHARD", &format!("{index}/2"));
    succeeds(&fixture.run_body("rust-gate mutants"));

    let artifact = artifacts.join(format!("fixture-mutants-{index}-of-2"));
    copy_tree(
        &fixture.root.join("mutants/mutants.out"),
        &artifact.join("mutants/mutants.out"),
    );
    for file in ["mutants.json", "mutants.txt", "mutants-shard.json"] {
        let source = fixture.root.join("reports").join(file);
        if source.is_file() {
            let destination = artifact.join("rust-reports").join(file);
            fs::create_dir_all(destination.parent().unwrap()).unwrap();
            fs::copy(source, destination).unwrap();
        }
    }
}

pub(super) fn aggregation_fixture(complete: bool) -> Fixture {
    let mut fixture = planning_fixture(5, 2, 1);
    succeeds(&fixture.run_body("rust-gate mutants-plan"));
    let listing: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("reports/mutants-list.json")).unwrap())
            .unwrap();
    let plan_path = fixture.root.join("mutation-plan.json");
    let listing_path = fixture.root.join("mutants-list.json");
    fs::copy(fixture.root.join("reports/mutation-plan.json"), &plan_path).unwrap();
    fs::copy(
        fixture.root.join("reports/mutants-list.json"),
        &listing_path,
    )
    .unwrap();
    fixture.set("MUTATION_PLAN", &plan_path.display().to_string());
    fixture.set("MUTATION_LIST", &listing_path.display().to_string());
    fixture.stub(
        "cargo",
        r#"[[ "$1" == mutants ]] || exit 88
out=""
while [[ $# -gt 0 ]]; do
  if [[ "$1" == --output ]]; then out=$2; shift 2; else shift; fi
done
mkdir -p "$out/mutants.out"
cp "$ASSIGNED_MUTANTS" "$out/mutants.out/mutants.json"
cp "$TEST_OUTCOMES" "$out/mutants.out/outcomes.json"
printf 'worker complete\n'"#,
    );
    let mut artifacts = fixture.root.join("artifacts");
    fs::create_dir_all(&artifacts).unwrap();
    for index in 0..2 {
        write_fixture_shard(&mut fixture, listing.as_array().unwrap(), &artifacts, index);
    }

    let summary = Fixture::new();
    let checks = summary.root.join("checks");
    fs::create_dir_all(&checks).unwrap();
    for file in ["mutation-plan.json", "mutants-list.json"] {
        fs::copy(fixture.root.join("reports").join(file), checks.join(file)).unwrap();
    }
    artifacts = summary.root.join("artifacts");
    fs::create_dir_all(&artifacts).unwrap();
    for index in 0..if complete { 2 } else { 1 } {
        let name = format!("fixture-mutants-{index}-of-2");
        copy_tree(
            &fixture.root.join("artifacts").join(&name),
            &artifacts.join(name),
        );
    }
    let mut summary = summary;
    for (key, value) in [
        ("CARGO_MUTANTS_VERSION", "27.1.0"),
        ("CHECKS_RESULT", "success"),
        ("MUTATIONS_RESULT", "success"),
        ("MUTATION_MODE", "sharded"),
        ("MUTATION_SHARDS", "2"),
        ("MUTATION_MATRIX", "[0,1]"),
        ("MUTATION_ARTIFACT_NAME", "fixture"),
    ] {
        summary.set(key, value);
    }
    summary.set("MUTATION_PLAN_DIR", &checks.display().to_string());
    summary.set("MUTATION_ARTIFACTS", &artifacts.display().to_string());
    summary
}

//! Independent package builds, evidence binding and package-local fallback.

use crate::harness::{
    compile_aggregate_fixture, compile_fixture, evidence_hash, refused, succeeds, summarize_engine,
};
use serde_json::{Value, json};
use std::fs;

#[test]
fn all_inactive_package_controls_write_one_complete_multi_owner_baseline() {
    let mut fixture = compile_fixture("packages-inactive");
    fixture.set("MUTATION_SHARD", "0/1");
    succeeds(&fixture.run_body("rust-gate mutants-engine-default"));
    let root = fixture.root.join("mutants-engine-default/mutants.out");
    let raw: Value =
        serde_json::from_slice(&fs::read(root.join("tested-outcomes.json")).unwrap()).unwrap();
    assert_eq!(raw["outcomes"].as_array().unwrap().len(), 1);
    assert_eq!(
        raw["outcomes"][0]["phase_results"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(raw["total_mutants"], 0);
    let complete: Value =
        serde_json::from_slice(&fs::read(root.join("outcomes.json")).unwrap()).unwrap();
    assert_eq!(complete["total_mutants"], 8);
    assert_eq!(complete["not_compiled_without_features"], 8);
    for (index, phase) in raw["outcomes"][0]["phase_results"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let build: Value = serde_json::from_slice(
            &fs::read(root.join(format!("builds/{index}/build-record.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(phase["argv"], build["argv"]);
    }
    let files = format!(
        "{}/builds/0/build-record.json {}/builds/1/build-record.json",
        root.display(),
        root.display()
    );
    let per_file = fixture.run_body(&format!("jaq -sc '.' {files}"));
    succeeds(&per_file);
    let documents: Vec<Value> = serde_json::Deserializer::from_slice(&per_file.stdout)
        .into_iter::<Value>()
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        documents.len(),
        2,
        "pinned jaq slurps each file independently"
    );
    let joined = fixture.run_body(&format!("cat {files} | jaq -nc '[inputs]'"));
    succeeds(&joined);
    let joined: Value = serde_json::from_slice(&joined.stdout).unwrap();
    assert_eq!(joined.as_array().unwrap().len(), 2);
}

#[test]
fn two_package_controls_build_each_owner_and_classify_only_its_evidence() {
    let fixture = compile_aggregate_fixture("packages");
    succeeds(&summarize_engine(&fixture));
    let root = fixture
        .root
        .join("engine-default/fixture-engine-default-mutants-0/mutants-engine-default/mutants.out");
    let outcomes: Value =
        serde_json::from_slice(&fs::read(root.join("outcomes.json")).unwrap()).unwrap();
    assert_eq!(outcomes["not_compiled_without_features"], 4);
    assert_eq!(outcomes["caught"], 4);
    for row in outcomes["outcomes"].as_array().unwrap() {
        if row["summary"] == "NotCompiledWithoutFeatures" {
            assert_eq!(row["phase_results"], json!([]));
        }
    }
    for owner in ["a", "b"] {
        assert!(
            fixture
                .trace()
                .contains(&format!("--package=crate-{owner}@0.1.0 --locked"))
        );
    }
}

#[test]
fn dependency_features_really_construct_the_owned_source_hiding_case() {
    let mut fixture = compile_fixture("packages");
    let source = fs::read_to_string(fixture.root.join("project/crates/a/src/lib.rs")).unwrap();
    assert!(
        source.contains(
            "#[cfg(any(feature = \"engine\", not(feature = \"other\")))]\npub mod engine;"
        ),
        "missing feature-hiding fixture guard: {source}"
    );
    fixture.set("MUTATION_SHARD", "0/1");
    succeeds(&fixture.run_body("rust-gate mutants-engine-default"));
    let root = fixture.root.join("mutants-engine-default/mutants.out");
    for (index, owner, expected) in [(0, "crate-a", true), (1, "crate-b", false)] {
        let dep_info = root.join(format!("builds/{index}/dep-info"));
        let contains = fs::read_dir(dep_info).unwrap().any(|entry| {
            fs::read_to_string(entry.unwrap().path())
                .unwrap()
                .contains("crates/a/src/engine.rs")
        });
        assert_eq!(
            contains, expected,
            "A's engine source in {owner}'s dep-info"
        );
    }
}

#[test]
fn two_package_control_rejects_a_default_compiled_survivor() {
    let mut fixture = compile_fixture("packages-survivor");
    fixture.set("MUTATION_SHARD", "0/1");
    refused(
        &fixture.run_body("rust-gate mutants-engine-default"),
        "partition contains a survivor, timeout or untested mutant",
    );
}

#[test]
fn package_version_fallback_does_not_disable_verified_owner_survivor_rejection() {
    for probe in ["packages", "packages-fallback-survivor"] {
        let mut fixture = compile_fixture(probe);
        fixture.set("MUTATION_SHARD", "0/1");
        let metadata =
            fixture.run_body("cd project && cargo metadata --format-version 1 --no-deps --locked");
        succeeds(&metadata);
        let mut metadata: Value = serde_json::from_slice(&metadata.stdout).unwrap();
        for package in metadata["packages"].as_array_mut().unwrap() {
            if package["name"] == "crate-b" {
                package["version"] = json!(null);
            }
        }
        let executable = fixture.run_body("command -v cargo");
        succeeds(&executable);
        fixture.set(
            "REAL_CARGO",
            String::from_utf8(executable.stdout).unwrap().trim(),
        );
        fixture.set("METADATA", &metadata.to_string());
        fixture.stub(
            "cargo",
            r#"if [[ $1 == metadata && " $* " == *" --locked "* &&
 ! -e "$RUNNER_TEMP/metadata-used" ]]; then
 touch "$RUNNER_TEMP/metadata-used"
 printf '%s' "$METADATA"
 exit
fi
exec "$REAL_CARGO" "$@""#,
        );
        let result = fixture.run_body("rust-gate mutants-engine-default");
        if probe == "packages-fallback-survivor" {
            refused(
                &result,
                "partition contains a survivor, timeout or untested mutant",
            );
        } else {
            succeeds(&result);
            let root = fixture.root.join("mutants-engine-default/mutants.out");
            let outcomes: Value =
                serde_json::from_slice(&fs::read(root.join("outcomes.json")).unwrap()).unwrap();
            assert_eq!(outcomes["not_compiled_without_features"], 2);
            assert_eq!(outcomes["caught"], 4);
            assert_eq!(outcomes["missed"], 2);
            let binding: Value =
                serde_json::from_slice(&fs::read(root.join("compile-membership.json")).unwrap())
                    .unwrap();
            assert_ne!(binding["packages"][0]["target"], "");
            assert_eq!(binding["packages"][1]["target"], "");
        }
    }
}

#[test]
fn aggregation_rechecks_each_package_build_argv_after_digest_refresh() {
    let fixture = compile_aggregate_fixture("packages");
    let root = fixture
        .root
        .join("engine-default/fixture-engine-default-mutants-0")
        .join("mutants-engine-default/mutants.out");
    let manifest = root.join("compile-membership.json");
    let saved = fs::read(&manifest).unwrap();
    succeeds(&summarize_engine(&fixture));
    for index in 0..2 {
        let build = root.join(format!("builds/{index}/build-record.json"));
        let original = fs::read(&build).unwrap();
        let mut record: Value = serde_json::from_slice(&original).unwrap();
        record["argv"][6] = json!("--workspace");
        fs::write(&build, record.to_string()).unwrap();
        let mut binding: Value = serde_json::from_slice(&saved).unwrap();
        binding["packages"][index]["build_sha256"] = json!(evidence_hash(&build));
        fs::write(&manifest, binding.to_string()).unwrap();
        refused(
            &summarize_engine(&fixture),
            "featureless compile membership command is not equivalent",
        );
        fs::write(&build, original).unwrap();
        fs::write(&manifest, &saved).unwrap();
    }
    succeeds(&summarize_engine(&fixture));
}

#[test]
fn featureless_package_builds_share_the_existing_non_increasing_worker_budget() {
    let mut fixture = compile_fixture("packages");
    fixture.set("MUTATION_SHARD", "0/1");
    let executable = fixture.run_body("command -v cargo");
    succeeds(&executable);
    fixture.set(
        "REAL_CARGO",
        String::from_utf8(executable.stdout).unwrap().trim(),
    );
    let delay_seconds = 2;
    fixture.set("FIRST_BUILD_DELAY_SECONDS", &delay_seconds.to_string());
    fixture.stub(
        "cargo",
        r#"if [[ " $* " == *" --no-run "* && " $* " == *" --package=crate-a@0.1.0 "* &&
 ! -e "$RUNNER_TEMP/first-build-delayed" ]]; then
 touch "$RUNNER_TEMP/first-build-delayed"
 sleep "$FIRST_BUILD_DELAY_SECONDS"
fi
exec "$REAL_CARGO" "$@""#,
    );
    fs::write(fixture.root.join("trace"), "").unwrap();
    succeeds(&fixture.run_body("rust-gate mutants-engine-default"));
    let trace = fixture.trace();
    let commands: Vec<_> = trace
        .lines()
        .filter(|line| line.contains("timeout --kill-after=1m"))
        .collect();
    assert_eq!(commands.len(), 3);
    assert!(fixture.root.join("first-build-delayed").is_file());
    let mut allowances = Vec::new();
    let mut previous = 1800;
    for command in commands {
        let timeout = command
            .split("--kill-after=1m ")
            .nth(1)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap();
        let seconds = if let Some(minutes) = timeout.strip_suffix('m') {
            minutes.parse::<u64>().unwrap() * 60
        } else {
            timeout.strip_suffix('s').unwrap().parse::<u64>().unwrap()
        };
        assert!(seconds > 0 && seconds <= previous, "{command}");
        previous = seconds;
        allowances.push(seconds);
    }
    assert!(
        allowances[0] - allowances[1] >= delay_seconds,
        "the second package must debit the first build's {delay_seconds}s delay: {allowances:?}"
    );
}

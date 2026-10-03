//! Input-only ownership follows immutable checks outputs through every mutation worker.

use crate::harness::{
    Fixture, copy_tree, engine_fixture, engine_workspace, fixture_git, output, refused, succeeds,
    workflow,
};
use serde_json::Value;
use std::fs;

#[test]
fn unconfigured_validation_preserves_global_mutants_features_but_configured_ownership_refuses_them()
{
    let mut fixture = Fixture::new();
    fs::create_dir_all(fixture.root.join("project/.cargo")).unwrap();
    fs::write(
        fixture.root.join("project/.cargo/mutants.toml"),
        "features = [\"default\"]\n",
    )
    .unwrap();
    run(&fixture, "validate");
    assert_eq!(output(&fixture, "mutation-engine-features"), "[]");
    assert_eq!(output(&fixture, "mutation-engine-files"), "[]");
    fixture.set(
        "MUTATION_ENGINE_POLICY",
        "{\"features\":[\"engine\"],\"files\":[\"src/lib.rs\"]}",
    );
    refused(
        &fixture.run_body("rust-gate validate"),
        ".cargo/mutants.toml must not set global features or test_workspace",
    );
}

#[test]
fn every_mutation_worker_uses_the_checks_jobs_resolved_engine_policy_and_ownership() {
    let ci = workflow("ci");
    assert_eq!(
        ci["jobs"]["checks"]["outputs"]["mutation-engine-policy"],
        "${{ steps.validate.outputs.mutation-engine-policy }}"
    );
    for job in [
        "mutations",
        "mutation-engine",
        "mutation-engine-default",
        "mutation-windows",
    ] {
        for (variable, field) in [
            ("MUTATION_ENGINE_POLICY", "policy"),
            ("MUTATION_ENGINE_FEATURES", "features"),
            ("MUTATION_ENGINE_FILES", "files"),
        ] {
            assert_eq!(
                ci["jobs"][job]["env"][variable].as_str().unwrap_or(""),
                format!("${{{{ needs.checks.outputs.mutation-engine-{field} }}}}"),
                "{job}"
            );
        }
    }
}

/// Run the real installed toolchain offline, with bounded fixture commands.
fn run(fixture: &Fixture, step: &str) {
    succeeds(&fixture.run_body(&format!("timeout --kill-after=5s 120s rust-gate {step}")));
}

/// Recreate GitHub's job-output binding, then consume validation's ownership exports.
fn worker_ownership(fixture: &mut Fixture, job: &str) {
    let ci = workflow("ci");
    for variable in [
        "MUTATION_ENGINE_POLICY",
        "MUTATION_ENGINE_FEATURES",
        "MUTATION_ENGINE_FILES",
    ] {
        let expression = ci["jobs"][job]["env"][variable].as_str().unwrap_or("");
        let field = expression
            .strip_prefix("${{ needs.checks.outputs.")
            .and_then(|value| value.strip_suffix(" }}"))
            .unwrap_or("missing-output");
        fixture.set(variable, &output(fixture, field));
    }
    run(fixture, "validate");
    let exported = fs::read_to_string(fixture.root.join("environment")).unwrap();
    for variable in ["MUTATION_ENGINE_FEATURES", "MUTATION_ENGINE_FILES"] {
        let value = exported
            .lines()
            .rev()
            .find_map(|line| line.strip_prefix(&format!("{variable}=")))
            .unwrap();
        fixture.set(variable, value);
    }
}

/// Preserve exactly the uploaded raw tree and worker reports before the next worker runs.
fn worker(fixture: &mut Fixture, job: &str, mode: &str, index: usize, artifact: &str) {
    worker_ownership(fixture, job);
    fixture.set("MUTATION_SHARD", &format!("{index}/2"));
    run(fixture, mode);
    let listing: Value = serde_json::from_slice(
        &fs::read(fixture.root.join(mode).join("mutants.out/mutants.json")).unwrap(),
    )
    .unwrap();
    if job == "mutations" {
        assert!(
            listing
                .as_array()
                .unwrap()
                .iter()
                .all(|mutant| mutant["file"] != "crates/a/src/engine.rs")
        );
        assert_eq!(listing.as_array().unwrap().len(), 3);
    }
    let directory = fixture.root.join(job);
    let name = if job == "mutations" {
        format!("{artifact}-mutants-{index}-of-2")
    } else {
        format!(
            "{artifact}-{}-mutants-{index}",
            job.trim_start_matches("mutation-")
        )
    };
    let destination = directory.join(name);
    copy_tree(
        &fixture.root.join(mode).join("mutants.out"),
        &destination.join(mode).join("mutants.out"),
    );
    copy_tree(
        &fixture.root.join("reports"),
        &destination.join("rust-reports"),
    );
    fs::remove_dir_all(fixture.root.join(mode)).unwrap();
    fs::remove_dir_all(fixture.root.join("reports")).unwrap();
    fs::create_dir_all(fixture.root.join("reports")).unwrap();
}

/// A real source with no repository ownership policy, configured only by caller input.
fn input_fixture() -> Fixture {
    let mut fixture = Fixture::new();
    let project = fixture.root.join("project");
    engine_workspace(&project, true);
    fs::remove_file(project.join("maestro-quality.toml")).unwrap();
    for (key, value) in [
        ("MUTATION_TEST", "true"),
        ("MUTATION_SHARDS", "2"),
        ("CARGO_BUILD_JOBS", "3"),
        ("CARGO_NET_OFFLINE", "true"),
        ("CARGO_INCREMENTAL", "0"),
        ("CARGO_MUTANTS_VERSION", "27.1.0"),
        (
            "MUTATION_ENGINE_POLICY",
            "{\"features\":[\"engine\"],\"files\":[\"crates/a/src/engine.rs\"]}",
        ),
    ] {
        fixture.set(key, value);
    }
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
            "input-only fixture",
        ],
    );
    fixture.set("GITHUB_SHA", &fixture_git(&project, &["rev-parse", "HEAD"]));
    fixture
}

#[test]
fn input_only_engine_ownership_survives_both_default_shards_and_complete_mode_aggregation() {
    let mut fixture = input_fixture();
    let project = fixture.root.join("project");
    run(&fixture, "validate");
    fixture.set(
        "MUTATION_ENGINE_FEATURES",
        &output(&fixture, "mutation-engine-features"),
    );
    fixture.set(
        "MUTATION_ENGINE_FILES",
        &output(&fixture, "mutation-engine-files"),
    );
    run(&fixture, "mutants-plan");
    assert_eq!(output(&fixture, "mutation-count"), "6");
    assert_eq!(output(&fixture, "mutation-shards"), "2");
    assert_eq!(output(&fixture, "mutation-engine-shards"), "2");
    let artifact = output(&fixture, "artifact-name");
    copy_tree(&fixture.root.join("reports"), &fixture.root.join("plan"));
    for (key, name) in [
        ("MUTATION_PLAN", "mutation-plan.json"),
        ("MUTATION_LIST", "mutants-list.json"),
        ("MUTATION_ENGINE_PLAN", "mutation-engine-plan.json"),
        ("MUTATION_ENGINE_LIST", "mutation-engine-list.json"),
        (
            "MUTATION_ENGINE_DEFAULT_LIST",
            "mutation-engine-default-list.json",
        ),
    ] {
        fixture.set(
            key,
            &fixture.root.join("plan").join(name).display().to_string(),
        );
    }
    // The original caller input is no longer consulted; the repository still has no policy.
    fixture.set("MUTATION_ENGINE_POLICY", "{}");
    for (job, mode) in [
        ("mutations", "mutants"),
        ("mutation-engine", "mutants-engine"),
        ("mutation-engine-default", "mutants-engine-default"),
    ] {
        for index in 0..2 {
            worker(&mut fixture, job, mode, index, &artifact);
        }
    }
    complete(&mut fixture, &artifact);
    assert!(!project.join("maestro-quality.toml").exists());
    eprintln!(
        "INPUT-ONLY GREEN: two default shards caught 3/3 each; complete 10 mode obligations."
    );
}

/// Aggregate the exact six worker artifacts against the immutable checks plans.
fn complete(fixture: &mut Fixture, artifact: &str) {
    for (key, value) in [
        ("CHECKS_RESULT", "success"),
        ("MUTATIONS_RESULT", "success"),
        ("ENGINE_MUTATIONS_RESULT", "success"),
        ("ENGINE_DEFAULT_MUTATIONS_RESULT", "success"),
        ("MUTATION_MODE", "sharded"),
        ("MUTATION_ATTEMPT", "1"),
        ("MUTATION_COUNT", "6"),
        ("MUTATION_MATRIX", "[0,1]"),
        ("MUTATION_ENGINE_COUNT", "2"),
        ("MUTATION_ENGINE_SHARDS", "2"),
        ("MUTATION_ENGINE_MATRIX", "[0,1]"),
        ("MUTATION_ENGINE_DEFAULT_COUNT", "2"),
        ("MUTATION_ENGINE_DEFAULT_SHARDS", "2"),
        ("MUTATION_ENGINE_DEFAULT_MATRIX", "[0,1]"),
    ] {
        fixture.set(key, value);
    }
    fixture.set("MUTATION_ARTIFACT_NAME", artifact);
    for (variable, directory) in [
        ("MUTATION_PLAN_DIR", "plan"),
        ("MUTATION_ENGINE_PLAN_DIR", "plan"),
        ("MUTATION_ARTIFACTS", "mutations"),
        ("MUTATION_ENGINE_ARTIFACTS", "mutation-engine"),
        (
            "MUTATION_ENGINE_DEFAULT_ARTIFACTS",
            "mutation-engine-default",
        ),
    ] {
        fixture.set(
            variable,
            &fixture.root.join(directory).display().to_string(),
        );
    }
    run(fixture, "mutants-aggregate");
    fixture.set("RESULT", "success");
    fixture.set("RUNNERS", "");
    fixture.set("MUTATION_SUMMARY_RESULT", "success");
    run(fixture, "required");
    let merged: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("reports/mutants.json")).unwrap())
            .unwrap();
    assert_eq!(merged["total_mutants"], 10);
    assert_eq!(merged["caught"], 8);
    assert_eq!(
        merged["inactive_without_features_caught_with_engine"]["total"],
        2
    );
}

#[test]
fn engine_mutation_timeouts_include_cold_build_and_reporting_margin() {
    let ci = workflow("ci");
    for (job, id, inner, step, total, enabled) in [
        ("mutation-engine", "engine-mutation-run", 60, 65, 75, true),
        (
            "mutation-engine-default",
            "engine-default-mutation-run",
            30,
            35,
            45,
            false,
        ),
    ] {
        let fixture = engine_fixture(true);
        let steps = ci["jobs"][job]["steps"].as_array().unwrap();
        let execution = steps.iter().find(|step| step["id"] == id).unwrap();
        succeeds(&fixture.run("ci", id));
        let trace = fixture.trace();
        let command = trace
            .lines()
            .find(|line| line.contains("timeout --kill-after=1m"))
            .unwrap();
        let duration: u64 = command
            .split("--kill-after=1m ")
            .nth(1)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .trim_end_matches('m')
            .parse()
            .unwrap();
        assert_eq!(duration, inner, "{job}: {command}");
        assert_eq!(execution["timeout-minutes"], step, "{job}");
        assert_eq!(ci["jobs"][job]["timeout-minutes"], total, "{job}");
        assert!(duration + 1 < execution["timeout-minutes"].as_u64().unwrap());
        assert!(execution["timeout-minutes"].as_u64().unwrap() < total);
        assert_eq!(command.contains("--features engine"), enabled, "{command}");
    }
}

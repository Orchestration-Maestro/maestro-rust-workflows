//! A synthetic consumer with a tracked host owner and full deterministic listing.

use super::engine_workspace::fixture_git;
use super::fixture::{Fixture, succeeds};
use super::mutation_shards::copy_tree;
use serde_json::{Value, json};
use std::fmt::Write as _;
use std::fs;

/// A tracked consumer and a deterministic full host discovery.
pub(crate) fn host_fixture() -> Fixture {
    let mut fixture = Fixture::new();
    let project = fixture.root.join("project");
    fs::create_dir_all(project.join(".github/scripts")).unwrap();
    fs::write(
        project.join(".github/scripts/host.sh"),
        "#!/bin/bash\nexit 0\n",
    )
    .unwrap();
    fs::write(
        project.join("src/host.rs"),
        "pub fn host() -> bool { true }\n",
    )
    .unwrap();
    fs::write(
        project.join("maestro-quality.toml"),
        concat!(
            "[ci.mutation-provisioned-host]\nfiles=['src/host.rs']\n",
            "features=['fixture/host-tests']\nprovisioner='.github/scripts/host.sh'\n"
        ),
    )
    .unwrap();
    fs::write(
        project.join("Cargo.toml"),
        concat!(
            "[package]\nname='fixture'\nversion='0.1.0'\nedition='2024'\n",
            "[features]\nhost-tests=[]\n"
        ),
    )
    .unwrap();
    fixture_git(&project, &["init", "--quiet"]);
    fixture_git(&project, &["config", "user.name", "Fixture"]);
    fixture_git(
        &project,
        &["config", "user.email", "fixture@example.invalid"],
    );
    fixture_git(&project, &["add", "."]);
    fixture_git(
        &project,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "base",
        ],
    );
    fixture_git(
        &project,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--allow-empty",
            "--quiet",
            "-m",
            "head",
        ],
    );
    fixture.set("GITHUB_SHA", &fixture_git(&project, &["rev-parse", "HEAD"]));
    fixture.set("GITHUB_WORKSPACE", &project.display().to_string());
    fixture.set("DIRECTORY", ".");
    fixture.set("MUTATION_TEST", "true");
    fixture.set("CARGO_MUTANTS_VERSION", "27.1.0");
    let metadata = json!({"workspace_members":["fixture"], "packages":[{
        "id":"fixture", "name":"fixture", "manifest_path":project.join("Cargo.toml"),
        "features":{"host-tests":[]}
    }]});
    fs::write(
        fixture.root.join("host-metadata.json"),
        metadata.to_string(),
    )
    .unwrap();
    fixture.stub(
        "cargo",
        r#"if [[ "$1" == metadata ]]; then
cat "$RUNNER_TEMP/host-metadata.json"
elif [[ "$*" == *--file* ]]; then cat "$RUNNER_TEMP/host-list.json"
else printf '[]\n'; fi"#,
    );
    let span = json!({"start":{"line":1,"column":1},"end":{"line":1,"column":30}});
    let mutant = json!({"package":"fixture",
        "name":"src/host.rs:1:1: replace host -> bool with false",
        "file":"src/host.rs", "replacement":"false", "diff":"patch bytes", "span":span,
        "function":{"function_name":"host", "span":span}});
    fs::write(
        fixture.root.join("host-list.json"),
        json!([mutant]).to_string(),
    )
    .unwrap();
    fixture
}

/// Create real changed identities and the pending report through the gate.
pub(crate) fn host_coverage_fixture(misses: usize) -> Fixture {
    let mut fixture = host_fixture();
    let project = fixture.root.join("project");
    fs::write(
        project.join("src/host.rs"),
        "pub fn host() -> bool { false }\n",
    )
    .unwrap();
    fs::write(
        project.join("src/lib.rs"),
        "pub fn ordinary() {}\n".repeat(3),
    )
    .unwrap();
    fixture_git(&project, &["add", "."]);
    fixture_git(
        &project,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "sources",
        ],
    );
    fixture.set("GITHUB_SHA", &fixture_git(&project, &["rev-parse", "HEAD"]));
    fixture.set("GITHUB_BASE_REF", "main");
    fixture.set("PULL_REQUEST_TITLE", "feat(ci): host owner");
    let planning = fixture.root.join("planning");
    fixture.set("REPORTS", &planning.display().to_string());
    succeeds(&fixture.run_body("rust-gate mutants-plan"));
    fixture.set(
        "REPORTS",
        &fixture.root.join("reports").display().to_string(),
    );
    copy_tree(&planning, &fixture.root.join("reports"));
    let lcov = format!(
        "SF:{}/src/host.rs\nDA:1,0\nend_of_record\nSF:{}/src/lib.rs\n{}end_of_record\n",
        project.display(),
        project.display(),
        (1..=3).fold(String::new(), |mut text, line| {
            writeln!(text, "DA:{line},{}", u8::from(line > misses)).unwrap();
            text
        })
    );
    fs::write(fixture.root.join("reports/coverage.lcov"), lcov).unwrap();
    fixture
}

/// Hash raw fixture evidence with the same standard SHA utility, outside gate trust.
pub(crate) fn host_evidence_digest(fixture: &Fixture, path: &str) -> String {
    let result = fixture.run_body(&format!("sha256sum '{path}'"));
    succeeds(&result);
    String::from_utf8(result.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned()
}

/// Synthetic schema-1 receipt consumed by the permanent aggregation path, not production.
pub(crate) fn host_evidence_fixture() -> Fixture {
    let mut fixture = host_coverage_fixture(1);
    succeeds(&fixture.run_body("rust-gate changed-coverage"));
    let checks = fixture.root.join("checks");
    copy_tree(&fixture.root.join("reports"), &checks);
    let root = fixture.root.join("host-evidence");
    fs::create_dir(&root).unwrap();
    fs::write(
        root.join("phases.log"),
        "selected host assertion failed; cleanup completed\n",
    )
    .unwrap();
    let plan: Value =
        serde_json::from_slice(&fs::read(checks.join("mutation-host-plan.json")).unwrap()).unwrap();
    let log_digest = host_evidence_digest(&fixture, &root.join("phases.log").display().to_string());
    let receipt = json!({
        "build":"passed", "provision":"passed", "test":"passed", "cleanup":"passed",
        "selected_tests":["host_assertion"], "passed":1, "failed":0, "ignored":0,
        "phases":{"normal":"passed","abandon":"passed","preparing":"passed",
            "recover_abandon":"passed","recover_preparing":"passed"},
        "test_sha256":"c".repeat(64), "bootstrap_sha256":"d".repeat(64),
        "logs":{"phases.log":log_digest}
    });
    let mut caught = receipt.clone();
    caught["test"] = json!("failed");
    caught["passed"] = json!(0);
    caught["failed"] = json!(1);
    caught["test_failure"] = json!(["host_assertion"]);
    let plan_digest = host_evidence_digest(
        &fixture,
        &checks.join("mutation-host-plan.json").display().to_string(),
    );
    let outcomes = json!({
        "schema":1,"sha":fixture.env["GITHUB_SHA"],"run_id":"123","attempt":"1",
        "plan_sha256":plan_digest,
        "policy_sha256":plan["policy_sha256"],"provisioner_sha256":plan["provisioner_sha256"],
        "source_sha256":plan["source_sha256"],"baseline_before":receipt,"baseline_after":receipt,
        "outcomes":[{"mutant":plan["mutants"][0],"outcome":"caught",
            "patched_source_sha256":"e".repeat(64), "receipt":caught}]
    });
    fs::write(root.join("host-outcomes.json"), outcomes.to_string()).unwrap();
    for (key, value) in [
        ("MUTATION_HOST_COUNT", "1"),
        ("HOST_MUTATIONS_RESULT", "success"),
        ("CHECKS_RESULT", "success"),
        ("MUTATION_MODE", "empty"),
    ] {
        fixture.set(key, value);
    }
    fixture.set("MUTATION_PLAN_DIR", &checks.display().to_string());
    fixture.set("MUTATION_HOST_ARTIFACTS", &root.display().to_string());
    fixture
}

//! A synthetic consumer with a tracked host owner and full deterministic listing.

use super::engine_workspace::fixture_git;
use super::fixture::Fixture;
use serde_json::json;
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

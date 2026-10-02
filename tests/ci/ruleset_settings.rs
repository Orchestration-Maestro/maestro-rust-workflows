//! `ci.yml` as an organization ruleset runs it: no workflow called it, so
//! `validate` takes its inputs from the `[ci]` table of the base commit, and
//! mutation ownership policies from the pull request's head.

use crate::harness::{Fixture, refused, succeeds, workflow};
use serde_json::{Value, json};
use std::fs;

/// A run no workflow called, the one an organization ruleset starts, whose
/// base commit, the checkout's first parent, holds `base` as its
/// `maestro-quality.toml` and whose head, the checkout itself, holds `head`,
/// either one none when empty. The project holds `src/windows.rs` too.
fn uncalled(base: &str, head: &str) -> Fixture {
    let mut fixture = Fixture::new();
    fixture.set("CALLED", "false");
    fixture.set("BASE_CONFIG", base);
    fixture.set("HEAD_CONFIG", head);
    fs::write(
        fixture.root.join("project/src/windows.rs"),
        "//! Windows.\n",
    )
    .unwrap();
    fixture.stub(
        "git",
        r#"case "$*" in
  *' ls-tree --name-only HEAD^1 -- maestro-quality.toml') [[ -z "$BASE_CONFIG" ]] || echo x ;;
  *' show HEAD^1:maestro-quality.toml') printf '%s' "$BASE_CONFIG" ;;
  *' ls-tree --name-only HEAD -- maestro-quality.toml') [[ -z "$HEAD_CONFIG" ]] || echo x ;;
  *' show HEAD:maestro-quality.toml') printf '%s' "$HEAD_CONFIG" ;;
  *) exit 88 ;;
esac"#,
    );
    fixture
}

/// Configure the fixture package's declared engine feature and source file.
fn set_engine_metadata(fixture: &mut Fixture) {
    fs::write(
        fixture.root.join("project/Cargo.toml"),
        concat!(
            "[package]\nname=\"fixture\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
            "[features]\nengine=[]\n"
        ),
    )
    .unwrap();
    fs::write(
        fixture.root.join("project/src/engine.rs"),
        "pub fn run() {}\n",
    )
    .unwrap();
    fixture.set(
        "METADATA",
        &json!({"workspace_members":["fixture"], "packages":[{
            "id":"fixture", "name":"fixture",
            "manifest_path":fixture.root.join("project/Cargo.toml"),
            "features":{"engine":[],"default":[]},
            "targets":[{"src_path":fixture.root.join("project/src/lib.rs")}]
        }]})
        .to_string(),
    );
    fixture.stub(
        "cargo",
        "[[ $1 == metadata ]] && printf '%s' \"$METADATA\"; exit 0",
    );
}

/// The `KEY=value` lines `validate` exported.
fn exported(fixture: &Fixture) -> String {
    fs::read_to_string(fixture.root.join("environment")).unwrap()
}

/// Assert that `validate` exported `line`.
fn assert_exported(fixture: &Fixture, line: &str) {
    let environment = exported(fixture);
    assert!(
        environment.lines().any(|set| set == line),
        "{line}: {environment}"
    );
}

#[test]
fn a_run_no_workflow_called_takes_the_ci_table_of_the_base_commit() {
    let fixture = uncalled(
        concat!(
            "[ci]\nworking-directory = \"project\"\ncoverage-threshold = 95\n",
            "mutation-test = true\nmutation-shards = 0\n",
            "mutation-mutants-per-shard = 25\nmutation-windows = [\"src/lib.rs\"]\n",
            "platforms = \"macos windows linux-arm\"\n",
        ),
        // The pull request's own file loosens nothing: the base commit's rules
        // hold for every input but the Windows-owned files, the head's.
        concat!(
            "[ci]\nworking-directory = \"project\"\nunsafe-policy = \"allow\"\n",
            "coverage-threshold = 10\nmutation-test = false\nmutation-shards = 12\n",
            "mutation-mutants-per-shard = 900\nmutation-windows = [\"src/windows.rs\"]\n",
        ),
    );
    succeeds(&fixture.run("ci", "validate"));
    for line in [
        "COVERAGE=95",
        "MUTATION_TEST=true",
        "MUTATION_SHARDS=0",
        "MUTATION_MUTANTS_PER_SHARD=25",
        "MUTATION_WINDOWS=[\"src/windows.rs\"]",
        "UNSAFE_POLICY=deny",
        "DEPENDENCY_AUDIT=true",
    ] {
        assert_exported(&fixture, line);
    }
    let written = fs::read_to_string(fixture.root.join("output")).unwrap();
    for line in [
        r#"platforms=["macos-15","windows-2025","ubuntu-24.04-arm"]"#,
        "directory=project",
    ] {
        assert!(written.lines().any(|set| set == line), "{line}: {written}");
    }
}

#[test]
fn the_head_lists_windows_files_the_base_commit_does_not_know() {
    let fixture = uncalled(
        "[ci]\nworking-directory = \"project\"\n",
        "[ci]\nworking-directory = \"project\"\nmutation-windows = [\"src/windows.rs\"]\n",
    );
    succeeds(&fixture.run("ci", "validate"));
    assert_exported(&fixture, "MUTATION_WINDOWS=[\"src/windows.rs\"]");
}

#[test]
fn engine_owned_files_and_features_come_from_the_tested_head() {
    let base = r#"[ci]
working-directory = "project"
"#;
    let policy = r#"[ci]
working-directory = "project"

[ci.mutation-engine]
features = ["engine"]
files = ["src/engine.rs"]
"#;
    let mut fixture = uncalled(base, policy);
    set_engine_metadata(&mut fixture);
    succeeds(&fixture.run("ci", "validate"));
    assert_exported(&fixture, r#"MUTATION_ENGINE_FEATURES=["engine"]"#);
    assert_exported(&fixture, r#"MUTATION_ENGINE_FILES=["src/engine.rs"]"#);

    let mut fixture = uncalled(policy, base);
    set_engine_metadata(&mut fixture);
    succeeds(&fixture.run("ci", "validate"));
    assert_exported(&fixture, "MUTATION_ENGINE_FEATURES=[]");
    assert_exported(&fixture, "MUTATION_ENGINE_FILES=[]");
}

#[test]
fn a_head_that_drops_the_windows_files_mutates_them_on_linux() {
    let fixture = uncalled(
        "[ci]\nworking-directory = \"project\"\nmutation-windows = [\"src/windows.rs\"]\n",
        "[ci]\nworking-directory = \"project\"\n",
    );
    succeeds(&fixture.run("ci", "validate"));
    assert_exported(&fixture, "MUTATION_WINDOWS=[]");
}

#[test]
fn a_head_listing_a_missing_windows_file_is_refused() {
    let fixture = uncalled(
        "[ci]\nworking-directory = \"project\"\n",
        "[ci]\nworking-directory = \"project\"\nmutation-windows = [\"src/gone.rs\"]\n",
    );
    refused(
        &fixture.run("ci", "validate"),
        "MUTATION_WINDOWS file `src/gone.rs` does not exist",
    );
    assert!(!fixture.root.join("output").exists());
}

#[test]
fn without_a_ci_table_the_run_takes_every_input_default_and_three_platforms() {
    let mut fixture = uncalled("", "");
    let project = fixture.root.join("project").display().to_string();
    fixture.set("GITHUB_WORKSPACE", &project);
    succeeds(&fixture.run("ci", "validate"));
    let inputs = &workflow("ci")["on"]["workflow_call"]["inputs"];
    let environment = exported(&fixture);
    for (input, variable) in [
        ("coverage-threshold", "COVERAGE"),
        ("license-policy", "LICENSE_POLICY"),
        ("mutation-test", "MUTATION_TEST"),
        ("mutation-shards", "MUTATION_SHARDS"),
        ("mutation-mutants-per-shard", "MUTATION_MUTANTS_PER_SHARD"),
        ("mutation-windows", "MUTATION_WINDOWS"),
        ("api-compatibility", "API_COMPATIBILITY"),
        ("sarif-reports", "SARIF_REPORTS"),
        ("unsafe-policy", "UNSAFE_POLICY"),
        ("dependency-audit", "DEPENDENCY_AUDIT"),
        ("clippy-level", "CLIPPY_LEVEL"),
        ("unused-dependencies", "UNUSED_DEPENDENCIES"),
    ] {
        let default = match &inputs[input]["default"] {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        let line = format!("{variable}={default}");
        assert!(
            environment.lines().any(|set| set == line),
            "{line}: {environment}"
        );
    }
    // Linux, macOS and Windows for every Rust repository, and the compiler
    // the project pins.
    let written = fs::read_to_string(fixture.root.join("output")).unwrap();
    for line in [
        r#"platforms=["macos-15","windows-2025"]"#,
        "directory=.",
        "toolchain=1.98.1",
    ] {
        assert!(written.lines().any(|set| set == line), "{line}: {written}");
    }
}

#[test]
fn a_ci_table_that_drops_macos_or_windows_is_refused_with_the_fix() {
    let fixture = uncalled("[ci]\nplatforms = \"linux-arm\"\n", "");
    refused(
        &fixture.run("ci", "validate"),
        "maestro-quality.toml: [ci] platforms `linux-arm` drops macos and windows, which every \
         Rust repository tests: set it to `macos windows linux-arm`",
    );
    assert!(!fixture.root.join("output").exists());
}

#[test]
fn coverage_selection_comes_from_head_even_when_the_base_differs() {
    let base = "[ci]\nworking-directory='project'\ncoverage-features=['fixture/default']\n";
    let head = "[ci]\nworking-directory='project'\ncoverage-features=['fixture/engine']\n";
    let mut fixture = uncalled(base, head);
    set_engine_metadata(&mut fixture);
    succeeds(&fixture.run("ci", "validate"));
    assert_exported(&fixture, "COVERAGE_FEATURES=[\"fixture/engine\"]");
    // The identical head-removal behavior is deliberate, matching E04a.
    let mut fixture = uncalled(head, "[ci]\nworking-directory='project'\n");
    set_engine_metadata(&mut fixture);
    succeeds(&fixture.run("ci", "validate"));
    assert_exported(&fixture, "COVERAGE_FEATURES=");
}

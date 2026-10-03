//! Strict tested-head host policy, overlaps and all discovery modes.

use crate::harness::{Fixture, fixture_git, host_fixture, output, refused, succeeds};
use serde_json::{Value, json};
use std::fs;
use std::os::unix::fs::symlink;

/// Commit a tested policy edit, retaining a real first parent.
fn commit_policy(fixture: &Fixture, text: &str) {
    let project = fixture.root.join("project");
    fs::write(project.join("maestro-quality.toml"), text).unwrap();
    fixture_git(&project, &["add", "."]);
    fixture_git(
        &project,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "policy",
        ],
    );
}

#[test]
fn host_policy_refuses_wrong_tables_empty_lists_and_unknown_fields() {
    let fixture = host_fixture();
    for text in [
        "[ci]\nmutation-provisioned-host = []\n",
        "[ci.mutation-provisioned-host]\n",
        "[ci.mutation-provisioned-host]\nfiles=[]\nfeatures=[]\nprovisioner='src/host.rs'\n",
        "[ci.mutation-provisioned-host]\nfiles=['src/host.rs']\n\
        features=['fixture/host-tests']\nprovisioner='.github/scripts/host.sh'\nexcludes=[]\n",
    ] {
        commit_policy(&fixture, text);
        if text.contains("= []") {
            refused(
                &fixture.run_body("rust-gate validate"),
                "maestro-quality.toml: [ci] mutation-provisioned-host must be a table",
            );
        } else {
            refused(
                &fixture.run_body("rust-gate validate"),
                "[ci.mutation-provisioned-host] requires only nonempty files, \
                features and provisioner",
            );
        }
    }
}

#[test]
fn host_policy_refuses_duplicate_unsafe_untracked_and_overlapping_files() {
    let fixture = host_fixture();
    let original = fs::read_to_string(fixture.root.join("project/maestro-quality.toml")).unwrap();
    for (files, message) in [
        (
            "'src/host.rs','src/host.rs'",
            "host mutation file `src/host.rs` must be unique Rust source",
        ),
        (
            "'../src/host.rs'",
            "host policy path `../src/host.rs` must be an exact relative tracked file",
        ),
        (
            "'src/*.rs'",
            "host policy path `src/*.rs` must be an exact relative tracked file",
        ),
        (
            "'.github/scripts/host.sh'",
            "host mutation file `.github/scripts/host.sh` must be unique Rust source",
        ),
    ] {
        commit_policy(&fixture, &original.replace("'src/host.rs'", files));
        refused(&fixture.run_body("rust-gate validate"), message);
    }
    commit_policy(&fixture, &original);
    let mut fixture = fixture;
    fixture.set("MUTATION_WINDOWS", "[\"src/host.rs\"]");
    refused(
        &fixture.run_body("rust-gate validate"),
        "host mutation file `src/host.rs` overlaps Windows or engine ownership",
    );
    fixture.set("MUTATION_WINDOWS", "[]");
    fixture.set(
        "MUTATION_ENGINE_POLICY",
        "{\"features\":[\"host-tests\"],\"files\":[\"src/host.rs\"]}",
    );
    refused(
        &fixture.run_body("rust-gate validate"),
        "host mutation file `src/host.rs` overlaps Windows or engine ownership",
    );
}

#[test]
fn host_ownership_uses_tested_head_not_the_base_or_caller() {
    let mut fixture = host_fixture();
    let project = fixture.root.join("project");
    commit_policy(&fixture, "[ci]\nmutation-test=false\n");
    fixture_git(
        &project,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--allow-empty",
            "--quiet",
            "-m",
            "base-no-host",
        ],
    );
    commit_policy(
        &fixture,
        "[ci.mutation-provisioned-host]\nfiles=['src/host.rs']\n\
        features=['fixture/host-tests']\nprovisioner='.github/scripts/host.sh'\n",
    );
    fixture.set("CALLED", "false");
    fixture.set("GITHUB_WORKSPACE", &project.display().to_string());
    refused(
        &fixture.run_body("rust-gate validate"),
        "[ci.mutation-provisioned-host] requires mutation-test=true",
    );
    // Called inputs can enable mutation, but cannot replace the ownership table.
    fixture.set("CALLED", "true");
    fixture.set("DIRECTORY", ".");
    succeeds(&fixture.run_body("rust-gate validate"));
    assert_eq!(output(&fixture, "mutation-host-files"), "[\"src/host.rs\"]");
    fs::write(
        project.join(".github/scripts/host.sh"),
        "changed uncommitted script",
    )
    .unwrap();
    refused(
        &fixture.run_body("rust-gate validate"),
        "host policy path `.github/scripts/host.sh` must match its tracked tested HEAD bytes",
    );
}

#[test]
fn host_plans_force_discovery_in_serial_auto_and_fixed_default_modes() {
    let mut fixture = host_fixture();
    for shards in ["1", "0", "2"] {
        fs::write(fixture.root.join("output"), "").unwrap();
        fixture.set("MUTATION_SHARDS", shards);
        succeeds(&fixture.run_body("rust-gate mutants-plan"));
        assert_eq!(output(&fixture, "mutation-host-count"), "1");
        assert_eq!(output(&fixture, "mutation-mode"), "empty");
    }
    fs::write(fixture.root.join("host-list.json"), "[]").unwrap();
    refused(
        &fixture.run_body("rust-gate mutants-plan"),
        "host discovery requires nonempty exact files, patches and enclosing functions",
    );
}

#[test]
fn removed_host_policy_restores_full_mutation_obligations_without_source_edits() {
    let fixture = host_fixture();
    commit_policy(&fixture, "[ci]\nmutation-test=true\n");
    succeeds(&fixture.run_body("rust-gate mutants-plan"));
    assert!(fixture.trace().contains("cargo mutants --list --json"));
    let diff = fs::read_to_string(fixture.root.join("reports/mutants.diff")).unwrap();
    assert!(
        diff.contains("+++ b/src/host.rs\n@@ -0,0 +1,1 @@\n+pub fn host()"),
        "{diff}"
    );
    assert!(!fixture.calls().contains("--exclude src/host.rs"));
}

#[test]
fn host_source_requires_regular_nonsymlink_tracked_and_instrumentable_files() {
    let fixture = host_fixture();
    let project = fixture.root.join("project");
    let path = project.join("src/host.rs");
    for attribute in ["mutants::skip", "coverage(off)"] {
        fs::write(&path, format!("#[{attribute}] pub fn host() {{}}\n")).unwrap();
        fixture_git(&project, &["add", "."]);
        fixture_git(
            &project,
            &[
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--quiet",
                "-m",
                "skip",
            ],
        );
        refused(
            &fixture.run_body("rust-gate validate"),
            "host mutation file `src/host.rs` disables mutation or coverage",
        );
    }
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    refused(
        &fixture.run_body("rust-gate validate"),
        "host policy path `src/host.rs` must be a regular file",
    );
    fs::remove_dir(&path).unwrap();
    symlink("lib.rs", &path).unwrap();
    refused(
        &fixture.run_body("rust-gate validate"),
        "host policy path `src/host.rs` must not contain symlinks",
    );
}

#[test]
fn host_policy_reuses_declared_qualified_feature_validation() {
    let fixture = host_fixture();
    let original = fs::read_to_string(fixture.root.join("project/maestro-quality.toml")).unwrap();
    for (feature, message) in [
        (
            "fixture/no-such-feature",
            "coverage-features package `fixture` does not declare feature `no-such-feature`",
        ),
        (
            "unknown/host-tests",
            "coverage-features package `unknown` is not a workspace member",
        ),
        (
            "host-tests",
            "coverage-features entry `host-tests` must be package/feature",
        ),
    ] {
        commit_policy(&fixture, &original.replace("fixture/host-tests", feature));
        refused(&fixture.run_body("rust-gate validate"), message);
    }
}

#[test]
fn host_discovery_refuses_missing_patches_functions_and_foreign_population() {
    let fixture = host_fixture();
    let path = fixture.root.join("host-list.json");
    let original: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for (pointer, value) in [
        ("/0/diff", json!("")),
        ("/0/function", json!(null)),
        ("/0/file", json!("src/lib.rs")),
        ("/0/function/span/start/line", json!(0)),
        ("/0/function/span/start/line", json!(0.5)),
        ("/0/function/span/end/line", json!(1.5)),
        ("/0/function/span/start/line", json!(2)),
        ("/0/function/span/end/line", json!(0)),
    ] {
        let mut listing = original.clone();
        *listing.pointer_mut(pointer).unwrap() = value;
        fs::write(&path, listing.to_string()).unwrap();
        refused(
            &fixture.run_body("rust-gate mutants-plan"),
            "host discovery requires nonempty exact files, patches and enclosing functions",
        );
    }
}

#[test]
fn windows_worker_accepts_a_disjoint_current_host_policy() {
    let mut fixture = host_fixture();
    fixture.set("MUTATION_WINDOWS", "[\"src/lib.rs\"]");
    succeeds(&fixture.run_body("rust-gate mutants-windows"));
    assert!(!fixture.calls().contains("--file src/host.rs"));
}

#[test]
fn policy_only_transfer_to_windows_requires_full_file_mutants() {
    let mut fixture = host_fixture();
    commit_policy(
        &fixture,
        "[ci]\nmutation-test=true\nmutation-windows=['src/host.rs']\n",
    );
    fixture.set("MUTATION_WINDOWS", "[\"src/host.rs\"]");
    fs::write(fixture.root.join("host-list.json"), "[]").unwrap();
    refused(
        &fixture.run_body("rust-gate mutants-windows"),
        "src/host.rs produced no mutants",
    );
}

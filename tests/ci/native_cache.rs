//! Native cache policy, private restore transport and coverage-only injection.

use crate::harness::{
    Fixture, cache_fixture, coverage_child_fixture, output, refused, succeeds, workflow,
};
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;

#[test]
fn native_policy_refuses_unknown_fields_and_unsafe_values() {
    for (change, message) in [
        (
            "extra=true\n",
            "[native-cache] must contain only environment, platforms, key-files and published",
        ),
        (
            "environment='BASH_ENV'\n",
            "[native-cache] environment is malformed or reserved",
        ),
        (
            "environment='bad-name'\n",
            "[native-cache] environment is malformed or reserved",
        ),
        (
            "key-files=['../Cargo.lock']\n",
            "[native-cache] key file must exist inside the project",
        ),
        (
            "key-files=['missing']\n",
            "[native-cache] key file must exist inside the project",
        ),
        (
            "published=['../entry-*']\n",
            "[native-cache] published selectors must name root children",
        ),
        (
            "platforms=['windows']\n",
            "[native-cache] platforms must name linux or macos",
        ),
    ] {
        let fixture = cache_fixture();
        let path = fixture.root.join("project/maestro-quality.toml");
        let original = fs::read_to_string(&path).unwrap();
        let key = change.split('=').next().unwrap();
        let mut policy = original
            .lines()
            .filter(|line| !line.starts_with(&format!("{key}=")))
            .collect::<Vec<_>>()
            .join("\n");
        policy.push('\n');
        policy.push_str(change);
        fs::write(path, policy).unwrap();
        refused(&fixture.run_body("rust-gate native-cache-prepare"), message);
    }
}

#[test]
fn absent_policy_leaves_generated_consumers_and_commands_unchanged() {
    let mut fixture = Fixture::new();
    fixture.set("RUST_WORKFLOWS_PIN", &format!("{} v4.6.0", "a".repeat(40)));
    succeeds(&fixture.run_body("cd project && rust-gate sync"));
    let before = fs::read(fixture.root.join("project/.pre-commit-config.yaml")).unwrap();
    succeeds(&fixture.run_body("rust-gate native-cache-prepare"));
    succeeds(&fixture.run_body("cd project && rust-gate sync"));
    assert_eq!(
        before,
        fs::read(fixture.root.join("project/.pre-commit-config.yaml")).unwrap()
    );
    assert!(!fixture.root.join("native-cache").exists());
}

#[test]
fn windows_never_prepares_even_with_selected_features() {
    let mut fixture = cache_fixture();
    fixture.set("NATIVE_CACHE_OS", "windows");
    succeeds(&fixture.run_body("rust-gate native-cache-prepare"));
    assert_eq!(output(&fixture, "enabled"), "false");
    assert!(!fixture.root.join("native-cache").exists());
}

#[test]
fn unsafe_restored_objects_fall_back_without_adopting_bytes() {
    for kind in ["symlink", "hardlink", "fifo"] {
        let mut fixture = cache_fixture();
        succeeds(&fixture.run_body("rust-gate native-cache-prepare"));
        let root = output(&fixture, "root");
        let entry = Path::new(&root).join("entry-one");
        match kind {
            "symlink" => symlink(fixture.root.join("project"), &entry).unwrap(),
            "hardlink" => {
                fs::write(fixture.root.join("outside"), "untouched").unwrap();
                fs::hard_link(fixture.root.join("outside"), &entry).unwrap();
            }
            _ => {
                succeeds(&fixture.run_body(&format!("mkfifo {root}/entry-one")));
            }
        }
        fixture.set("NATIVE_CACHE_ROOT", &root);
        fixture.stub(
            "git",
            "[[ $1 != rev-parse ]] || printf '%s' \"$GITHUB_SHA\"; exit 0",
        );
        fixture.stub(
            "cargo",
            concat!(
                "if [[ $1 == metadata ]]; then printf '%s' '",
                "{\"workspace_members\":[\"a\"],\"packages\":[",
                "{\"id\":\"a\",\"name\":\"fixture\",\"features\":{\"engine\":[]}}]}'; fi\n",
                "if [[ $* == *--features* ]]; then ",
                "printf '%s' \"${FIXTURE_NATIVE_CACHE_DIR:-unset}\" ",
                "> \"$RUNNER_TEMP/child-root\"; fi\n",
                "while [[ $# -gt 0 ]]; do ",
                "if [[ $1 == --output-path ]]; then echo LCOV > \"$2\"; fi; shift; done"
            ),
        );
        succeeds(&fixture.run("ci", "coverage"));
        let selected = fs::read_to_string(fixture.root.join("child-root")).unwrap();
        assert_ne!(selected, root);
        assert!(!Path::new(&selected).join("entry-one").exists());
        assert_eq!(
            fs::metadata(selected).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
}

#[test]
fn coverage_cache_actions_are_pinned_and_save_is_success_guarded() {
    let ci = workflow("ci");
    let steps = ci["jobs"]["checks"]["steps"].as_array().unwrap();
    let restore = steps
        .iter()
        .find(|step| step["id"] == "native-cache-restore")
        .unwrap();
    assert_eq!(
        restore["uses"],
        "actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9"
    );
    assert_eq!(restore["with"]["enableCrossOsArchive"], false);
    let save = steps
        .iter()
        .find(|step| step["id"] == "native-cache-save")
        .unwrap();
    assert_eq!(
        save["uses"],
        "actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9"
    );
    assert!(save["if"].as_str().unwrap().contains("success()"));
    assert_eq!(steps.last().unwrap()["id"], "native-cache-save");
}

#[test]
fn only_changed_nonempty_published_inventory_can_be_saved() {
    for state in [
        "empty",
        "staging",
        "unchanged",
        "emptied",
        "unsafe",
        "published",
        "failed",
        "pr",
        "fallback",
    ] {
        let mut fixture = coverage_child_fixture();
        succeeds(&fixture.run_body("rust-gate native-cache-prepare"));
        let root = output(&fixture, "root");
        fixture.set("NATIVE_CACHE_ROOT", &root);
        if state == "unchanged" || state == "emptied" {
            fs::create_dir(Path::new(&root).join("entry-one")).unwrap();
        }
        succeeds(&fixture.run("ci", "coverage"));
        if state == "emptied" {
            fs::remove_dir(Path::new(&root).join("entry-one")).unwrap();
        } else if state == "unsafe" {
            symlink(
                fixture.root.join("project"),
                Path::new(&root).join("entry-one"),
            )
            .unwrap();
        } else if state != "empty" && state != "unchanged" {
            fs::create_dir(Path::new(&root).join(if state == "staging" {
                "staging-one"
            } else {
                "entry-one"
            }))
            .unwrap();
        }
        fixture.set(
            "EVENT",
            if state == "pr" {
                "pull_request"
            } else {
                "push"
            },
        );
        fixture.set("REF", "refs/heads/main");
        fixture.set("DEFAULT_BRANCH", "main");
        fixture.set(
            "JOB_SUCCESS",
            if state == "failed" { "false" } else { "true" },
        );
        if state == "fallback" {
            fixture.set("NATIVE_CACHE_ROOT", "unrelated");
        }
        fs::write(fixture.root.join("output"), "").unwrap();
        succeeds(&fixture.run_body("rust-gate native-cache-inventory"));
        assert_eq!(
            output(&fixture, "save"),
            if state == "published" {
                "true"
            } else {
                "false"
            },
            "{state}"
        );
    }
}

#[test]
fn escaping_key_symlinks_and_nonstring_lists_are_refused() {
    let fixture = cache_fixture();
    let policy = fixture.root.join("project/maestro-quality.toml");
    let text = fs::read_to_string(&policy).unwrap();
    fs::write(fixture.root.join("outside-key"), "key").unwrap();
    symlink(
        fixture.root.join("outside-key"),
        fixture.root.join("project/key-link"),
    )
    .unwrap();
    fs::write(
        &policy,
        text.replace("key-files=['Cargo.lock']", "key-files=['key-link']"),
    )
    .unwrap();
    refused(
        &fixture.run_body("rust-gate native-cache-prepare"),
        "[native-cache] key file must exist inside the project",
    );
    for value in ["[]", "[1]", "'Cargo.lock'", "[\"Cargo.lock\\n\"]"] {
        fs::write(
            &policy,
            text.replace("key-files=['Cargo.lock']", &format!("key-files={value}")),
        )
        .unwrap();
        refused(
            &fixture.run_body("rust-gate native-cache-prepare"),
            "[native-cache] lists must be nonempty arrays of strings",
        );
    }
}

#[test]
fn hosted_native_fixture_exercises_unix_transport_and_windows_source_fallback() {
    let data = workflow("native-cache-fixture");
    let job = &data["jobs"]["native-cache"];
    let runners = job["strategy"]["matrix"]["include"].as_array().unwrap();
    assert_eq!(runners.len(), 3);
    assert!(runners.iter().any(|row| row["runner"] == "macos-15-intel"));
    assert!(runners.iter().any(|row| row["runner"] == "windows-2025"));
    let steps = job["steps"].as_array().unwrap();
    let coverage = steps.iter().find(|step| step["id"] == "coverage").unwrap();
    assert_eq!(coverage["run"], "rust-gate coverage");
    assert_eq!(
        coverage["env"]["NATIVE_CACHE_ROOT"],
        "${{ steps.native-cache-prepare.outputs.root }}"
    );
    let save = steps
        .iter()
        .find(|step| step["id"] == "native-cache-save")
        .unwrap();
    assert!(save["if"].as_str().unwrap().contains("success()"));
    assert_eq!(data["on"]["push"]["branches"][0], "main");
    let verify = steps
        .iter()
        .find(|step| {
            step["run"]
                .as_str()
                .is_some_and(|run| run.contains("verify_the_executed"))
        })
        .unwrap();
    assert_eq!(
        verify["env"]["NATIVE_RESTORE_MATCHED_KEY"],
        "${{ steps.native-cache-restore.outputs.cache-matched-key }}"
    );
    assert_eq!(
        verify["env"]["NATIVE_RESTORE_HIT"],
        "${{ steps.native-cache-restore.outputs.cache-hit }}"
    );
}

#[test]
fn relative_key_files_refuse_traversal_even_when_the_result_is_contained() {
    let fixture = cache_fixture();
    let policy = fixture.root.join("project/maestro-quality.toml");
    let text = fs::read_to_string(&policy).unwrap();
    fs::create_dir(fixture.root.join("project/sub")).unwrap();
    for key in ["sub/../Cargo.lock", "src"] {
        fs::write(
            &policy,
            text.replace("key-files=['Cargo.lock']", &format!("key-files=['{key}']")),
        )
        .unwrap();
        refused(
            &fixture.run_body("rust-gate native-cache-prepare"),
            "[native-cache] key file must exist inside the project",
        );
    }
}

/// A second independent prepare in the same fixture, as on another fresh hosted runner.
fn next_cache_key(fixture: &Fixture) -> String {
    let parent = fixture.root.join("native-cache");
    if parent.exists() {
        fs::remove_dir_all(parent).unwrap();
    }
    fs::write(fixture.root.join("output"), "").unwrap();
    succeeds(&fixture.run_body("rust-gate native-cache-prepare"));
    assert_eq!(output(fixture, "enabled"), "true");
    output(fixture, "key")
}

#[test]
fn cache_keys_bind_policy_lockfile_and_native_inputs_but_not_unrelated_source() {
    let fixture = cache_fixture();
    let project = fixture.root.join("project");
    let policy = project.join("maestro-quality.toml");
    let text = fs::read_to_string(&policy)
        .unwrap()
        .replace("key-files=['Cargo.lock']", "key-files=['native-key']");
    fs::write(&policy, &text).unwrap();
    fs::write(project.join("native-key"), "native inputs").unwrap();
    let first = next_cache_key(&fixture);
    assert!(first.starts_with("native-v1-linux-X64-1.98.1-"));
    assert!(first.ends_with("-coverage-123-1"));
    fs::write(project.join("README.md"), "unrelated documentation").unwrap();
    assert_eq!(next_cache_key(&fixture), first);
    fs::write(project.join("Cargo.lock"), "version = 4\n# changed\n").unwrap();
    let lock = next_cache_key(&fixture);
    assert_ne!(lock, first);
    fs::write(project.join("native-key"), "changed native inputs").unwrap();
    let native = next_cache_key(&fixture);
    assert_ne!(native, lock);
    fs::write(
        policy,
        text.replace("FIXTURE_NATIVE_CACHE_DIR", "OTHER_NATIVE_CACHE_DIR"),
    )
    .unwrap();
    assert_ne!(next_cache_key(&fixture), native);
}

#[test]
fn unsupported_host_and_unselected_coverage_never_prepare_transport() {
    for state in ["platform", "unselected"] {
        let mut fixture = coverage_child_fixture();
        if state == "unselected" {
            fixture.set("COVERAGE_FEATURES", "");
        } else {
            let policy = fixture.root.join("project/maestro-quality.toml");
            let text = fs::read_to_string(&policy)
                .unwrap()
                .replace("platforms=['linux','macos']", "platforms=['macos']");
            fs::write(policy, text).unwrap();
        }
        succeeds(&fixture.run_body("rust-gate native-cache-prepare"));
        assert_eq!(output(&fixture, "enabled"), "false");
        assert!(!fixture.root.join("native-cache").exists());
        if state == "platform" {
            fixture.set("NATIVE_CACHE_ROOT", "never-adopt");
            succeeds(&fixture.run("ci", "coverage"));
            assert!(!fixture.trace().contains("FIXTURE_NATIVE_CACHE_DIR="));
        }
    }
}

#[test]
fn preexisting_parent_disables_transport_and_selects_a_private_source_root() {
    let fixture = cache_fixture();
    fs::create_dir(fixture.root.join("native-cache")).unwrap();
    succeeds(&fixture.run_body("rust-gate native-cache-prepare"));
    assert_eq!(output(&fixture, "enabled"), "false");
    let root = output(&fixture, "root");
    assert!(!root.starts_with(&fixture.root.join("native-cache").display().to_string()));
    assert_eq!(
        fs::metadata(root).unwrap().permissions().mode() & 0o777,
        0o700
    );
}

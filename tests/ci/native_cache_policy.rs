//! Native policy parser isolation and coverage child environment regressions.

use crate::harness::{Fixture, cache_fixture, coverage_child_fixture, output, refused, succeeds};
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

#[test]
fn consumer_policy_variable_is_never_exported_to_parser_children() {
    let mut fixture = coverage_child_fixture();
    let policy = fixture.root.join("project/maestro-quality.toml");
    let text = fs::read_to_string(&policy).unwrap();
    fs::write(
        policy,
        text.replace("FIXTURE_NATIVE_CACHE_DIR", "NATIVE_POLICY"),
    )
    .unwrap();
    let script = fs::read_to_string(fixture.root.join("bin/cargo")).unwrap();
    fixture.stub(
        "cargo",
        &script.replace("FIXTURE_NATIVE_CACHE_DIR", "NATIVE_POLICY"),
    );
    succeeds(&fixture.run_body("rust-gate native-cache-prepare"));
    assert!(
        !fixture
            .trace()
            .lines()
            .any(|line| line.starts_with("NATIVE_POLICY=")),
        "{}",
        fixture.trace()
    );
    let root = output(&fixture, "root");
    fixture.set("NATIVE_CACHE_ROOT", &root);
    succeeds(&fixture.run("ci", "coverage"));
    let injected: Vec<_> = fixture
        .trace()
        .lines()
        .filter(|line| line.starts_with("NATIVE_POLICY="))
        .map(str::to_owned)
        .collect();
    assert_eq!(injected.len(), 1, "{injected:?}");
    assert!(injected[0].contains("--features fixture/engine"));
    let children = fs::read_to_string(fixture.root.join("child-env")).unwrap();
    assert!(children.lines().next().unwrap().ends_with(" unset"));
    assert!(children.lines().last().unwrap().ends_with(&root));
    assert!(
        !fs::read_to_string(fixture.root.join("environment"))
            .unwrap_or_default()
            .contains("NATIVE_POLICY=")
    );
}

/// Restore test-directory access for Fixture cleanup, after the step has finished.
fn restore_directory_access(fixture: &Fixture) {
    succeeds(&fixture.run_body("chmod 700 \"$RUNNER_TEMP\"/native-*"));
}

#[test]
fn owner_stripping_umask_keeps_private_fallback_unset_when_unusable() {
    let fixture = cache_fixture();
    // The trace and outputs are not the private directories under test.
    for name in ["trace", "output"] {
        fs::write(fixture.root.join(name), "").unwrap();
    }
    let result = fixture.run_body("umask 0777; rust-gate native-cache-prepare");
    restore_directory_access(&fixture);
    succeeds(&result);
    assert_eq!(output(&fixture, "enabled"), "false");
    assert_eq!(output(&fixture, "root"), "");
    let mut fixture = coverage_child_fixture();
    succeeds(&fixture.run_body("rust-gate native-cache-prepare"));
    let root = output(&fixture, "root");
    symlink(
        fixture.root.join("project"),
        Path::new(&root).join("entry-one"),
    )
    .unwrap();
    fixture.set("NATIVE_CACHE_ROOT", &root);
    for name in [
        "calls",
        "child-env",
        "summary",
        "reports/coverage.lcov",
        "reports/all-child-env",
    ] {
        fs::write(fixture.root.join(name), "").unwrap();
    }
    let result = fixture.run_body("umask 0777; rust-gate coverage");
    restore_directory_access(&fixture);
    succeeds(&result);
    let children = fs::read_to_string(fixture.root.join("child-env")).unwrap();
    assert!(
        children.lines().all(|line| line.ends_with(" unset")),
        "{children}"
    );
    assert!(
        !fixture
            .root
            .join("reports/native-cache-before.txt")
            .exists()
    );
}

#[test]
fn present_false_native_cache_policy_refuses_the_non_table_value() {
    for value in ["false", "true", "1", "'cache'", "[]"] {
        let fixture = cache_fixture();
        fs::write(
            fixture.root.join("project/maestro-quality.toml"),
            format!("native-cache = {value}\n"),
        )
        .unwrap();
        refused(
            &fixture.run_body("rust-gate native-cache-prepare"),
            "[native-cache] must be a table",
        );
    }
}

#[test]
fn only_feature_coverage_receives_the_private_native_variable() {
    let mut fixture = coverage_child_fixture();
    fixture.set("FIXTURE_NATIVE_CACHE_DIR", "/tmp/shared-native-cache");
    succeeds(&fixture.run_body("rust-gate native-cache-prepare"));
    let root = output(&fixture, "root");
    fixture.set("NATIVE_CACHE_ROOT", &root);
    succeeds(&fixture.run_body(concat!(
        "rust-gate coverage; ",
        "printf '%s' \"$FIXTURE_NATIVE_CACHE_DIR\" > \"$REPORTS/job-env\""
    )));
    assert_eq!(
        fs::read_to_string(fixture.root.join("reports/job-env")).unwrap(),
        "/tmp/shared-native-cache"
    );
    let all = fs::read_to_string(fixture.root.join("reports/all-child-env")).unwrap();
    assert_eq!(all.lines().count(), 5, "{all}");
    for line in all.lines() {
        assert!(
            line.ends_with(if line.contains("--features") {
                &root
            } else {
                " unset"
            }),
            "{line}"
        );
    }
    let child = fs::read_to_string(fixture.root.join("child-env")).unwrap();
    assert_eq!(
        child.lines().collect::<Vec<_>>(),
        [
            "llvm-cov --workspace --locked --no-report --no-rustc-wrapper unset".to_owned(),
            format!(
                "llvm-cov --workspace --locked --no-report --no-rustc-wrapper \
                 --features fixture/engine {root}"
            )
        ]
    );
    assert_eq!(
        fixture.trace().matches("FIXTURE_NATIVE_CACHE_DIR=").count(),
        1
    );
    assert!(
        !fs::read_to_string(fixture.root.join("environment"))
            .unwrap_or_default()
            .contains("FIXTURE_NATIVE_CACHE_DIR=")
    );
}

#[test]
fn absent_policy_preserves_inherited_coverage_child_environments() {
    for selection in ["", "[\"fixture/engine\"]"] {
        let mut fixture = coverage_child_fixture();
        fs::remove_file(fixture.root.join("project/maestro-quality.toml")).unwrap();
        fixture.set("COVERAGE_FEATURES", selection);
        fixture.set("FIXTURE_NATIVE_CACHE_DIR", "/tmp/shared-native-cache");
        succeeds(&fixture.run("ci", "coverage"));
        let all = fs::read_to_string(fixture.root.join("reports/all-child-env")).unwrap();
        assert_eq!(
            all.lines().count(),
            if selection.is_empty() { 1 } else { 5 }
        );
        assert!(
            all.lines()
                .all(|line| line.ends_with(" /tmp/shared-native-cache")),
            "{all}"
        );
        assert!(!fixture.trace().contains("env -u"));
        assert!(!fixture.trace().contains("FIXTURE_NATIVE_CACHE_DIR="));
    }
}

#[test]
fn failed_private_allocation_leaves_the_child_variable_unset() {
    let mut fixture = coverage_child_fixture();
    let script = fs::read_to_string(fixture.root.join("bin/cargo"))
        .unwrap()
        .replace("$RUNNER_TEMP/child-env", "$REPORTS/child-env");
    fixture.stub("cargo", &script);
    fixture.set(
        "RUNNER_TEMP",
        &fixture.root.join("missing").display().to_string(),
    );
    fixture.set("NATIVE_CACHE_ROOT", "rejected-root");
    fixture.set("FIXTURE_NATIVE_CACHE_DIR", "/tmp/shared-native-cache");
    succeeds(&fixture.run("ci", "coverage"));
    let children = fs::read_to_string(fixture.root.join("reports/child-env")).unwrap();
    assert!(
        children.lines().all(|line| line.ends_with(" unset")),
        "{children}"
    );
    let all = fs::read_to_string(fixture.root.join("reports/all-child-env")).unwrap();
    assert_eq!(all.lines().count(), 5, "{all}");
    assert!(all.lines().all(|line| line.ends_with(" unset")), "{all}");
    assert!(!fixture.trace().contains("FIXTURE_NATIVE_CACHE_DIR="));
    assert!(
        !fixture
            .root
            .join("reports/native-cache-before.txt")
            .exists()
    );
    succeeds(&fixture.run_body("rust-gate native-cache-prepare"));
    assert_eq!(output(&fixture, "enabled"), "false");
}

#[test]
fn native_coverage_disables_the_wrapper_only_on_opted_in_platforms() {
    for selection in ["", "[\"fixture/engine\"]"] {
        for state in ["enabled", "absent", "platform"] {
            let mut fixture = coverage_child_fixture();
            fixture.set("COVERAGE_FEATURES", selection);
            let policy = fixture.root.join("project/maestro-quality.toml");
            if state == "absent" {
                fs::remove_file(&policy).unwrap();
            } else if state == "platform" {
                let text = fs::read_to_string(&policy).unwrap();
                fs::write(policy, text.replace("['linux','macos']", "['macos']")).unwrap();
            }
            succeeds(&fixture.run("ci", "coverage"));
            let calls = fixture.calls();
            let executions: Vec<_> = calls
                .lines()
                .filter(|line| line.starts_with("llvm-cov --workspace"))
                .collect();
            assert_eq!(executions.len(), if selection.is_empty() { 1 } else { 2 });
            for command in executions {
                assert_eq!(
                    command.contains("--no-rustc-wrapper"),
                    state == "enabled",
                    "{state}/{selection}: {command}"
                );
            }
            assert_eq!(calls.matches("--fail-under-lines 90").count(), 1);
            if !selection.is_empty() {
                let binding =
                    fs::read_to_string(fixture.root.join("reports/coverage-binding.txt")).unwrap();
                assert_eq!(
                    binding.matches("--no-rustc-wrapper").count(),
                    if state == "enabled" { 2 } else { 0 }
                );
            }
        }
    }
}

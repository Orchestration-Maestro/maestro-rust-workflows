//! Real fresh-target builds prove optional consumer cache reuse, not a command stub.

use crate::harness::{Fixture, engine_workspace, output, succeeds};
use std::fs;

/// Build one feature execution from a fresh Cargo target directory.
fn fresh_build(fixture: &Fixture, target: &str, cache: Option<&str>, flags: &str) {
    let cache = cache.map_or(String::new(), |root| {
        format!("FIXTURE_NATIVE_CACHE_DIR='{root}' ")
    });
    succeeds(&fixture.run_body(&format!(
        concat!(
            "cd project && {}RUSTFLAGS=\"{}\" CARGO_TARGET_DIR=\"$RUNNER_TEMP/{}\" ",
            "FIXTURE_NATIVE_BUILD_LOG=\"$RUNNER_TEMP/build-count\" ",
            "cargo test --offline --workspace --features crate-a/engine"
        ),
        cache, flags, target
    )));
}

#[test]
fn fresh_targets_rebuild_without_provisioning_and_reuse_private_entries() {
    let mut fixture = Fixture::new();
    engine_workspace(&fixture.root.join("project"), true);
    fresh_build(&fixture, "red-one", None, "");
    fresh_build(&fixture, "red-two", None, "");
    let count = fixture.root.join("build-count");
    let builds = fs::read_to_string(&count).unwrap_or_default();
    assert_eq!(
        builds.lines().count(),
        2,
        "fresh targets must rebuild without provisioning"
    );
    println!("RED transport control: two fresh targets, two source builds");
    fs::remove_file(&count).unwrap();
    fixture.set("COVERAGE_FEATURES", "[\"crate-a/engine\"]");
    fixture.set("NATIVE_CACHE_MODE", "coverage");
    fixture.set("NATIVE_CACHE_OS", "linux");
    fixture.set("NATIVE_CACHE_ARCH", "X64");
    succeeds(&fixture.run_body("cd project && cargo generate-lockfile --offline"));
    succeeds(&fixture.run_body("rust-gate native-cache-prepare"));
    let root = fs::read_to_string(fixture.root.join("output"))
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix("root="))
        .unwrap()
        .to_owned();
    fresh_build(&fixture, "green-one", Some(&root), "");
    fresh_build(&fixture, "green-two", Some(&root), "");
    assert_eq!(fs::read_to_string(count).unwrap().lines().count(), 1);
    println!("GREEN transport: two fresh targets, one source build, one inner verified reuse");
}

#[test]
fn default_workspace_execution_does_not_build_optional_native_dependency() {
    let fixture = Fixture::new();
    engine_workspace(&fixture.root.join("project"), true);
    succeeds(&fixture.run_body(concat!(
        "cd project && FIXTURE_NATIVE_BUILD_LOG=\"$RUNNER_TEMP/build-count\" ",
        "cargo test --offline --workspace"
    )));
    assert!(!fixture.root.join("build-count").exists());
}

/// Snapshot only the published inventory, like the gate's pre-execution record.
fn record_fixture_inventory(fixture: &Fixture, root: &str) {
    let mut names = fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    names.sort();
    fs::write(
        fixture.root.join("reports/native-cache-before.txt"),
        format!("{root}\n{}", names.join("\n")),
    )
    .unwrap();
}

#[test]
fn different_native_flags_publish_distinct_immutable_entries() {
    let mut fixture = Fixture::new();
    engine_workspace(&fixture.root.join("project"), true);
    fixture.set("COVERAGE_FEATURES", "[\"crate-a/engine\"]");
    fixture.set("NATIVE_CACHE_MODE", "coverage");
    fixture.set("NATIVE_CACHE_OS", "linux");
    fixture.set("NATIVE_CACHE_ARCH", "X64");
    succeeds(&fixture.run_body("cd project && cargo generate-lockfile --offline"));
    succeeds(&fixture.run_body("rust-gate native-cache-prepare"));
    let root = fs::read_to_string(fixture.root.join("output"))
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix("root="))
        .unwrap()
        .to_owned();
    fresh_build(
        &fixture,
        "coverage-flag-zero",
        Some(&root),
        "-C opt-level=0",
    );
    record_fixture_inventory(&fixture, &root);
    fresh_build(&fixture, "mutation-flag-one", Some(&root), "-C opt-level=1");
    for (key, value) in [
        ("EVENT", "push"),
        ("REF", "refs/heads/main"),
        ("DEFAULT_BRANCH", "main"),
        ("JOB_SUCCESS", "true"),
        ("NATIVE_CACHE_ROOT", &root),
    ] {
        fixture.set(key, value);
    }
    succeeds(&fixture.run_body("rust-gate native-cache-inventory"));
    assert_eq!(output(&fixture, "save"), "true");
    record_fixture_inventory(&fixture, &root);
    fs::write(fixture.root.join("output"), "").unwrap();
    fixture.set("NATIVE_CACHE_MODE", "mutation");
    assert_eq!(
        fs::read_to_string(fixture.root.join("build-count"))
            .unwrap()
            .lines()
            .count(),
        2
    );
    assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
    fresh_build(&fixture, "coverage-flag-one", Some(&root), "-C opt-level=1");
    fresh_build(
        &fixture,
        "mutation-flag-zero",
        Some(&root),
        "-C opt-level=0",
    );
    assert_eq!(
        fs::read_to_string(fixture.root.join("build-count"))
            .unwrap()
            .lines()
            .count(),
        2
    );
    assert_eq!(fs::read_dir(root).unwrap().count(), 2);
    succeeds(&fixture.run_body("rust-gate native-cache-inventory"));
    assert_eq!(output(&fixture, "save"), "false");
}

//! Real fresh-target builds prove optional consumer cache reuse, not a command stub.

use crate::harness::{Fixture, engine_workspace, succeeds};
use std::fs;

/// Build one feature execution from a fresh Cargo target directory.
fn fresh_build(fixture: &Fixture, target: &str, cache: Option<&str>) {
    let cache = cache.map_or(String::new(), |root| {
        format!("FIXTURE_NATIVE_CACHE_DIR='{root}' ")
    });
    succeeds(&fixture.run_body(&format!(
        concat!(
            "cd project && {}CARGO_TARGET_DIR=\"$RUNNER_TEMP/{}\" ",
            "FIXTURE_NATIVE_BUILD_LOG=\"$RUNNER_TEMP/build-count\" ",
            "cargo test --offline --workspace --features crate-a/engine"
        ),
        cache, target
    )));
}

#[test]
fn fresh_targets_rebuild_without_provisioning_and_reuse_private_entries() {
    let mut fixture = Fixture::new();
    engine_workspace(&fixture.root.join("project"), true);
    fresh_build(&fixture, "red-one", None);
    fresh_build(&fixture, "red-two", None);
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
    fresh_build(&fixture, "green-one", Some(&root));
    fresh_build(&fixture, "green-two", Some(&root));
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

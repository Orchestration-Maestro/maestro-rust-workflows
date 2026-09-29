//! Repository-only synthetic mutation-shard input validation.

use crate::harness::{Fixture, refused};
use std::fs;

#[test]
fn selftest_refuses_when_the_example_source_drifted() {
    let mut fixture = Fixture::new();
    let source = fixture.root.join("project/core/src/arithmetic.rs");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(source, "//! A different fixture.\\n").unwrap();
    fixture.set("MUTATION_TEST", "true");
    fixture.set("INTERNAL_SHARD_SELFTEST", "true");
    fixture.set("MUTATION_SHARDS", "2");
    refused(
        &fixture.run_body("rust-gate mutants-plan"),
        "internal shard self-test source no longer matches its fixture",
    );
}

#[test]
fn selftest_refuses_to_rewrite_a_different_workspace() {
    let mut fixture = Fixture::new();
    fixture.set("MUTATION_TEST", "true");
    fixture.set("INTERNAL_SHARD_SELFTEST", "true");
    fixture.set("MUTATION_SHARDS", "2");
    refused(
        &fixture.run_body("rust-gate mutants-plan"),
        "cannot read internal shard self-test source",
    );
}

#[test]
fn internal_shard_selftest_input_requires_a_boolean() {
    let mut fixture = Fixture::new();
    fixture.set("INTERNAL_SHARD_SELFTEST", "yes");
    refused(
        &fixture.run("ci", "validate"),
        "internal-shard-selftest must be true or false",
    );
}

#[test]
fn an_external_consumer_cannot_enable_the_internal_shard_selftest() {
    let mut fixture = Fixture::new();
    fixture.set("INTERNAL_SHARD_SELFTEST", "true");
    fixture.set("GITHUB_REPOSITORY", "consumer/project");
    refused(
        &fixture.run("ci", "validate"),
        concat!(
            "internal-shard-selftest is only allowed in ",
            "Orchestration-Maestro/maestro-rust-workflows"
        ),
    );
}

#[test]
fn internal_shard_selftest_requires_its_fixed_workspace_and_shard_count() {
    let mut fixture = Fixture::new();
    fixture.set("INTERNAL_SHARD_SELFTEST", "true");
    refused(
        &fixture.run("ci", "validate"),
        concat!(
            "internal-shard-selftest requires mutation-test=true, mutation-shards=2, and ",
            "working-directory=examples/workspace"
        ),
    );
}

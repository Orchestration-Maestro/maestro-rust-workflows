//! Mutation shard input validation before environment exports.

use crate::harness::{Fixture, refused, succeeds};
use std::fs;

#[test]
fn mutation_shard_numbers_are_validated_before_any_export() {
    for (name, value, message) in [
        (
            "MUTATION_SHARDS",
            "-1",
            "mutation-shards must be a whole number between 0 and 256",
        ),
        (
            "MUTATION_SHARDS",
            "1.5",
            "mutation-shards must be a whole number between 0 and 256",
        ),
        (
            "MUTATION_SHARDS",
            "257",
            "mutation-shards must be a whole number between 0 and 256",
        ),
        (
            "MUTATION_SHARDS",
            "true",
            "mutation-shards must be a whole number between 0 and 256",
        ),
        (
            "MUTATION_SHARDS",
            "nan",
            "mutation-shards must be a whole number between 0 and 256",
        ),
        (
            "MUTATION_MUTANTS_PER_SHARD",
            "0",
            "mutation-mutants-per-shard must be a whole number between 1 and 1000",
        ),
        (
            "MUTATION_MUTANTS_PER_SHARD",
            "1001",
            "mutation-mutants-per-shard must be a whole number between 1 and 1000",
        ),
        (
            "MUTATION_MUTANTS_PER_SHARD",
            "2.5",
            "mutation-mutants-per-shard must be a whole number between 1 and 1000",
        ),
    ] {
        let mut fixture = Fixture::new();
        fixture.set("MUTATION_TEST", "false");
        fixture.set(name, value);
        refused(&fixture.run("ci", "validate"), message);
        assert!(!fixture.root.join("environment").exists(), "{name}={value}");
        assert!(!fixture.root.join("output").exists(), "{name}={value}");
    }
    let mut valid = Fixture::new();
    valid.set("MUTATION_SHARDS", "200");
    valid.set("MUTATION_MUTANTS_PER_SHARD", "1000");
    succeeds(&valid.run("ci", "validate"));
    let environment = fs::read_to_string(valid.root.join("environment")).unwrap();
    assert!(
        environment
            .lines()
            .any(|line| line == "MUTATION_SHARDS=200")
    );
    assert!(
        environment
            .lines()
            .any(|line| line == "MUTATION_MUTANTS_PER_SHARD=1000")
    );
}

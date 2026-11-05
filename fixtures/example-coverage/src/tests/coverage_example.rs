//! Tests the shipped example through its single source file.

#[path = "../../examples/coverage.rs"]
mod example;

/// Runs the shipped example from the crate's own tests.
#[test]
fn example_runs() {
    example::main();
}

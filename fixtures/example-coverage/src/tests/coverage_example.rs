//! Tests the shipped example through its single source file.

#[path = "../../examples/coverage.rs"]
mod example;

#[test]
fn example_runs() {
    example::main();
}

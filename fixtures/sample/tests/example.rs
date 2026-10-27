//! Exercises the shipped example through the same source file.

#[path = "../examples/coverage.rs"]
mod example;

#[test]
fn example_runs() {
    example::main();
}

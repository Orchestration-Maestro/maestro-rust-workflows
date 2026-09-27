//! Prepare the repository-owned mutation-shard fixture before planning or execution.

use crate::runner::{Cmd, Job, Outcome, optional};
use std::fs;

/// The expression the repository-owned fixture begins with.
const BEFORE: &str = "    right.checked_add(left)\n";
/// Its behavior-equivalent form, which yields cargo-mutants cases.
const AFTER: &str = "    left.checked_add(right)\n";

/// Change exactly one fixture expression; refuse source that has drifted.
fn rewritten_source(contents: &str) -> Result<String, &'static str> {
    if contents.matches(BEFORE).count() != 1 {
        return Err("internal shard self-test source no longer matches its fixture");
    }
    Ok(contents.replacen(BEFORE, AFTER, 1))
}

/// Change only the owned workspace fixture and commit it to create a diff.
pub(super) fn prepare(job: &Job) -> Outcome {
    match optional("INTERNAL_SHARD_SELFTEST")?.as_str() {
        "" | "false" => return Ok(()),
        "true" => {}
        _ => return Err("internal-shard-selftest must be true or false".into()),
    }

    let source = job.project.join("core/src/lib.rs");
    let contents = fs::read_to_string(&source)
        .map_err(|error| format!("cannot read internal shard self-test source: {error}"))?;
    let contents = rewritten_source(&contents).map_err(str::to_owned)?;
    fs::write(&source, contents)
        .map_err(|error| format!("cannot write internal shard self-test source: {error}"))?;

    Cmd::new("git add -- core/src/lib.rs")
        .cwd(&job.project)
        .run()?;
    Cmd::new("git")
        .args([
            "-c",
            "user.name=Rust workflow self-test",
            "-c",
            "user.email=ci-selftest@example.invalid",
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "--quiet",
            "--no-gpg-sign",
            "-m",
            "test: create internal mutation shard diff",
        ])
        .cwd(&job.project)
        .run()
}

#[cfg(test)]
mod tests {
    use super::rewritten_source;

    #[test]
    fn selftest_reverses_only_the_checked_add_operands() {
        assert_eq!(
            rewritten_source("    right.checked_add(left)\n").unwrap(),
            "    left.checked_add(right)\n"
        );
    }

    #[test]
    fn selftest_refuses_a_source_that_does_not_match_its_fixture() {
        assert_eq!(
            rewritten_source("pub fn unrelated() {}\n").unwrap_err(),
            "internal shard self-test source no longer matches its fixture"
        );
    }
}

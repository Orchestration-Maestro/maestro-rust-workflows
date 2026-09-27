//! Validate cargo-mutants outcome paths and produce one canonical JSON document.

use super::artifacts::{safe_file, safe_relative};
use crate::runner::{Cmd, Failure, Outcome, input};
use std::fs;
use std::path::{Path, PathBuf};

/// Select the raw logs and diffs that must remain inside the shard directory.
const OUTCOME_PATHS: &str = concat!(
    "[.outcomes[] | [.log_path // \"\", .diff_path // \"\"] ",
    "| .[] | select(. != \"\")] | .[]"
);
/// Merge shard results and rewrite their paths relative to the canonical reports bundle.
const AGGREGATE_OUTCOMES: &str = concat!(
    "{outcomes:[range(0;length) as $i | .[$i].outcomes[] ",
    "| .log_path=(if (.log_path|type)==\"string\" then ",
    "\"mutation-shards/\\($i)/mutants/mutants.out/\" + .log_path ",
    "else .log_path end) ",
    "| .diff_path=(if (.diff_path|type)==\"string\" then ",
    "\"mutation-shards/\\($i)/mutants/mutants.out/\" + .diff_path ",
    "else .diff_path end)], ",
    "total_mutants:(map(.total_mutants)|add), caught:(map(.caught)|add), ",
    "missed:(map(.missed)|add), timeout:(map(.timeout)|add), ",
    "unviable:(map(.unviable)|add), success:(map(.success)|add), ",
    "start_time:(map(.start_time)|min), end_time:(map(.end_time)|max), ",
    "cargo_mutants_version:$version}"
);

/// Validate every raw log and diff path before reporting it from merged JSON.
pub(super) fn validate_result_paths(root: &Path, outcomes: &Path) -> Outcome {
    let paths = Cmd::new("jaq -r")
        .arg(OUTCOME_PATHS)
        .arg(outcomes)
        .capture()?;
    for value in paths.lines() {
        if !safe_relative(value) {
            return Err("outcomes contain an unsafe log or diff path".into());
        }
        safe_file(&root.join(value))?;
    }
    Ok(())
}

/// Merge complete outcome documents and namespace paths into retained shard folders.
pub(super) fn aggregate_outcomes(paths: &[PathBuf]) -> Result<String, Failure> {
    let version = input("CARGO_MUTANTS_VERSION")?;
    let mut documents = Vec::new();
    for path in paths {
        documents.extend_from_slice(
            &fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?,
        );
        documents.push(b'\n');
    }
    Cmd::new("jaq -s -c")
        .args(["--arg", "version", version.as_str()])
        .arg(AGGREGATE_OUTCOMES)
        .stdin_bytes(&documents)
        .capture()
}

/// Extract one JSON field through the pinned reader.
pub(super) fn jaq(expression: &str, path: &Path) -> Result<String, Failure> {
    Cmd::new("jaq -r")
        .arg(expression)
        .arg(path)
        .capture()
        .map(|value| value.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::super::artifacts::safe_relative;

    #[test]
    fn outcome_paths_cannot_escape_their_shard() {
        assert!(safe_relative("mutants.out/logs/baseline.log"));
        assert!(!safe_relative("../../outside.log"));
    }
}

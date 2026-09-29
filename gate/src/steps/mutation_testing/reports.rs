//! Preserve standard mutation summaries and reject incomplete outcomes.

use crate::runner::{Cmd, Job, Outcome, non_empty, output, tee_line};
use std::fs;
use std::path::Path;

/// Interpret only successful tool runs, after raw outcomes were preserved.
pub(super) fn report_outcomes(
    job: &Job,
    outcomes: &Path,
    change: Option<&str>,
    expected_mutants: Option<usize>,
) -> Outcome {
    let report = job.report("mutants.txt")?;
    if !outcomes.exists() {
        let log = fs::read_to_string(&report)
            .map_err(|error| format!("{}: {error}", report.display()))?;
        if recognized_no_work(&log, change.is_some()) {
            if expected_mutants.is_some_and(|expected| expected > 0) {
                return Err(
                    "cargo-mutants reported no work for a shard with planned mutants".into(),
                );
            }
            let message = format!(
                "SKIPPED: no mutants apply to {}",
                change.unwrap_or("this workspace")
            );
            tee_line(&message, &report, true)?;
            return output("applied", "false");
        }
    }
    non_empty(outcomes).map_err(|_| "cargo-mutants produced no outcomes")?;
    Cmd::new("jaq -er")
        .arg(
            "\"caught=\\(.caught) missed=\\(.missed) timeout=\\(.timeout) unviable=\\(.unviable)\"",
        )
        .arg(outcomes)
        .tee(&report, true)?;
    Cmd::new("jaq -e")
        .arg(".missed == 0 and .timeout == 0")
        .arg(outcomes)
        .capture()
        .map(|_| ())
        .map_err(|_| "Surviving or timed-out mutants; strengthen the tests that should fail")?;
    let applied = Cmd::new("jaq -r")
        .arg("(.caught + .unviable) > 0")
        .arg(outcomes)
        .capture()?;
    output("applied", applied.trim())
}

/// Recognize only cargo-mutants' pinned, explicit no-work diagnostics.
fn recognized_no_work(log: &str, has_change: bool) -> bool {
    log.lines().any(|line| {
        line.trim() == "WARN No mutants found under the active filters"
            || (has_change
                && matches!(
                    line.trim(),
                    "INFO Diff file is empty"
                        | "INFO Diff changes no Rust source files"
                        | "INFO No mutants to filter"
                ))
    })
}

#[cfg(test)]
mod tests {
    use super::recognized_no_work;

    #[test]
    fn only_explicit_pinned_no_work_messages_allow_missing_outcomes() {
        assert!(recognized_no_work(
            "WARN No mutants found under the active filters\n",
            false
        ));
        assert!(recognized_no_work("INFO Diff file is empty\n", true));
        assert!(!recognized_no_work("INFO Diff file is empty\n", false));
        assert!(!recognized_no_work("", true));
    }
}

//! Merge validated outcomes and derive the final mutation verdict.

use super::evidence::{Counts, Evidence, Plan};
use super::outcomes::aggregate_outcomes;
use crate::runner::{Failure, Job, input, summary, tee_line, write};
use std::fs;
use std::path::{Path, PathBuf};

/// Merge only after the matrix, every identity and every outcome is complete.
pub(super) fn finish_aggregate(
    job: &Job,
    report: &Path,
    plan: &Plan,
    evidence: &[Evidence],
) -> Result<bool, Failure> {
    let total = sum_counts(evidence)?;
    let outcomes: Vec<PathBuf> = evidence.iter().map(|item| item.outcomes.clone()).collect();
    let merged = aggregate_outcomes(&outcomes)?;
    write(&job.report("mutants.json")?, merged.as_bytes(), false)?;
    tee_line(
        &format!(
            concat!(
                "Aggregate complete: {} mutants; {} shards; ",
                "caught={} missed={} timeout={} unviable={} success={}"
            ),
            plan.mutants,
            plan.shards,
            total.caught,
            total.missed,
            total.timeout,
            total.unviable,
            total.success
        ),
        report,
        true,
    )?;
    let baseline_ok = evidence.iter().all(|item| item.baseline_ok);
    let passed = input("MUTATIONS_RESULT")? == "success"
        && input("CHECKS_RESULT")? == "success"
        && baseline_ok
        && total.missed == 0
        && total.timeout == 0;
    summary(&fs::read_to_string(report).unwrap_or_default())?;
    Ok(passed)
}

/// Sum only already validated per-shard counters, refusing integer overflow.
fn sum_counts(evidence: &[Evidence]) -> Result<Counts, Failure> {
    evidence
        .iter()
        .try_fold(Counts::default(), |mut total, item| {
            total.total = checked_sum(total.total, item.counts.total)?;
            total.caught = checked_sum(total.caught, item.counts.caught)?;
            total.missed = checked_sum(total.missed, item.counts.missed)?;
            total.timeout = checked_sum(total.timeout, item.counts.timeout)?;
            total.unviable = checked_sum(total.unviable, item.counts.unviable)?;
            total.success = checked_sum(total.success, item.counts.success)?;
            Ok(total)
        })
}

/// Add two validated counters without wrapping.
fn checked_sum(left: usize, right: usize) -> Result<usize, Failure> {
    left.checked_add(right)
        .ok_or_else(|| "mutation counters overflow".into())
}

#[cfg(test)]
mod tests {
    use super::checked_sum;

    #[test]
    fn overflow_counters_are_refused() {
        assert_eq!(checked_sum(2, 3).unwrap(), 5);
        assert_eq!(
            checked_sum(usize::MAX, 1).unwrap_err().message.as_deref(),
            Some("mutation counters overflow")
        );
    }
}

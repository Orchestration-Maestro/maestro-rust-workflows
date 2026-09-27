//! Cross-check one shard's receipt, identity assignment and raw outcomes.

use super::artifacts::safe_file;
use super::outcomes::{jaq, validate_result_paths};
use crate::runner::{Cmd, Failure, Outcome, input};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Required identity fields that bind a receipt to its planned shard.
const RECEIPT_IDENTITY: &str = concat!(
    ".sha == $sha and .first_parent == ",
    "(if $parent == \"\" then null else $parent end) and .directory == $directory ",
    "and .config_sha256 == $config and .diff_sha256 == $diff and .run_id == $run ",
    "and .attempt == $attempt and .toolchain == $toolchain ",
    "and .cargo_mutants_version == $version and .mutant_count == $count ",
    "and .shard_count == $shards and .shard_index == $index ",
    "and .expected_mutants == $expected"
);
/// cargo-mutants predicate requiring one baseline with successful phases.
const BASELINE_SUCCESS: &str = concat!(
    "([.outcomes[] | select(.scenario == \"Baseline\")] | length) == 1 and ",
    "([.outcomes[] | select(.scenario == \"Baseline\")][0] | ",
    ".summary == \"Success\" and (.phase_results | type == \"array\" and length > 0 ",
    "and all(.[]; .process_status == \"Success\")))"
);
/// jaq projection of every stable mutant identity field.
const IDENTITY_ROWS: &str = concat!(
    "[.[] | [.package,.name,.file,",
    ".span.start.line,.span.start.column,.span.end.line,.span.end.column,",
    ".replacement] | @tsv] | .[]"
);
/// jaq projection of mutant identities in the raw outcome document.
const OUTCOME_ROWS: &str = concat!(
    "[.outcomes[] | select((.scenario | type) == \"object\") ",
    "| .scenario.Mutant | [.package,.name,.file,",
    ".span.start.line,.span.start.column,.span.end.line,.span.end.column,",
    ".replacement] | @tsv] | .[]"
);

/// One complete shard's counters and raw outcomes, after identity validation.
pub(super) struct Evidence {
    /// The full filtered mutant count assigned to this shard.
    pub(super) expected: usize,
    /// The raw outcome document retained under the canonical reports bundle.
    pub(super) outcomes: PathBuf,
    /// Its verified outcome counters.
    pub(super) counts: Counts,
    /// Tool-reported start time.
    pub(super) started: String,
    /// Tool-reported completion time.
    pub(super) ended: String,
    /// Whether the baseline completed successfully.
    pub(super) baseline_ok: bool,
}

/// cargo-mutants' nonnegative integral counters.
#[derive(Clone, Copy, Default)]
pub(super) struct Counts {
    /// Every mutant scenario, excluding the baseline.
    pub(super) total: usize,
    /// Mutants caught by a test failure.
    pub(super) caught: usize,
    /// Mutants that passed the tests.
    pub(super) missed: usize,
    /// Timed-out mutants.
    pub(super) timeout: usize,
    /// Mutants that could not be built or tested.
    pub(super) unviable: usize,
    /// Other successful mutant test runs.
    pub(super) success: usize,
}

/// The validated checks artifact and the complete matrix it planned.
pub(super) struct Plan {
    /// The immutable identity and scope manifest.
    pub(super) manifest: PathBuf,
    /// The complete unsharded filtered listing.
    pub(super) identities: Vec<String>,
    /// The count of distinct planned mutants.
    pub(super) mutants: usize,
    /// The number of nonempty matrix workers.
    pub(super) shards: usize,
    /// The root report artifact name that scopes worker names.
    pub(super) artifact_name: String,
}

/// Validate one shard's receipt, assignment, outcomes, logs and counters.
pub(super) fn read_shard(
    source: &Path,
    copied: &Path,
    plan: &Plan,
    index: usize,
) -> Result<Evidence, Failure> {
    let receipt = safe_file(&source.join("rust-reports/mutants-shard.json"))?;
    let outcomes = safe_file(&source.join("mutants/mutants.out/outcomes.json"))?;
    let discovered = safe_file(&source.join("mutants/mutants.out/mutants.json"))?;
    let step_report = safe_file(&source.join("rust-reports/mutants.txt"))?;
    fs::read_to_string(&step_report)
        .map_err(|error| format!("cannot read shard report: {error}"))?;
    let expected = assigned_count(plan.mutants, index, plan.shards);
    let planned_rows = assigned_rows(&plan.identities, plan.shards, index);
    validate_receipt(&receipt, plan, index, expected)?;
    super::super::plan::validate_listing(&discovered)?;
    let discovered_rows = identity_rows(&discovered)?;
    same_rows(&planned_rows, &discovered_rows)
        .map_err(|_| Failure::from("shard discovery differs from round-robin assignment"))?;
    validate_outcomes(&outcomes)?;
    validate_result_paths(&source.join("mutants/mutants.out"), &outcomes)?;
    let result_rows = outcome_rows(&outcomes)?;
    same_rows(&discovered_rows, &result_rows)
        .map_err(|_| Failure::from("completed outcomes omit or duplicate a planned mutant"))?;
    let counts = read_counts(&outcomes)?;
    if counts.total != expected || counts.total != result_rows.len() {
        return Err("outcome counters do not cover every assigned mutant".into());
    }
    if !summary_counts_match(&outcomes, counts)? {
        return Err("outcome counters disagree with completed mutant scenarios".into());
    }
    let started = jaq(".start_time", &outcomes)?;
    let ended = jaq(".end_time", &outcomes)?;
    if ended == "null" || started.is_empty() || ended.is_empty() {
        return Err("outcomes document is incomplete".into());
    }
    let baseline_ok = baseline_succeeded(&outcomes)?;
    Ok(Evidence {
        expected,
        outcomes: copied.join("mutants/mutants.out/outcomes.json"),
        counts,
        started,
        ended,
        baseline_ok,
    })
}

/// Match the receipt to the full plan and to this exact worker index.
fn validate_receipt(receipt: &Path, plan: &Plan, index: usize, expected: usize) -> Outcome {
    let manifest = &plan.manifest;
    let plan_sha = jaq(".sha", manifest)?;
    let plan_parent = jaq(".first_parent // \"\"", manifest)?;
    let plan_directory = jaq(".directory", manifest)?;
    let config = jaq(".config_sha256", manifest)?;
    let diff = jaq(".diff_sha256", manifest)?;
    let mut args = Vec::new();
    for (name, value) in [
        ("sha", plan_sha.as_str()),
        ("parent", plan_parent.as_str()),
        ("directory", plan_directory.as_str()),
        ("config", config.as_str()),
        ("diff", diff.as_str()),
    ] {
        args.extend(["--arg".to_owned(), name.to_owned(), value.to_owned()]);
    }
    for (name, value) in [
        ("count", plan.mutants),
        ("shards", plan.shards),
        ("index", index),
        ("expected", expected),
    ] {
        args.extend(["--argjson".to_owned(), name.to_owned(), value.to_string()]);
    }
    for (name, value) in [
        ("run", jaq(".run_id", manifest)?),
        ("attempt", jaq(".attempt", manifest)?),
        ("toolchain", jaq(".toolchain", manifest)?),
        ("version", jaq(".cargo_mutants_version", manifest)?),
    ] {
        args.extend(["--arg".to_owned(), name.to_owned(), value]);
    }
    Cmd::new("jaq -e")
        .args(args)
        .arg(RECEIPT_IDENTITY)
        .arg(receipt)
        .capture()
        .map(|_| ())
        .map_err(|_| Failure::from("shard receipt identity or denominator differs"))
}

/// Validate the pinned cargo-mutants result shape, counters and tool version.
fn validate_outcomes(path: &Path) -> Outcome {
    const SHAPE: &str = concat!(
        "type == \"object\" and (.outcomes | type == \"array\") ",
        "and ([.total_mutants,.caught,.missed,.timeout,.unviable,.success] ",
        "| all(.[]; type == \"number\" and floor == . and . >= 0)) ",
        "and (.cargo_mutants_version | type == \"string\") ",
        "and (.start_time | type == \"string\") and (.end_time | type == \"string\") ",
        "and all(.outcomes[]; ",
        "(.scenario == \"Baseline\" or ((.scenario | type) == \"object\" ",
        "and (.scenario | has(\"Mutant\")))) and (.summary | type == \"string\") ",
        "and (.phase_results | type == \"array\" and length > 0 and all(.[]; ",
        "(.phase == \"Check\" or .phase == \"Build\" or .phase == \"Test\") ",
        "and (.duration | type == \"number\" and . >= 0) ",
        "and (.argv | type == \"array\" and length > 0 and all(.[]; type == \"string\")) ",
        "and (((.process_status | type) == \"string\" ",
        "and (.process_status == \"Success\" or .process_status == \"Timeout\" ",
        "or .process_status == \"Other\")) or ((.process_status | type) == \"object\" ",
        "and ((.process_status | keys) == [\"Failure\"] ",
        "or (.process_status | keys) == [\"Signalled\"]) ",
        "and ((.process_status.Failure // .process_status.Signalled) ",
        "| type == \"number\" and floor == .))))))"
    );
    const PHASE_SUMMARIES: &str = concat!(
        "all(.outcomes[]; if .scenario == \"Baseline\" then ",
        "(if any(.phase_results[]; .process_status == \"Timeout\") ",
        "then .summary == \"Timeout\" ",
        "elif all(.phase_results[]; .process_status == \"Success\") ",
        "then .summary == \"Success\" else .summary == \"Failure\" end) ",
        "else (if any(.phase_results[]; .phase != \"Test\" ",
        "and ((.process_status | type) == \"object\" ",
        "and (.process_status | has(\"Failure\")))) ",
        "then .summary == \"Unviable\" ",
        "elif any(.phase_results[]; .process_status == \"Timeout\") ",
        "then .summary == \"Timeout\" ",
        "elif .phase_results[-1].phase == \"Test\" ",
        "and ((.phase_results[-1].process_status | type) == \"object\" ",
        "and (.phase_results[-1].process_status | has(\"Failure\"))) ",
        "then .summary == \"CaughtMutant\" ",
        "elif .phase_results[-1].phase == \"Test\" ",
        "and .phase_results[-1].process_status == \"Success\" ",
        "then .summary == \"MissedMutant\" ",
        "elif .phase_results[-1].process_status == \"Success\" ",
        "then .summary == \"Success\" else .summary == \"Failure\" end) end)"
    );
    Cmd::new("jaq -e")
        .arg(SHAPE)
        .arg(path)
        .capture()
        .map_err(|_| Failure::from("outcomes JSON is malformed or has invalid counters"))?;
    Cmd::new("jaq -e")
        .arg(PHASE_SUMMARIES)
        .arg(path)
        .capture()
        .map_err(|_| Failure::from("outcome summary does not match its phase results"))?;
    let version = input("CARGO_MUTANTS_VERSION")?;
    Cmd::new("jaq -e")
        .args(["--arg", "version", version.as_str()])
        .arg(".cargo_mutants_version == $version")
        .arg(path)
        .capture()
        .map_err(|_| Failure::from("outcomes cargo-mutants version differs from the plan"))?;
    Ok(())
}

/// Return true only for exactly one baseline with successful build and test phases.
fn baseline_succeeded(path: &Path) -> Result<bool, Failure> {
    Cmd::new("jaq -e")
        .arg(BASELINE_SUCCESS)
        .arg(path)
        .capture()
        .map(|_| true)
        .or(Ok(false))
}

/// Read and cross-check every counter against the details it summarizes.
fn read_counts(path: &Path) -> Result<Counts, Failure> {
    let values = Cmd::new("jaq -er")
        .arg("[.total_mutants,.caught,.missed,.timeout,.unviable,.success] | @tsv")
        .arg(path)
        .capture()?;
    parse_counts(&values)
}

/// Parse exactly the six counters returned by the pinned JSON query.
fn parse_counts(values: &str) -> Result<Counts, Failure> {
    let mut values = values.trim().split('\t').map(str::parse::<usize>);
    let (
        Some(Ok(total)),
        Some(Ok(caught)),
        Some(Ok(missed)),
        Some(Ok(timeout)),
        Some(Ok(unviable)),
        Some(Ok(success)),
    ) = (
        values.next(),
        values.next(),
        values.next(),
        values.next(),
        values.next(),
        values.next(),
    )
    else {
        return Err("outcome counters are not nonnegative integers".into());
    };
    if values.next().is_some() {
        return Err("outcome counters have an unexpected shape".into());
    }
    Ok(Counts {
        total,
        caught,
        missed,
        timeout,
        unviable,
        success,
    })
}

/// Whether the five mutant counters equal the number of detailed outcomes.
fn summary_counts_match(path: &Path, counts: Counts) -> Result<bool, Failure> {
    let rows = Cmd::new("jaq -r")
        .arg("[.outcomes[] | select(.scenario != \"Baseline\") | .summary] | .[]")
        .arg(path)
        .capture()?;
    let mut actual = Counts {
        total: 0,
        ..Counts::default()
    };
    for row in rows.lines() {
        actual.total += 1;
        match row {
            "CaughtMutant" => actual.caught += 1,
            "MissedMutant" => actual.missed += 1,
            "Timeout" => actual.timeout += 1,
            "Unviable" => actual.unviable += 1,
            "Success" => actual.success += 1,
            _ => return Ok(false),
        }
    }
    Ok(actual.total == counts.total
        && actual.caught == counts.caught
        && actual.missed == counts.missed
        && actual.timeout == counts.timeout
        && actual.unviable == counts.unviable
        && actual.success == counts.success)
}

/// Stable identity and source details from one cargo-mutants listing.
pub(super) fn identity_rows(path: &Path) -> Result<Vec<String>, Failure> {
    let rows = Cmd::new("jaq -r").arg(IDENTITY_ROWS).arg(path).capture()?;
    Ok(rows.lines().map(str::to_owned).collect())
}

/// Mutant identities from completed tool outcomes, excluding the baseline.
fn outcome_rows(path: &Path) -> Result<Vec<String>, Failure> {
    let rows = Cmd::new("jaq -r").arg(OUTCOME_ROWS).arg(path).capture()?;
    Ok(rows.lines().map(str::to_owned).collect())
}

/// A mutant identity set differs or contains duplicates.
#[derive(Debug)]
struct RowMismatch;

/// Compare identities as sets and reject duplicate records in either document.
fn same_rows(planned: &[String], observed: &[String]) -> Result<(), RowMismatch> {
    let expected: BTreeSet<&str> = planned.iter().map(String::as_str).collect();
    let actual: BTreeSet<&str> = observed.iter().map(String::as_str).collect();
    if expected.len() != planned.len() || actual.len() != observed.len() || expected != actual {
        Err(RowMismatch)
    } else {
        Ok(())
    }
}

/// Full listing entries assigned to one shard by the pinned round-robin rule.
fn assigned_rows(rows: &[String], shards: usize, index: usize) -> Vec<String> {
    rows.iter()
        .enumerate()
        .filter(|(position, _)| position % shards == index)
        .map(|(_, row)| row.clone())
        .collect()
}

/// Number of list positions assigned to a shard without division overflow.
fn assigned_count(total: usize, index: usize, shards: usize) -> usize {
    if index >= total {
        0
    } else {
        (total - index).div_ceil(shards)
    }
}

#[cfg(test)]
mod tests {
    use super::{assigned_count, assigned_rows, parse_counts};

    #[test]
    fn round_robin_assignment_covers_every_identity_once() {
        let all: Vec<String> = (0..5).map(|index| index.to_string()).collect();
        assert_eq!(assigned_rows(&all, 2, 0), ["0", "2", "4"]);
        assert_eq!(assigned_rows(&all, 2, 1), ["1", "3"]);
        assert_eq!(assigned_count(5, 0, 2), 3);
        assert_eq!(assigned_count(5, 1, 2), 2);
    }

    #[test]
    fn outcome_counters_require_six_nonnegative_integers() {
        assert_eq!(parse_counts("5\t5\t0\t0\t0\t0").unwrap().total, 5);
        assert_eq!(
            parse_counts("5\tx\t0\t0\t0\t0")
                .err()
                .expect("invalid counter")
                .message
                .as_deref(),
            Some("outcome counters are not nonnegative integers")
        );
        assert_eq!(
            parse_counts("5\t5\t0\t0\t0\t0\t0")
                .err()
                .expect("extra counter")
                .message
                .as_deref(),
            Some("outcome counters have an unexpected shape")
        );
    }
}

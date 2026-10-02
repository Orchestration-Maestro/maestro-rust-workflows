//! `rust-gate required`: the one status a branch protection can require.
//! It fails on required upstream jobs that fail, cancel or unexpectedly skip.
//! Shard status is checked against the complete matrix before branch protection turns green.

use crate::runner::{Outcome, Step, input, optional, summary};

/// What this step declares: its inputs, its tools and its reports.
pub(crate) const STEPS: &[Step] = &[Step {
    workflow: "ci",
    id: "required",
    summary: "Require every check",
    inputs: &[
        "GITHUB_RUN_ATTEMPT",
        "INTERNAL_SHARD_SELFTEST",
        "MUTATION_ATTEMPT",
        "MUTATION_COUNT",
        "MUTATION_ENGINE_COUNT",
        "MUTATION_ENGINE_DEFAULT_COUNT",
        "ENGINE_DEFAULT_MUTATIONS_RESULT",
        "MUTATION_ENGINE_FILES",
        "ENGINE_MUTATIONS_RESULT",
        "MUTATION_MATRIX",
        "MUTATION_MODE",
        "MUTATION_SHARDS",
        "MUTATION_SUMMARY_RESULT",
        "MUTATION_TEST",
        "MUTATION_WINDOWS",
        "MUTATIONS_RESULT",
        "WINDOWS_MUTATIONS_RESULT",
        "PORTABILITY",
        "RESULT",
        "RUNNERS",
    ],
    tools: &[],
    reports: &[],
    run,
}];

/// Require core, portability and every configured mutation partition.
fn run() -> Outcome {
    let result = input("RESULT")?;
    summary(&format!("Rust checks: {result}\n"))?;
    if result != "success" {
        return Err("Required Rust checks failed or were skipped".into());
    }
    if !optional("RUNNERS")?.is_empty() {
        let portability = optional("PORTABILITY")?;
        summary(&format!("Portability: {portability}\n"))?;
        if portability != "success" {
            return Err("Portability checks failed or were skipped".into());
        }
    }
    require_default_mutations()?;
    require_engine_mutations()?;
    require_windows_mutations()
}

/// Verify the featureless plan, its ordinary workers and whether aggregation ran.
fn require_default_mutations() -> Outcome {
    let enabled = input("MUTATION_TEST")? == "true";
    let mode = input("MUTATION_MODE")?;
    let count = input("MUTATION_COUNT")?;
    let shards = input("MUTATION_SHARDS")?;
    let matrix = input("MUTATION_MATRIX")?;
    let workers = input("MUTATIONS_RESULT")?;
    let aggregate = input("MUTATION_SUMMARY_RESULT")?;
    let engine_count = input("MUTATION_ENGINE_COUNT")?
        .parse::<usize>()
        .unwrap_or(usize::MAX);
    summary(&format!(
        "Mutation plan: {mode} (mutants={count}, shards={shards}, matrix={matrix})\n"
    ))?;
    if optional("INTERNAL_SHARD_SELFTEST")? == "true" && (mode != "sharded" || shards != "2") {
        return Err("internal shard self-test did not run exactly two shards".into());
    }
    validate_default_plan(&DefaultPlan {
        enabled,
        mode: &mode,
        count: &count,
        shards: &shards,
        matrix: &matrix,
        workers: &workers,
        aggregate: &aggregate,
    })?;
    let aggregate_expected =
        mode == "sharded" || engine_count > 0 || input("MUTATION_ENGINE_DEFAULT_COUNT")? != "0";
    if mode != "sharded" && workers != "skipped" {
        return Err("Mutation jobs were not intentionally skipped".into());
    }
    if !aggregate_expected && aggregate != "skipped" {
        return Err("Mutation jobs were not intentionally skipped".into());
    }
    if aggregate_expected && aggregate != "success" {
        return Err("Mutation partition aggregation failed or was skipped".into());
    }
    Ok(())
}

/// One default mutation plan and the outcomes that must satisfy it.
struct DefaultPlan<'a> {
    /// Whether mutation testing is enabled.
    enabled: bool,
    /// The plan's routing mode.
    mode: &'a str,
    /// The discovered mutant count.
    count: &'a str,
    /// Number of planned shards.
    shards: &'a str,
    /// Complete matrix index list.
    matrix: &'a str,
    /// Default mutation worker result.
    workers: &'a str,
    /// Aggregate job result.
    aggregate: &'a str,
}

/// Validate default mode, matrix, plan attempt and worker result.
fn validate_default_plan(plan: &DefaultPlan<'_>) -> Outcome {
    match (plan.enabled, plan.mode) {
        (false, "disabled") | (true, "empty")
            if plan.count == "0" && plan.shards == "0" && plan.matrix == "[]" =>
        {
            Ok(())
        }
        (true, "inline")
            if plan.shards == "1"
                && plan.matrix == "[]"
                && (plan.count.is_empty()
                    || plan
                        .count
                        .parse::<usize>()
                        .is_ok_and(|number| number > 0 && number.to_string() == plan.count)) =>
        {
            Ok(())
        }
        (true, "sharded") => validate_shard_plan(
            plan.count,
            plan.shards,
            plan.matrix,
            plan.workers,
            plan.aggregate,
        ),
        _ => Err("Mutation shard plan is missing or invalid".into()),
    }
}

/// Refuse incomplete matrix evidence or a plan from another attempt.
fn validate_shard_plan(
    count: &str,
    shards: &str,
    matrix: &str,
    workers: &str,
    aggregate: &str,
) -> Outcome {
    let shard_count = shards.parse::<usize>().unwrap_or(0);
    let mutant_count = count.parse::<usize>().unwrap_or(0);
    let expected = format!(
        "[{}]",
        (0..shard_count)
            .map(|shard| shard.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );
    if mutant_count == 0
        || mutant_count.to_string() != count
        || !(2..=256).contains(&shard_count)
        || shard_count > mutant_count
        || shard_count.to_string() != shards
        || matrix != expected
    {
        return Err("Mutation shard plan is missing or invalid".into());
    }
    if input("MUTATION_ATTEMPT")? != input("GITHUB_RUN_ATTEMPT")? {
        return Err("Mutation plan belongs to a different run attempt; Re-run all jobs".into());
    }
    if workers != "success" || aggregate != "success" {
        return Err("Mutation shards failed or were incomplete".into());
    }
    summary(&format!(
        "Mutation shards: {workers}; aggregation: {aggregate}\n"
    ))
}

/// Require exactly the engine mutation job planned by the feature partition.
fn require_engine_mutations() -> Outcome {
    let enabled = input("MUTATION_TEST")? == "true";
    require_engine_mode(enabled, "MUTATION_ENGINE_COUNT", "ENGINE_MUTATIONS_RESULT")?;
    require_engine_mode(
        enabled,
        "MUTATION_ENGINE_DEFAULT_COUNT",
        "ENGINE_DEFAULT_MUTATIONS_RESULT",
    )
}

/// Require each independent engine mode's worker result, including empty-mode intentional skips.
fn require_engine_mode(enabled: bool, count_input: &str, result_input: &str) -> Outcome {
    let count = input(count_input)?
        .parse::<usize>()
        .map_err(|_| "Engine mutation count is invalid")?;
    let files = input("MUTATION_ENGINE_FILES")?;
    let result = input(result_input)?;
    if count > 0 && (!enabled || files == "[]" || result != "success") {
        return Err("Engine-owned mutation job failed or was skipped".into());
    }
    if count == 0 && result != "skipped" {
        return Err("Engine-owned mutation job ran without planned mutants".into());
    }
    summary(&format!(
        "Engine-owned mutations: {result} (planned={count})\n"
    ))
}

/// Require exactly the Windows mutation job selected by policy.
fn require_windows_mutations() -> Outcome {
    let enabled = input("MUTATION_TEST")? == "true";
    let files = optional("MUTATION_WINDOWS")?;
    let result = optional("WINDOWS_MUTATIONS_RESULT")?;
    let expected = enabled && files != "[]";
    if (expected && result != "success") || (!expected && result != "skipped") {
        return Err(if expected {
            "Windows-owned mutation job failed or was skipped".into()
        } else {
            "Windows-owned mutation job ran without configured files".into()
        });
    }
    summary(&format!("Windows-owned mutations: {result}\n"))?;
    Ok(())
}

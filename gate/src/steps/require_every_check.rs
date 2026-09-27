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
        "MUTATION_MATRIX",
        "MUTATION_MODE",
        "MUTATION_SHARDS",
        "MUTATION_SUMMARY_RESULT",
        "MUTATION_TEST",
        "MUTATIONS_RESULT",
        "PORTABILITY",
        "RESULT",
        "RUNNERS",
    ],
    tools: &[],
    reports: &[],
    run,
}];

/// Run the step.
fn run() -> Outcome {
    let result = input("RESULT")?;
    summary(&format!("Rust checks: {result}\n"))?;
    if result != "success" {
        return Err("Required Rust checks failed or were skipped".into());
    }
    // No platform named means the portability job was skipped on purpose.
    if !optional("RUNNERS")?.is_empty() {
        let portability = optional("PORTABILITY")?;
        summary(&format!("Portability: {portability}\n"))?;
        if portability != "success" {
            return Err("Portability checks failed or were skipped".into());
        }
    }

    let enabled = input("MUTATION_TEST")? == "true";
    let mode = input("MUTATION_MODE")?;
    let count = input("MUTATION_COUNT")?;
    let shards = input("MUTATION_SHARDS")?;
    let matrix = input("MUTATION_MATRIX")?;
    let mutations = input("MUTATIONS_RESULT")?;
    let aggregate = input("MUTATION_SUMMARY_RESULT")?;
    summary(&format!(
        "Mutation plan: {mode} (mutants={count}, shards={shards}, matrix={matrix})\n"
    ))?;
    if optional("INTERNAL_SHARD_SELFTEST")? == "true" && (mode != "sharded" || shards != "2") {
        return Err("internal shard self-test did not run exactly two shards".into());
    }

    match (enabled, mode.as_str()) {
        (false, "disabled") | (true, "empty")
            if count == "0" && shards == "0" && matrix == "[]" => {}
        (true, "inline")
            if shards == "1"
                && matrix == "[]"
                && (count.is_empty()
                    || count
                        .parse::<usize>()
                        .is_ok_and(|number| number > 0 && number.to_string() == count)) => {}
        (true, "sharded") => {
            let shard_count = shards.parse::<usize>().unwrap_or(0);
            let mutant_count = count.parse::<usize>().unwrap_or(0);
            let expected = (0..shard_count)
                .map(|shard| shard.to_string())
                .collect::<Vec<_>>();
            let expected = format!("[{}]", expected.join(","));
            if mutant_count == 0
                || mutant_count.to_string() != count
                || !(2..=32).contains(&shard_count)
                || shard_count > mutant_count
                || shard_count.to_string() != shards
                || matrix != expected
            {
                return Err("Mutation shard plan is missing or invalid".into());
            }
            if input("MUTATION_ATTEMPT")? != input("GITHUB_RUN_ATTEMPT")? {
                return Err(
                    "Mutation plan belongs to a different run attempt; Re-run all jobs".into(),
                );
            }
            if mutations != "success" || aggregate != "success" {
                return Err("Mutation shards failed or were incomplete".into());
            }
            summary(&format!(
                "Mutation shards: {mutations}; aggregation: {aggregate}\n"
            ))?;
        }
        _ => return Err("Mutation shard plan is missing or invalid".into()),
    }

    if mode != "sharded" && (mutations != "skipped" || aggregate != "skipped") {
        return Err("Mutation jobs were not intentionally skipped".into());
    }
    if mode != "sharded" {
        summary(&format!(
            "Mutation workers: {mutations}; aggregation: {aggregate} (intentionally skipped)\n"
        ))?;
    }
    Ok(())
}

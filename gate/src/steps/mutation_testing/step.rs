//! `rust-gate mutants`: cargo-mutants over the checked source scope, failing on
//! every survivor, timeout, baseline failure or incomplete shard execution.

use super::{aggregate, plan, reports, scope, selftest};
use crate::runner::{Cmd, Failure, Job, Outcome, Step, flag, optional, output, tee_line};
use std::fs;
use std::path::PathBuf;

/// What the mutation family declares: planning, execution and aggregation.
pub(crate) const STEPS: &[Step] = &[
    Step {
        workflow: "ci",
        id: "mutants-plan",
        summary: "Plan mutation shard routing",
        inputs: &[
            "CARGO_MUTANTS_VERSION",
            "GITHUB_BASE_REF",
            "GITHUB_RUN_ATTEMPT",
            "GITHUB_RUN_ID",
            "GITHUB_SHA",
            "GITHUB_WORKSPACE",
            "INTERNAL_SHARD_SELFTEST",
            "MUTATION_MUTANTS_PER_SHARD",
            "MUTATION_SHARDS",
            "MUTATION_TEST",
            "MUTATION_WINDOWS",
            "RUSTUP_TOOLCHAIN",
        ],
        tools: &["cargo mutants", "git", "jaq"],
        reports: &[
            "mutants-plan.txt",
            "mutants-plan.log",
            "mutants-list.json",
            "mutation-plan.json",
            "mutants.diff",
        ],
        run: plan::run,
    },
    Step {
        workflow: "ci",
        id: "mutants",
        summary: "Mutation testing",
        inputs: &[
            "CARGO_MUTANTS_VERSION",
            "GITHUB_BASE_REF",
            "GITHUB_RUN_ATTEMPT",
            "GITHUB_RUN_ID",
            "GITHUB_SHA",
            "GITHUB_WORKSPACE",
            "INTERNAL_SHARD_SELFTEST",
            "MUTATION_LIST",
            "MUTATION_PLAN",
            "MUTATION_SHARD",
            "MUTATION_SHARDS",
            "MUTATION_TEST",
            "MUTATION_WINDOWS",
            "RUSTUP_TOOLCHAIN",
        ],
        tools: &["cargo mutants", "git", "jaq", "timeout"],
        reports: &[
            "mutants.json",
            "mutants.txt",
            "mutants-shard.json",
            "mutants.diff",
        ],
        run,
    },
    Step {
        workflow: "ci",
        id: "mutants-windows",
        summary: "Windows mutation testing",
        inputs: &[
            "CARGO_MUTANTS_VERSION",
            "GITHUB_BASE_REF",
            "GITHUB_WORKSPACE",
            "MUTATION_TEST",
            "MUTATION_WINDOWS",
            "PROJECT",
            "RUSTUP_TOOLCHAIN",
        ],
        tools: &["cargo mutants", "git", "jaq"],
        reports: &["mutants.json", "mutants.txt", "mutants.diff"],
        run: super::windows::run,
    },
    Step {
        workflow: "ci",
        id: "mutants-aggregate",
        summary: "Aggregate complete mutation shard evidence",
        inputs: &[
            "CARGO_MUTANTS_VERSION",
            "CHECKS_RESULT",
            "GITHUB_RUN_ATTEMPT",
            "GITHUB_RUN_ID",
            "GITHUB_SHA",
            "MUTATION_ARTIFACT_NAME",
            "MUTATION_ARTIFACTS",
            "MUTATION_MATRIX",
            "MUTATION_MODE",
            "MUTATION_PLAN_DIR",
            "MUTATION_SHARDS",
            "MUTATIONS_RESULT",
            "RUSTUP_TOOLCHAIN",
        ],
        tools: &["jaq"],
        reports: &["mutation-shards", "mutants.json", "mutants.txt"],
        run: aggregate::run,
    },
];

/// Execute the current unsharded path, or one checked worker shard.
fn run() -> Outcome {
    let job = Job::current()?;
    let report = job.report("mutants.txt")?;
    if !flag("MUTATION_TEST")? {
        tee_line("SKIPPED: mutation-test=false", &report, false)?;
        return output("applied", "false");
    }
    selftest::prepare(&job)?;
    let shard_value = optional("MUTATION_SHARD")?;
    let shard = if shard_value.is_empty() {
        None
    } else {
        Some(plan::parse_shard(&shard_value)?)
    };
    let base = optional("GITHUB_BASE_REF")?;
    let (scope, expected_mutants) = if let Some((index, count)) = shard {
        let source_scope = scope::prepare(&job, &base)?;
        let manifest = required_plan_path("MUTATION_PLAN")?;
        let listing = required_plan_path("MUTATION_LIST")?;
        let expected =
            plan::verify_worker(&job, &source_scope, &manifest, &listing, (index, count))?;
        let receipt = job.report("mutants-shard.json")?;
        plan::write_receipt(&receipt, &manifest, index, count, expected)?;
        tee_line(
            &format!(
                "shard {index}/{count}: planned {expected} of {} mutants",
                plan::manifest_count(&manifest)?
            ),
            &report,
            false,
        )?;
        output("shard", &format!("{index}/{count}"))?;
        output("planned-mutants", &expected.to_string())?;
        (source_scope, Some(expected))
    } else {
        (plan::planned_scope(&job, &base)?, None)
    };
    if !base.is_empty() && scope.parent.is_some() {
        tee_line(&format!("scope: changes against {base}"), &report, true)?;
    } else if scope.parent.is_some() {
        tee_line("scope: changes in the last commit", &report, true)?;
    } else {
        tee_line("scope: full workspace, a first commit", &report, true)?;
    }

    let output_dir = job.temp.join("mutants");
    let command_line = if shard.is_some() {
        concat!(
            "timeout --kill-after=1m 30m ",
            "cargo mutants --no-shuffle --cargo-arg=--locked --colors=never --level=info"
        )
    } else {
        "cargo mutants --no-shuffle --cargo-arg=--locked --colors=never --level=info"
    };
    let mut command = Cmd::new(command_line);
    if let Some(diff) = &scope.diff {
        let diff = diff.to_string_lossy().into_owned();
        command = command.args(["--in-diff", &diff]);
    }
    command = scope::exclude_windows_files(command, &job.project)?;
    if let Some((index, count)) = shard {
        command = command
            .arg("--shard")
            .arg(format!("{index}/{count}"))
            .args(["--sharding", "round-robin"]);
    }
    let verdict = command
        .arg("--output")
        .arg(&output_dir)
        .cwd(&job.project)
        .tee(&report, true);
    let outcomes = output_dir.join("mutants.out/outcomes.json");
    let saved = if outcomes.is_file() {
        fs::copy(&outcomes, job.report("mutants.json")?)
            .map(|_| ())
            .map_err(|error| format!("cannot copy the outcomes: {error}"))
    } else {
        Ok(())
    };
    verdict?;
    saved?;
    reports::report_outcomes(&job, &outcomes, scope.change.as_deref(), expected_mutants)
}

/// Resolve a worker's plan input without accepting a missing path.
fn required_plan_path(name: &str) -> Result<PathBuf, Failure> {
    required_path(name, &optional(name)?)
}

/// Refuse a missing shard input with its exact exported name.
fn required_path(name: &str, value: &str) -> Result<PathBuf, Failure> {
    if value.is_empty() {
        return Err(format!("{name} is required for a mutation shard").into());
    }
    Ok(value.into())
}

#[cfg(test)]
mod tests {
    use super::required_path;
    use std::path::PathBuf;

    #[test]
    fn shard_plan_inputs_are_required_by_name() {
        assert_eq!(
            required_path("MUTATION_PLAN", "")
                .unwrap_err()
                .message
                .as_deref(),
            Some("MUTATION_PLAN is required for a mutation shard")
        );
        assert_eq!(
            required_path("MUTATION_LIST", "listing.json").unwrap(),
            PathBuf::from("listing.json")
        );
    }
}

//! Run the aggregate job and validate the full plan before accepting worker evidence.

use super::artifacts::{
    copy_artifact, copy_tree, has_outcome_files, preserve_foreign_artifacts, preserve_unverified,
    safe_component, safe_directory, safe_file, validate_tree,
};
use super::evidence::{Evidence, Plan, identity_rows, read_shard, untested_mutants};
use super::merge::finish_aggregate;
use super::outcomes::{jaq, merge_partitions};
use super::{engine_evidence, evidence, windows_evidence};
use crate::runner::{Cmd, Failure, Job, Outcome, input, output, summary, tee_line, write};
use std::fs;
use std::path::Path;

/// Require the current workflow run to match the plan's immutable identity.
const PLAN_IDENTITY: &str = concat!(
    ".sha == $sha and .run_id == $run and .attempt == $attempt ",
    "and .toolchain == $toolchain and .cargo_mutants_version == $version ",
    "and .mutant_count == $count and .shard_count == $shards"
);

/// Aggregate the evidence even on failure, so diagnostics remain available.
pub(in super::super) fn run() -> Outcome {
    let job = Job::current()?;
    fs::create_dir_all(&job.reports).map_err(|error| {
        format!(
            "cannot create report directory {}: {error}",
            job.reports.display()
        )
    })?;
    let report = job.report("mutants.txt")?;
    tee_line("Mutation shard aggregation", &report, false)?;
    match aggregate(&job, &report) {
        Ok(passed) => {
            output("mutation-state", if passed { "passed" } else { "failed" })?;
            if passed {
                Ok(())
            } else {
                Err("mutation shard evidence is complete, but a required result failed".into())
            }
        }
        Err(error) => {
            preserve_unverified(&job, &report)?;
            let present = has_outcome_files(&job);
            tee_line(
                &format!(
                    "Aggregation incomplete: {}",
                    error.message.as_deref().unwrap_or("invalid shard evidence")
                ),
                &report,
                true,
            )?;
            tee_line(
                if present {
                    "Aggregate total unavailable; partial shard diagnostics are retained."
                } else {
                    "No mutation execution evidence is present; mutation state is not-run."
                },
                &report,
                true,
            )?;
            summary(&fs::read_to_string(&report).unwrap_or_default())?;
            output("mutation-state", if present { "failed" } else { "not-run" })?;
            Err(error)
        }
    }
}

/// Check the run plan, preserve every safe artifact, then aggregate only complete evidence.
fn aggregate(job: &Job, report: &Path) -> Result<bool, Failure> {
    let engine =
        input("MUTATION_ENGINE_COUNT")? != "0" || input("MUTATION_ENGINE_DEFAULT_COUNT")? != "0";
    if engine {
        return aggregate_partitions(job, report);
    }
    let root = safe_directory(job, Path::new(&input("MUTATION_ARTIFACTS")?))?;
    let plan = read_plan(job, report)?;
    let evidence = collect_evidence(job, report, &root, &plan)?;
    finish_aggregate(job, report, &plan, &evidence)
}

/// Join inline or sharded default evidence with both mode-aware engine obligations.
fn aggregate_partitions(job: &Job, report: &Path) -> Result<bool, Failure> {
    if input("CHECKS_RESULT")? != "success" {
        return Err("default mutation checks failed or were skipped".into());
    }
    let mode = input("MUTATION_MODE")?;
    let passed = if mode == "sharded" {
        let root = safe_directory(job, Path::new(&input("MUTATION_ARTIFACTS")?))?;
        let plan = read_plan(job, report)?;
        let evidence = collect_evidence(job, report, &root, &plan)?;
        finish_aggregate(job, report, &plan, &evidence)?
    } else {
        if !matches!(mode.as_str(), "inline" | "empty") {
            return Err("engine aggregation requires a valid default plan mode".into());
        }
        let directory = safe_directory(job, Path::new(&input("MUTATION_PLAN_DIR")?))?;
        validate_tree(&directory)?;
        copy_tree(&directory, &job.reports)?;
        true
    };
    let manifest = safe_file(&job.reports.join("mutation-plan.json"))?;
    let listing = safe_file(&job.reports.join("mutants-list.json"))?;
    let count = super::super::plan_identity::listing_count(&listing)?;
    if super::super::plan_identity::manifest_count(&manifest)? != count {
        return Err("default partition plan count differs from its complete listing".into());
    }
    let mut paths = Vec::new();
    if count > 0 {
        let outcomes = safe_file(&job.reports.join(if mode == "sharded" {
            "mutants.json"
        } else {
            "mutants/mutants.out/outcomes.json"
        }))?;
        // Sharded totals contain multiple baselines already checked per worker.
        if mode != "sharded" {
            evidence::partition_counts(&listing, &outcomes)?;
        }
        paths.push(engine_evidence::tagged(
            job, &outcomes, "default", "default",
        )?);
    }
    paths.extend(engine_evidence::collect(job, report, &manifest)?);
    paths.extend(windows_evidence::collect(job, report, &manifest)?);
    write(
        &job.report("mutants.json")?,
        merge_partitions(&paths)?.as_bytes(),
        false,
    )?;
    tee_line(
        "All default and engine mode-aware obligations are complete",
        report,
        true,
    )?;
    Ok(passed)
}

/// Read the checks bundle and confirm its exact matrix before examining workers.
fn read_plan(job: &Job, report: &Path) -> Result<Plan, Failure> {
    let plan_dir = safe_directory(job, Path::new(&input("MUTATION_PLAN_DIR")?))?;
    validate_tree(&plan_dir)?;
    copy_tree(&plan_dir, &job.reports)?;
    let manifest = safe_file(&plan_dir.join("mutation-plan.json"))?;
    let listing = safe_file(&plan_dir.join("mutants-list.json"))?;
    super::super::plan_identity::validate_listing(&listing)?;
    let identities = identity_rows(&listing)?;
    let mutants = super::super::plan_identity::manifest_count(&manifest)?;
    if mutants == 0 || identities.len() != mutants {
        return Err("mutation plan count differs from its filtered listing".into());
    }
    let shards = input("MUTATION_SHARDS")?
        .parse::<usize>()
        .map_err(|_| "MUTATION_SHARDS is not an integer")?;
    if !(2..=256).contains(&shards) {
        return Err("sharded aggregation requires 2 through 256 planned shards".into());
    }
    validate_plan(&manifest, mutants, shards)?;
    let matrix = format!(
        "[{}]",
        (0..shards)
            .map(|index| index.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );
    if input("MUTATION_MODE")? != "sharded" || input("MUTATION_MATRIX")? != matrix {
        return Err("mutation mode or matrix differs from the complete plan".into());
    }
    let artifact_name = input("MUTATION_ARTIFACT_NAME")?;
    if !safe_component(&artifact_name) {
        return Err("mutation artifact root is not a safe name".into());
    }
    tee_line(
        &format!(
            "scope: {}; first parent: {}",
            jaq(".directory", &manifest)?,
            jaq(".first_parent // \"no parent\"", &manifest)?
        ),
        report,
        true,
    )?;
    tee_line(
        &format!("planned: {mutants} mutants across {shards} shards"),
        report,
        true,
    )?;
    Ok(Plan {
        manifest,
        identities,
        mutants,
        shards,
        artifact_name,
    })
}

/// Preserve the raw artifact and validate every expected shard independently.
fn collect_evidence(
    job: &Job,
    report: &Path,
    artifacts: &Path,
    plan: &Plan,
) -> Result<Vec<Evidence>, Failure> {
    let directory = job.report("mutation-shards")?;
    fs::create_dir_all(&directory)
        .map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
    let mut evidence = Vec::new();
    let mut missing = Vec::new();
    let mut untested = 0usize;
    let mut incomplete_shards = 0usize;
    for index in 0..plan.shards {
        let name = format!("{}-mutants-{index}-of-{}", plan.artifact_name, plan.shards);
        let source = artifacts.join(&name);
        let destination = directory.join(index.to_string());
        if !source.exists() {
            untested += untested_mutants(&source, plan, index);
            incomplete_shards += 1;
            tee_line(
                &format!("shard {index}/{}: missing artifact {name}", plan.shards),
                report,
                true,
            )?;
            missing.push(index);
            continue;
        }
        if let Err(error) = copy_artifact(&source, &destination) {
            untested += untested_mutants(&source, plan, index);
            incomplete_shards += 1;
            tee_line(
                &format!(
                    "shard {index}/{}: invalid artifact: {}",
                    plan.shards,
                    error.message.as_deref().unwrap_or("unsafe evidence")
                ),
                report,
                true,
            )?;
            missing.push(index);
            continue;
        }
        match read_shard(&source, &destination, plan, index) {
            Ok(item) => {
                tee_line(
                    &format!(
                        concat!(
                            "shard {}/{}: {} mutants; started {}; ended {}; ",
                            "caught={} missed={} timeout={} unviable={}"
                        ),
                        index,
                        plan.shards,
                        item.expected,
                        item.started,
                        item.ended,
                        item.counts.caught,
                        item.counts.missed,
                        item.counts.timeout,
                        item.counts.unviable
                    ),
                    report,
                    true,
                )?;
                evidence.push(item);
            }
            Err(error) => {
                untested += untested_mutants(&source, plan, index);
                incomplete_shards += 1;
                tee_line(
                    &format!(
                        "shard {index}/{}: incomplete: {}",
                        plan.shards,
                        error.message.as_deref().unwrap_or("invalid evidence")
                    ),
                    report,
                    true,
                )?;
                missing.push(index);
            }
        }
    }
    preserve_foreign_artifacts(
        artifacts,
        &directory,
        &plan.artifact_name,
        plan.shards,
        report,
    )?;
    if !missing.is_empty() {
        return fail_incomplete(report, &missing, untested, incomplete_shards);
    }
    Ok(evidence)
}

/// Report the incomplete shard count in both the summary and failed exit reason.
fn fail_incomplete(
    report: &Path,
    missing: &[usize],
    untested: usize,
    incomplete_shards: usize,
) -> Result<Vec<Evidence>, Failure> {
    tee_line(
        &format!("{untested} mutants untested in {incomplete_shards} shards"),
        report,
        true,
    )?;
    tee_line(
        &format!(
            "Incomplete shard indices: {}",
            missing
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(",")
        ),
        report,
        true,
    )?;
    Err(format!(
        concat!(
            "{} mutants untested in {} shards; ",
            "one or more expected mutation shards are missing or incomplete"
        ),
        untested, incomplete_shards
    )
    .into())
}

/// The plan's immutable run identity must match the current workflow attempt.
fn validate_plan(manifest: &Path, mutants: usize, shards: usize) -> Outcome {
    let tested = input("GITHUB_SHA")?;
    let run = input("GITHUB_RUN_ID")?;
    let attempt = input("GITHUB_RUN_ATTEMPT")?;
    let toolchain = input("RUSTUP_TOOLCHAIN")?;
    let version = input("CARGO_MUTANTS_VERSION")?;
    let manifest_attempt = jaq(".attempt", manifest)?;
    if manifest_attempt != attempt {
        return Err("sharded plan attempt differs from this run; use Re-run all jobs".into());
    }
    let mut args = Vec::new();
    for (name, value) in [
        ("sha", tested.as_str()),
        ("run", run.as_str()),
        ("attempt", attempt.as_str()),
        ("toolchain", toolchain.as_str()),
        ("version", version.as_str()),
    ] {
        args.extend(["--arg".to_owned(), name.to_owned(), value.to_owned()]);
    }
    for (name, value) in [("count", mutants), ("shards", shards)] {
        args.extend(["--argjson".to_owned(), name.to_owned(), value.to_string()]);
    }
    Cmd::new("jaq -e")
        .args(args)
        .arg(PLAN_IDENTITY)
        .arg(manifest)
        .capture()
        .map(|_| ())
        .map_err(|_| Failure::from("mutation plan identity differs from this workflow run"))
}

#[cfg(test)]
mod tests {
    use super::PLAN_IDENTITY;

    #[test]
    fn plan_identity_binds_the_run_attempt_toolchain_and_matrix() {
        for field in [
            ".sha == $sha",
            ".run_id == $run",
            ".attempt == $attempt",
            ".toolchain == $toolchain",
            ".cargo_mutants_version == $version",
            ".mutant_count == $count",
            ".shard_count == $shards",
        ] {
            assert!(PLAN_IDENTITY.contains(field), "{field}");
        }
    }
}

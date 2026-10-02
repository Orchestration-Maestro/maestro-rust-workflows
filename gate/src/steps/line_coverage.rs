//! `rust-gate coverage`: default coverage, or a same-source default/feature profile union.

use crate::checks::coverage_features::coverage_features;
use crate::checks::inputs::coverage_threshold;
use crate::checks::native_cache::{
    NativeCache, native_cache, native_cache_command, native_command,
};
use crate::runner::{Cmd, Failure, Job, Outcome, Step, input, non_empty, optional, summary, write};
use std::path::Path;
use std::time::Instant;

/// What this step declares: its inputs, its tools and its reports.
pub(crate) const STEPS: &[Step] = &[Step {
    workflow: "ci",
    id: "coverage",
    summary: "Line coverage gate",
    inputs: &[
        "COVERAGE",
        "COVERAGE_FEATURES",
        "GITHUB_SHA",
        "NATIVE_CACHE_ROOT",
    ],
    tools: &["cargo llvm-cov", "cargo metadata", "git", "jaq"],
    reports: &[
        "coverage.lcov",
        "coverage-binding.txt",
        "native-cache-before.txt",
    ],
    run,
}];

/// Keep the legacy command untouched when selection is absent.
fn run() -> Outcome {
    let job = Job::current()?;
    let lcov = job.report("coverage.lcov")?;
    let policy = native_cache(&job.project)?;
    let features = coverage_features(&optional("COVERAGE_FEATURES")?, || {
        native_cache_command(
            policy.as_ref(),
            "cargo metadata --format-version 1 --no-deps --locked",
        )
        .cwd(&job.project)
        .capture()
    })?;
    if features.is_empty() {
        native_cache_command(
            policy.as_ref(),
            "cargo llvm-cov --workspace --locked --lcov --output-path",
        )
        .arg(&lcov)
        .arg("--fail-under-lines")
        .arg(coverage_threshold()?)
        .cwd(&job.project)
        .run()?;
    } else {
        merged_coverage(&job, &lcov, &features, policy.as_ref())?;
    }
    non_empty(&lcov)
}

/// Start with clean profiles, retain the first run, then evaluate one locally generated report.
fn merged_coverage(
    job: &Job,
    lcov: &Path,
    features: &[String],
    policy: Option<&NativeCache>,
) -> Outcome {
    let sha = input("GITHUB_SHA")?;
    bound_checkout(job, &sha)?;
    write(lcov, b"", false)?;
    let features = features.join(",");
    let default = "cargo llvm-cov --workspace --locked --no-report";
    let feature = format!("{default} --features {features}");
    let report = format!(
        "cargo llvm-cov report --lcov --output-path {} --fail-under-lines {}",
        lcov.display(),
        coverage_threshold()?
    );
    // --no-report retains binaries and profiles. Explicit cleaning prevents
    // cached or previously downloaded coverage from entering the verdict.
    native_cache_command(policy, "cargo llvm-cov clean --workspace")
        .cwd(&job.project)
        .run()?;
    let started = Instant::now();
    native_cache_command(policy, default)
        .cwd(&job.project)
        .run()?;
    let default_seconds = started.elapsed().as_secs_f64();
    let started = Instant::now();
    let command = native_cache_command(
        policy,
        "cargo llvm-cov --workspace --locked --no-report --features",
    )
    .arg(&features)
    .cwd(&job.project);
    native_command(job, command, policy)?.run()?;
    let feature_seconds = started.elapsed().as_secs_f64();
    bound_checkout(job, &sha)?;
    native_cache_command(policy, "cargo llvm-cov report --lcov --output-path")
        .arg(lcov)
        .arg("--fail-under-lines")
        .arg(coverage_threshold()?)
        .cwd(&job.project)
        .run()?;
    non_empty(lcov)?;
    bound_checkout(job, &sha)?;
    let binding = format!(
        "Revision: {sha}\nFeatures: {features}\nDefault: {default}\nFeature: {feature}\n\
         Report: {report}\nDefault wall seconds: {default_seconds:.3}\n\
         Feature wall seconds: {feature_seconds:.3}\n"
    );
    write(
        &job.report("coverage-binding.txt")?,
        binding.as_bytes(),
        false,
    )?;
    summary(&format!("## Merged coverage\n\n{binding}\n"))
}

/// Both executions and reporting must use the job's committed source, never a foreign artifact.
fn bound_checkout(job: &Job, sha: &str) -> Outcome {
    let actual = Cmd::new("git rev-parse HEAD").cwd(&job.project).capture()?;
    let changes = Cmd::new("git status --porcelain --untracked-files=all")
        .cwd(&job.project)
        .capture()?;
    if actual.trim() != sha || !changes.trim().is_empty() {
        return Err(Failure::from(
            "coverage: checkout does not match the job source SHA",
        ));
    }
    Ok(())
}

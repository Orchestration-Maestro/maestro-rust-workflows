//! `rust-gate coverage`: default coverage, or a same-source default/feature profile union.

use crate::checks::coverage_features::coverage_features;
use crate::checks::inputs::coverage_threshold;
use crate::checks::native_cache::{cache_platform, native_cache};
use crate::checks::native_cache_inventory::published_inventory;
use crate::checks::native_cache_roots::{fallback_root, normalize_restore};
use crate::runner::{Cmd, Failure, Job, Outcome, Step, input, non_empty, optional, summary, write};
use std::env::consts::OS;
use std::path::{Path, PathBuf};
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
    let features = coverage_features(&optional("COVERAGE_FEATURES")?, || {
        Cmd::new("cargo metadata --format-version 1 --no-deps --locked")
            .cwd(&job.project)
            .capture()
    })?;
    if features.is_empty() {
        Cmd::new("cargo llvm-cov --workspace --locked --lcov --output-path")
            .arg(&lcov)
            .arg("--fail-under-lines")
            .arg(coverage_threshold()?)
            .cwd(&job.project)
            .run()?;
    } else {
        merged_coverage(&job, &lcov, &features)?;
    }
    non_empty(&lcov)
}

/// Start with clean profiles, retain the first run, then evaluate one locally generated report.
fn merged_coverage(job: &Job, lcov: &Path, features: &[String]) -> Outcome {
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
    Cmd::new("cargo llvm-cov clean --workspace")
        .cwd(&job.project)
        .run()?;
    let started = Instant::now();
    Cmd::new(default).cwd(&job.project).run()?;
    let default_seconds = started.elapsed().as_secs_f64();
    let started = Instant::now();
    let command = Cmd::new("cargo llvm-cov --workspace --locked --no-report --features")
        .arg(&features)
        .cwd(&job.project);
    native_command(job, command)?.run()?;
    let feature_seconds = started.elapsed().as_secs_f64();
    bound_checkout(job, &sha)?;
    Cmd::new("cargo llvm-cov report --lcov --output-path")
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

/// Only feature execution receives the consumer variable, after restored bytes are checked.
fn native_command(job: &Job, command: Cmd) -> Result<Cmd, Failure> {
    let requested = optional("NATIVE_CACHE_ROOT")?;
    if requested.is_empty() {
        return Ok(command);
    }
    let Some(policy) = native_cache(&job.project)? else {
        return Ok(command);
    };
    if !cache_platform(OS, &policy.platforms) {
        return Ok(command);
    }
    let root = {
        let root = PathBuf::from(requested);
        match normalize_restore(&root, &job.temp) {
            Ok(()) => Ok(root),
            Err(error) => {
                eprintln!("Native cache fallback: {error}");
                fallback_root(&job.temp)
            }
        }
    };
    let root = match root {
        Ok(root) => root,
        Err(error) => {
            eprintln!("Native cache disabled: {error}");
            return Ok(command);
        }
    };
    let before = published_inventory(&root, &policy.published)?;
    write(
        &job.report("native-cache-before.txt")?,
        format!("{}\n{before}", root.display()).as_bytes(),
        false,
    )?;
    Ok(command.env(&policy.environment, &root))
}

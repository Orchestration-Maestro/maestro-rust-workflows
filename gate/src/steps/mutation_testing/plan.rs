//! Route the compatible default mode or plan every engine/default obligation.

use super::scope::{self, Scope};
use super::{engine_plan, plan_identity, selftest};
use crate::checks::inputs::{mutation_mutants_per_shard, mutation_shards};
use crate::runner::{Cmd, Failure, Job, Outcome, flag, optional, output, tee_line, write};
use std::path::Path;

/// Run planning for this job and publish only the small routing values.
pub(super) fn run() -> Outcome {
    let job = Job::current()?;
    let report = job.report("mutants-plan.txt")?;
    if !flag("MUTATION_TEST")? {
        engine_plan::disabled()?;
        tee_line("Mutation plan: disabled", &report, false)?;
        return routing("disabled", Some(0), 0, &[]);
    }
    selftest::prepare(&job)?;
    let requested = mutation_shards()?;
    let engine = scope::has_engine_files()?;
    if !engine {
        engine_plan::disabled()?;
    }
    if inline_without_discovery(requested, engine) {
        tee_line(
            "Mutation plan: inline (serial default; discovery not run)",
            &report,
            false,
        )?;
        return routing("inline", None, 1, &[]);
    }

    let scope = scope::prepare(&job, &optional("GITHUB_BASE_REF")?)?;
    let listing_path = job.report("mutants-list.json")?;
    let log = job.report("mutants-plan.log")?;
    if engine {
        engine_plan::discover(&job, &scope, &listing_path, &log)?;
    } else {
        listing(&job.project, &scope, &listing_path, &log)?;
    }
    let mutants = plan_identity::listing_count(&listing_path)?;
    let target = mutation_mutants_per_shard()?;
    let (mode, shards, matrix) = plan_identity::selection(mutants, requested, target)?;
    let manifest = job.report("mutation-plan.json")?;
    plan_identity::write_manifest(&manifest, &job.project, &scope, mutants, shards)?;
    tee_line(
        &format!("Mutation plan: {mode}; {mutants} mutants; {shards} shard(s)"),
        &report,
        false,
    )?;
    routing(mode, Some(mutants), shards, &matrix)
}

/// Run the pinned listing once, preserving its JSON and logs separately.
fn listing(project: &Path, scope: &Scope, json: &Path, log: &Path) -> Outcome {
    let mut command = Cmd::new(
        "cargo mutants --list --json --no-shuffle --cargo-arg=--locked --colors=never --level=info",
    );
    if let Some(diff) = &scope.diff {
        let diff = diff.to_string_lossy().into_owned();
        command = command.args(["--in-diff", &diff]);
    }
    command = scope::exclude_default_files(command, project)?;
    let result = command.cwd(project).capture_output()?;
    plan_identity::show(&result)?;
    write(log, &result.stderr, false)?;
    write(json, &result.stdout, false)?;
    if !result.status.success() {
        return Err(Failure::status(result.status.code().unwrap_or(1)));
    }
    if result.stdout.iter().all(u8::is_ascii_whitespace) {
        if plan_identity::recognized_no_work(&result.stderr, scope.change.is_some()) {
            write(json, b"[]\n", false)?;
        } else {
            return Err(
                "cargo-mutants listing was empty without a recognized no-work result".into(),
            );
        }
    }
    plan_identity::validate_listing(json)
}

/// Publish the only plan data needed in GitHub's job matrix.
fn routing(mode: &str, mutants: Option<usize>, shards: usize, matrix: &[usize]) -> Outcome {
    output("mutation-mode", mode)?;
    output(
        "mutation-count",
        &mutants.map_or_else(String::new, |count| count.to_string()),
    )?;
    output("mutation-shards", &shards.to_string())?;
    output(
        "mutation-matrix",
        &format!(
            "[{}]",
            matrix
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(",")
        ),
    )
}

/// The serial default avoids discovery only when there is no feature-owned partition.
fn inline_without_discovery(requested: usize, engine: bool) -> bool {
    requested == 1 && !engine
}

#[cfg(test)]
mod tests {
    use super::inline_without_discovery;

    #[test]
    fn engine_ownership_always_requires_explicit_default_discovery() {
        assert!(inline_without_discovery(1, false));
        assert!(!inline_without_discovery(0, false));
        assert!(!inline_without_discovery(2, false));
        assert!(!inline_without_discovery(1, true));
    }
}

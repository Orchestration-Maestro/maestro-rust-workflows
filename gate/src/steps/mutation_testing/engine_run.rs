//! Execute exact shard obligations independently in featureless and engine modes.

use super::{engine_plan, plan, reports, scope};
use crate::runner::{Cmd, Job, Outcome, flag, input, optional, write};
use std::path::Path;

/// Verify the dual-mode plan before executing either mode.
pub(super) fn run() -> Outcome {
    run_mode(true)
}

/// Run the independently required, featureless engine-file control worker.
pub(super) fn run_default() -> Outcome {
    run_mode(false)
}

/// Select one bound mode without ever applying features to the control.
fn run_mode(enabled: bool) -> Outcome {
    let job = Job::current()?;
    if !flag("MUTATION_TEST")? {
        return Err("engine mutation evidence cannot be skipped".into());
    }
    let manifest = plan::safe_plan_file(&job, Path::new(&input("MUTATION_ENGINE_PLAN")?))?;
    let engine = plan::safe_plan_file(&job, Path::new(&input("MUTATION_ENGINE_LIST")?))?;
    let default = plan::safe_plan_file(&job, Path::new(&input("MUTATION_ENGINE_DEFAULT_LIST")?))?;
    let selected = input("MUTATION_SHARD")?;
    let shard = if selected == "0/1" {
        (0, 1)
    } else {
        plan::parse_shard(&selected)?
    };
    let source = scope::prepare(&job, &optional("GITHUB_BASE_REF")?)?;
    let total = plan::manifest_count(&manifest)?;
    let field = if enabled {
        ".engine_shards"
    } else {
        ".default_shards"
    };
    let planned_shards = Cmd::new("jaq -r").arg(field).arg(&manifest).capture()?;
    if planned_shards.trim() != shard.1.to_string()
        || input("MUTATION_SHARDS")? != shard.1.to_string()
    {
        return Err("engine mode worker differs from its planned shard matrix".into());
    }
    let manifest_shards = Cmd::new("jaq -r")
        .arg(".shard_count")
        .arg(&manifest)
        .capture()?;
    let manifest_shards = manifest_shards
        .trim()
        .parse()
        .map_err(|_| "engine manifest shard count is invalid")?;
    plan::verify_identity(&job, &source, &manifest, total, manifest_shards)?;
    engine_plan::verify_modes(&job, &manifest, &engine, &default)?;
    let receipt = if enabled {
        "mutants-engine-shard.json"
    } else {
        "mutants-engine-default-shard.json"
    };
    let mode_count = plan::listing_count(if enabled { &engine } else { &default })?;
    if mode_count < shard.1 {
        return Err("engine mode plan assigns an empty shard".into());
    }
    let expected = (mode_count - shard.0).div_ceil(shard.1);
    plan::write_receipt(&job.report(receipt)?, &manifest, shard.0, shard.1, expected)?;
    let mode_receipt = Cmd::new("jaq -c")
        .args(["--argjson", "count", &mode_count.to_string()])
        .args(["--arg", "mode", if enabled { "engine" } else { "default" }])
        .arg(". + {mutant_count:$count, mode:$mode}")
        .arg(job.report(receipt)?)
        .capture()?;
    write(&job.report(receipt)?, mode_receipt.as_bytes(), false)?;
    execute_mode(
        &job,
        &source,
        if enabled { &engine } else { &default },
        shard,
        enabled,
    )
}

/// Each mode gets its own baseline, outcome identity and bounded command.
fn execute_mode(
    job: &Job,
    source: &scope::Scope,
    listing: &Path,
    shard: (usize, usize),
    enabled: bool,
) -> Outcome {
    let assigned = Cmd::new("jaq -c")
        .args(["--argjson", "index", &shard.0.to_string()])
        .args(["--argjson", "shards", &shard.1.to_string()])
        .arg("[to_entries[] | select(.key % $shards == $index) | .value]")
        .arg(listing)
        .capture()?;
    let name = if enabled {
        "mutants-engine"
    } else {
        "mutants-engine-default"
    };
    let assigned_path = job.temp.join(format!("{name}-assigned.json"));
    write(&assigned_path, assigned.as_bytes(), false)?;
    let count = plan::listing_count(&assigned_path)?;
    if count == 0 {
        return Ok(());
    }
    let mut command = Cmd::new(concat!(
        "timeout --kill-after=1m 30m cargo mutants ",
        "--no-shuffle --cargo-arg=--locked --colors=never --level=info"
    ));
    if let Some(diff) = &source.diff {
        command = command.args(["--in-diff", &diff.to_string_lossy()]);
    }
    if enabled {
        command = scope::engine_selection(command, &job.project)?;
    } else {
        for file in scope::engine_files()? {
            command = command.args(["--file", &file]);
        }
    }
    let output = job.temp.join(name);
    let verdict = command
        .args(["--shard", &format!("{}/{}", shard.0, shard.1)])
        .args(["--sharding", "round-robin"])
        .arg("--output")
        .arg(&output)
        .cwd(&job.project)
        .tee(
            &job.report(if enabled {
                "mutants-engine.txt"
            } else {
                "mutants-engine-default.txt"
            })?,
            true,
        );
    verdict?;
    let outcomes = output.join("mutants.out/outcomes.json");
    plan::validate_execution(&assigned_path, &outcomes)?;
    reports::report_outcomes(job, &outcomes, source.change.as_deref(), Some(count))
}

#[cfg(test)]
mod tests {
    use super::plan;
    use std::{env, fs, process};

    #[test]
    fn every_mode_requires_exactly_its_planned_completed_mutants() {
        let root = env::temp_dir().join(format!("engine-execution-{}", process::id()));
        fs::create_dir_all(&root).unwrap();
        let listing = root.join("listing.json");
        let outcomes = root.join("outcomes.json");
        fs::write(&listing, "[]").unwrap();
        fs::write(&outcomes, "{\"outcomes\":[]}").unwrap();
        assert!(plan::validate_execution(&listing, &outcomes).is_ok());
        fs::write(&listing, "[{\"file\":\"src/run.rs\"}]").unwrap();
        assert_eq!(
            plan::validate_execution(&listing, &outcomes)
                .unwrap_err()
                .message
                .as_deref(),
            Some("mutation outcomes do not equal their complete mode-aware plan")
        );
        fs::remove_dir_all(root).unwrap();
    }
}

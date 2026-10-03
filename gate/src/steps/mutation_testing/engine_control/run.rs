//! Run tests only for compiled assigned mutants, preserving the exact complete control plan.

use super::{membership, outcomes, source};
use crate::checks::native_cache::{native_cache, native_cache_command};
use crate::runner::{Cmd, Job, Outcome, input, output, tee_line, write};
use std::fs;
use std::path::Path;
use std::time::Instant;

/// Classify non-members from a clean build and retain both raw and complete outcomes.
pub(in super::super) fn execute(
    job: &Job,
    assigned: &Path,
    receipt: &Path,
    diff: Option<&Path>,
) -> Outcome {
    let started = Instant::now();
    let directory = job.temp.join("mutants-engine-default");
    let root = directory.join("mutants.out");
    if directory.exists() {
        fs::remove_dir_all(&directory)
            .map_err(|error| format!("cannot clear control output: {error}"))?;
    }
    fs::create_dir_all(&root).map_err(|error| format!("cannot create control output: {error}"))?;
    let policy = native_cache(&job.project)?;
    membership::build(job, &root, receipt, assigned, policy.as_ref())?;
    let members = membership::verify(&root, receipt, assigned)?;
    let listing = outcomes::tested_listing(assigned, &members)?;
    let tested_listing = root.join("tested-mutants.json");
    write(&tested_listing, listing.as_bytes(), false)?;
    let count = super::super::plan_identity::listing_count(&tested_listing)?;
    if count > 0 {
        let remaining = super::package_build::remaining_budget(started.elapsed())?;
        let mut command = native_cache_command(policy.as_ref(), "timeout --kill-after=1m")
            .arg(format!("{}s", remaining.as_secs().max(1)))
            .args([
                "cargo",
                "mutants",
                "--no-shuffle",
                "--cargo-arg=--locked",
                "--colors=never",
                "--level=info",
            ]);
        command = command
            .arg("--config")
            .arg(source::selection_config(&root)?);
        if let Some(diff) = diff {
            command = command.arg("--in-diff").arg(diff);
        }
        for file in super::super::scope::engine_files()? {
            command = command.args(["--file", &file]);
        }
        let names = Cmd::new("jaq -r")
            .arg(".[] | .name")
            .arg(&tested_listing)
            .capture()?;
        let regex = exact_names(&names);
        let tested = job.temp.join("engine-control-tested");
        let verdict = command
            .args(["--re", &regex])
            .arg("--output")
            .arg(&tested)
            .cwd(&job.project)
            .tee(&job.report("mutants-engine-default.txt")?, true);
        if let Err(failure) = &verdict
            && failure.code != 2
        {
            return verdict;
        }
        copy_results(&tested.join("mutants.out"), &root)?;
        fs::rename(
            root.join("outcomes.json"),
            root.join("tested-outcomes.json"),
        )
        .map_err(|error| format!("cannot preserve tested control outcomes: {error}"))?;
        fs::rename(root.join("mutants.json"), &tested_listing)
            .map_err(|error| format!("cannot preserve tested control listing: {error}"))?;
    } else {
        outcomes::build_baseline(&root, &input("CARGO_MUTANTS_VERSION")?, members.len())?;
    }
    let combined = outcomes::compose(assigned, &root.join("tested-outcomes.json"), &members)?;
    write(&root.join("outcomes.json"), combined.as_bytes(), false)?;
    fs::copy(assigned, root.join("mutants.json"))
        .map_err(|error| format!("cannot retain complete control listing: {error}"))?;
    super::super::plan_identity::validate_execution(assigned, &root.join("outcomes.json"))?;
    tee_line(
        if members.values().any(Option::is_some) {
            concat!(
                "Featureless control complete; non-members verified from compiler dep-info; ",
                "compiled mutants tested"
            )
        } else {
            "Featureless control complete; membership unverified; every assigned mutant tested"
        },
        &job.report("mutants-engine-default.txt")?,
        true,
    )?;
    output("applied", "true")
}

/// Anchor each escaped full cargo-mutants name, not a substring or file-level sample.
fn exact_names(names: &str) -> String {
    let escaped: Vec<_> = names
        .lines()
        .map(|name| {
            let mut result = String::new();
            for character in name.chars() {
                if ".+*?()|[]{}^$\\".contains(character) {
                    result.push('\\');
                }
                result.push(character);
            }
            result
        })
        .collect();
    format!("^(?:{})$", escaped.join("|"))
}

/// Preserve raw paths and logs alongside membership without overwriting its evidence.
fn copy_results(source: &Path, destination: &Path) -> Outcome {
    fs::create_dir_all(destination)
        .map_err(|error| format!("cannot retain control results: {error}"))?;
    for entry in
        fs::read_dir(source).map_err(|error| format!("cannot read control results: {error}"))?
    {
        let entry = entry.map_err(|error| format!("cannot inspect control result: {error}"))?;
        let target = destination.join(entry.file_name());
        if entry.path().is_dir() {
            copy_results(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)
                .map_err(|error| format!("cannot copy control result: {error}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::exact_names;

    #[test]
    fn exact_selection_escapes_regex_metacharacters_and_anchors_every_name() {
        assert_eq!(
            exact_names("src/a.rs: replace f() -> x+y?\nother[0]\n"),
            "^(?:src/a\\.rs: replace f\\(\\) -> x\\+y\\?|other\\[0\\])$"
        );
    }
}

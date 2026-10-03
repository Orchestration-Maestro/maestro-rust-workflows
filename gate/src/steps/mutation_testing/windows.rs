//! Select and run mutations for Windows-owned files only.

use super::{plan_identity, reports, scope};
use crate::checks::native_cache::{native_cache, native_cache_command};
use crate::checks::quality_config::mutation_windows;
use crate::runner::{Cmd, Failure, Job, Outcome, flag, input, optional, output, tee_line, write};
use std::fs;
use std::path::Path;

/// Run the configured Windows-owned files under the same commit scope as Linux.
pub(super) fn run() -> Outcome {
    let job = Job::current()?;
    fs::create_dir_all(&job.reports)
        .map_err(|error| format!("cannot create {}: {error}", job.reports.display()))?;
    let report = job.report("mutants.txt")?;
    if !flag("MUTATION_TEST")? {
        tee_line("SKIPPED: mutation-test=false", &report, false)?;
        return output("applied", "false");
    }
    let configured = mutation_windows(&job.project, &input("MUTATION_WINDOWS")?)?;
    if configured.is_empty() {
        tee_line("No Windows-owned files are configured", &report, false)?;
        return output("applied", "false");
    }
    let source_scope = scope::prepare(&job, &optional("GITHUB_BASE_REF")?)?;
    write(&job.report("mutation-windows-list.json")?, b"[]", false)?;
    let diff_run = source_scope.diff.is_some();
    let touched = if diff_run {
        Cmd::new("git -c core.quotePath=false diff --relative --name-only HEAD^1 HEAD -- .")
            .cwd(&job.project)
            .capture()?
    } else {
        String::new()
    };
    let returned = scope::returned_host_files(&job)?;
    let touched: Vec<_> = touched
        .lines()
        .chain(returned.iter().map(String::as_str))
        .collect();
    let configured: Vec<_> = configured.iter().map(String::as_str).collect();
    let selected = files_for_run(&configured, &touched, diff_run);
    if selected.is_empty() && diff_run {
        save_windows_plan(&job, &source_scope)?;
        tee_line("No Windows-owned file changed", &report, false)?;
        return output("applied", "false");
    }
    let mut counts = Vec::with_capacity(selected.len());
    for file in &selected {
        let available = list_file(&job, &source_scope, file, false)?;
        require_mutants(&[*file], &[available])?;
        let count = if diff_run {
            let changed = list_file(&job, &source_scope, file, true)?;
            if changed == 0 {
                tee_line(
                    &format!("SKIPPED: no mutants in the changed lines of {file}"),
                    &report,
                    false,
                )?;
            } else {
                tee_line(&format!("{file}: {changed} mutants"), &report, false)?;
            }
            changed
        } else {
            tee_line(&format!("{file}: {available} mutants"), &report, false)?;
            available
        };
        counts.push(count);
    }
    save_windows_plan(&job, &source_scope)?;
    let verdict = run_files(&job, &source_scope, &selected, &report);
    let expected = counts.into_iter().sum();
    let outcomes = job.temp.join("mutants/mutants.out/outcomes.json");
    if outcomes.is_file() {
        fs::copy(&outcomes, job.report("mutants.json")?)
            .map_err(|error| format!("cannot copy the outcomes: {error}"))?;
    }
    verdict?;
    if expected > 0 {
        plan_identity::validate_execution(&job.report("mutation-windows-list.json")?, &outcomes)?;
    }
    reports::report_outcomes(
        &job,
        &outcomes,
        source_scope.change.as_deref(),
        Some(expected),
    )?;
    output("applied", "true")
}

/// List one exact file so empty per-file scopes cannot pass silently.
fn list_file(job: &Job, scope: &scope::Scope, file: &str, in_diff: bool) -> Result<usize, Failure> {
    let listing = job.temp.join("windows-mutants-list.json");
    let mut command = Cmd::new(
        "cargo mutants --list --json --no-shuffle --cargo-arg=--locked --colors=never --level=info",
    )
    .arg("--file")
    .arg(file);
    if in_diff && let Some(diff) = &scope.diff {
        command = command.args(["--in-diff", &diff.to_string_lossy()]);
    }
    let listed = command.cwd(&job.project).capture()?;
    fs::write(&listing, &listed).map_err(|error| format!("{}: {error}", listing.display()))?;
    if listed.trim().is_empty() {
        fs::write(&listing, "[]")
            .map_err(|error| format!("cannot save empty Windows listing: {error}"))?;
    }
    if in_diff || scope.diff.is_none() {
        let target = job.report("mutation-windows-list.json")?;
        let mut documents = fs::read(&target)
            .map_err(|error| format!("cannot read Windows plan listing: {error}"))?;
        documents.push(b'\n');
        documents.extend(
            fs::read(&listing)
                .map_err(|error| format!("cannot read Windows discovery: {error}"))?,
        );
        let json = Cmd::new("jaq -sc")
            .arg("add")
            .stdin_bytes(&documents)
            .capture()?;
        write(&target, json.as_bytes(), false)?;
    }
    let count = Cmd::new("jaq -er").arg("length").arg(&listing).capture()?;
    parse_listing_count(&count)
}

/// Preserve a Windows-owned plan even when no owned changed lines need execution.
fn save_windows_plan(job: &Job, source: &scope::Scope) -> Outcome {
    let listing = job.report("mutation-windows-list.json")?;
    let count = plan_identity::listing_count(&listing)?;
    let manifest = job.report("mutation-windows-plan.json")?;
    plan_identity::write_manifest(&manifest, &job.project, source, count, 1)?;
    let json = Cmd::new("jaq -c")
        .args(["--argjson", "files", &input("MUTATION_WINDOWS")?])
        .arg(". + {partition:\"windows\", os:\"windows\", files:$files}")
        .arg(&manifest)
        .capture()?;
    write(&manifest, json.as_bytes(), false)
}

/// Parse the listing count, rejecting invalid output as a tool failure.
fn parse_listing_count(count: &str) -> Result<usize, Failure> {
    count
        .trim()
        .parse::<usize>()
        .map_err(|_| Failure::from("cargo-mutants listing count is invalid"))
}

/// Run the whole selected set once into the standard mutation artifact layout.
fn run_files(job: &Job, scope: &scope::Scope, files: &[&str], report: &Path) -> Outcome {
    let policy = native_cache(&job.project)?;
    let mut command = native_cache_command(
        policy.as_ref(),
        "cargo mutants --no-shuffle --cargo-arg=--locked --colors=never --level=info",
    );
    for file in files {
        command = command.args(["--file", file]);
    }
    command = command.arg("--output").arg(job.temp.join("mutants"));
    if let Some(diff) = &scope.diff {
        command = command.args(["--in-diff", &diff.to_string_lossy()]);
    }
    command.cwd(&job.project).tee(report, true)
}

/// Keep only files touched by a pull-request diff, or all files in full scope.
fn files_for_run<'a>(configured: &'a [&'a str], touched: &[&str], diff: bool) -> Vec<&'a str> {
    if diff {
        configured
            .iter()
            .copied()
            .filter(|file| touched.contains(file))
            .collect()
    } else {
        configured.to_vec()
    }
}

/// Refuse a required file whose scoped listing produced no mutants.
fn require_mutants(files: &[&str], counts: &[usize]) -> Result<(), String> {
    for (index, file) in files.iter().enumerate() {
        if counts.get(index).copied().unwrap_or_default() == 0 {
            return Err(format!("{file} produced no mutants"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{files_for_run, parse_listing_count, require_mutants};

    #[test]
    fn an_invalid_cargo_mutants_listing_count_is_refused_by_name() {
        assert_eq!(
            parse_listing_count("not-a-number")
                .unwrap_err()
                .message
                .as_deref(),
            Some("cargo-mutants listing count is invalid")
        );
    }

    #[test]
    fn a_pull_request_without_a_windows_owned_change_is_a_valid_empty_scope() {
        let files = ["src/windows.rs", "src/os.rs"];
        assert!(files_for_run(&files, &[], true).is_empty());
    }

    #[test]
    fn a_pull_request_requires_mutants_only_for_touched_windows_files() {
        let files = ["src/windows.rs", "src/os.rs"];
        let touched = ["src/os.rs"];
        let selected = files_for_run(&files, &touched, true);
        assert_eq!(selected, ["src/os.rs"]);
        assert!(require_mutants(&selected, &[1]).is_ok());
    }

    #[test]
    fn a_full_run_requires_mutants_in_every_windows_owned_file() {
        let files = ["src/windows.rs", "src/os.rs"];
        let selected = files_for_run(&files, &[], false);
        assert_eq!(selected, files);
        assert_eq!(
            require_mutants(&selected, &[1, 0]),
            Err("src/os.rs produced no mutants".to_owned())
        );
    }
}

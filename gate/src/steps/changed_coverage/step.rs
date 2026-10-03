//! `rust-gate changed-coverage`: COV-002, the lines a pull request adds held
//! to a floor of their own. The coverage step's LCOV says which lines are
//! coverable and which ran; the change against the base branch says which
//! lines are new. A `feat` or `fix` pull request covers 95 % of its coverable
//! new lines, any other 90 %, and a few lines may stay uncovered so that a
//! three-line change is not failed by one: `max(1, floor((100 - target) % of
//! n))`. A push has no base to compare with, and nothing is measured.

use super::pending;
use crate::checks::findings::relative;
use crate::checks::mutation_host;
use crate::checks::pull_request::{added_lines, pull_request_diff, title_type};
use crate::runner::{Failure, Job, Outcome, Step, input, optional, output, summary, write};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

/// What this step declares: its inputs, its tools and its reports.
pub(crate) const STEPS: &[Step] = &[Step {
    workflow: "ci",
    id: "changed-coverage",
    summary: "Coverage of the lines a pull request adds",
    inputs: &[
        "GITHUB_BASE_REF",
        "GITHUB_WORKSPACE",
        "PULL_REQUEST_TITLE",
        "GITHUB_SHA",
        "GITHUB_RUN_ID",
        "GITHUB_RUN_ATTEMPT",
    ],
    tools: &["git", "jaq", "cargo metadata"],
    reports: &["changed-coverage.txt", "changed-coverage-pending.json"],
    run,
}];

/// What a pull request's new lines came to.
#[derive(Debug, PartialEq, Eq)]
struct Judgement {
    /// The floor, in percent, the title's type sets.
    target: u32,
    /// The coverable new lines.
    coverable: usize,
    /// The coverable new lines that never ran, as `path:line`.
    uncovered: Vec<String>,
    /// How many may stay uncovered.
    allowed: usize,
}

/// Run the step.
fn run() -> Outcome {
    let job = Job::current()?;
    let report = job.report("changed-coverage.txt")?;
    let host = mutation_host::host_policy(&job.project)?;
    let base = optional("GITHUB_BASE_REF")?;
    if base.is_empty() && host.is_none() {
        write(
            &report,
            b"NOT APPLICABLE: a push has no base to compare with\n",
            false,
        )?;
        return output("applied", "false");
    }
    output("applied", "true")?;
    let workspace = PathBuf::from(input("GITHUB_WORKSPACE")?);
    let lcov = fs::read_to_string(job.earlier("coverage.lcov"))
        .map_err(|error| format!("changed-coverage: the coverage report: {error}"))?;
    let title = optional("PULL_REQUEST_TITLE")?;
    let target = if matches!(title_type(&title), Some("feat" | "fix")) {
        95
    } else {
        90
    };
    let mut added = if base.is_empty() {
        BTreeMap::new()
    } else {
        added_lines(&pull_request_diff(&workspace)?)
    };
    let coverage_root = if host.is_some() {
        if let Ok(prefix) = job.project.strip_prefix(&workspace)
            && !prefix.as_os_str().is_empty()
        {
            let prefix = format!("{}/", prefix.display());
            added = added
                .into_iter()
                .filter_map(|(file, lines)| {
                    file.strip_prefix(&prefix)
                        .map(|file| (file.to_owned(), lines))
                })
                .collect();
        }
        &job.project
    } else {
        &workspace
    };
    let executed = executed_lines(&lcov, coverage_root);
    if let Some(host) = &host {
        pending::pending_coverage(&job, host, target, &added, &executed)?;
        output("applied", "pending-host")?;
        let text = pending::ordinary_verdict(&job)?;
        write(&report, text.as_bytes(), false)?;
        return summary(&text);
    }
    let judgement = judge(target, &added, &executed);
    let mut text = format!(
        "{} coverable new lines, {} uncovered, {} allowed at {target} %\n",
        judgement.coverable,
        judgement.uncovered.len(),
        judgement.allowed
    );
    for line in &judgement.uncovered {
        let _ = writeln!(text, "UNCOVERED {line}");
    }
    write(&report, text.as_bytes(), false)?;
    summary(&format!("## Coverage of the new lines\n\n{text}"))?;
    if judgement.uncovered.len() <= judgement.allowed {
        return Ok(());
    }
    Err(Failure::from(format!(
        "changed-coverage: {} of {} new lines never run; this pull request covers {target} % of \
         them, leaving at most {}; changed-coverage.txt names each",
        judgement.uncovered.len(),
        judgement.coverable,
        judgement.allowed
    )))
}

/// How often each line of each file ran, from an LCOV report whose paths are
/// under `workspace`, keyed by the path relative to it.
fn executed_lines(lcov: &str, workspace: &Path) -> BTreeMap<String, BTreeMap<usize, u64>> {
    let mut files: BTreeMap<String, BTreeMap<usize, u64>> = BTreeMap::new();
    let mut file = String::new();
    for line in lcov.lines() {
        if let Some(path) = line.strip_prefix("SF:") {
            // Named as the diff names it, `/`-separated under the checkout,
            // on Windows and through a symbolic link too.
            file = relative(workspace, Path::new(path));
        } else if let Some(record) = line.strip_prefix("DA:") {
            let mut fields = record.split(',');
            let number = fields.next().and_then(|field| field.parse().ok());
            let hits = fields.next().and_then(|field| field.parse().ok());
            if let (Some(number), Some(hits)) = (number, hits) {
                files.entry(file.clone()).or_default().insert(number, hits);
            }
        }
    }
    files
}

/// The judgement of the `added` lines against how often they ran.
fn judge(
    target: u32,
    added: &BTreeMap<String, Vec<usize>>,
    executed: &BTreeMap<String, BTreeMap<usize, u64>>,
) -> Judgement {
    let mut coverable = 0;
    let mut uncovered = Vec::new();
    for (file, lines) in added {
        let Some(counts) = executed.get(file) else {
            continue;
        };
        for line in lines {
            match counts.get(line) {
                Some(0) => uncovered.push(format!("{file}:{line}")),
                Some(_) => {}
                None => continue,
            }
            coverable += 1;
        }
    }
    let share = usize::try_from(100 - target).unwrap_or(0);
    let allowed = (share * coverable / 100).max(1);
    Judgement {
        target,
        coverable,
        uncovered,
        allowed,
    }
}

#[cfg(test)]
mod tests {
    use super::{executed_lines, judge};
    use std::collections::BTreeMap;
    use std::path::Path;

    #[test]
    fn lcov_paths_are_read_relative_to_the_workspace() {
        let lcov = "SF:/w/src/lib.rs\nDA:3,1\nDA:4,0\nend_of_record\n";
        let executed = executed_lines(lcov, Path::new("/w"));
        assert_eq!(executed["src/lib.rs"][&3], 1);
        assert_eq!(executed["src/lib.rs"][&4], 0);
    }

    #[test]
    fn only_coverable_new_lines_count_and_one_may_stay_uncovered() {
        let added = BTreeMap::from([
            ("src/lib.rs".to_owned(), vec![1, 2, 3, 4]),
            ("tests/it.rs".to_owned(), vec![1]),
        ]);
        let counts = BTreeMap::from([(1, 0), (2, 5), (3, 0)]);
        let executed = BTreeMap::from([("src/lib.rs".to_owned(), counts)]);
        let judged = judge(95, &added, &executed);
        assert_eq!(judged.coverable, 3);
        assert_eq!(judged.uncovered, ["src/lib.rs:1", "src/lib.rs:3"]);
        assert_eq!(judged.allowed, 1);
        let many: Vec<usize> = (1..=100).collect();
        let executed = BTreeMap::from([(
            "src/lib.rs".to_owned(),
            many.iter().map(|line| (*line, 1)).collect(),
        )]);
        let added = BTreeMap::from([("src/lib.rs".to_owned(), many)]);
        assert_eq!(judge(90, &added, &executed).allowed, 10);
    }
}

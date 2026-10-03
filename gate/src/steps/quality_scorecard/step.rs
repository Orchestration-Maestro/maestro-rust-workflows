//! `rust-gate scorecard`: what this run actually enforced, as data, prose and
//! a self-contained badge. A consumer can see which of the available controls
//! they switched on instead of assuming the defaults cover everything, and
//! it never claims a control that did not run.

use super::scorecard::{Control, Scorecard, State};
use crate::checks::inputs::{LicensePolicy, UnsafePolicy, license_policy, unsafe_policy};
use crate::runner::{Cmd, Failure, Job, Outcome, Step, flag, input, optional, summary, write};
use std::fs;
use std::path::Path;

/// What this step declares: its inputs, its tools and its reports.
pub(crate) const STEPS: &[Step] = &[
    Step {
        workflow: "ci",
        id: "scorecard",
        summary: "Build quality scorecard",
        inputs: &[
            "API_APPLIED",
            "API_COMPATIBILITY",
            "CHANGED_COVERAGE_APPLIED",
            "DEPENDENCY_AUDIT",
            "FEATURES_APPLIED",
            "GITHUB_SHA",
            "HOOKS_APPLIED",
            "LICENSE_POLICY",
            "MUTANTS_APPLIED",
            "MUTATION_TEST",
            "MUTATION_HOST_COUNT",
            "OUT_API",
            "OUT_ARCHITECTURE",
            "OUT_AUDIT",
            "OUT_CHANGED_COVERAGE",
            "OUT_COVERAGE",
            "OUT_DUPLICATION",
            "OUT_FEATURES",
            "OUT_HOOKS",
            "OUT_HYGIENE",
            "OUT_LICENCES",
            "OUT_MANAGED_FILES",
            "OUT_MSRV",
            "OUT_MUTANTS",
            "OUT_PERFORMANCE",
            "OUT_PULL_REQUEST",
            "OUT_QUALITY",
            "OUT_SECRETS",
            "OUT_STAGE",
            "OUT_UNUSED",
            "OUT_VET",
            "PERFORMANCE_APPLIED",
            "PULL_REQUEST_APPLIED",
            "RUSTUP_TOOLCHAIN",
            "SARIF_REPORTS",
            "UNSAFE_POLICY",
            "UNUSED_DEPENDENCIES",
        ],
        tools: &[],
        reports: &["scorecard.json", "scorecard.md", "scorecard.svg"],
        run,
    },
    Step {
        workflow: "ci",
        id: "scorecard-finalize",
        summary: "Finalize mutation scorecard",
        inputs: &[
            "MUTATION_STATE",
            "MUTATION_HOST_COUNT",
            "CHANGED_COVERAGE_STATE",
        ],
        tools: &["jaq"],
        reports: &["scorecard.json", "scorecard.md", "scorecard.svg"],
        run: finalize,
    },
];

/// Run the step: the controls gathered once, then written as JSON, as
/// Markdown for the log and the summary, and as the badge.
fn run() -> Outcome {
    let job = Job::current()?;
    // The tools step creates the reports directory; a step failing before it
    // must still be the one the scorecard names.
    fs::create_dir_all(&job.reports)
        .map_err(|error| format!("cannot create {}: {error}", job.reports.display()))?;
    // A failed coverage gate leaves no report; that is a missing value, not
    // an error to swallow, so the absence is handled rather than silenced.
    let coverage = fs::read_to_string(job.earlier("coverage.lcov"))
        .ok()
        .and_then(|lcov| line_coverage(&lcov));
    let mut controls = controls()?;
    controls.extend(organization_controls()?);
    let controls = controls
        .into_iter()
        .map(|(name, kind, state)| (name.to_owned(), kind.to_owned(), state))
        .collect();
    // Informational, so its absence is not an error: the line appears only
    // when the complexity step ran.
    let complexity = fs::read_to_string(job.earlier("complexity.json")).ok();
    let scorecard = Scorecard {
        controls,
        revision: input("GITHUB_SHA")?,
        toolchain: input("RUSTUP_TOOLCHAIN")?,
        coverage,
        complexity: complexity.as_deref().map(|data| data.trim().to_owned()),
    };
    write(
        &job.report("scorecard.json")?,
        scorecard.json().as_bytes(),
        false,
    )?;
    let mut markdown = scorecard.markdown();
    if let Some(data) = &complexity {
        markdown.push_str(&complexity_line(data));
    }
    print!("{markdown}");
    write(&job.report("scorecard.md")?, markdown.as_bytes(), false)?;
    summary(&markdown)?;
    write(
        &job.report("scorecard.svg")?,
        scorecard.badge().as_bytes(),
        false,
    )
}

/// Rebuild the reports after shard aggregation changes mutation from not-run.
fn finalize() -> Outcome {
    let job = Job::current()?;
    let mutation_state = State::parse(&input("MUTATION_STATE")?)
        .filter(|state| matches!(state, State::Passed | State::Failed | State::NotRun))
        .ok_or("MUTATION_STATE must be passed, failed or not-run")?;
    let path = job.earlier("scorecard.json");
    let rows = Cmd::new("jaq -r")
        .arg(".controls[] | [.control,.kind,.state] | @tsv")
        .arg(&path)
        .capture()?;
    let mut controls = Vec::new();
    let mut mutation_seen = false;
    for row in rows.lines() {
        let mut columns = row.split('\t');
        let name = columns.next().unwrap_or_default();
        let kind = columns.next().unwrap_or_default();
        let state_text = columns.next().unwrap_or_default();
        if name.is_empty() || kind.is_empty() || columns.next().is_some() {
            return Err("scorecard controls have an invalid shape".into());
        }
        let mut state =
            State::parse(state_text).ok_or("scorecard contains an unknown control state")?;
        if name == "mutation testing" {
            if mutation_seen || kind != "optional" {
                return Err("scorecard mutation control is duplicated or invalid".into());
            }
            mutation_seen = true;
            state = mutation_state;
        }
        if name == "changed-line coverage"
            && optional("MUTATION_HOST_COUNT")?
                .parse::<usize>()
                .is_ok_and(|count| count > 0)
        {
            state = State::parse(&optional("CHANGED_COVERAGE_STATE")?)
                .filter(|state| matches!(state, State::Passed | State::Failed | State::NotRun))
                .unwrap_or(State::NotRun);
        }
        controls.push((name.to_owned(), kind.to_owned(), state));
    }
    if !mutation_seen {
        return Err("scorecard has no mutation testing control".into());
    }
    let revision = field(&path, ".revision")?;
    let toolchain = field(&path, ".toolchain")?;
    if revision.is_empty() || toolchain.is_empty() {
        return Err("scorecard revision or toolchain is missing".into());
    }
    let coverage = field(&path, ".coverage // empty")?;
    let complexity = Cmd::new("jaq -c")
        .arg(".complexity // empty")
        .arg(&path)
        .capture()?;
    let complexity = (!complexity.trim().is_empty()).then(|| complexity.trim().to_owned());
    let scorecard = Scorecard {
        controls,
        revision,
        toolchain,
        coverage: (!coverage.is_empty()).then_some(coverage),
        complexity: complexity.clone(),
    };
    write(&path, scorecard.json().as_bytes(), false)?;
    let mut markdown = scorecard.markdown();
    if let Some(data) = &complexity {
        markdown.push_str(&complexity_line(data));
    }
    print!("{markdown}");
    write(&job.report("scorecard.md")?, markdown.as_bytes(), false)?;
    summary(&markdown)?;
    write(
        &job.report("scorecard.svg")?,
        scorecard.badge().as_bytes(),
        false,
    )
}

/// Read one already checked field from the previous scorecard.
fn field(path: &Path, expression: &str) -> Result<String, Failure> {
    Cmd::new("jaq -r")
        .arg(expression)
        .arg(path)
        .capture()
        .map(|value| value.trim().to_owned())
}

/// Selection alone never proves execution. Steps that can find no applicable
/// work also supply an explicit application result; an absent result stays unrun.
fn controls() -> Result<Vec<Control>, Failure> {
    let sarif = match (
        input("OUT_QUALITY")?.as_str(),
        input("OUT_SECRETS")?.as_str(),
    ) {
        ("failure", _) | (_, "failure") => "failure",
        ("success", "success") => "success",
        _ => "skipped",
    };
    let state = |name: &str, enabled: bool, applied: &str| -> Result<State, Failure> {
        Ok(State::from_outcome(&input(name)?, enabled, applied))
    };
    Ok(vec![
        (
            "formatting, clippy, tests, rustdoc",
            "enforced",
            state("OUT_QUALITY", true, "true")?,
        ),
        (
            "line coverage",
            "enforced",
            state("OUT_COVERAGE", true, "true")?,
        ),
        ("advisories", "enforced", state("OUT_AUDIT", true, "true")?),
        (
            "secret scan",
            "enforced",
            state("OUT_SECRETS", true, "true")?,
        ),
        (
            "declared MSRV",
            "enforced",
            state("OUT_MSRV", true, "true")?,
        ),
        (
            "feature combinations",
            "enforced",
            state("OUT_FEATURES", true, &optional("FEATURES_APPLIED")?)?,
        ),
        (
            "packaging and SBOM",
            "enforced",
            state("OUT_STAGE", true, "true")?,
        ),
        (
            "sources, versions and licences",
            "enforced",
            state(
                "OUT_LICENCES",
                license_policy()? != LicensePolicy::Off,
                "true",
            )?,
        ),
        (
            "mutation testing",
            "optional",
            state(
                "OUT_MUTANTS",
                flag("MUTATION_TEST")?,
                &if optional("MUTATION_HOST_COUNT")?
                    .parse::<usize>()
                    .is_ok_and(|count| count > 0)
                {
                    "pending-host".to_owned()
                } else {
                    optional("MUTANTS_APPLIED")?
                },
            )?,
        ),
        (
            "API compatibility",
            "optional",
            state(
                "OUT_API",
                flag("API_COMPATIBILITY")?,
                &optional("API_APPLIED")?,
            )?,
        ),
        (
            "unused dependencies",
            "optional",
            state("OUT_UNUSED", flag("UNUSED_DEPENDENCIES")?, "true")?,
        ),
        (
            "unsafe denied",
            "optional",
            state(
                "OUT_QUALITY",
                unsafe_policy()? == UnsafePolicy::Deny,
                "true",
            )?,
        ),
        (
            "SARIF reports",
            "optional",
            State::from_outcome(sarif, flag("SARIF_REPORTS")?, "true"),
        ),
    ])
}

/// One control per family of the organization's rules, each enforced
/// wherever it applies, and the two a repository can switch: the recorded
/// dependency audits and the performance budget.
fn organization_controls() -> Result<Vec<Control>, Failure> {
    let state = |name: &str, enabled: bool, applied: &str| -> Result<State, Failure> {
        Ok(State::from_outcome(&input(name)?, enabled, applied))
    };
    Ok(vec![
        (
            "source rules",
            "enforced",
            state("OUT_ARCHITECTURE", true, "true")?,
        ),
        (
            "repository hygiene",
            "enforced",
            state("OUT_HYGIENE", true, "true")?,
        ),
        (
            "managed files",
            "enforced",
            state("OUT_MANAGED_FILES", true, "true")?,
        ),
        (
            "commit hooks",
            "enforced",
            state("OUT_HOOKS", true, &optional("HOOKS_APPLIED")?)?,
        ),
        (
            "duplication, the rule of three",
            "enforced",
            state("OUT_DUPLICATION", true, "true")?,
        ),
        (
            "changed-line coverage",
            "enforced",
            state(
                "OUT_CHANGED_COVERAGE",
                true,
                &optional("CHANGED_COVERAGE_APPLIED")?,
            )?,
        ),
        (
            "pull request rules",
            "enforced",
            state("OUT_PULL_REQUEST", true, &optional("PULL_REQUEST_APPLIED")?)?,
        ),
        (
            "recorded dependency audits",
            "optional",
            state("OUT_VET", flag("DEPENDENCY_AUDIT")?, "true")?,
        ),
        (
            "performance budget",
            "optional",
            state("OUT_PERFORMANCE", true, &optional("PERFORMANCE_APPLIED")?)?,
        ),
    ])
}

/// The informational line under the table: what the complexity step counted,
/// or that it could not measure.
fn complexity_line(data: &str) -> String {
    let number = |field: &str| -> String {
        data.split_once(&format!("\"{field}\":"))
            .map(|(_, rest)| rest.chars().take_while(char::is_ascii_digit).collect())
            .unwrap_or_default()
    };
    if data.contains("\"measured\":true") {
        format!(
            "\nComplexity, informational: {} functions over the size thresholds, {} files over \
             300 lines of code; see complexity.txt.\n",
            number("functions_over"),
            number("files_over")
        )
    } else {
        "\nComplexity, informational: not measured.\n".to_owned()
    }
}

/// Covered lines over instrumented lines from an LCOV report, one decimal,
/// or nothing when the report instruments no line.
fn line_coverage(lcov: &str) -> Option<String> {
    let (mut found, mut hit) = (0u32, 0u32);
    for line in lcov.lines() {
        if let Some(n) = line.strip_prefix("LF:") {
            found = found.saturating_add(n.trim().parse::<u32>().unwrap_or(0));
        } else if let Some(n) = line.strip_prefix("LH:") {
            hit = hit.saturating_add(n.trim().parse::<u32>().unwrap_or(0));
        }
    }
    (found > 0).then(|| format!("{:.1}", 100.0 * f64::from(hit) / f64::from(found)))
}

#[cfg(test)]
mod tests {
    use super::line_coverage;

    #[test]
    fn coverage_is_hit_over_found_lines() {
        assert_eq!(
            line_coverage("LF:10\nLH:9\nLF:10\nLH:8\n"),
            Some("85.0".into())
        );
        assert_eq!(line_coverage("LF:0\n"), None);
        assert_eq!(line_coverage(""), None);
    }
}

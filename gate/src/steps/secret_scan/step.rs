//! `rust-gate secrets`: built-in Gitleaks rules, no consumer allowlist, and
//! organization-owned exceptions bound to the exact flagged content.

use super::archive::relocate_ignore;
use super::policy::exceptions;
use super::reports::{findings, reports};
use crate::runner::{Cmd, Failure, Job, Outcome, Step, flag, optional, path};
use std::fs;
use std::path::Path;

/// What this step declares: its inputs, its tools and its reports.
pub(crate) const STEPS: &[Step] = &[Step {
    workflow: "ci",
    id: "secrets",
    summary: "Redacted source secret scan",
    inputs: &[
        "GITHUB_WORKSPACE",
        "GITHUB_ACTIONS",
        "GITHUB_REPOSITORY",
        "SARIF_REPORTS",
    ],
    tools: &["git", "gitleaks", "tar", "jaq"],
    reports: &["secrets.json", "secrets.sarif"],
    run,
}];

/// Built-in rules plus an explicit finding for every inline suppression marker.
const CONFIG: &str = concat!(
    "[extend]\nuseDefault = true\n\n[[rules]]\nid = 'gitleaks-allow'\n",
    "description = 'Inline allow marker requires organization review'\n",
    "regex = '(?m)^.*gitleaks",
    ":allow.*$'\n",
    "keywords = ['gitleaks",
    ":allow']\n",
);

/// Scan into a private pipe, never a raw report file or the job log.
fn scan(source: &Path, config: &Path, ignore: &Path) -> Result<Vec<u8>, Failure> {
    let output = Cmd::new("gitleaks dir")
        .arg(source)
        .arg("--config")
        .arg(config)
        .arg("--gitleaks-ignore-path")
        .arg(ignore)
        .args([
            "--ignore-gitleaks-allow",
            "--redact=0",
            "--no-banner",
            "--exit-code",
            "0",
            "--report-format",
            "json",
            "--report-path",
            "-",
        ])
        .capture_output()?;
    if !output.status.success() {
        return Err(Failure::status(output.status.code().unwrap_or(1)));
    }
    // Findings use exit 0; scanner execution errors still propagate. Parsing the
    // report is mandatory; an absent/invalid report must never count as clean.
    if output.stdout.is_empty() {
        return Err("Secret scanner produced no report".into());
    }
    Ok(output.stdout)
}

/// Only hosted runner identity can activate approvals; local scans stay fully strict.
fn repository(hosted: bool, supplied: &str) -> Result<&str, Failure> {
    if !hosted {
        println!(
            "::notice::Reviewed secret-scan exceptions apply only in hosted CI; \
                  scanning without exceptions"
        );
        return Ok("");
    }
    if supplied.is_empty() {
        return Err("Hosted secret scan requires GITHUB_REPOSITORY".into());
    }
    Ok(supplied)
}

/// Archive this revision, classify every finding, report stale approvals, then fail closed.
fn run() -> Outcome {
    let job = Job::current()?;
    let supplied = optional("GITHUB_REPOSITORY")?;
    let repository = repository(optional("GITHUB_ACTIONS")? == "true", &supplied)?;
    let source = job.temp.join("secret-source");
    fs::create_dir_all(&source)
        .map_err(|error| format!("cannot create {}: {error}", source.display()))?;
    let archive = Cmd::new("git -C")
        .arg(path("GITHUB_WORKSPACE")?)
        .args(["archive", "HEAD"])
        .capture_bytes()?;
    Cmd::new("tar -x -f - -C")
        .arg(&source)
        .stdin_bytes(&archive)
        .run()?;
    let relocated = relocate_ignore(&source)?;
    let config = job.temp.join("gitleaks.toml");
    fs::write(&config, CONFIG)
        .map_err(|error| format!("cannot write {}: {error}", config.display()))?;
    let raw = scan(&source, &config, &job.temp.join("no-ignore-file"))?;
    let findings = findings(&raw, &source, relocated.as_deref())?;
    let entries = exceptions()?;
    let approvals: Vec<_> = findings
        .iter()
        .map(|finding| {
            entries.iter().find(|entry| {
                entry.matches(repository, &finding.path, &finding.rule, &finding.hash)
            })
        })
        .collect();
    reports(&job, &findings, &approvals, flag("SARIF_REPORTS")?)?;
    for (finding, approval) in findings.iter().zip(&approvals) {
        let status = if approval.is_some() {
            "excepted"
        } else {
            "finding"
        };
        println!(
            "{status}: {:?}:{} rule={:?} sha256={}",
            finding.path, finding.start, finding.rule, finding.hash
        );
        if let Some(entry) = approval {
            println!(
                "  approved by {} on {}: {}",
                entry.approved_by, entry.approved_on, entry.reason
            );
        }
    }
    for entry in &entries {
        if entry.repository == repository
            && !findings.iter().any(|finding| {
                entry.matches(repository, &finding.path, &finding.rule, &finding.hash)
            })
        {
            println!(
                "::warning::stale secret-scan exception: {} rule={} sha256={}",
                entry.path, entry.rule, entry.sha256
            );
        }
    }
    if approvals.iter().any(Option::is_none) {
        return Err(Failure::status(1));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{repository, scan};
    use std::env;

    #[test]
    fn only_hosted_nonempty_identity_can_activate_approvals() {
        assert_eq!(repository(true, "owner/repo").unwrap(), "owner/repo");
        assert!(repository(true, "").is_err());
        assert_eq!(repository(false, "owner/repo").unwrap(), "");
        assert_eq!(repository(false, "").unwrap(), "");
    }

    #[test]
    fn scanner_execution_errors_never_become_clean_reports() {
        let missing = env::temp_dir().join("missing-secret-scan-config");
        let error = scan(&missing, &missing, &missing).unwrap_err();
        assert_eq!(error.code, 1);
        assert!(error.message.is_none());
    }
}

//! `rust-gate licenses`: DEP-001, the organization's dependency policy, for
//! every repository: one version of each crate, no wildcard requirement,
//! crates.io alone, no yanked or unmaintained crate, and the licences the
//! organization reviewed, with the `LICENSE_ALLOWLIST` organization variable's
//! licences added when administrators set it. The gate renders it at run time
//! with the duplicate versions `maestro-quality.toml` excuses and the
//! organization's own git repositories it allows; a repository does not
//! commit a `deny.toml` of its own, and one that does is refused.
//! No repository opts out: `validate` refuses `license-policy: off`.

use crate::checks::organization_config::is_generated;
use crate::checks::quality_config::{QualityConfig, read_config};
use crate::runner::{Cmd, Failure, Job, Outcome, Step, input, path, tee_line};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

/// What this step declares: its inputs, its tools and its reports.
pub(crate) const STEPS: &[Step] = &[Step {
    workflow: "ci",
    id: "licenses",
    summary: "Licence, dependency-ban and source policy",
    inputs: &["DENY_CONFIG", "GITHUB_WORKSPACE", "LICENSE_ALLOWLIST"],
    tools: &["cargo deny", "jaq"],
    reports: &["licenses.txt"],
    run,
}];

/// The licences the organization reviewed.
const LICENCES: [&str; 5] = ["Apache-2.0", "MIT", "MIT-0", "Unicode-3.0", "Unlicense"];

/// The one git source DEP-001 allows: a repository of the organization.
const ORGANIZATION_GIT: &str = "https://github.com/Orchestration-Maestro/";

/// DEP-001's cargo-deny policy after its licence list, up to the git
/// repositories it allows.
const POLICY: &str = concat!(
    "confidence-threshold = 0.93\n",
    "unused-allowed-license = \"allow\"\n",
    "private = { ignore = true }\n",
    "\n",
    "[advisories]\n",
    "yanked = \"deny\"\n",
    "unmaintained = \"all\"\n",
    "\n",
    "[sources]\n",
    "unknown-registry = \"deny\"\n",
    "unknown-git = \"deny\"\n",
    "allow-registry = [\"https://github.com/rust-lang/crates.io-index\"]\n",
);

/// DEP-001's bans, up to the duplicate versions it excuses.
const BANS: &str = concat!(
    "\n",
    "[bans]\n",
    "multiple-versions = \"deny\"\n",
    "wildcards = \"deny\"\n",
);

/// Run the step.
fn run() -> Outcome {
    let job = Job::current()?;
    let report = job.report("licenses.txt")?;
    let committed = input("DENY_CONFIG")?;
    if !committed.is_empty() && !is_generated(Path::new(&committed)) {
        return Err(
            "deny.toml: the organization's DEP-001 policy applies to every repository; \
                    delete deny.toml and take exceptions in maestro-quality.toml"
                .into(),
        );
    }
    let allowlist = input("LICENSE_ALLOWLIST")?;
    if !allowlist.is_empty() && !is_spdx_list(&allowlist) {
        return Err("LICENSE_ALLOWLIST must be comma-separated SPDX identifiers".into());
    }
    let added: Vec<&str> = allowlist.split(',').filter(|id| !id.is_empty()).collect();
    let config = read_config(&path("GITHUB_WORKSPACE")?)?;
    let policy = job.temp.join("organization-deny.toml");
    fs::write(&policy, deny_policy(&config, &added)?)
        .map_err(|error| format!("cannot write {}: {error}", policy.display()))?;
    tee_line(
        "policy: the organization's DEP-001, rendered by the gate: one version of each crate, \
         no wildcard, crates.io alone, no yanked or unmaintained crate, reviewed licences",
        &report,
        false,
    )?;
    if !added.is_empty() {
        tee_line(
            &format!("organization licence allowlist: {allowlist}"),
            &report,
            true,
        )?;
    }
    // cargo-audit reports published vulnerabilities; `advisories` is here
    // for what it does not cover, a crate yanked from the registry.
    Cmd::new("cargo deny --config")
        .arg(&policy)
        .args(["check", "licenses", "bans", "sources", "advisories"])
        .cwd(&job.project)
        .tee(&report, true)
}

/// DEP-001's policy: the reviewed licences and the `added` ones; as
/// cargo-deny's `allow-git`, the exceptions whose `path` is a repository of
/// the organization; then, as its skips, the duplicate versions
/// `maestro-quality.toml` excuses, `path` naming the crate and its version,
/// `windows-sys@0.52`. A `path` holding `:` or `/` names a source, never a
/// crate, and one outside the organization is refused.
fn deny_policy(config: &QualityConfig, added: &[&str]) -> Result<String, Failure> {
    let mut text = String::from("[graph]\nall-features = true\n\n[licenses]\nallow = [\n");
    let extra = added.iter().filter(|id| !LICENCES.contains(id));
    for licence in LICENCES.iter().chain(extra) {
        let _ = writeln!(text, "  \"{licence}\",");
    }
    text.push_str("]\n");
    text.push_str(POLICY);
    let (sources, skips): (Vec<_>, Vec<_>) = config
        .exceptions
        .iter()
        .filter(|exception| exception.rule == "DEP-001")
        .partition(|exception| exception.path.contains([':', '/']));
    if !sources.is_empty() {
        text.push_str("allow-git = [\n");
        for exception in sources {
            if !is_organization_repository(&exception.path) {
                return Err(format!(
                    "maestro-quality.toml: the DEP-001 exception {} is not a repository of the \
                     organization; DEP-001 allows only {ORGANIZATION_GIT}<name>",
                    exception.path
                )
                .into());
            }
            let _ = writeln!(text, "  \"{}\",", exception.path);
        }
        text.push_str("]\n");
    }
    text.push_str(BANS);
    if !skips.is_empty() {
        text.push_str("skip = [\n");
        for exception in skips {
            let reason = exception.reason.replace('\\', "\\\\").replace('"', "\\\"");
            let _ = writeln!(
                text,
                "  {{ crate = \"{}\", reason = \"{reason}\" }},",
                exception.path
            );
        }
        text.push_str("]\n");
    }
    Ok(text)
}

/// An https URL of one repository of the organization: a name after
/// `ORGANIZATION_GIT` that starts with a letter or digit and holds only
/// letters, digits, `-`, `_` and `.`, so it needs no escaping in TOML.
fn is_organization_repository(url: &str) -> bool {
    url.strip_prefix(ORGANIZATION_GIT).is_some_and(|name| {
        name.starts_with(|character: char| character.is_ascii_alphanumeric())
            && name
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "-_.".contains(character))
    })
}

/// Comma-separated SPDX identifiers: letters, digits, dot, plus, space and
/// hyphen, none of them empty.
fn is_spdx_list(value: &str) -> bool {
    value.split(',').all(|id| {
        !id.is_empty()
            && id
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || ".+ -".contains(character))
    })
}

#[cfg(test)]
mod tests {
    use super::{deny_policy, is_spdx_list};
    use crate::checks::quality_config::{Exception, QualityConfig};

    #[test]
    fn allowlists_are_comma_separated_identifiers() {
        assert!(is_spdx_list(
            "MIT,Apache-2.0,Apache-2.0 WITH LLVM-exception"
        ));
        for bad in ["", "MIT;GPL-3.0", "MIT,", ",MIT", "MIT\"", "MIT\n"] {
            assert!(!is_spdx_list(bad), "{bad:?}");
        }
    }

    #[test]
    fn the_policy_adds_listed_licences_and_excused_duplicates() {
        let plain = deny_policy(&QualityConfig::default(), &[]).unwrap();
        assert!(plain.starts_with(
            "[graph]\nall-features = true\n\n[licenses]\nallow = [\n  \"Apache-2.0\",\n"
        ));
        assert!(plain.ends_with("multiple-versions = \"deny\"\nwildcards = \"deny\"\n"));
        let config = QualityConfig {
            exceptions: vec![Exception {
                rule: "DEP-001".to_owned(),
                path: "windows-sys@0.52".to_owned(),
                item: String::new(),
                reason: "two \"platform\" crates".to_owned(),
            }],
            ..QualityConfig::default()
        };
        let text = deny_policy(&config, &["MIT", "ISC"]).unwrap();
        assert_eq!(text.matches("\"MIT\"").count(), 1);
        assert!(text.contains("  \"Unlicense\",\n  \"ISC\",\n]\n"), "{text}");
        assert!(text.ends_with(
            "skip = [\n  { crate = \"windows-sys@0.52\", reason = \"two \\\"platform\\\" \
             crates\" },\n]\n"
        ));
    }

    #[test]
    fn an_organization_repository_renders_as_an_allowed_git_source() {
        let exception = |path: &str| Exception {
            rule: "DEP-001".to_owned(),
            path: path.to_owned(),
            item: String::new(),
            reason: "a fork".to_owned(),
        };
        let config = QualityConfig {
            exceptions: vec![
                exception("https://github.com/Orchestration-Maestro/lbug"),
                exception("windows-sys@0.52"),
                exception("https://github.com/Orchestration-Maestro/lbug.git"),
            ],
            ..QualityConfig::default()
        };
        let text = deny_policy(&config, &[]).unwrap();
        assert!(
            text.contains(
                "allow-git = [\n  \"https://github.com/Orchestration-Maestro/lbug\",\n  \
                 \"https://github.com/Orchestration-Maestro/lbug.git\",\n]\n\n[bans]\n"
            ),
            "{text}"
        );
        assert!(
            text.ends_with(
                "skip = [\n  { crate = \"windows-sys@0.52\", reason = \"a fork\" },\n]\n"
            ),
            "{text}"
        );
        for outside in [
            "https://github.com/Orchestration-Maestro/",
            "https://github.com/Orchestration-Maestro/.hidden",
            "https://github.com/Orchestration-Maestro/a\"b",
            "https://github.com/Orchestration-Maestro/lbug?rev=1",
            "https://github.com/Orchestration-Maestro-evil/lbug",
            "git@github.com:Orchestration-Maestro/lbug",
        ] {
            let config = QualityConfig {
                exceptions: vec![exception(outside)],
                ..QualityConfig::default()
            };
            assert!(deny_policy(&config, &[]).is_err(), "{outside}");
        }
    }
}

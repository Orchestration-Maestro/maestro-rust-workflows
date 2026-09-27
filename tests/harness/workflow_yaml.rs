//! Readers of the workflow and action YAML: whole documents, one step's body,
//! the tool rows a step installs, and `jaq` queries for anything else.

use super::repository::{root, tool};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

pub(crate) fn yaml(path: PathBuf) -> Value {
    assert!(path.is_file(), "Missing file: {}", path.display());
    let output = tool("jaq")
        .args(["--from", "yaml", "."])
        .arg(path)
        .output()
        .expect("pinned jaq must be installed: just setup");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("jaq must emit valid JSON")
}

pub(crate) fn workflow(name: &str) -> Value {
    yaml(root().join(format!(".github/workflows/{name}.yml")))
}

/// Every job of every workflow, sorted by workflow: the workflow's file
/// stem, the job's id and the job.
fn workflow_jobs() -> Vec<(String, String, Value)> {
    let mut names: Vec<String> = fs::read_dir(root().join(".github/workflows"))
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            path.file_stem().unwrap().to_string_lossy().into_owned()
        })
        .collect();
    names.sort();
    let mut jobs = Vec::new();
    for name in names {
        let data = workflow(&name);
        for (id, job) in data["jobs"].as_object().into_iter().flatten() {
            jobs.push((name.clone(), id.clone(), job.clone()));
        }
    }
    jobs
}

/// Every step of every workflow, in job order: the workflow's file stem, the
/// job's id and the step.
pub(crate) fn workflow_steps() -> Vec<(String, String, Value)> {
    workflow_jobs()
        .into_iter()
        .flat_map(|(name, id, job)| {
            let steps = job["steps"].as_array().cloned().unwrap_or_default();
            steps
                .into_iter()
                .map(move |step| (name.clone(), id.clone(), step))
        })
        .collect()
}

/// An action shipped in this repository, under `.github/actions/`.
pub(crate) fn action(name: &str) -> Value {
    yaml(root().join(format!(".github/actions/{name}/action.yml")))
}

/// One line of a `rust-gate install-tools` table: the executable's name, its
/// GitHub release asset path, the digest, and the member inside an archive.
pub(crate) struct ToolRow {
    pub(crate) name: String,
    pub(crate) asset: String,
    pub(crate) digest: String,
    pub(crate) member: Option<String>,
}

/// The table a `rust-gate install-tools` step installs, parsed once for every
/// reader; empty for any other step. A malformed line is a defect in the
/// workflow.
pub(crate) fn tool_rows(step: &Value) -> Vec<ToolRow> {
    let is_install = step["run"]
        .as_str()
        .is_some_and(|run| run.trim() == "rust-gate install-tools");
    if !is_install {
        return Vec::new();
    }
    step["env"]["TOOLS"]
        .as_str()
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
        .map(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            assert!(
                words.len() == 3 || words.len() == 4,
                "malformed tool line: {line}"
            );
            ToolRow {
                name: words[0].to_owned(),
                asset: words[1].to_owned(),
                digest: words[2].to_owned(),
                member: words.get(3).map(|member| (*member).to_owned()),
            }
        })
        .collect()
}

/// Read one step's shell body straight out of the workflow it lives in.
pub(crate) fn step(name: &str, id: &str) -> String {
    let workflow = workflow(name);
    let found = workflow["jobs"]
        .as_object()
        .unwrap()
        .values()
        .filter_map(|job| job["steps"].as_array())
        .flatten()
        .find(|step| step["id"] == id)
        .unwrap_or_else(|| panic!("Missing executable boundary: {name}/{id}"))
        .clone();
    found["run"].as_str().unwrap().to_owned()
}

/// The `ci.yml` steps the example gate replays, in workflow order. The gate
/// and the North Star test read this one list.
pub(crate) const GATE_STEPS: [&str; 12] = [
    "quality",
    "complexity",
    "duplication",
    "coverage",
    "mutants-plan",
    "mutants",
    "msrv",
    "features",
    "unused",
    "build",
    "hardening",
    "stage",
];

/// One jaq query over a YAML or TOML file, as the repository tests read
/// metadata. jaq needs the format spelled out, and the file extension gives it.
pub(crate) fn query(path: &Path, expression: &str) -> String {
    let format = if path.extension().is_some_and(|kind| kind == "toml") {
        "toml"
    } else {
        "yaml"
    };
    let output = tool("jaq")
        .args(["--from", format, "-r", expression])
        .arg(path)
        .output()
        .expect("install the pinned jaq with the development toolbelt");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

//! GitHub and runner default variables cannot be overwritten by workflow environment keys.

use crate::harness::{root, workflow};
use std::fs;
use std::iter;

#[test]
fn workflows_never_override_github_or_runner_default_environment() {
    let mut violations = Vec::new();
    for entry in fs::read_dir(root().join(".github/workflows")).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_stem().unwrap().to_str().unwrap();
        let data = workflow(name);
        let jobs = data["jobs"].as_object().unwrap();
        let items = iter::once(&data).chain(jobs.values()).chain(
            jobs.values()
                .flat_map(|job| job["steps"].as_array().into_iter().flatten()),
        );
        violations.extend(
            items
                .flat_map(|item| {
                    item["env"]
                        .as_object()
                        .into_iter()
                        .flat_map(|env| env.keys())
                })
                .filter(|key| key.starts_with("GITHUB_") || key.starts_with("RUNNER_"))
                .map(|key| format!("{name}: {key}")),
        );
    }
    assert!(
        violations.is_empty(),
        "reserved workflow env: {violations:?}"
    );
}

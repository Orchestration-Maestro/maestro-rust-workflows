//! Shared real A/B/C source and Git setup for mode and input-only ownership regressions.

use super::fixture::succeeds;
use super::repository::tool;
use std::path::Path;
use workflow_contract_tests::engine_workspace as generate_workspace;

/// Run real Git, never a fixture stand-in, and return its trimmed output.
pub(crate) fn fixture_git(project: &Path, args: &[&str]) -> String {
    let result = tool("git")
        .args(args)
        .current_dir(project)
        .output()
        .unwrap();
    succeeds(&result);
    String::from_utf8(result.stdout).unwrap().trim().to_owned()
}

/// Keep the fixture constructor's existing infallible test API.
pub(crate) fn engine_workspace(project: &Path, killing: bool) {
    generate_workspace(project, killing).unwrap();
}

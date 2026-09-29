//! The existing first-parent scope, named relative to the checkout and hashed
//! with the files that affect mutation selection or execution.

use crate::checks::checkout_paths::canonical;
use crate::checks::digests::sha256_hex;
use crate::checks::quality_config::mutation_windows;
use crate::runner::{Cmd, Failure, Job, input, optional};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The exact source scope every listing and worker must use.
pub(super) struct Scope {
    /// The first parent, absent only for a parentless commit.
    pub(super) parent: Option<String>,
    /// The saved first-parent diff, absent for a parentless commit.
    pub(super) diff: Option<PathBuf>,
    /// The human-readable change kind, absent for a full-workspace run.
    pub(super) change: Option<String>,
    /// SHA-256 of the diff, or the explicit full-workspace marker.
    pub(super) diff_digest: String,
}

/// Recreate the current scope and preserve its diff in both runner temp and reports.
pub(super) fn prepare(job: &Job, base: &str) -> Result<Scope, Failure> {
    let parent = match Cmd::new("git rev-parse --verify -q HEAD^1")
        .cwd(&job.project)
        .capture()
    {
        Ok(parent) => {
            let parent = parent.trim().to_owned();
            if !valid_sha(&parent) {
                return Err("git returned an invalid first-parent SHA".into());
            }
            Some(parent)
        }
        Err(failure) if failure.code == 1 && failure.message.is_none() => None,
        Err(failure) => return Err(failure),
    };
    if !base.is_empty() && parent.is_none() {
        return Err("pull request checkout must include the base parent".into());
    }
    let (diff, change, diff_digest) = if parent.is_some() {
        let diff = job.temp.join("mutants.diff");
        Cmd::new("git diff --relative HEAD^1 HEAD -- .")
            .cwd(&job.project)
            .stdout_to(&diff)?;
        fs::copy(&diff, job.report("mutants.diff")?)
            .map_err(|error| format!("cannot preserve the mutation diff: {error}"))?;
        let bytes =
            fs::read(&diff).map_err(|error| format!("cannot read {}: {error}", diff.display()))?;
        let change = if base.is_empty() {
            "this commit"
        } else {
            "this pull request"
        };
        (Some(diff), Some(change.to_owned()), sha256_hex(&bytes))
    } else {
        (None, None, sha256_hex(b"full-workspace;no-first-parent"))
    };
    Ok(Scope {
        parent,
        diff,
        change,
        diff_digest,
    })
}

/// Exclude every Windows-owned file from the Linux mutation listing or run.
pub(super) fn exclude_windows_files(mut command: Cmd, project: &Path) -> Result<Cmd, Failure> {
    for file in mutation_windows(project, &optional("MUTATION_WINDOWS")?)? {
        command = command.args(["--exclude", &file]);
    }
    Ok(command)
}

/// The project path the checks and worker can both independently resolve.
pub(super) fn normalized_directory() -> Result<String, Failure> {
    let root = canonical(Path::new(&input("GITHUB_WORKSPACE")?))?;
    let project = canonical(Path::new(&input("PROJECT")?))?;
    match project.strip_prefix(&root) {
        Ok(rest) if rest.as_os_str().is_empty() => Ok(".".to_owned()),
        Ok(rest) => Ok(rest.display().to_string()),
        Err(_) => Err("working-directory escapes checkout".into()),
    }
}

/// Digest inputs that can change the mutant set, compiler or test execution.
pub(super) fn configuration_digest(project: &Path) -> Result<String, Failure> {
    let mut bytes = Vec::new();
    for name in [
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        ".cargo/config.toml",
        ".cargo/mutants.toml",
    ] {
        bytes.extend_from_slice(name.as_bytes());
        bytes.push(0);
        match fs::read(project.join(name)) {
            Ok(contents) => {
                bytes.extend_from_slice(
                    &u64::try_from(contents.len())
                        .unwrap_or(u64::MAX)
                        .to_be_bytes(),
                );
                bytes.extend_from_slice(&contents);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                bytes.extend_from_slice(&0_u64.to_be_bytes());
            }
            Err(error) => return Err(format!("cannot read {name}: {error}").into()),
        }
        bytes.push(0);
    }
    Ok(sha256_hex(&bytes))
}

/// A full Git SHA in either supported object format.
fn valid_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::valid_sha;

    #[test]
    fn full_git_sha_formats_are_accepted_and_abbreviations_are_refused() {
        assert!(valid_sha(&"a".repeat(40)));
        assert!(valid_sha(&"A".repeat(64)));
        for value in ["", "abc", &"g".repeat(40), &"a".repeat(41)] {
            assert!(!valid_sha(value), "{value}");
        }
    }
}

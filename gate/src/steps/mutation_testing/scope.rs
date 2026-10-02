//! The first-parent or explicit full-project scope, named relative to the checkout and hashed
//! with the files that affect mutation selection or execution.

use crate::checks::checkout_paths::canonical;
use crate::checks::digests::sha256_hex;
use crate::checks::mutation_engine;
use crate::checks::native_cache::{NativeCache, native_cache_command};
use crate::checks::quality_config::mutation_windows;
use crate::runner::{Cmd, Failure, Job, input, optional};
use std::collections::BTreeSet;
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

/// Marker binding an explicitly requested full-project run to its workers.
pub(super) const FULL_SCOPE: &[u8] = b"full-workspace;explicit";

/// Read the optional full-project selection without changing the reusable CI default.
pub(super) fn full_scope() -> Result<bool, Failure> {
    match optional("MUTATION_FULL_SCOPE")?.as_str() {
        "" | "false" => Ok(false),
        "true" => Ok(true),
        _ => Err("mutation-full-scope must be true or false".into()),
    }
}

/// Recreate the current scope and preserve its diff in both runner temp and reports.
pub(super) fn prepare(job: &Job, base: &str) -> Result<Scope, Failure> {
    let full = full_scope()?;
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
    let (diff, change, diff_digest) = if full {
        (None, None, sha256_hex(FULL_SCOPE))
    } else if parent.is_some() {
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

/// Exclude every Windows-owned file from a Linux mutation listing or run.
pub(super) fn exclude_windows_files(mut command: Cmd, project: &Path) -> Result<Cmd, Failure> {
    for file in mutation_windows(project, &optional("MUTATION_WINDOWS")?)? {
        command = command.args(["--exclude", &file]);
    }
    Ok(command)
}

/// Exclude Windows and engine-owned files from the featureless default mutation mode.
pub(super) fn exclude_default_files(command: Cmd, project: &Path) -> Result<Cmd, Failure> {
    let mut command = exclude_windows_files(command, project)?;
    for file in engine_files()? {
        command = command.args(["--exclude", &file]);
    }
    Ok(command)
}

/// Restrict a feature-enabled engine listing or run to its exact owned files.
pub(super) fn engine_selection(
    mut command: Cmd,
    project: &Path,
    cache: Option<&NativeCache>,
) -> Result<Cmd, Failure> {
    let json = Cmd::new("jaq -cn")
        .args([
            "--argjson",
            "features",
            &optional("MUTATION_ENGINE_FEATURES")?,
        ])
        .args(["--argjson", "files", &optional("MUTATION_ENGINE_FILES")?])
        .arg("{features:$features, files:$files}")
        .capture()?;
    let policy = mutation_engine::engine_policy(project, &json, &[], || {
        native_cache_command(
            cache,
            "cargo metadata --format-version 1 --no-deps --locked",
        )
        .cwd(project)
        .capture()
    })?;
    let features = policy.features;
    let files = policy.files;
    if !features.is_empty() {
        command = command.args(["--features", &features.join(",")]);
    }
    for package in selected_packages(project, &files, cache)? {
        command = command.args(["--package", &package]);
    }
    for file in files {
        command = command.args(["--file", &file]);
    }
    Ok(command)
}

/// Select only packages that own engine files so package-local features stay local.
pub(super) fn engine_packages(project: &Path, files: &[String]) -> Result<Vec<String>, Failure> {
    selected_packages(project, files, None)
}

/// Execution metadata is isolated; pre-validation metadata preserves its environment.
fn selected_packages(
    project: &Path,
    files: &[String],
    cache: Option<&NativeCache>,
) -> Result<Vec<String>, Failure> {
    if files.is_empty() {
        return Ok(Vec::new());
    }
    let metadata = native_cache_command(cache, "cargo metadata --format-version 1 --no-deps")
        .cwd(project)
        .capture()?;
    let rows = Cmd::new("jaq -r")
        .arg(".packages[] | [.name, .manifest_path] | @tsv")
        .stdin_bytes(metadata.as_bytes())
        .capture()?;
    let packages = rows
        .lines()
        .filter_map(|row| {
            let (name, manifest) = row.split_once('\t')?;
            Some((name, Path::new(manifest).parent()?.to_path_buf()))
        })
        .collect::<Vec<_>>();
    let mut owners = BTreeSet::new();
    for file in files {
        let path = project.join(file);
        let owner = packages
            .iter()
            .filter(|(_, root)| path.starts_with(root))
            .max_by_key(|(_, root)| root.components().count());
        let Some((name, _)) = owner else {
            return Err(format!("engine mutation file `{file}` has no Cargo package owner").into());
        };
        owners.insert((*name).to_owned());
    }
    Ok(owners.into_iter().collect())
}

/// Decode one output list as strings, allowing an unconfigured empty partition.
pub(super) fn json_strings(name: &str) -> Result<Vec<String>, Failure> {
    let value = optional(name)?;
    let value = if value.is_empty() { "[]" } else { &value };
    let listed = Cmd::new("jaq -nr")
        .env("MUTATION_ENGINE_LIST", &value)
        .arg(concat!(
            "$ENV.MUTATION_ENGINE_LIST | fromjson | if type == \"array\" ",
            "and all(.[]; type == \"string\") then .[] ",
            "else error(\"expected string array\") end"
        ))
        .capture()
        .map_err(|_| format!("{name} must be a JSON array of strings"))?;
    Ok(listed.lines().map(str::to_owned).collect())
}

/// Exact paths transferred away from the featureless default mutation partition.
pub(super) fn transferred_files(project: &Path) -> Result<BTreeSet<String>, Failure> {
    let mut files: BTreeSet<String> = mutation_windows(project, &optional("MUTATION_WINDOWS")?)?
        .into_iter()
        .collect();
    files.extend(engine_files()?);
    Ok(files)
}

/// Whether the current run owns a nonempty engine partition.
pub(super) fn has_engine_files() -> Result<bool, Failure> {
    Ok(!engine_files()?.is_empty())
}

/// The exact engine-owned file list, empty when the feature partition is disabled.
pub(super) fn engine_files() -> Result<Vec<String>, Failure> {
    json_strings("MUTATION_ENGINE_FILES")
}

/// The project path the checks and worker can both independently resolve.
pub(super) fn normalized_directory() -> Result<String, Failure> {
    let root = canonical(Path::new(&input("GITHUB_WORKSPACE")?))?;
    let project = canonical(Path::new(&input("PROJECT")?))?;
    match project.strip_prefix(&root) {
        Ok(rest) if rest.as_os_str().is_empty() => Ok(".".to_owned()),
        Ok(rest) => Ok(rest.display().to_string().replace('\\', "/")),
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
        "maestro-quality.toml",
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

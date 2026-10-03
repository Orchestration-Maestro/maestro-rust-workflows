//! The first-parent or explicit full-project scope, named relative to the checkout and hashed
//! with the files that affect mutation selection or execution.

use crate::checks::checkout_paths::canonical;
use crate::checks::digests::sha256_hex;
use crate::checks::native_cache::{NativeCache, native_cache_command};
use crate::checks::quality_config::mutation_windows;
use crate::checks::{mutation_engine, mutation_host};
use crate::runner::{Cmd, Failure, Job, input, optional};
use std::collections::BTreeSet;
use std::fmt::Write as _;
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
        transferred_host_diff(job, &diff)?;
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

/// A policy-only transfer restores the still-existing file's complete new-owner obligation.
fn transferred_host_diff(job: &Job, diff: &Path) -> Result<(), Failure> {
    let mut text =
        fs::read_to_string(diff).map_err(|error| format!("cannot read mutation diff: {error}"))?;
    for file in returned_host_files(job)? {
        let path = job.project.join(&file);
        if !path.is_file() {
            continue;
        }
        let source = fs::read_to_string(&path).map_err(|error| format!("{file}: {error}"))?;
        let _ = writeln!(
            text,
            "diff --git a/{file} b/{file}\n--- /dev/null\n+++ b/{file}"
        );
        let _ = writeln!(text, "@@ -0,0 +1,{} @@", source.lines().count());
        for line in source.lines() {
            let _ = writeln!(text, "+{line}");
        }
    }
    fs::write(diff, text)
        .map_err(|error| format!("cannot preserve transferred host diff: {error}").into())
}

/// Existing files whose previous host ownership has been removed require full new-owner tests.
pub(super) fn returned_host_files(job: &Job) -> Result<Vec<String>, Failure> {
    let root = PathBuf::from(input("GITHUB_WORKSPACE")?);
    if !root.join(".git").exists() && !job.project.join(".git").exists() {
        return Ok(Vec::new());
    }
    let parent = Cmd::new("git rev-parse --verify -q HEAD^1")
        .cwd(&job.project)
        .capture();
    match parent {
        Err(failure) if failure.code == 1 && failure.message.is_none() => return Ok(Vec::new()),
        Err(failure) => return Err(failure),
        Ok(_) => {}
    }
    let previous = Cmd::new("git ls-tree --name-only HEAD^1 -- maestro-quality.toml")
        .cwd(&job.project)
        .capture()?;
    if previous.trim().is_empty() {
        return Ok(Vec::new());
    }
    let prefix = Cmd::new("git rev-parse --show-prefix")
        .cwd(&job.project)
        .capture()?;
    let base = Cmd::new("git show")
        .arg(format!("HEAD^1:{}maestro-quality.toml", prefix.trim()))
        .cwd(&job.project)
        .capture()?;
    let old = Cmd::new("jaq --from toml -r")
        .arg(".ci[\"mutation-provisioned-host\"].files // [] | .[]")
        .stdin_bytes(base.as_bytes())
        .capture()?;
    let current = host_files(&job.project)?;
    Ok(old
        .lines()
        .filter(|file| !current.iter().any(|name| name == file) && job.project.join(file).is_file())
        .map(str::to_owned)
        .collect())
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
    let mut command = exclude_host_files(exclude_windows_files(command, project)?, project)?;
    for file in engine_files()? {
        command = command.args(["--exclude", &file]);
    }
    Ok(command)
}

/// Keep host-owned source out of both raw engine/control discoveries and ordinary runs.
pub(super) fn exclude_host_files(mut command: Cmd, project: &Path) -> Result<Cmd, Failure> {
    for file in host_files(project)? {
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
    files.extend(host_files(project)?);
    Ok(files)
}

/// Host-owned files transfer to required full-file host discovery.
fn host_files(project: &Path) -> Result<Vec<String>, Failure> {
    Ok(mutation_host::host_policy(project)?.map_or_else(Vec::new, |policy| policy.files))
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

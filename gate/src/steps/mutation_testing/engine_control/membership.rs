//! Retain and verify the default compiler's dep-info, bound to the worker receipt.

use super::{
    dep_info::{dep_paths, dependencies, normalized},
    source,
};
use crate::checks::digests::sha256_hex;
use crate::checks::native_cache::{NativeCache, native_cache_command};
use crate::runner::{Cmd, Failure, Job, Outcome, tee_line, write};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Match pinned cargo-mutants 27.1.0 default Cargo build arguments before instrumentation.
const BUILD_COMMAND: &str = "cargo test --no-run --verbose";

/// Build into a fresh target tree; never infer membership from stale cached dep-info.
pub(super) fn build(
    job: &Job,
    root: &Path,
    receipt: &Path,
    assigned: &Path,
    policy: Option<&NativeCache>,
) -> Outcome {
    let metadata = native_cache_command(
        policy,
        "cargo metadata --format-version 1 --no-deps --locked",
    )
    .cwd(&job.project)
    .capture()?;
    let workspace = Cmd::new("jaq -er")
        .arg(".workspace_root")
        .stdin_bytes(metadata.as_bytes())
        .capture()?;
    let workspace = Path::new(workspace.trim());
    source::retain(job, root, &metadata)?;
    let package = source::package(root, assigned)?;
    let config = workspace.join(".cargo/mutants.toml");
    let configuration = if config.exists() {
        fs::read(&config)
            .map_err(|error| format!("cannot retain compile configuration: {error}"))?
    } else {
        Vec::new()
    };
    write(&root.join("compile-config.toml"), &configuration, false)?;
    let fallback = compilation_config(&config) || package.is_none() || job.project != workspace;
    if fallback {
        tee_line(
            concat!(
                "Compile membership fallback: package-scoped compile equivalence is unverified; ",
                "testing every assigned mutant"
            ),
            &job.report("mutants-engine-default.txt")?,
            true,
        )?;
        return save_binding(root, receipt, (&job.project, workspace), None);
    }
    let target = job.temp.join("engine-control-target");
    if target.exists() {
        fs::remove_dir_all(&target)
            .map_err(|error| format!("cannot clear control target: {error}"))?;
    }
    compile(
        job,
        (root, receipt),
        policy,
        workspace,
        &package.unwrap_or_default(),
    )
}

/// Instrument the equivalent single-package command without altering compiler flag resolution.
fn compile(
    job: &Job,
    records: (&Path, &Path),
    policy: Option<&NativeCache>,
    workspace: &Path,
    package: &str,
) -> Outcome {
    let (root, receipt) = records;
    let target = job.temp.join("engine-control-target");
    let started = Cmd::new("jaq -nr").arg("now | todateiso8601").capture()?;
    let clock = Instant::now();
    let result = native_cache_command(policy, "timeout --kill-after=1m 30m")
        .args(BUILD_COMMAND.split_whitespace())
        .arg(format!("--package={package}"))
        .arg("--locked")
        .arg("--message-format=json")
        .arg("--target-dir")
        .arg(&target)
        .env("INSTA_UPDATE", "no")
        .env("INSTA_FORCE_PASS", "0")
        .cwd(&job.project)
        .capture_output()?;
    write(&root.join("cargo-build.json"), &result.stdout, false)?;
    write(&root.join("cargo-build.log"), &result.stderr, false)?;
    if !result.status.success() {
        return Err(Failure::status(result.status.code().unwrap_or(1)));
    }
    let elapsed = clock.elapsed().as_secs_f64();
    let build = Cmd::new("jaq -cn")
        .args(["--arg", "started", started.trim()])
        .args(["--argjson", "elapsed", &elapsed.to_string()])
        .args(["--arg", "target"])
        .arg(&target)
        .args(["--arg", "package", package])
        .arg(concat!(
            "{start_time:$started, end_time:(now | todateiso8601), duration:$elapsed, ",
            r#"argv:["cargo","test","--no-run","--verbose",("--package="+$package),"--locked","#,
            r#""--message-format=json","--target-dir",$target]}"#
        ))
        .capture()?;
    write(&root.join("build-record.json"), build.as_bytes(), false)?;
    let paths = dep_paths(&root.join("cargo-build.json"))?;
    let directory = root.join("dep-info");
    fs::create_dir_all(&directory).map_err(|error| format!("cannot retain dep-info: {error}"))?;
    for (index, path) in paths.iter().enumerate() {
        let path = Path::new(path);
        if !path.starts_with(&target) {
            return Err("featureless build dep-info escapes its clean target".into());
        }
        fs::copy(path, directory.join(format!("{index}.d")))
            .map_err(|error| format!("cannot retain featureless dep-info: {error}"))?;
    }
    save_binding(root, receipt, (&job.project, workspace), Some(&target))
}

/// Unknown config keys conservatively disable absence classification as well.
fn compilation_config(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }
    let safe = Cmd::new("jaq --from toml -e")
        .arg(concat!(
            "(.cap_lints // false) == false and (.test_tool // \"cargo\") == \"cargo\" and ",
            "(keys | all(.[]; . == \"cap_lints\" or . == \"test_tool\" or ",
            ". == \"exclude_globs\" or ",
            ". == \"examine_globs\" or . == \"exclude_re\" or . == \"examine_re\" or ",
            ". == \"skip_calls\" or . == \"skip_calls_defaults\" or ",
            ". == \"timeout_multiplier\" or . == \"minimum_test_timeout\" or ",
            ". == \"build_timeout_multiplier\" or . == \"build_timeout\" or ",
            ". == \"timeout\"))"
        ))
        .arg(path)
        .capture();
    safe.is_err()
}

/// Save schema, exact worker identity and each retained compiler record's digest.
fn save_binding(
    root: &Path,
    receipt: &Path,
    source: (&Path, &Path),
    target: Option<&Path>,
) -> Outcome {
    let paths = if target.is_some() {
        dep_paths(&root.join("cargo-build.json"))?
    } else {
        Vec::new()
    };
    let mut digests = Vec::new();
    for index in 0..paths.len() {
        digests.push(digest(&root.join(format!("dep-info/{index}.d")))?);
    }
    let value = Cmd::new("jaq -cn")
        .args(["--slurpfile", "receipt"])
        .arg(receipt)
        .args(["--arg", "project"])
        .arg(source.0)
        .args(["--arg", "workspace"])
        .arg(source.1)
        .args(["--arg", "target"])
        .arg(target.unwrap_or(Path::new("")))
        .args([
            "--arg",
            "cargo_digest",
            &if target.is_some() {
                digest(&root.join("cargo-build.json"))?
            } else {
                String::new()
            },
        ])
        .args([
            "--arg",
            "build_digest",
            &if target.is_some() {
                digest(&root.join("build-record.json"))?
            } else {
                String::new()
            },
        ])
        .args([
            "--arg",
            "source_digest",
            &digest(&root.join("source-metadata.json"))?,
        ])
        .args([
            "--arg",
            "config_digest",
            &digest(&root.join("compile-config.toml"))?,
        ])
        .args(["--argjson", "digests", &strings(&digests)?])
        .arg(concat!(
            "{schema:2, source_sha256:$source_digest, config_sha256:$config_digest, ",
            "binding:$receipt[0], project:$project, workspace:$workspace, ",
            "target:$target, cargo_sha256:$cargo_digest, build_sha256:$build_digest, ",
            "dep_sha256:$digests}"
        ))
        .capture()?;
    write(
        &root.join("compile-membership.json"),
        value.as_bytes(),
        false,
    )
}

/// Absence is trusted only after rechecking the raw evidence and source-bound receipt.
pub(super) fn verify(
    root: &Path,
    receipt: &Path,
    assigned: &Path,
) -> Result<Option<BTreeSet<String>>, Failure> {
    let manifest = root.join("compile-membership.json");
    verify_binding(root, receipt, assigned)?;
    let target = field(&manifest, ".target")?;
    if target.is_empty() {
        Cmd::new("jaq -e")
            .arg(".cargo_sha256 == \"\" and .build_sha256 == \"\" and .dep_sha256 == []")
            .arg(&manifest)
            .capture()
            .map_err(|_| "featureless fallback contains unexpected compile evidence")?;
        return Ok(None);
    }
    if compilation_config(&root.join("compile-config.toml"))
        || field(&manifest, ".project")? != field(&manifest, ".workspace")?
    {
        return Err("featureless compile membership command is not equivalent".into());
    }
    let package = source::package(root, assigned)?
        .ok_or("featureless compile membership command is not equivalent")?;
    let build_record = root.join("build-record.json");
    Cmd::new("jaq -e")
        .args(["--arg", "target", &target])
        .args(["--arg", "package", &package])
        .arg(concat!(
            r#".argv == ["cargo","test","--no-run","--verbose",("--package="+$package),"#,
            r#""--locked","#,
            r#""--message-format=json","--target-dir",$target]"#
        ))
        .arg(&build_record)
        .capture()
        .map_err(|_| "featureless compile membership command is not equivalent")?;
    let cargo = root.join("cargo-build.json");
    if digest(&cargo)? != field(&manifest, ".cargo_sha256")? {
        return Err("featureless compile membership cargo digest differs".into());
    }
    if digest(&root.join("build-record.json"))? != field(&manifest, ".build_sha256")? {
        return Err("featureless compile membership build record digest differs".into());
    }
    let project = PathBuf::from(field(&manifest, ".project")?);
    let workspace = PathBuf::from(field(&manifest, ".workspace")?);
    let digests = field(&manifest, ".dep_sha256[]")?;
    let paths = dep_paths(&cargo)?;
    let digests: Vec<_> = digests.lines().collect();
    if paths.len() != digests.len() {
        return Err("featureless compile membership omits dep-info units".into());
    }
    let mut members = BTreeSet::new();
    for (index, (path, expected_digest)) in paths.iter().zip(digests).enumerate() {
        if !Path::new(path).starts_with(&target) {
            return Err("featureless build dep-info escapes its clean target".into());
        }
        let retained = root.join(format!("dep-info/{index}.d"));
        if digest(&retained)? != expected_digest {
            return Err("featureless compile membership dep-info digest differs".into());
        }
        let text = fs::read_to_string(&retained)
            .map_err(|error| format!("cannot read retained dep-info: {error}"))?;
        for dependency in dependencies(&text)? {
            let path = Path::new(&dependency);
            let path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                workspace.join(path)
            };
            let path = normalized(&path);
            if let Ok(relative) = path.strip_prefix(&project) {
                members.insert(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let sources = Cmd::new("jaq -sr")
        .arg(".[] | select(.reason == \"compiler-artifact\") | .target.src_path")
        .arg(cargo)
        .capture()?;
    for source in sources.lines() {
        if let Ok(relative) = Path::new(source).strip_prefix(&project)
            && !members.contains(&relative.to_string_lossy().replace('\\', "/"))
        {
            return Err("featureless compile membership omits an artifact source".into());
        }
    }
    Ok(Some(members))
}

/// Check the versioned envelope before translating any compiler coordinates.
fn verify_binding(root: &Path, receipt: &Path, assigned: &Path) -> Outcome {
    let manifest = root.join("compile-membership.json");
    if !manifest.is_file() {
        return Err("featureless compile membership evidence is missing".into());
    }
    Cmd::new("jaq -e")
        .args(["--slurpfile", "receipt"])
        .arg(receipt)
        .arg(concat!(
            r#"(keys | sort) == ["binding","build_sha256","cargo_sha256","config_sha256","#,
            r#""dep_sha256","#,
            r#""project","schema","source_sha256","target","workspace"] and "#,
            ".schema == 2 and .binding == $receipt[0] and ",
            "(.target | type == \"string\") and (.dep_sha256 | type == \"array\")"
        ))
        .arg(&manifest)
        .capture()
        .map_err(|_| "featureless compile membership binding differs from its plan")?;
    if digest(&root.join("source-metadata.json"))? != field(&manifest, ".source_sha256")? {
        return Err("featureless compile membership source metadata digest differs".into());
    }
    source::verify(root, receipt, assigned, &manifest)?;
    if digest(&root.join("compile-config.toml"))? != field(&manifest, ".config_sha256")? {
        return Err("featureless compile membership configuration digest differs".into());
    }
    Ok(())
}

/// Read one field with the same pinned JSON reader used for all evidence.
fn field(path: &Path, query: &str) -> Result<String, Failure> {
    Cmd::new("jaq -r")
        .arg(query)
        .arg(path)
        .capture()
        .map(|value| value.trim().to_owned())
}

/// Serialize digest strings without adding a second JSON parser.
fn strings(values: &[String]) -> Result<String, Failure> {
    Cmd::new("jaq -cn")
        .arg("$ARGS.positional")
        .arg("--args")
        .args(values)
        .capture()
}

/// Hash exactly the retained evidence bytes, never a normalized reserialization.
fn digest(path: &Path) -> Result<String, Failure> {
    fs::read(path)
        .map(|bytes| sha256_hex(&bytes))
        .map_err(|error| format!("cannot read compile membership evidence: {error}").into())
}

#[cfg(test)]
mod tests {
    use super::compilation_config;
    use std::{env, fs, process};

    #[test]
    fn compilation_settings_trigger_conservative_membership_fallback() {
        let path = env::temp_dir().join(format!("control-config-{}.toml", process::id()));
        assert!(!compilation_config(&path));
        fs::write(&path, "cap_lints = false\ntest_tool = 'cargo'\n").unwrap();
        assert!(!compilation_config(&path));
        fs::write(&path, "test_tool = 'nextest'\n").unwrap();
        assert!(compilation_config(&path));
        for setting in [
            "cap_lints = true",
            "features = ['engine']",
            "additional_cargo_args = ['--release']",
            "additional_cargo_test_args = ['--features','engine']",
            "unknown = true",
        ] {
            fs::write(&path, setting).unwrap();
            assert!(compilation_config(&path));
        }
        fs::remove_file(path).unwrap();
    }
}

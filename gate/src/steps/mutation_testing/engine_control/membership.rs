//! Bind independent owning-package builds to the complete control shard.

use super::package_build::{digest, field, strings};
use super::{package_build, source};
use crate::checks::native_cache::{NativeCache, native_cache_command};
use crate::runner::{Cmd, Failure, Job, Outcome, tee_line, write};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Verified source membership per owner; None means only that owner falls back to testing.
pub(super) type Membership = BTreeMap<String, Option<BTreeSet<String>>>;

/// Build each assigned owner separately so dependency feature unification cannot hide its sources.
pub(super) fn build(
    job: &Job,
    root: &Path,
    receipt: &Path,
    assigned: &Path,
    policy: Option<&NativeCache>,
) -> Outcome {
    let started = Instant::now();
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
    let config = workspace.join(".cargo/mutants.toml");
    let configuration = if config.exists() {
        fs::read(&config)
            .map_err(|error| format!("cannot retain compile configuration: {error}"))?
    } else {
        Vec::new()
    };
    write(&root.join("compile-config.toml"), &configuration, false)?;
    let fallback = compilation_config(&config) || job.project != workspace;
    let mut builds = Vec::new();
    for (index, owner) in source::owners(assigned)?.iter().enumerate() {
        let package = source::package(root, owner)?;
        let directory = root.join(format!("builds/{index}"));
        fs::create_dir_all(&directory)
            .map_err(|error| format!("cannot retain package build: {error}"))?;
        let record = if fallback || package.is_none() {
            tee_line(
                &format!(
                    concat!(
                        "Compile membership fallback: package-scoped compile equivalence ",
                        "is unverified; ",
                        "testing every assigned mutant for {}; ",
                        "compiled-survivor rejection inactive"
                    ),
                    owner
                ),
                &job.report("mutants-engine-default.txt")?,
                true,
            )?;
            package_build::save(&directory, None, 0)?
        } else {
            let target = job.temp.join(format!("engine-control-target/{index}"));
            if target.exists() {
                fs::remove_dir_all(&target)
                    .map_err(|error| format!("cannot clear control target: {error}"))?;
            }
            package_build::compile(
                job,
                (&directory, root),
                policy,
                (&target, package_build::remaining_budget(started.elapsed())?),
                &package.unwrap_or_default(),
            )?
        };
        builds.push(
            Cmd::new("jaq -cn")
                .args(["--arg", "package", owner])
                .args(["--argjson", "build", &record])
                .arg("$build + {package:$package}")
                .capture()?,
        );
    }
    let value = Cmd::new("jaq -cn")
        .args(["--slurpfile", "receipt"])
        .arg(receipt)
        .args(["--arg", "project"])
        .arg(&job.project)
        .args(["--arg", "workspace"])
        .arg(workspace)
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
        .args(["--argjson", "packages", &format!("[{}]", builds.join(","))])
        .arg(concat!(
            "{schema:4, source_sha256:$source_digest, config_sha256:$config_digest, ",
            "binding:$receipt[0], project:$project, workspace:$workspace, packages:$packages}"
        ))
        .capture()?;
    write(
        &root.join("compile-membership.json"),
        value.as_bytes(),
        false,
    )
}

/// Recheck every package command and classify only against that package's evidence.
pub(super) fn verify(root: &Path, receipt: &Path, assigned: &Path) -> Result<Membership, Failure> {
    let manifest = root.join("compile-membership.json");
    verify_binding(root, receipt, assigned)?;
    let project = PathBuf::from(field(&manifest, ".project")?);
    let workspace = PathBuf::from(field(&manifest, ".workspace")?);
    let fallback = compilation_config(&root.join("compile-config.toml")) || project != workspace;
    let mut members = Membership::new();
    for (index, owner) in source::owners(assigned)?.iter().enumerate() {
        let record = field(&manifest, &format!(".packages[{index}] | del(.package)"))?;
        let target = field(&manifest, &format!(".packages[{index}].target"))?;
        let package = source::package(root, owner)?;
        if !target.is_empty() && fallback {
            return Err("featureless compile membership command is not equivalent".into());
        }
        let membership = package_build::verify(
            &root.join(format!("builds/{index}")),
            root,
            (&project, &workspace),
            package.as_deref(),
            &record,
        )?;
        members.insert(owner.clone(), membership);
    }
    Ok(members)
}

/// Unknown configuration keys conservatively disable absence classification for every owner.
fn compilation_config(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }
    Cmd::new("jaq --from toml -e")
        .arg(concat!(
            "(.cap_lints // false) == false and ",
            "((.test_tool // \"cargo\") == \"cargo\" or .test_tool == \"nextest\") and ",
            "(keys | all(.[]; . == \"cap_lints\" or . == \"test_tool\" or ",
            ". == \"exclude_globs\" or . == \"examine_globs\" or . == \"exclude_re\" or ",
            ". == \"examine_re\" or . == \"skip_calls\" or . == \"skip_calls_defaults\" or ",
            ". == \"timeout_multiplier\" or . == \"minimum_test_timeout\" or ",
            ". == \"build_timeout_multiplier\" or . == \"build_timeout\" or . == \"timeout\"))"
        ))
        .arg(path)
        .capture()
        .is_err()
}

/// Require the exact source frame, receipt and complete ordered owner set before using any build.
fn verify_binding(root: &Path, receipt: &Path, assigned: &Path) -> Outcome {
    let manifest = root.join("compile-membership.json");
    if !manifest.is_file() {
        return Err("featureless compile membership evidence is missing".into());
    }
    Cmd::new("jaq -e")
        .arg(".schema == 4")
        .arg(&manifest)
        .capture()
        .map_err(|_| "featureless compile membership requires schema 4 compiler logs")?;
    Cmd::new("jaq -e")
        .args(["--slurpfile", "receipt"])
        .arg(receipt)
        .args(["--argjson", "owners", &strings(&source::owners(assigned)?)?])
        .arg(concat!(
            r#"(keys | sort) == ["binding","config_sha256","packages","project","schema","#,
            r#""source_sha256","workspace"] and "#,
            ".binding == $receipt[0] and ",
            "(.packages | type == \"array\") and ([.packages[].package] == $owners)"
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
        assert!(!compilation_config(&path));
        for setting in [
            "test_tool = 'unsupported'",
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

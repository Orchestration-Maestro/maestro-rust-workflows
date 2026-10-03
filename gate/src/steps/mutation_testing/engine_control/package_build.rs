//! Retain and verify one owning package's independent compiler evidence.

use super::dep_info::{dep_paths, dependencies, normalized};
use crate::checks::digests::sha256_hex;
use crate::checks::native_cache::{NativeCache, native_cache_command};
use crate::runner::{Cmd, Failure, Job, write};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

/// Membership and mutant execution share the existing 30-minute worker allowance.
pub(super) fn remaining_budget(elapsed: Duration) -> Result<Duration, Failure> {
    let remaining = Duration::from_secs(30 * 60).saturating_sub(elapsed);
    if remaining.is_zero() {
        return Err(Failure::status(124));
    }
    Ok(remaining)
}

/// Instrument the equivalent single-package command without altering compiler flag resolution.
pub(super) fn compile(
    job: &Job,
    records: (&Path, &Path),
    policy: Option<&NativeCache>,
    budget: (&Path, Duration),
    package: &str,
) -> Result<String, Failure> {
    let (target, remaining) = budget;
    let (root, configuration) = records;
    let started = Cmd::new("jaq -nr").arg("now | todateiso8601").capture()?;
    let clock = Instant::now();
    let argv = build_arguments(configuration, package, target)?;
    let result = native_cache_command(policy, "timeout --kill-after=1m")
        .arg(format!("{}s", remaining.as_secs().max(1)))
        .args(&argv)
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
        .args(["--argjson", "argv", &strings(&argv)?])
        .arg("{start_time:$started, end_time:(now | todateiso8601), duration:$elapsed, argv:$argv}")
        .capture()?;
    write(&root.join("build-record.json"), build.as_bytes(), false)?;
    let paths = dep_paths(&root.join("cargo-build.json"))?;
    let directory = root.join("dep-info");
    fs::create_dir_all(&directory).map_err(|error| format!("cannot retain dep-info: {error}"))?;
    for (index, path) in paths.iter().enumerate() {
        let path = Path::new(path);
        if !path.starts_with(target) {
            return Err("featureless build dep-info escapes its clean target".into());
        }
        fs::copy(path, directory.join(format!("{index}.d")))
            .map_err(|error| format!("cannot retain featureless dep-info: {error}"))?;
    }
    save(root, Some(target))
}

/// Match cargo-mutants 27.1.0's selected build tool, adding only evidence instrumentation.
fn build_arguments(
    configuration: &Path,
    package: &str,
    target: &Path,
) -> Result<Vec<String>, Failure> {
    let nextest = Cmd::new("jaq --from toml -r")
        .arg(".test_tool // \"cargo\"")
        .arg(configuration.join("compile-config.toml"))
        .capture()?;
    let (command, message_format) = if nextest.trim() == "nextest" {
        (
            "cargo nextest run --no-run --verbose",
            "--cargo-message-format=json",
        )
    } else {
        ("cargo test --no-run --verbose", "--message-format=json")
    };
    let mut argv: Vec<String> = command.split_whitespace().map(str::to_owned).collect();
    argv.extend([
        format!("--package={package}"),
        "--locked".into(),
        message_format.into(),
        "--target-dir".into(),
        target.to_string_lossy().into_owned(),
    ]);
    Ok(argv)
}

/// Bind the exact retained compiler bytes to this package build.
pub(super) fn save(root: &Path, target: Option<&Path>) -> Result<String, Failure> {
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
        .args(["--argjson", "digests", &strings(&digests)?])
        .arg(concat!(
            "{target:$target, cargo_sha256:$cargo_digest, build_sha256:$build_digest, ",
            "dep_sha256:$digests}"
        ))
        .capture()?;
    Ok(value)
}

/// Absence is trusted only after rechecking the raw evidence and source-bound receipt.
pub(super) fn verify(
    root: &Path,
    configuration: &Path,
    frame: (&Path, &Path),
    package: Option<&str>,
    binding: &str,
) -> Result<Option<BTreeSet<String>>, Failure> {
    Cmd::new("jaq -e")
        .arg(concat!(
            r#"(keys | sort) == ["build_sha256","cargo_sha256","dep_sha256","target"] and "#,
            "(.target | type == \"string\") and (.dep_sha256 | type == \"array\")"
        ))
        .stdin_bytes(binding.as_bytes())
        .capture()
        .map_err(|_| "featureless compile membership binding differs from its plan")?;
    let target = json_field(binding, ".target")?;
    if target.is_empty() {
        Cmd::new("jaq -e")
            .arg(".cargo_sha256 == \"\" and .build_sha256 == \"\" and .dep_sha256 == []")
            .stdin_bytes(binding.as_bytes())
            .capture()
            .map_err(|_| "featureless fallback contains unexpected compile evidence")?;
        return Ok(None);
    }
    let package = package.ok_or("featureless compile membership command is not equivalent")?;
    let build_record = root.join("build-record.json");
    Cmd::new("jaq -e")
        .args([
            "--argjson",
            "argv",
            &strings(&build_arguments(
                configuration,
                package,
                Path::new(&target),
            )?)?,
        ])
        .arg(".argv == $argv")
        .arg(&build_record)
        .capture()
        .map_err(|_| "featureless compile membership command is not equivalent")?;
    let cargo = root.join("cargo-build.json");
    if digest(&cargo)? != json_field(binding, ".cargo_sha256")? {
        return Err("featureless compile membership cargo digest differs".into());
    }
    if digest(&root.join("build-record.json"))? != json_field(binding, ".build_sha256")? {
        return Err("featureless compile membership build record digest differs".into());
    }
    let (project, workspace) = frame;
    let digests = json_field(binding, ".dep_sha256[]")?;
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
            if let Ok(relative) = path.strip_prefix(project) {
                members.insert(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let sources = Cmd::new("jaq -sr")
        .arg(".[] | select(.reason == \"compiler-artifact\") | .target.src_path")
        .arg(cargo)
        .capture()?;
    for source in sources.lines() {
        if let Ok(relative) = Path::new(source).strip_prefix(project)
            && !members.contains(&relative.to_string_lossy().replace('\\', "/"))
        {
            return Err("featureless compile membership omits an artifact source".into());
        }
    }
    Ok(Some(members))
}

/// Read a field from one bound package build without persisting a second envelope.
fn json_field(value: &str, query: &str) -> Result<String, Failure> {
    Cmd::new("jaq -r")
        .arg(query)
        .stdin_bytes(value.as_bytes())
        .capture()
        .map(|value| value.trim().to_owned())
}

/// Read one field with the same pinned JSON reader used for all evidence.
pub(super) fn field(path: &Path, query: &str) -> Result<String, Failure> {
    Cmd::new("jaq -r")
        .arg(query)
        .arg(path)
        .capture()
        .map(|value| value.trim().to_owned())
}

/// Serialize digest strings without adding a second JSON parser.
pub(super) fn strings(values: &[String]) -> Result<String, Failure> {
    Cmd::new("jaq -cn")
        .arg("$ARGS.positional")
        .arg("--args")
        .arg("--")
        .args(values)
        .capture()
}

/// Hash exactly the retained evidence bytes, never a normalized reserialization.
pub(super) fn digest(path: &Path) -> Result<String, Failure> {
    fs::read(path)
        .map(|bytes| sha256_hex(&bytes))
        .map_err(|error| format!("cannot read compile membership evidence: {error}").into())
}

#[cfg(test)]
mod tests {
    use super::{remaining_budget, strings};
    use std::time::Duration;

    #[test]
    fn exhausted_shared_budget_refuses_at_and_beyond_the_deadline() {
        assert_eq!(
            remaining_budget(Duration::from_secs(1799)).unwrap(),
            Duration::from_secs(1)
        );
        for seconds in [1800, 1801] {
            assert_eq!(
                remaining_budget(Duration::from_secs(seconds))
                    .unwrap_err()
                    .code,
                124
            );
        }
    }

    #[test]
    fn package_arguments_keep_exact_string_boundaries() {
        assert_eq!(
            strings(&["a b".into(), "c".into()]).unwrap().trim(),
            "[\"a b\",\"c\"]"
        );
    }
}

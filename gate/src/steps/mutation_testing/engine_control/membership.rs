//! Retain and verify the default compiler's dep-info, bound to the worker receipt.

use crate::checks::digests::sha256_hex;
use crate::checks::native_cache::{NativeCache, native_cache_command};
use crate::runner::{Cmd, Failure, Job, Outcome, input, optional, tee_line, write};
use std::collections::BTreeSet;
use std::fs;
use std::mem;
use std::path::{Component, Path, PathBuf};
use std::time::Instant;

/// Default test-target compilation is a conservative superset of package-scoped mutant builds.
const BUILD_COMMAND: &str = "cargo test --workspace --all-targets --no-run --locked";

/// Build into a fresh target tree; never infer membership from stale cached dep-info.
pub(super) fn build(
    job: &Job,
    root: &Path,
    receipt: &Path,
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
    let config = workspace.join(".cargo/mutants.toml");
    let fallback = compilation_config(&config);
    if fallback {
        tee_line(
            concat!(
                "Compile membership fallback: cargo-mutants configuration changes compilation; ",
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
    let started = Cmd::new("jaq -nr").arg("now | todateiso8601").capture()?;
    let clock = Instant::now();
    let encoded = encoded_flags()?;
    let result = native_cache_command(policy, "timeout --kill-after=1m 30m")
        .args(BUILD_COMMAND.split_whitespace())
        .arg("--message-format=json")
        .arg("--target-dir")
        .arg(&target)
        .env("CARGO_ENCODED_RUSTFLAGS", &encoded)
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
        .args(["--arg", "flags", &encoded])
        .arg(concat!(
            "{start_time:$started, end_time:(now | todateiso8601), duration:$elapsed, ",
            "encoded_rustflags:$flags, ",
            r#"argv:["cargo","test","--workspace","--all-targets","--no-run","--locked","#,
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

/// Mirror pinned cargo-mutants 27.1.0's cap-lint flags, including its env precedence.
fn encoded_flags() -> Result<String, Failure> {
    let inherited = match input("CARGO_ENCODED_RUSTFLAGS") {
        Ok(flags) => flags,
        Err(_) => optional("RUSTFLAGS")?
            .split(' ')
            .filter(|flag| !flag.is_empty())
            .collect::<Vec<_>>()
            .join("\u{1f}"),
    };
    Ok(if inherited.is_empty() {
        "--cap-lints=warn".into()
    } else {
        format!("{inherited}\u{1f}--cap-lints=warn")
    })
}

/// Unknown config keys conservatively disable absence classification as well.
fn compilation_config(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }
    let safe = Cmd::new("jaq --from toml -e")
        .arg(concat!(
            "keys | all(.[]; . == \"test_tool\" or . == \"exclude_globs\" or ",
            ". == \"examine_globs\" or . == \"exclude_re\" or . == \"examine_re\" or ",
            ". == \"skip_calls\" or . == \"skip_calls_defaults\" or ",
            ". == \"timeout_multiplier\" or . == \"minimum_test_timeout\" or ",
            ". == \"build_timeout_multiplier\" or . == \"build_timeout\" or ",
            ". == \"timeout\")"
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
        .args(["--argjson", "digests", &strings(&digests)?])
        .arg(concat!(
            "{schema:1, binding:$receipt[0], project:$project, workspace:$workspace, ",
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

/// Verify raw successful Cargo output and require dep-info for every emitted non-build-script unit.
fn dep_paths(cargo: &Path) -> Result<Vec<String>, Failure> {
    Cmd::new("jaq -se")
        .arg(concat!(
            "([.[] | select(.reason == \"build-finished\")] | length == 1) and ",
            "([.[] | select(.reason == \"build-finished\")][0].success == true) and ",
            "any(.[]; .reason == \"compiler-artifact\") and ",
            "all(.[] | select(.reason == \"compiler-artifact\"); .fresh == false and ",
            "(.target.src_path | type == \"string\" and startswith(\"/\")) and ",
            "(.filenames | type == \"array\" and length > 0 and all(.[]; type == \"string\")))"
        ))
        .arg(cargo)
        .capture()
        .map_err(|_| "featureless build did not verify fresh compile membership")?;
    let paths = Cmd::new("jaq -sr")
        .arg(concat!(
            "[.[] | select(.reason == \"compiler-artifact\") | ",
            ".filenames[]] | unique | .[]"
        ))
        .arg(cargo)
        .capture()?;
    let mut deps = BTreeSet::new();
    for path in paths.lines() {
        let path = Path::new(path);
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or("featureless artifact filename is invalid")?;
        let script_stem;
        let stem = if stem == "build-script-build" {
            let hash = path
                .parent()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                .and_then(|name| name.rsplit_once('-'))
                .map(|(_, hash)| hash)
                .ok_or("featureless build-script artifact filename is invalid")?;
            script_stem = format!("build_script_build-{hash}");
            script_stem.as_str()
        } else {
            stem
        };
        let stem = match path.extension().and_then(|extension| extension.to_str()) {
            Some("rlib" | "rmeta" | "so" | "dylib" | "a") => {
                stem.strip_prefix("lib").unwrap_or(stem)
            }
            _ => stem,
        };
        deps.insert(
            path.with_file_name(format!("{stem}.d"))
                .to_string_lossy()
                .into_owned(),
        );
    }
    Ok(deps.into_iter().collect())
}

/// Absence is trusted only after rechecking the raw evidence and source-bound receipt.
pub(super) fn verify(root: &Path, receipt: &Path) -> Result<Option<BTreeSet<String>>, Failure> {
    let manifest = root.join("compile-membership.json");
    if !manifest.is_file() {
        return Err("featureless compile membership evidence is missing".into());
    }
    Cmd::new("jaq -e")
        .args(["--slurpfile", "receipt"])
        .arg(receipt)
        .arg(concat!(
            r#"(keys | sort) == ["binding","build_sha256","cargo_sha256","dep_sha256","#,
            r#""project","schema","target","workspace"] and "#,
            ".schema == 1 and .binding == $receipt[0] and ",
            "(.project | type == \"string\" and startswith(\"/\")) and ",
            "(.workspace | type == \"string\" and startswith(\"/\")) and ",
            "(.target | type == \"string\") and (.dep_sha256 | type == \"array\")"
        ))
        .arg(&manifest)
        .capture()
        .map_err(|_| "featureless compile membership binding differs from its plan")?;
    let target = field(&manifest, ".target")?;
    if target.is_empty() {
        Cmd::new("jaq -e")
            .arg(".cargo_sha256 == \"\" and .build_sha256 == \"\" and .dep_sha256 == []")
            .arg(&manifest)
            .capture()
            .map_err(|_| "featureless fallback contains unexpected compile evidence")?;
        return Ok(None);
    }
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

/// Parse rustc's first Make rule, including escaped spaces and line continuations.
fn dependencies(text: &str) -> Result<Vec<String>, Failure> {
    let joined = text.replace("\\\n", "");
    let line = joined
        .lines()
        .next()
        .ok_or("featureless dep-info is empty")?;
    let (_, dependencies) = line
        .split_once(": ")
        .ok_or("featureless dep-info has no dependency rule")?;
    let mut words = Vec::new();
    let mut word = String::new();
    let mut escaped = false;
    for character in dependencies.chars() {
        if escaped {
            word.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character.is_whitespace() {
            if !word.is_empty() {
                words.push(mem::take(&mut word));
            }
        } else {
            word.push(character);
        }
    }
    if escaped {
        return Err("featureless dep-info has an incomplete escape".into());
    }
    if !word.is_empty() {
        words.push(word);
    }
    if words.is_empty() {
        return Err("featureless dep-info has no source dependencies".into());
    }
    Ok(words)
}

/// Rust path attributes can contain parent components; normalize before matching plan paths.
fn normalized(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
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
    use super::{compilation_config, dep_paths, dependencies, normalized};
    use std::path::Path;
    use std::{env, fs, process};

    #[test]
    fn dep_info_escaping_and_phony_rules_preserve_exact_source_paths() {
        assert_eq!(
            dependencies("a: src/lib.rs src/a\\ b.rs \\\n src/c.rs\n\nsrc/lib.rs:\n").unwrap(),
            ["src/lib.rs", "src/a b.rs", "src/c.rs"]
        );
        for text in ["", "bad", "a: ", "a: src/unfinished\\"] {
            assert!(dependencies(text).is_err());
        }
    }

    #[test]
    fn cargo_artifact_names_select_library_test_and_build_script_dep_info() {
        let path = env::temp_dir().join(format!("control-artifacts-{}.json", process::id()));
        fs::write(&path, concat!(
            r#"{"reason":"compiler-artifact","fresh":false,"target":{"src_path":"/p/build.rs"},"#,
            r#""filenames":["/target/debug/build/probe-1234/build-script-build"]}"#, "\n",
            r#"{"reason":"compiler-artifact","fresh":false,"target":{"src_path":"/p/src/lib.rs"},"#,
            r#""filenames":["/target/debug/deps/libprobe-5678.rlib","#,
            r#""/target/debug/deps/libprobe-5678.rmeta","/target/debug/deps/probe-9012"]}"#, "\n",
            r#"{"reason":"build-finished","success":true}"#, "\n"
        )).unwrap();
        assert_eq!(
            dep_paths(&path).unwrap(),
            [
                "/target/debug/build/probe-1234/build_script_build-1234.d",
                "/target/debug/deps/probe-5678.d",
                "/target/debug/deps/probe-9012.d"
            ]
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn source_parent_components_do_not_hide_compiled_plan_paths() {
        assert_eq!(
            normalized(Path::new("/workspace/crates/a/src/../src/./engine.rs")),
            Path::new("/workspace/crates/a/src/engine.rs")
        );
    }

    #[test]
    fn compilation_settings_trigger_conservative_membership_fallback() {
        let path = env::temp_dir().join(format!("control-config-{}.toml", process::id()));
        assert!(!compilation_config(&path));
        fs::write(&path, "test_tool = 'nextest'\n").unwrap();
        assert!(!compilation_config(&path));
        for setting in [
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

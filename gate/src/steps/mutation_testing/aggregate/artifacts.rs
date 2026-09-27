//! Keep downloaded mutation evidence inside the runner's temporary directory.

use crate::runner::{Failure, Job, Outcome, tee_line};
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Resolve an artifact or plan directory inside this runner's temp tree.
pub(super) fn safe_directory(job: &Job, path: &Path) -> Result<PathBuf, Failure> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(format!("{} is not a real directory", path.display()).into());
    }
    let temp = fs::canonicalize(&job.temp)
        .map_err(|error| format!("cannot resolve runner temp: {error}"))?;
    let path = fs::canonicalize(path)
        .map_err(|error| format!("cannot resolve {}: {error}", path.display()))?;
    if !path.starts_with(temp) {
        return Err("mutation evidence escapes runner temp".into());
    }
    Ok(path)
}

/// Refuse a symlink, non-file or missing evidence file.
pub(super) fn safe_file(path: &Path) -> Result<PathBuf, Failure> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("missing mutation evidence {}: {error}", path.display()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(format!(
            "mutation evidence is not a regular file: {}",
            path.display()
        )
        .into());
    }
    fs::canonicalize(path)
        .map_err(|error| format!("cannot resolve {}: {error}", path.display()).into())
}

/// A relative path whose components cannot escape its artifact tree.
pub(super) fn safe_relative(value: &str) -> bool {
    !value.is_empty()
        && !value.contains('\\')
        && Path::new(value)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

/// One safe artifact name/path component.
pub(super) fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value.contains('/')
        && !value.contains('\\')
        && !value.contains('\0')
        && value.chars().all(|character| {
            character.is_alphanumeric() || matches!(character, '.' | '_' | '-' | '+' | '@')
        })
        && Path::new(value)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

/// Whether any raw outcome was available, controlling passed versus not-run scorecard state.
pub(super) fn has_outcome_files(job: &Job) -> bool {
    optional_root("MUTATION_ARTIFACTS")
        .is_some_and(|root| contains_named_file(Path::new(&root), "outcomes.json"))
        || job.reports.join("mutants.json").is_file()
        || contains_named_file(&job.reports.join("mutation-shards"), "outcomes.json")
}

/// Recursively check whether a file with this exact name is present.
fn contains_named_file(root: &Path, name: &str) -> bool {
    let Ok(entries) = fs::read_dir(root) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|entry| {
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            return false;
        };
        if metadata.file_type().is_symlink() {
            return false;
        }
        if metadata.is_file() {
            entry.file_name() == name
        } else if metadata.is_dir() {
            contains_named_file(&path, name)
        } else {
            false
        }
    })
}

/// The optional artifact path when an early setup failure left no download.
fn optional_root(name: &str) -> Option<String> {
    env::var(name).ok()
}

/// Copy every expected shard into the final bundle without following links.
pub(super) fn copy_artifact(source: &Path, destination: &Path) -> Outcome {
    let metadata = fs::symlink_metadata(source).map_err(|error| {
        format!(
            "cannot inspect shard artifact {}: {error}",
            source.display()
        )
    })?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("shard artifact is not a directory".into());
    }
    validate_tree(source)?;
    fs::create_dir_all(destination)
        .map_err(|error| format!("cannot create {}: {error}", destination.display()))?;
    copy_tree(source, destination)
}

/// Reject symlinks and special files anywhere in downloaded evidence.
pub(super) fn validate_tree(root: &Path) -> Outcome {
    for entry in
        fs::read_dir(root).map_err(|error| format!("cannot read {}: {error}", root.display()))?
    {
        let entry = entry.map_err(|error| format!("cannot read shard entry: {error}"))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!("shard evidence contains symlink {}", path.display()).into());
        }
        if metadata.is_dir() {
            validate_tree(&path)?;
        } else if !metadata.is_file() || !safe_component(&entry.file_name().to_string_lossy()) {
            return Err(
                format!("shard evidence contains an unsafe path {}", path.display()).into(),
            );
        }
    }
    Ok(())
}

/// Copy an already validated tree while preserving its directories and files.
pub(super) fn copy_tree(source: &Path, destination: &Path) -> Outcome {
    for entry in fs::read_dir(source)
        .map_err(|error| format!("cannot read {}: {error}", source.display()))?
    {
        let entry = entry.map_err(|error| format!("cannot read shard entry: {error}"))?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&from)
            .map_err(|error| format!("cannot inspect {}: {error}", from.display()))?;
        if metadata.is_dir() {
            fs::create_dir(&to)
                .map_err(|error| format!("cannot create {}: {error}", to.display()))?;
            copy_tree(&from, &to)?;
        } else {
            fs::copy(&from, &to)
                .map_err(|error| format!("cannot copy {}: {error}", from.display()))?;
        }
    }
    Ok(())
}

/// Preserve foreign but safe artifacts separately; they never count as evidence.
pub(super) fn preserve_foreign_artifacts(
    root: &Path,
    reports: &Path,
    artifact_name: &str,
    shards: usize,
    report: &Path,
) -> Outcome {
    let expected: BTreeSet<String> = (0..shards)
        .map(|index| format!("{artifact_name}-mutants-{index}-of-{shards}"))
        .collect();
    let mut foreign = Vec::new();
    for entry in
        fs::read_dir(root).map_err(|error| format!("cannot inspect downloaded shards: {error}"))?
    {
        let entry = entry.map_err(|error| format!("cannot inspect downloaded shard: {error}"))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if expected.contains(&name) {
            continue;
        }
        if safe_component(&name) {
            let destination = reports.join("unverified").join(&name);
            if let Err(error) = copy_artifact(&entry.path(), &destination) {
                tee_line(
                    &format!(
                        "Could not retain foreign artifact {name}: {}",
                        error.message.as_deref().unwrap_or("unsafe evidence")
                    ),
                    report,
                    true,
                )?;
            }
        }
        foreign.push(name);
    }
    if !foreign.is_empty() {
        tee_line(
            &format!("Rejected foreign shard artifacts: {}", foreign.join(", ")),
            report,
            true,
        )?;
        return Err("download contains foreign mutation shard artifacts".into());
    }
    Ok(())
}

/// Preserve data safely when plan validation prevents normal aggregation.
pub(super) fn preserve_unverified(job: &Job, report: &Path) -> Outcome {
    if !job.reports.join("mutation-plan.json").exists()
        && let Some(checks) = optional_root("MUTATION_PLAN_DIR")
        && let Ok(checks) = safe_directory(job, Path::new(&checks))
        && validate_tree(&checks).is_ok()
        && let Err(error) = copy_tree(&checks, &job.reports)
    {
        tee_line(
            &format!(
                "Could not retain checks reports: {}",
                error.message.as_deref().unwrap_or("copy failed")
            ),
            report,
            true,
        )?;
    }
    let destination = job.reports.join("mutation-shards");
    if fs::read_dir(&destination).is_ok_and(|mut entries| entries.next().is_some()) {
        return Ok(());
    }
    let Some(artifacts) = optional_root("MUTATION_ARTIFACTS") else {
        return Ok(());
    };
    let Ok(artifacts) = safe_directory(job, Path::new(&artifacts)) else {
        return Ok(());
    };
    for entry in fs::read_dir(artifacts).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if safe_component(&name)
            && let Err(error) =
                copy_artifact(&entry.path(), &destination.join("unverified").join(&name))
        {
            tee_line(
                &format!(
                    "Could not retain shard artifact {name}: {}",
                    error.message.as_deref().unwrap_or("unsafe evidence")
                ),
                report,
                true,
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        copy_artifact, safe_component, safe_directory, safe_file, safe_relative, validate_tree,
    };
    use crate::runner::Job;
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use std::process;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn test_root() -> PathBuf {
        let root = env::temp_dir().join(format!(
            "rust-gate-mutation-evidence-{}-{}",
            process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn paths_reject_escape_and_unsafe_names() {
        assert!(safe_component("rust-abc-0-of-2"));
        assert!(safe_component("module+@Módulo.rs_line_3_col_2.log"));
        assert!(!safe_component("."));
        assert!(!safe_component(".."));
        assert!(!safe_component("a/b"));
        assert!(!safe_component("a\\b"));
        assert!(!safe_component("nul\0name"));
        assert!(!safe_component("unsafe name"));
        assert!(!safe_component("../shard"));
        assert!(safe_relative("log/baseline.log"));
        assert!(!safe_relative("../../outside"));
        assert!(!safe_relative("C:\\outside"));
    }

    #[test]
    fn evidence_paths_reject_external_directories_nonfiles_and_unsafe_trees() {
        let root = test_root();
        let temp = root.join("temp");
        let reports = root.join("reports");
        fs::create_dir_all(&temp).unwrap();
        fs::create_dir_all(&reports).unwrap();
        let job = Job {
            project: root.clone(),
            reports,
            temp,
        };
        let outside = root.join("outside");
        fs::create_dir_all(&outside).unwrap();
        assert_eq!(
            safe_directory(&job, &outside)
                .unwrap_err()
                .message
                .as_deref(),
            Some("mutation evidence escapes runner temp")
        );

        let file = root.join("not-a-directory");
        fs::write(&file, "file").unwrap();
        assert!(
            safe_directory(&job, &file)
                .unwrap_err()
                .message
                .as_deref()
                .unwrap()
                .contains("is not a real directory")
        );
        assert_eq!(
            copy_artifact(&file, &root.join("copy"))
                .unwrap_err()
                .message
                .as_deref(),
            Some("shard artifact is not a directory")
        );

        let unsafe_tree = root.join("unsafe-tree");
        fs::create_dir_all(&unsafe_tree).unwrap();
        fs::write(unsafe_tree.join("unsafe name"), "not safe").unwrap();
        assert!(
            safe_file(&unsafe_tree)
                .unwrap_err()
                .message
                .as_deref()
                .unwrap()
                .contains("mutation evidence is not a regular file:")
        );
        assert!(
            validate_tree(&unsafe_tree)
                .unwrap_err()
                .message
                .as_deref()
                .unwrap()
                .contains("shard evidence contains an unsafe path")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn evidence_trees_reject_symlinks() {
        use std::os::unix::fs::symlink;

        let root = test_root();
        let tree = root.join("tree");
        fs::create_dir_all(&tree).unwrap();
        fs::write(root.join("target"), "target").unwrap();
        symlink(root.join("target"), tree.join("link")).unwrap();
        assert!(
            validate_tree(&tree)
                .unwrap_err()
                .message
                .as_deref()
                .unwrap()
                .contains("shard evidence contains symlink")
        );
        fs::remove_dir_all(root).unwrap();
    }
}

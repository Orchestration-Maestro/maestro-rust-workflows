//! Join the native Windows owner without ever applying engine features to it.

use super::artifacts::{copy_artifact, safe_directory, safe_file};
use super::engine_evidence::tagged;
use super::evidence::partition_counts;
use crate::runner::{Cmd, Failure, Job, Outcome, input, tee_line};
use std::fs;
use std::path::{Path, PathBuf};

/// A configured Windows job remains required even when its bound listing is empty.
pub(super) fn collect(
    job: &Job,
    report: &Path,
    default_manifest: &Path,
) -> Result<Vec<PathBuf>, Failure> {
    let files = input("MUTATION_WINDOWS")?;
    let state = input("WINDOWS_MUTATIONS_RESULT")?;
    if files == "[]" {
        if state != "skipped" {
            return Err("unconfigured Windows evidence was not skipped".into());
        }
        return Ok(Vec::new());
    }
    if state != "success" {
        return Err("Windows mutation evidence is missing, skipped or failed".into());
    }
    let directory = safe_directory(job, Path::new(&input("MUTATION_WINDOWS_ARTIFACTS")?))?;
    let copied = job.report("mutation-partitions")?.join("windows");
    copy_artifact(&directory, &copied)?;
    let manifest = safe_file(&directory.join("rust-reports/mutation-windows-plan.json"))?;
    let listing = safe_file(&directory.join("rust-reports/mutation-windows-list.json"))?;
    same_scope(&manifest, default_manifest, &files)?;
    super::super::plan_identity::validate_listing(&listing)?;
    let count = super::super::plan_identity::listing_count(&listing)?;
    if super::super::plan_identity::manifest_count(&manifest)? != count {
        return Err("Windows plan count differs from its complete owned listing".into());
    }
    Cmd::new("jaq -e")
        .args(["--argjson", "files", &files])
        .arg("all(.[]; .file as $f | ($files | any(.[]; . == $f)))")
        .arg(&listing)
        .capture()
        .map_err(|_| "Windows listing contains a mutant outside its exact owned files")?;
    if count == 0 {
        tee_line(
            "Windows-owned plan is complete with no changed-line mutants",
            report,
            true,
        )?;
        return Ok(Vec::new());
    }
    let outcomes = safe_file(&directory.join("mutants/mutants.out/outcomes.json"))?;
    partition_counts(&listing, &outcomes, false)?;
    Ok(vec![tagged(
        job,
        &copied.join("mutants/mutants.out/outcomes.json"),
        "windows",
        "windows",
    )?])
}

/// Compare identical source/config/toolchain identity across native runner path spellings.
fn same_scope(windows: &Path, default: &Path, files: &str) -> Outcome {
    let mut documents =
        fs::read(windows).map_err(|error| format!("cannot read Windows mutation plan: {error}"))?;
    documents.push(b'\n');
    documents.extend(
        fs::read(default).map_err(|error| format!("cannot read default mutation plan: {error}"))?,
    );
    Cmd::new("jaq -se")
        .args(["--argjson", "files", files])
        .arg(concat!(
            ".[0] as $w | .[1] as $d | $w.partition == \"windows\" ",
            "and $w.os == \"windows\" and $w.files == $files ",
            "and all([\"sha\",\"first_parent\",\"directory\",\"config_sha256\",\"diff_sha256\",",
            "\"run_id\",\"attempt\",\"toolchain\",\"cargo_mutants_version\"][]; $w[.] == $d[.])"
        ))
        .stdin_bytes(&documents)
        .capture()
        .map(|_| ())
        .map_err(|_| "Windows plan belongs to another source, policy or run identity".into())
}

#[cfg(test)]
mod tests {
    use super::same_scope;
    use std::{env, fs, process};

    #[test]
    fn windows_source_identity_cannot_drift_from_default_plan() {
        let root = env::temp_dir().join(format!("windows-mode-scope-{}", process::id()));
        fs::create_dir_all(&root).unwrap();
        let default = root.join("default.json");
        let windows = root.join("windows.json");
        fs::write(&default, "{\"sha\":\"a\"}").unwrap();
        fs::write(
            &windows,
            "{\"sha\":\"a\",\"partition\":\"windows\",\"os\":\"windows\",\"files\":[]}",
        )
        .unwrap();
        assert!(same_scope(&windows, &default, "[]").is_ok());
        fs::write(&default, "{\"sha\":\"b\"}").unwrap();
        assert_eq!(
            same_scope(&windows, &default, "[]")
                .unwrap_err()
                .message
                .as_deref(),
            Some("Windows plan belongs to another source, policy or run identity")
        );
        fs::remove_dir_all(root).unwrap();
    }
}

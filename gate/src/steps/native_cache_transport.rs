//! Guarded restore preparation and successful-job publication inventory.

use crate::checks::checkout_paths::{canonical, strictly_inside};
use crate::checks::digests::sha256_hex;
use crate::checks::native_cache::{cache_platform, native_cache};
use crate::checks::native_cache_inventory::published_inventory;
use crate::checks::native_cache_roots::{normalize_restore, prepare_root};
use crate::runner::{Failure, Job, Outcome, Step, input, optional, output, path, write};
use std::env::consts::OS;
use std::fs;
use std::path::Path;

/// Cache actions remain workflow data; the gate prepares and checks their paths.
pub(crate) const STEPS: &[Step] = &[
    Step {
        workflow: "ci",
        id: "native-cache-prepare",
        summary: "Prepare private native cache restore",
        inputs: &[
            "COVERAGE_FEATURES",
            "GITHUB_WORKSPACE",
            "NATIVE_CACHE_MODE",
            "NATIVE_CACHE_OS",
            "NATIVE_CACHE_ARCH",
            "RUSTUP_TOOLCHAIN",
            "GITHUB_REPOSITORY",
            "GITHUB_RUN_ID",
            "GITHUB_RUN_ATTEMPT",
        ],
        tools: &["jaq"],
        reports: &["native-cache-binding.txt"],
        run: prepare,
    },
    Step {
        workflow: "ci",
        id: "native-cache-inventory",
        summary: "Check native cache save inventory",
        inputs: &[
            "EVENT",
            "REF",
            "DEFAULT_BRANCH",
            "MERGE_GROUP_BASE_REF",
            "JOB_SUCCESS",
            "NATIVE_CACHE_ROOT",
        ],
        tools: &["jaq"],
        reports: &[],
        run: inventory,
    },
];

/// Absence and Windows stay disabled without allocating any path.
fn prepare() -> Outcome {
    let job = Job::current()?;
    let Some(policy) = native_cache(&job.project)? else {
        return output("enabled", "false");
    };
    let files_digest = key_files_digest(&job.project, &policy.key_files)?;
    let os = input("NATIVE_CACHE_OS")?.to_lowercase();
    if os != OS
        || !cache_platform(OS, &policy.platforms)
        || optional("COVERAGE_FEATURES")?.is_empty()
    {
        return output("enabled", "false");
    }
    let (root, restore) = match prepare_root(&job.temp) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("Native cache disabled: {error}");
            return output("enabled", "false");
        }
    };
    output("root", &root.display().to_string())?;
    if !restore {
        return output("enabled", "false");
    }
    let identity = format!(
        "{}\0{}\0{}",
        input("GITHUB_REPOSITORY")?,
        job.project
            .strip_prefix(path("GITHUB_WORKSPACE")?)
            .unwrap_or(&job.project)
            .display(),
        policy.policy_digest
    );
    let bucket = format!(
        "native-v1-{os}-{}-{}-{}-{}",
        input("NATIVE_CACHE_ARCH")?,
        input("RUSTUP_TOOLCHAIN")?,
        sha256_hex(identity.as_bytes()),
        files_digest
    );
    let mode = input("NATIVE_CACHE_MODE")?;
    let key = format!(
        "{bucket}-{mode}-{}-{}",
        input("GITHUB_RUN_ID")?,
        input("GITHUB_RUN_ATTEMPT")?
    );
    output("enabled", "true")?;
    output("bucket", &bucket)?;
    output("key", &key)?;
    output("mode", &mode)?;
    cache_paths(&root, &policy.published)?;
    write(
        &job.report("native-cache-binding.txt")?,
        format!(
            "Key: {key}\nPolicy: {}\nFiles: {}\n",
            policy.policy_digest, files_digest
        )
        .as_bytes(),
        false,
    )
}

/// Bind key files to the tested checkout, never a policy-only snapshot.
fn key_files_digest(project: &Path, files: &[String]) -> Result<String, Failure> {
    let project = canonical(project)?;
    let mut bytes = Vec::new();
    for name in files {
        let real = canonical(&project.join(name))
            .map_err(|_| "[native-cache] key file must exist inside the project")?;
        if !strictly_inside(&real, &project) || !real.is_file() {
            return Err("[native-cache] key file must exist inside the project".into());
        }
        bytes.extend_from_slice(name.as_bytes());
        bytes.push(0);
        let content = fs::read(real).map_err(|error| format!("native cache key file: {error}"))?;
        bytes.extend_from_slice(&(content.len() as u64).to_be_bytes());
        bytes.extend(content);
    }
    Ok(sha256_hex(&bytes))
}

/// Paths have no line breaks and the static delimiter cannot occur in selectors.
fn cache_paths(root: &Path, selectors: &[String]) -> Outcome {
    let paths = selectors
        .iter()
        .map(|name| root.join(name).display().to_string())
        .collect::<Vec<_>>()
        .join("\n");
    write(
        &path("GITHUB_OUTPUT")?,
        format!("paths<<NATIVE_PATHS_END\n{paths}\nNATIVE_PATHS_END\n").as_bytes(),
        true,
    )
}

/// Save only after success, on exactly the repository's default push or queue base.
fn inventory() -> Outcome {
    if !save_event(
        &input("EVENT")?,
        &input("REF")?,
        &input("DEFAULT_BRANCH")?,
        &optional("MERGE_GROUP_BASE_REF")?,
        input("JOB_SUCCESS")? == "true",
    ) {
        return output("save", "false");
    }
    let job = Job::current()?;
    let Some(policy) = native_cache(&job.project)? else {
        return output("save", "false");
    };
    let before = job.earlier("native-cache-before.txt");
    let Ok(record) = fs::read_to_string(before) else {
        return output("save", "false");
    };
    let Some((root, names)) = record.split_once('\n') else {
        return output("save", "false");
    };
    if root != optional("NATIVE_CACHE_ROOT")? {
        return output("save", "false");
    }
    let root = Path::new(root);
    if let Err(error) = normalize_restore(root, &job.temp) {
        eprintln!("Native cache save refused: {error}");
        return output("save", "false");
    }
    let after = published_inventory(root, &policy.published)?;
    if !after.is_empty() && after != names {
        cache_paths(root, &policy.published)?;
        output("save", "true")
    } else {
        output("save", "false")
    }
}

/// No caller flag can widen the trusted events; empty default branch fails closed.
fn save_event(event: &str, reference: &str, default: &str, base: &str, success: bool) -> bool {
    let expected = format!("refs/heads/{default}");
    success
        && !default.is_empty()
        && ((event == "push" && reference == expected)
            || (event == "merge_group" && base == expected))
}

#[cfg(test)]
mod tests {
    use super::save_event;

    #[test]
    fn save_event_truth_table_requires_success_and_exact_default_reference() {
        for event in [
            "push",
            "merge_group",
            "pull_request",
            "workflow_dispatch",
            "schedule",
            "pull_request_target",
        ] {
            for reference in [
                "refs/heads/main",
                "refs/heads/other",
                "refs/tags/main",
                "refs/pull/1/merge",
            ] {
                check_bases(event, reference);
            }
        }
        assert!(!save_event("push", "refs/heads/", "", "", true));
    }
    /// Both status values and every base spelling for one event/reference pair.
    fn check_bases(event: &str, reference: &str) {
        for base in ["refs/heads/main", "refs/heads/other", "main", ""] {
            for success in [true, false] {
                assert_eq!(
                    save_event(event, reference, "main", base, success),
                    success
                        && ((event == "push" && reference == "refs/heads/main")
                            || (event == "merge_group" && base == "refs/heads/main")),
                    "{event} {reference} {base} {success}"
                );
            }
        }
    }
}

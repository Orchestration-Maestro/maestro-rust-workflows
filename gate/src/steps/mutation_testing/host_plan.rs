//! Full-file host discovery, bound to the ordinary plan and tested source.

use super::{plan_identity, scope};
use crate::checks::digests::sha256_hex;
use crate::checks::mutation_host::{HostPolicy, host_policy, validate_owners};
use crate::checks::quality_config::mutation_windows;
use crate::runner::{Cmd, Failure, Job, Outcome, flag, optional, output, write};
use std::fs;
use std::path::Path;

/// Validate the host transfer before serial-mode shortcuts or disabled mutation.
pub(super) fn policy(job: &Job) -> Result<Option<HostPolicy>, Failure> {
    let policy = host_policy(&job.project)?;
    if let Some(policy) = &policy {
        validate_owners(
            policy,
            flag("MUTATION_TEST")?,
            &mutation_windows(&job.project, &optional("MUTATION_WINDOWS")?)?,
            &scope::engine_files()?,
        )?;
    }
    if policy.is_none() {
        output("mutation-host-count", "0")?;
    }
    Ok(policy)
}

/// Discover every owned file with no inherited exclusions, diff or iteration.
pub(super) fn discover(job: &Job, policy: &HostPolicy, manifest: &Path) -> Outcome {
    let listing = job.report("mutation-host-list.json")?;
    let mut command = Cmd::new(concat!(
        "cargo mutants --list --json --no-shuffle --no-config ",
        "--cargo-arg=--locked --colors=never --level=info"
    ))
    .args(["--features", &policy.features.join(",")]);
    let packages = scope::engine_packages(&job.project, &policy.files)?;
    for package in &packages {
        command = command.args(["--package", package]);
    }
    for file in &policy.files {
        command = command.args(["--file", file]);
    }
    let result = command.cwd(&job.project).capture_output()?;
    write(
        &job.report("mutation-host-plan.log")?,
        &result.stderr,
        false,
    )?;
    write(&listing, &result.stdout, false)?;
    if !result.status.success() {
        return Err(Failure::status(result.status.code().unwrap_or(1)));
    }
    plan_identity::validate_listing(&listing)?;
    let files = json_strings(&policy.files)?;
    validate_population(&listing, &files)?;
    let sources = source_hashes(job, &policy.files)?;
    let identity = fs::read_to_string(manifest)
        .map_err(|error| format!("cannot read default plan: {error}"))?;
    let population = fs::read_to_string(&listing)
        .map_err(|error| format!("cannot read host listing: {error}"))?;
    let json = Cmd::new("jaq -cn")
        .args(["--argjson", "identity", &identity])
        .args(["--argjson", "mutants", &population])
        .args(["--argjson", "files", &files])
        .args(["--argjson", "features", &json_strings(&policy.features)?])
        .args(["--argjson", "packages", &json_strings(&packages)?])
        .args(["--argjson", "sources", &sources])
        .args(["--arg", "policy", &policy.digest])
        .args(["--arg", "script", &policy.provisioner])
        .args(["--arg", "script_hash", &policy.provisioner_digest])
        .args(["--arg", "workflow", &optional("WORKFLOW_REVISION")?])
        .arg(concat!(
            "{schema:1, identity:$identity, mutants:$mutants, files:$files, ",
            "features:$features, packages:$packages, source_sha256:$sources, ",
            "policy_sha256:$policy, provisioner:$script, provisioner_sha256:$script_hash, ",
            "workflow_revision:$workflow}"
        ))
        .capture()?;
    write(
        &job.report("mutation-host-plan.json")?,
        json.as_bytes(),
        false,
    )?;
    output(
        "mutation-host-count",
        &plan_identity::listing_count(&listing)?.to_string(),
    )
}

/// Serialize validated strings with the installed JSON reader.
fn json_strings(values: &[String]) -> Result<String, Failure> {
    Cmd::new("jaq -cn")
        .arg("$ARGS.positional")
        .args(["--args"])
        .args(values)
        .capture()
}

/// Preserve file bytes as well as the complete enclosing-function and patch listing.
fn source_hashes(job: &Job, files: &[String]) -> Result<String, Failure> {
    let mut command = Cmd::new("jaq -cn").arg("$ARGS.named");
    for file in files {
        let bytes = fs::read(job.project.join(file)).map_err(|error| format!("{file}: {error}"))?;
        command = command.args(["--arg", file, &sha256_hex(&bytes)]);
    }
    command.capture()
}

/// Every file needs mutants, and every listed mutant needs a real enclosing function/patch.
fn validate_population(listing: &Path, files: &str) -> Outcome {
    Cmd::new("jaq -e")
        .args(["--argjson", "files", files])
        .arg(concat!(
            ". as $mutants | ([.[].file] | unique | sort) == ($files | sort) and ",
            "all($mutants[]; (.diff | type == \"string\" and length > 0) and ",
            "(.function.function_name | type == \"string\" and length > 0) and ",
            "(.function.span.start.line | type == \"number\" and floor == . and . > 0) and ",
            "(.function.span.end.line | type == \"number\" and floor == .) and ",
            ".function.span.start.line <= .span.start.line and ",
            ".span.end.line <= .function.span.end.line)"
        ))
        .arg(listing)
        .capture()
        .map(|_| ())
        .map_err(|_| {
            "host discovery requires nonempty exact files, patches and enclosing functions".into()
        })
}

#[cfg(test)]
mod tests {
    use super::json_strings;

    #[test]
    fn host_plan_strings_preserve_qualified_feature_and_path_bytes() {
        assert_eq!(
            json_strings(&["crate/feature".into()]).unwrap().trim(),
            "[\"crate/feature\"]"
        );
    }
}

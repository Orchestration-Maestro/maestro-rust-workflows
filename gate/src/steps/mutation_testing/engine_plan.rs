//! Discover both modes and preserve every featureless obligation before routing workers.

use super::{plan, scope};
use crate::checks::digests::sha256_hex;
use crate::checks::inputs::{mutation_mutants_per_shard, mutation_shards};
use crate::runner::{Cmd, Failure, Job, Outcome, input, output, write};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// List the full featureless control and owning packages with engine features.
pub(super) fn discover(job: &Job, source: &scope::Scope, listing: &Path, log: &Path) -> Outcome {
    let control = job.report("mutation-control-list.json")?;
    let feature = job.report("mutation-feature-list.json")?;
    save_discovery(job, listing_command(source), &control, log, source)?;
    let files = scope::engine_files()?;
    let features = scope::json_strings("MUTATION_ENGINE_FEATURES")?;
    let packages = scope::engine_packages(&job.project, &files)?;
    let mut enabled = listing_command(source).args(["--features", &features.join(",")]);
    for package in &packages {
        enabled = enabled.args(["--package", package]);
    }
    save_discovery(
        job,
        enabled,
        &feature,
        &job.report("mutation-engine-plan.log")?,
        source,
    )?;
    let engine = job.report("mutation-engine-list.json")?;
    let default = job.report("mutation-engine-default-list.json")?;
    filter_owned(&feature, &engine, &input("MUTATION_ENGINE_FILES")?, true)?;
    filter_owned(&control, &default, &input("MUTATION_ENGINE_FILES")?, true)?;
    let transferred = scope::transferred_files(&job.project)?;
    let transferred = string_array(&transferred.into_iter().collect::<Vec<_>>())?;
    filter_owned(&control, listing, &transferred, false)?;
    require_owned_union(&control, &feature, &files)?;
    let enabled_count = plan::listing_count(&engine)?;
    let default_count = plan::listing_count(&default)?;
    let shards = route_mode("mutation-engine", enabled_count)?;
    let default_shards = route_mode("mutation-engine-default", default_count)?;
    let manifest = job.report("mutation-engine-plan.json")?;
    plan::write_manifest(
        &manifest,
        &job.project,
        source,
        enabled_count + default_count,
        shards.max(default_shards),
    )?;
    bind_modes(&manifest, &engine, &default, &packages)
}

/// Apply the existing shard planner independently to one nonempty mode.
fn route_mode(prefix: &str, count: usize) -> Result<usize, Failure> {
    let (_, shards, _) = plan::selection(count, mutation_shards()?, mutation_mutants_per_shard()?)?;
    output(&format!("{prefix}-count"), &count.to_string())?;
    output(&format!("{prefix}-shards"), &shards.to_string())?;
    let matrix: Vec<_> = (0..shards).map(|index| index.to_string()).collect();
    output(
        &format!("{prefix}-matrix"),
        &format!("[{}]", matrix.join(",")),
    )?;
    Ok(shards)
}

/// Common listing flags keep discovery order identical to execution.
fn listing_command(source: &scope::Scope) -> Cmd {
    let mut command = Cmd::new(
        "cargo mutants --list --json --no-shuffle --cargo-arg=--locked --colors=never --level=info",
    );
    if let Some(diff) = &source.diff {
        command = command.args(["--in-diff", &diff.to_string_lossy()]);
    }
    command
}

/// Save real diagnostics and validate the pinned listing schema.
fn save_discovery(
    job: &Job,
    command: Cmd,
    path: &Path,
    log: &Path,
    source: &scope::Scope,
) -> Outcome {
    let result = command.cwd(&job.project).capture_output()?;
    plan::show(&result)?;
    write(log, &result.stderr, false)?;
    if !result.status.success() {
        return Err(Failure::status(result.status.code().unwrap_or(1)));
    }
    let bytes = if result.stdout.iter().all(u8::is_ascii_whitespace)
        && plan::recognized_no_work(&result.stderr, source.change.is_some())
    {
        b"[]\n".as_slice()
    } else {
        &result.stdout
    };
    write(path, bytes, false)?;
    plan::validate_listing(path)
}

/// Project a discovery into its exact owner without throwing away either mode.
fn filter_owned(source: &Path, target: &Path, files: &str, owned: bool) -> Outcome {
    let json = Cmd::new("jaq -c")
        .args(["--argjson", "files", files])
        .args(["--argjson", "owned", if owned { "true" } else { "false" }])
        .arg("[.[] | select(.file as $file | ($files | any(.[]; . == $file)) == $owned)]")
        .arg(source)
        .capture()?;
    write(target, json.as_bytes(), false)
}

/// Newly discovered mode-specific mutants must have an explicit engine owner.
fn require_owned_union(control: &Path, feature: &Path, files: &[String]) -> Outcome {
    let control_rows = identity_files(control)?;
    let feature_rows = identity_files(feature)?;
    let controls: BTreeSet<_> = control_rows.lines().collect();
    for row in feature_rows.lines() {
        let file = row.split('\t').next().unwrap_or_default();
        if !controls.contains(row) && !files.iter().any(|owned| owned == file) {
            return Err(
                "feature-enabled discovery contains an unowned mode-specific mutant".into(),
            );
        }
    }
    Ok(())
}

/// Include source, package and replacement in cross-mode identity comparison.
fn identity_files(path: &Path) -> Result<String, Failure> {
    Cmd::new("jaq -r")
        .arg(".[] | [.file, ([.package, .name, .span, .replacement] | @json)] | @tsv")
        .arg(path)
        .capture()
}

/// Serialize already-validated strings with the same JSON reader as policy transport.
fn string_array(values: &[String]) -> Result<String, Failure> {
    Cmd::new("jaq -cn")
        .arg("$ARGS.positional")
        .args(["--args"])
        .args(values)
        .capture()
}

/// Bind exact policy, package/OS owner and both listing digests into worker identity.
fn bind_modes(manifest: &Path, engine: &Path, default: &Path, packages: &[String]) -> Outcome {
    let (_, enabled_shards, _) = plan::selection(
        plan::listing_count(engine)?,
        mutation_shards()?,
        mutation_mutants_per_shard()?,
    )?;
    let (_, default_shards, _) = plan::selection(
        plan::listing_count(default)?,
        mutation_shards()?,
        mutation_mutants_per_shard()?,
    )?;
    let json = mode_command(engine, default, packages)?
        .args(["--argjson", "engine_shards", &enabled_shards.to_string()])
        .args(["--argjson", "default_shards", &default_shards.to_string()])
        .arg(concat!(
            ". + {partition: \"engine\", os: \"linux\", features:$features, files:$files, ",
            "packages:$packages, engine_count:$engine_count, default_count:$default_count, ",
            "engine_sha256:$engine_digest, default_sha256:$default_digest, ",
            "engine_shards:$engine_shards, default_shards:$default_shards}"
        ))
        .arg(manifest)
        .capture()?;
    write(manifest, json.as_bytes(), false)
}

/// Check each downloaded mode listing and the exact policy used for selection.
pub(super) fn verify_modes(job: &Job, manifest: &Path, engine: &Path, default: &Path) -> Outcome {
    plan::validate_listing(engine)?;
    plan::validate_listing(default)?;
    let packages = scope::engine_packages(&job.project, &scope::engine_files()?)?;
    mode_command(engine, default, &packages)?
        .arg(concat!(
            ".partition == \"engine\" and .os == \"linux\" and ",
            ".features == $features and .files == $files and .packages == $packages and ",
            ".engine_count == $engine_count and .default_count == $default_count and ",
            ".mutant_count == ($engine_count + $default_count) and ",
            ".engine_sha256 == $engine_digest and .default_sha256 == $default_digest"
        ))
        .arg(manifest)
        .arg("-e")
        .capture()
        .map(|_| ())
        .map_err(|_| "engine worker policy or mode listings differ from its plan".into())
}

/// Build the common digest and exact-policy arguments used by planning and workers.
fn mode_command(engine: &Path, default: &Path, packages: &[String]) -> Result<Cmd, Failure> {
    let digest = |path: &Path| {
        fs::read(path)
            .map(|bytes| sha256_hex(&bytes))
            .map_err(|error| Failure::from(format!("cannot hash mode listing: {error}")))
    };
    let command = Cmd::new("jaq -c")
        .args(["--argjson", "features", &input("MUTATION_ENGINE_FEATURES")?])
        .args(["--argjson", "files", &input("MUTATION_ENGINE_FILES")?])
        .args(["--argjson", "packages", &string_array(packages)?])
        .args([
            "--argjson",
            "engine_count",
            &plan::listing_count(engine)?.to_string(),
        ])
        .args([
            "--argjson",
            "default_count",
            &plan::listing_count(default)?.to_string(),
        ])
        .args(["--arg", "engine_digest", &digest(engine)?])
        .args(["--arg", "default_digest", &digest(default)?]);
    Ok(command)
}

/// Publish the empty engine routing when policy or mutation testing is disabled.
pub(super) fn disabled() -> Outcome {
    output("mutation-engine-count", "0")?;
    output("mutation-engine-shards", "0")?;
    output("mutation-engine-matrix", "[]")?;
    output("mutation-engine-default-count", "0")?;
    output("mutation-engine-default-shards", "0")?;
    output("mutation-engine-default-matrix", "[]")
}

#[cfg(test)]
mod tests {
    use super::require_owned_union;
    use std::{env, fs, process};

    #[test]
    fn distinct_mode_bodies_require_explicit_feature_file_ownership() {
        let root = env::temp_dir().join(format!("engine-union-{}", process::id()));
        fs::create_dir_all(&root).unwrap();
        let control = root.join("control.json");
        let feature = root.join("feature.json");
        let row =
            r#"[{"file":"src/engine.rs","package":"a","name":"run","span":{},"replacement":"0"}]"#;
        fs::write(&control, row).unwrap();
        fs::write(&feature, row).unwrap();
        assert!(require_owned_union(&control, &feature, &[]).is_ok());
        fs::write(&feature, row.replace("run", "other")).unwrap();
        assert_eq!(
            require_owned_union(&control, &feature, &[])
                .unwrap_err()
                .message
                .as_deref(),
            Some("feature-enabled discovery contains an unowned mode-specific mutant")
        );
        assert!(require_owned_union(&control, &feature, &["src/engine.rs".into()]).is_ok());
        fs::remove_dir_all(root).unwrap();
    }
}

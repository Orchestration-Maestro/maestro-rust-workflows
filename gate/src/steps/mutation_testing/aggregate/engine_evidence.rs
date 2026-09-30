//! Harvest both required engine modes, preserving exact policy and shard identity.

use super::artifacts::{copy_artifact, safe_directory, safe_file, validate_tree};
use super::evidence::partition_counts;
use super::viability;
use crate::checks::digests::sha256_hex;
use crate::runner::{Cmd, Failure, Job, Outcome, input, tee_line, write};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Validate the shared plan and both complete, independently required mode matrices.
pub(super) fn collect(
    job: &Job,
    report: &Path,
    default_manifest: &Path,
) -> Result<Vec<PathBuf>, Failure> {
    let directory = safe_directory(job, Path::new(&input("MUTATION_ENGINE_PLAN_DIR")?))?;
    validate_tree(&directory)?;
    let manifest = safe_file(&directory.join("mutation-engine-plan.json"))?;
    let engine = safe_file(&directory.join("mutation-engine-list.json"))?;
    let default = safe_file(&directory.join("mutation-engine-default-list.json"))?;
    validate_plan(&manifest, default_manifest, &engine, &default)?;
    let controls = collect_mode(job, report, &manifest, &default, false)?;
    let engines = collect_mode(job, report, &manifest, &engine, true)?;
    viability::compare(&controls, &engines, report)?;
    let mut paths = Vec::new();
    for (mode, outcomes) in [("engine-default", controls), ("engine", engines)] {
        for (index, path) in outcomes.iter().enumerate() {
            paths.push(tagged(job, path, &format!("{mode}-{index}"), mode)?);
        }
    }
    Ok(paths)
}

/// Bind both discoveries to the current run and the same scope as the default plan.
fn validate_plan(
    manifest: &Path,
    default_manifest: &Path,
    engine: &Path,
    default: &Path,
) -> Outcome {
    super::super::plan_identity::validate_listing(engine)?;
    super::super::plan_identity::validate_listing(default)?;
    let engine_digest = file_digest(engine)?;
    let default_digest = file_digest(default)?;
    let valid = Cmd::new("jaq -se")
        .args(["--arg", "sha", &input("GITHUB_SHA")?])
        .args(["--arg", "run", &input("GITHUB_RUN_ID")?])
        .args(["--arg", "attempt", &input("GITHUB_RUN_ATTEMPT")?])
        .args(["--arg", "toolchain", &input("RUSTUP_TOOLCHAIN")?])
        .args(["--arg", "version", &input("CARGO_MUTANTS_VERSION")?])
        .args(["--argjson", "features", &input("MUTATION_ENGINE_FEATURES")?])
        .args(["--argjson", "files", &input("MUTATION_ENGINE_FILES")?])
        .args([
            "--argjson",
            "engine_count",
            &super::super::plan_identity::listing_count(engine)?.to_string(),
        ])
        .args([
            "--argjson",
            "default_count",
            &super::super::plan_identity::listing_count(default)?.to_string(),
        ])
        .args(["--arg", "engine_digest", &engine_digest])
        .args(["--arg", "default_digest", &default_digest])
        .arg(concat!(
            ".[0] as $p | .[1] as $d | $p.partition == \"engine\" and $p.os == \"linux\" ",
            "and $p.sha == $sha and $p.run_id == $run and $p.attempt == $attempt ",
            "and $p.toolchain == $toolchain and $p.cargo_mutants_version == $version ",
            "and $p.features == $features and $p.files == $files ",
            "and $p.engine_count == $engine_count and $p.default_count == $default_count ",
            "and $p.mutant_count == ($engine_count + $default_count) ",
            "and $p.engine_sha256 == $engine_digest and $p.default_sha256 == $default_digest ",
            "and all([\"sha\",\"first_parent\",\"directory\",\"config_sha256\",\"diff_sha256\",",
            "\"run_id\",\"attempt\",\"toolchain\",\"cargo_mutants_version\"][]; $p[.] == $d[.])"
        ))
        .stdin_bytes(&documents(&[manifest, default_manifest])?)
        .capture();
    valid.map_err(|_| "engine plan policy, source identity or mode listing digest differs")?;
    let owners = Cmd::new("jaq -se")
        .arg(concat!(
            ".[0] as $p | all((.[1] + .[2])[]; .file as $f | .package as $pkg | ",
            "($p.files | any(.[]; . == $f)) and ($p.packages | any(.[]; . == $pkg)))"
        ))
        .stdin_bytes(&documents(&[manifest, engine, default])?)
        .capture();
    owners
        .map(|_| ())
        .map_err(|_| "engine plan contains mutants outside its exact owners".into())
}

/// One mode must have precisely the expected artifact names and all assigned outcomes.
fn collect_mode(
    job: &Job,
    report: &Path,
    manifest: &Path,
    listing: &Path,
    enabled: bool,
) -> Result<Vec<PathBuf>, Failure> {
    let [
        prefix,
        mode,
        result,
        artifacts,
        count_input,
        shards_input,
        matrix_input,
    ] = mode_names(enabled);
    let count = super::super::plan_identity::listing_count(listing)?;
    let shards = mode_shards(manifest, enabled)?;
    let expected_matrix = format!(
        "[{}]",
        (0..shards)
            .map(|index| index.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );
    if input(count_input)? != count.to_string()
        || input(shards_input)? != shards.to_string()
        || input(matrix_input)? != expected_matrix
        || shards > count
        || (count > 0 && !(1..=256).contains(&shards))
    {
        return Err("engine mode matrix differs from the complete planned obligations".into());
    }
    let state = input(result)?;
    if count == 0 {
        if state != "skipped" {
            return Err("empty engine mode was not intentionally skipped".into());
        }
        return Ok(Vec::new());
    }
    if state != "success" {
        return Err(if enabled {
            "engine mutation mode is missing, skipped or failed"
        } else {
            "engine default mode is missing, skipped or failed"
        }
        .into());
    }
    let root = safe_directory(job, Path::new(&input(artifacts)?))?;
    validate_tree(&root)?;
    let artifact = input("MUTATION_ARTIFACT_NAME")?;
    let expected: BTreeSet<_> = (0..shards)
        .map(|index| format!("{artifact}-{prefix}-mutants-{index}"))
        .collect();
    let present = fs::read_dir(&root)
        .map_err(|error| format!("cannot read mode artifacts: {error}"))?
        .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(|error| format!("cannot inspect mode artifact: {error}"))?;
    if present != expected {
        return Err("engine mode has missing or unexpected shard artifacts".into());
    }
    let mut paths = Vec::new();
    for index in 0..shards {
        let source = root.join(format!("{artifact}-{prefix}-mutants-{index}"));
        let copied = job
            .report("mutation-partitions")?
            .join(format!("{prefix}-{index}"));
        copy_artifact(&source, &copied)?;
        validate_receipt(
            &source.join(format!("rust-reports/{mode}-shard.json")),
            manifest,
            (index, shards, count),
            if enabled { "engine" } else { "default" },
        )?;
        let assigned = Cmd::new("jaq -c")
            .args(["--argjson", "index", &index.to_string()])
            .args(["--argjson", "shards", &shards.to_string()])
            .arg("[to_entries[] | select(.key % $shards == $index) | .value]")
            .arg(listing)
            .capture()?;
        let assigned_path = job.temp.join(format!("{prefix}-{index}-assigned.json"));
        write(&assigned_path, assigned.as_bytes(), false)?;
        let outcomes = safe_file(&source.join(format!("{mode}/mutants.out/outcomes.json")))?;
        let discovered = safe_file(&source.join(format!("{mode}/mutants.out/mutants.json")))?;
        super::super::plan_identity::validate_execution(&assigned_path, &outcomes)?;
        let counts = partition_counts(&discovered, &outcomes)?;
        tee_line(
            &format!(
                "{prefix} shard {index}/{shards}: total={} unviable={}",
                counts.total, counts.unviable
            ),
            report,
            true,
        )?;
        paths.push(copied.join(format!("{mode}/mutants.out/outcomes.json")));
    }
    Ok(paths)
}

/// Concrete artifact and transport names for the two independent engine modes.
fn mode_names(enabled: bool) -> [&'static str; 7] {
    if enabled {
        [
            "engine",
            "mutants-engine",
            "ENGINE_MUTATIONS_RESULT",
            "MUTATION_ENGINE_ARTIFACTS",
            "MUTATION_ENGINE_COUNT",
            "MUTATION_ENGINE_SHARDS",
            "MUTATION_ENGINE_MATRIX",
        ]
    } else {
        [
            "engine-default",
            "mutants-engine-default",
            "ENGINE_DEFAULT_MUTATIONS_RESULT",
            "MUTATION_ENGINE_DEFAULT_ARTIFACTS",
            "MUTATION_ENGINE_DEFAULT_COUNT",
            "MUTATION_ENGINE_DEFAULT_SHARDS",
            "MUTATION_ENGINE_DEFAULT_MATRIX",
        ]
    }
}

/// The independent planner stores a checked denominator for each mode.
fn mode_shards(manifest: &Path, enabled: bool) -> Result<usize, Failure> {
    let value = Cmd::new("jaq -r")
        .arg(if enabled {
            ".engine_shards"
        } else {
            ".default_shards"
        })
        .arg(manifest)
        .capture()?;
    value
        .trim()
        .parse()
        .map_err(|_| "engine mode manifest has an invalid denominator".into())
}

/// Compare the entire receipt, including both policy digests, not just summary counters.
fn validate_receipt(
    receipt: &Path,
    manifest: &Path,
    shard: (usize, usize, usize),
    mode: &str,
) -> Outcome {
    let receipt = safe_file(receipt)?;
    let (index, shards, count) = shard;
    let expected = (count - index).div_ceil(shards);
    Cmd::new("jaq -se")
        .args(["--arg", "mode", mode])
        .args(["--argjson", "count", &count.to_string()])
        .args(["--argjson", "index", &index.to_string()])
        .args(["--argjson", "shards", &shards.to_string()])
        .args(["--argjson", "expected", &expected.to_string()])
        .arg(concat!(
            ".[1] == (.[0] + {mutant_count:$count, mode:$mode, ",
            "shard_index:$index, shard_count:$shards, expected_mutants:$expected})"
        ))
        .stdin_bytes(&documents(&[manifest, &receipt])?)
        .capture()
        .map(|_| ())
        .map_err(|_| "engine mode receipt differs from its complete policy and plan".into())
}

/// Preserve the execution mode even when two obligations have identical source identities.
pub(super) fn tagged(job: &Job, source: &Path, name: &str, mode: &str) -> Result<PathBuf, Failure> {
    let path = job.temp.join(format!("{name}-tagged.json"));
    let parent = source
        .parent()
        .ok_or("partition outcome has no parent directory")?;
    let prefix = parent
        .strip_prefix(&job.reports)
        .map_err(|_| "tagged mutation outcomes must belong to retained reports")?;
    let prefix = prefix.to_string_lossy().replace('\\', "/");
    let prefix = if prefix.is_empty() {
        prefix
    } else {
        format!("{prefix}/")
    };
    let value = Cmd::new("jaq -c")
        .args(["--arg", "mode", mode])
        .args(["--arg", "prefix", &prefix])
        .arg(concat!(
            ".outcomes |= map(. + {mutation_mode:$mode} | ",
            ".log_path = (if (.log_path | type) == \"string\" then ",
            "$prefix + .log_path else .log_path end) | ",
            ".diff_path = (if (.diff_path | type) == \"string\" then ",
            "$prefix + .diff_path else .diff_path end))"
        ))
        .arg(source)
        .capture()?;
    write(&path, value.as_bytes(), false)?;
    Ok(path)
}

/// jaq slurps each input file independently, so mode joins use one stdin stream.
fn documents(paths: &[&Path]) -> Result<Vec<u8>, Failure> {
    let mut bytes = Vec::new();
    for path in paths {
        bytes
            .extend(fs::read(path).map_err(|error| format!("cannot read mode evidence: {error}"))?);
        bytes.push(b'\n');
    }
    Ok(bytes)
}

/// Digest exactly the immutable listing bytes kept in this workflow run.
fn file_digest(path: &Path) -> Result<String, Failure> {
    fs::read(path)
        .map(|bytes| sha256_hex(&bytes))
        .map_err(|error| format!("cannot hash downloaded mode listing: {error}").into())
}

#[cfg(test)]
mod tests {
    use super::{mode_shards, tagged};
    use crate::runner::Job;
    use std::path::{Path, PathBuf};
    use std::{env, fs, process};

    #[test]
    fn independent_mode_denominators_refuse_missing_or_fractional_values() {
        let path = env::temp_dir().join(format!("engine-denominator-{}.json", process::id()));
        fs::write(&path, "{\"engine_shards\":1,\"default_shards\":2}").unwrap();
        assert_eq!(mode_shards(&path, true).unwrap(), 1);
        assert_eq!(mode_shards(&path, false).unwrap(), 2);
        fs::write(&path, "{\"engine_shards\":1.5}").unwrap();
        assert_eq!(
            mode_shards(&path, true).unwrap_err().message.as_deref(),
            Some("engine mode manifest has an invalid denominator")
        );
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn tagged_evidence_rejects_unretained_or_parentless_source_paths() {
        let job = Job {
            project: PathBuf::from("project"),
            reports: PathBuf::from("reports"),
            temp: PathBuf::from("temp"),
        };
        assert_eq!(
            tagged(&job, Path::new(""), "mode", "engine")
                .unwrap_err()
                .message
                .as_deref(),
            Some("partition outcome has no parent directory")
        );
        assert_eq!(
            tagged(&job, Path::new("outside/outcomes.json"), "mode", "engine")
                .unwrap_err()
                .message
                .as_deref(),
            Some("tagged mutation outcomes must belong to retained reports")
        );
    }
}

//! Decide whether the current mutation run stays inline or needs every shard.

use super::scope::{self, Scope};
use super::selftest;
use crate::checks::digests::sha256_hex;
use crate::checks::inputs::{mutation_mutants_per_shard, mutation_shards};
use crate::runner::{Cmd, Failure, Job, Outcome, flag, input, optional, output, tee_line, write};
use std::fs;
use std::io;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Output;

/// Serialize the complete scope and execution identity into the plan manifest.
const MANIFEST_QUERY: &str = concat!(
    "{sha:$sha, first_parent:(if $parent == \"\" then null else $parent end), ",
    "directory:$directory, toolchain:$toolchain, ",
    "cargo_mutants_version:$version, run_id:$run, attempt:$attempt, ",
    "config_sha256:$config, diff_sha256:$diff, mutant_count:$count, shard_count:$shards}"
);
/// Require a worker's revision, scope, attempt and tool identity to match the plan.
const WORKER_IDENTITY_QUERY: &str = concat!(
    ".sha == $sha and .first_parent == ",
    "(if $parent == \"\" then null else $parent end) and .directory == $directory ",
    "and .toolchain == $toolchain and .cargo_mutants_version == $version ",
    "and .run_id == $run and .attempt == $attempt ",
    "and .config_sha256 == $config and .diff_sha256 == $diff ",
    "and .mutant_count == $count and .shard_count == $shards"
);

/// Run planning for this job and publish only the small routing values.
pub(super) fn run() -> Outcome {
    let job = Job::current()?;
    let report = job.report("mutants-plan.txt")?;
    if !flag("MUTATION_TEST")? {
        tee_line("Mutation plan: disabled", &report, false)?;
        return routing("disabled", Some(0), 0, &[]);
    }
    selftest::prepare(&job)?;
    let requested = mutation_shards()?;
    if requested == 1 {
        tee_line(
            "Mutation plan: inline (serial default; discovery not run)",
            &report,
            false,
        )?;
        return routing("inline", None, 1, &[]);
    }

    let scope = scope::prepare(&job, &optional("GITHUB_BASE_REF")?)?;
    let listing_path = job.report("mutants-list.json")?;
    let log = job.report("mutants-plan.log")?;
    listing(&job.project, &scope, &listing_path, &log)?;
    let mutants = listing_count(&listing_path)?;
    let target = mutation_mutants_per_shard()?;
    let (mode, shards, matrix, ceiling) = selection(mutants, requested, target);
    let manifest = job.report("mutation-plan.json")?;
    write_manifest(&manifest, &job.project, &scope, mutants, shards)?;
    plan_report(&report, mode, mutants, shards, ceiling)?;
    routing(mode, Some(mutants), shards, &matrix)
}

/// Run the pinned listing once, preserving its JSON and logs separately.
fn listing(project: &Path, scope: &Scope, json: &Path, log: &Path) -> Outcome {
    let mut command = Cmd::new(
        "cargo mutants --list --json --no-shuffle --cargo-arg=--locked --colors=never --level=info",
    );
    if let Some(diff) = &scope.diff {
        let diff = diff.to_string_lossy().into_owned();
        command = command.args(["--in-diff", &diff]);
    }
    let result = command.cwd(project).capture_output()?;
    show(&result)?;
    write(log, &result.stderr, false)?;
    write(json, &result.stdout, false)?;
    if !result.status.success() {
        return Err(Failure::status(result.status.code().unwrap_or(1)));
    }
    if result.stdout.iter().all(u8::is_ascii_whitespace) {
        if recognized_no_work(&result.stderr, scope.change.is_some()) {
            write(json, b"[]\n", false)?;
        } else {
            return Err(
                "cargo-mutants listing was empty without a recognized no-work result".into(),
            );
        }
    }
    validate_listing(json)
}

/// Print both streams just as the original step did, while retaining the JSON.
fn show(result: &Output) -> Outcome {
    io::stderr()
        .write_all(&result.stderr)
        .map_err(|error| format!("cannot print cargo-mutants diagnostics: {error}"))?;
    Ok(())
}

/// Only the pinned tool's known no-work diagnostics justify an empty listing.
fn recognized_no_work(stderr: &[u8], has_diff: bool) -> bool {
    let text = String::from_utf8_lossy(stderr);
    text.lines().any(|line| {
        line.trim() == "WARN No mutants found under the active filters"
            || (has_diff
                && matches!(
                    line.trim(),
                    "INFO Diff file is empty"
                        | "INFO Diff changes no Rust source files"
                        | "INFO No mutants to filter"
                ))
    })
}

/// Validate the versioned listing schema and unique, stable mutant identities.
pub(super) fn validate_listing(listing: &Path) -> Outcome {
    const SCHEMA: &str = concat!(
        "type == \"array\" and all(.[]; ",
        "(.package | type == \"string\" and length > 0) ",
        "and (.name | type == \"string\" and length > 0) ",
        "and (.file | type == \"string\" and length > 0) ",
        "and (.replacement | type == \"string\") ",
        "and (.span.start.line | type == \"number\" and floor == .) ",
        "and (.span.start.column | type == \"number\" and floor == .) ",
        "and (.span.end.line | type == \"number\" and floor == .) ",
        "and (.span.end.column | type == \"number\" and floor == .))"
    );
    const UNIQUE_IDENTITIES: &str = concat!(
        "([.[] | [.package, .name] | tojson] | length) == ",
        "([.[] | [.package, .name] | tojson] | unique | length)"
    );
    Cmd::new("jaq -e")
        .arg(SCHEMA)
        .arg(listing)
        .capture()
        .map_err(|_| "cargo-mutants listing is malformed JSON")?;
    Cmd::new("jaq -e")
        .arg(UNIQUE_IDENTITIES)
        .arg(listing)
        .capture()
        .map_err(|_| "cargo-mutants listing contains duplicate mutant identities")?;
    Ok(())
}

/// The number of complete mutant records in a validated listing.
fn listing_count(listing: &Path) -> Result<usize, Failure> {
    let count = Cmd::new("jaq -er").arg("length").arg(listing).capture()?;
    parse_count(&count, "cargo-mutants listing has no integral length")
}

/// Compute every nonempty shard, never sampling the listing.
fn selection(
    mutants: usize,
    requested: usize,
    target: usize,
) -> (&'static str, usize, Vec<usize>, bool) {
    if mutants == 0 {
        return ("empty", 0, Vec::new(), false);
    }
    let (shards, ceiling) = if requested == 0 {
        let desired = mutants.div_ceil(target);
        (desired.min(32), desired > 32)
    } else {
        (requested.min(mutants), false)
    };
    let matrix = if shards == 1 {
        Vec::new()
    } else {
        (0..shards).collect()
    };
    (
        if shards == 1 { "inline" } else { "sharded" },
        shards,
        matrix,
        ceiling,
    )
}

/// Save the full run identity outside job outputs; workers and aggregation verify it.
fn write_manifest(
    path: &Path,
    project: &Path,
    scope: &Scope,
    mutants: usize,
    shards: usize,
) -> Outcome {
    let tested = input("GITHUB_SHA")?;
    let run = input("GITHUB_RUN_ID")?;
    let attempt = input("GITHUB_RUN_ATTEMPT")?;
    let toolchain = input("RUSTUP_TOOLCHAIN")?;
    let version = input("CARGO_MUTANTS_VERSION")?;
    let parent = scope.parent.as_deref().unwrap_or_default();
    let directory = scope::normalized_directory()?;
    let config_digest = scope::configuration_digest(project)?;
    let mut args = Vec::new();
    for (name, value) in [
        ("sha", tested.as_str()),
        ("parent", parent),
        ("directory", directory.as_str()),
        ("toolchain", toolchain.as_str()),
        ("version", version.as_str()),
        ("run", run.as_str()),
        ("attempt", attempt.as_str()),
        ("config", config_digest.as_str()),
        ("diff", scope.diff_digest.as_str()),
    ] {
        args.extend(["--arg".to_owned(), name.to_owned(), value.to_owned()]);
    }
    for (name, value) in [("count", mutants), ("shards", shards)] {
        args.extend(["--argjson".to_owned(), name.to_owned(), value.to_string()]);
    }
    let json = Cmd::new("jaq -c -n")
        .args(args)
        .arg(MANIFEST_QUERY)
        .capture()?;
    write(path, format!("{json}\n").as_bytes(), false)
}

/// Write one human-readable, complete selection decision.
fn plan_report(path: &Path, mode: &str, mutants: usize, shards: usize, ceiling: bool) -> Outcome {
    let mut text = format!("Mutation plan: {mode}; {mutants} mutants; {shards} shard(s)");
    if ceiling {
        text.push_str("; automatic 32-shard ceiling reached, target exceeded");
    }
    tee_line(&text, path, false)
}

/// Reuse the planning diff for inline work, or build the original scope once.
pub(super) fn planned_scope(job: &Job, base: &str) -> Result<Scope, Failure> {
    let manifest = job.earlier("mutation-plan.json");
    if !manifest.is_file() {
        return scope::prepare(job, base);
    }
    let parent = Cmd::new("jaq -r")
        .arg(".first_parent // \"\"")
        .arg(&manifest)
        .capture()?
        .trim()
        .to_owned();
    if !base.is_empty() && parent.is_empty() {
        return Err("pull request checkout must include the base parent".into());
    }
    let digest = Cmd::new("jaq -r")
        .arg(".diff_sha256")
        .arg(&manifest)
        .capture()?
        .trim()
        .to_owned();
    let (parent, diff, change) = if parent.is_empty() {
        if job.earlier("mutants.diff").exists() {
            return Err("parentless mutation plan unexpectedly contains a diff".into());
        }
        if digest != sha256_hex(b"full-workspace;no-first-parent") {
            return Err("mutation plan has an invalid full-workspace digest".into());
        }
        (None, None, None)
    } else {
        let diff = job.earlier("mutants.diff");
        let bytes = fs::read(&diff)
            .map_err(|error| format!("cannot read planned diff {}: {error}", diff.display()))?;
        if sha256_hex(&bytes) != digest {
            return Err("mutation plan diff digest does not match its report".into());
        }
        let change = if base.is_empty() {
            "this commit"
        } else {
            "this pull request"
        };
        (Some(parent), Some(diff), Some(change.to_owned()))
    };
    Ok(Scope {
        parent,
        diff,
        change,
        diff_digest: digest,
    })
}

/// Check a worker's plan, source tree and all run identity before mutation.
pub(super) fn verify_worker(
    job: &Job,
    source_scope: &Scope,
    manifest: &Path,
    listing: &Path,
    shard: (usize, usize),
) -> Result<usize, Failure> {
    let (index, shards) = shard;
    let manifest = safe_plan_file(job, manifest)?;
    let listing = safe_plan_file(job, listing)?;
    validate_listing(&listing)?;
    let mutants = listing_count(&listing)?;
    if mutants == 0 || manifest_count(&manifest)? != mutants {
        return Err("mutation plan count does not match its complete listing".into());
    }
    if mutation_shards()? != shards {
        return Err("mutation worker shard count differs from its planned matrix".into());
    }
    let tested = input("GITHUB_SHA")?;
    let run = input("GITHUB_RUN_ID")?;
    let attempt = input("GITHUB_RUN_ATTEMPT")?;
    let toolchain = input("RUSTUP_TOOLCHAIN")?;
    let version = input("CARGO_MUTANTS_VERSION")?;
    let directory = scope::normalized_directory()?;
    let config_digest = scope::configuration_digest(&job.project)?;
    let parent = source_scope.parent.as_deref().unwrap_or_default();
    let mut args = Vec::new();
    for (name, value) in [
        ("sha", tested.as_str()),
        ("parent", parent),
        ("directory", directory.as_str()),
        ("toolchain", toolchain.as_str()),
        ("version", version.as_str()),
        ("run", run.as_str()),
        ("attempt", attempt.as_str()),
        ("config", config_digest.as_str()),
        ("diff", source_scope.diff_digest.as_str()),
    ] {
        args.extend(["--arg".to_owned(), name.to_owned(), value.to_owned()]);
    }
    for (name, value) in [("count", mutants), ("shards", shards)] {
        args.extend(["--argjson".to_owned(), name.to_owned(), value.to_string()]);
    }
    let identity = Cmd::new("jaq -e")
        .args(args)
        .arg(WORKER_IDENTITY_QUERY)
        .arg(&manifest)
        .capture();
    identity
        .map_err(|_| "mutation worker identity or scope differs from its plan; Re-run all jobs")?;
    let expected = if index >= mutants {
        0
    } else {
        (mutants - index).div_ceil(shards)
    };
    if expected == 0 {
        return Err("mutation plan assigns an empty shard".into());
    }
    Ok(expected)
}

/// Write the worker receipt before the long mutation command starts.
pub(super) fn write_receipt(
    path: &Path,
    manifest: &Path,
    index: usize,
    shards: usize,
    expected: usize,
) -> Outcome {
    let receipt = Cmd::new("jaq -c")
        .args(["--argjson", "index", &index.to_string()])
        .args(["--argjson", "shards", &shards.to_string()])
        .args(["--argjson", "expected", &expected.to_string()])
        .arg(". + {shard_index:$index, shard_count:$shards, expected_mutants:$expected}")
        .arg(manifest)
        .capture()?;
    write(path, format!("{receipt}\n").as_bytes(), false)
}

/// A manifest's checked mutant count.
pub(super) fn manifest_count(path: &Path) -> Result<usize, Failure> {
    let count = Cmd::new("jaq -r")
        .arg(".mutant_count")
        .arg(path)
        .capture()?;
    parse_count(&count, "mutation plan has no integral mutant count")
}

/// Parse one unsigned integral count from the pinned JSON reader.
fn parse_count(value: &str, message: &'static str) -> Result<usize, Failure> {
    value.trim().parse().map_err(|_| message.into())
}

/// A plan or listing is data only when a regular file inside runner temp.
fn safe_plan_file(job: &Job, path: &Path) -> Result<PathBuf, Failure> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect mutation plan {}: {error}", path.display()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(format!(
            "mutation plan input is not a regular file: {}",
            path.display()
        )
        .into());
    }
    let root = fs::canonicalize(&job.temp)
        .map_err(|error| format!("cannot resolve runner temp: {error}"))?;
    let path = fs::canonicalize(path)
        .map_err(|error| format!("cannot resolve mutation plan file: {error}"))?;
    if !path.starts_with(root) {
        return Err("mutation plan input escapes runner temp".into());
    }
    Ok(path)
}

/// Parse only a zero-based shard index/count the complete matrix can contain.
pub(super) fn parse_shard(value: &str) -> Result<(usize, usize), Failure> {
    let refusal = || "MUTATION_SHARD must be a zero-based K/N with 2 <= N <= 32 and K < N";
    let Some((index, count)) = value.split_once('/') else {
        return Err(refusal().into());
    };
    if count.contains('/') {
        return Err(refusal().into());
    }
    let (Ok(index), Ok(count)) = (index.parse::<usize>(), count.parse::<usize>()) else {
        return Err(refusal().into());
    };
    if !(2..=32).contains(&count) || index >= count {
        return Err(refusal().into());
    }
    Ok((index, count))
}

/// Publish the only plan data needed in GitHub's job matrix.
fn routing(mode: &str, mutants: Option<usize>, shards: usize, matrix: &[usize]) -> Outcome {
    output("mutation-mode", mode)?;
    output(
        "mutation-count",
        &mutants.map_or_else(String::new, |count| count.to_string()),
    )?;
    output("mutation-shards", &shards.to_string())?;
    output(
        "mutation-matrix",
        &format!(
            "[{}]",
            matrix
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(",")
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::{parse_count, parse_shard, safe_plan_file, selection};
    use crate::runner::Job;
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use std::process;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn test_root() -> PathBuf {
        let root = env::temp_dir().join(format!(
            "rust-gate-mutation-plan-{}-{}",
            process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn selection_and_worker_numbers_are_complete_and_bounded() {
        assert_eq!(selection(0, 0, 50), ("empty", 0, vec![], false));
        assert_eq!(selection(51, 0, 50), ("sharded", 2, vec![0, 1], false));
        assert_eq!(parse_shard("31/32").unwrap(), (31, 32));
        for value in ["", "2/2", "0/33", "0/x"] {
            assert_eq!(
                parse_shard(value).unwrap_err().message.as_deref(),
                Some("MUTATION_SHARD must be a zero-based K/N with 2 <= N <= 32 and K < N")
            );
        }
    }

    #[test]
    fn json_counts_require_unsigned_integral_values() {
        assert_eq!(parse_count("5", "unused").unwrap(), 5);
        assert_eq!(
            parse_count(
                "not-a-count",
                "cargo-mutants listing has no integral length"
            )
            .unwrap_err()
            .message
            .as_deref(),
            Some("cargo-mutants listing has no integral length")
        );
        assert_eq!(
            parse_count("5.5", "mutation plan has no integral mutant count")
                .unwrap_err()
                .message
                .as_deref(),
            Some("mutation plan has no integral mutant count")
        );
    }

    #[test]
    fn worker_plan_inputs_stay_inside_temp_and_are_regular_files() {
        let root = test_root();
        let temp = root.join("temp");
        fs::create_dir_all(&temp).unwrap();
        let job = Job {
            project: root.clone(),
            reports: root.join("reports"),
            temp,
        };
        let outside = root.join("outside.json");
        fs::write(&outside, "{}").unwrap();
        assert_eq!(
            safe_plan_file(&job, &outside)
                .unwrap_err()
                .message
                .as_deref(),
            Some("mutation plan input escapes runner temp")
        );
        let directory = job.temp.join("directory.json");
        fs::create_dir(&directory).unwrap();
        assert!(
            safe_plan_file(&job, &directory)
                .unwrap_err()
                .message
                .as_deref()
                .unwrap()
                .contains("mutation plan input is not a regular file:")
        );
        fs::remove_dir_all(root).unwrap();
    }
}

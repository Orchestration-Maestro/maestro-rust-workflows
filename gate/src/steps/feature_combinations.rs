//! `rust-gate features`: every feature the workspace declares must compile.
//! A default build proves one combination out of many, so a feature nobody
//! selects in CI can stop compiling and ship that way: the consumer who turns
//! it on is the one who finds out. `cargo hack --each-feature` builds each
//! feature on its own, the default set, none of them and all of them. It does
//! not enumerate the powerset or every optional feature with defaults enabled.
//! A workspace declaring no feature has nothing to check, and the
//! step says so rather than spending a compile to prove it.

use crate::runner::{Cmd, Failure, Job, Outcome, Step, output};
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

/// What this step declares: its inputs, its tools and its reports.
pub(crate) const STEPS: &[Step] = &[Step {
    workflow: "ci",
    id: "features",
    summary: "Every declared feature compiles",
    inputs: &[],
    tools: &["cargo metadata", "cargo hack", "jaq"],
    reports: &["features.txt"],
    run,
}];

/// Every feature name the workspace members declare, one per line. `default`
/// is one of them: a crate whose only feature is `default` still has the
/// combination where it is off, which is exactly the one nobody builds. A
/// member without the field is an empty map rather than an error, and the
/// query stays out of `jaq -e`, whose exit status turns no output into a
/// failure when having no feature at all is the ordinary case.
const DECLARED: &str = ".workspace_members as $m | .packages[] |
  select(.id as $i | $m | index($i)) | (.features // {}) | keys[]";

/// Canonical identities from the real TOML parser. JSON preserves absent
/// source/checksum fields distinctly from empty strings; the name prefixes
/// each record so a refusal can identify the changed package.
const LOCKED_PACKAGES: &str = ".package[] | .name + \"\\t\" +
  ([.name, .version, .source, .checksum] | tojson)";

/// Only removal is allowed: dependency edges may disappear with dev-deps, but
/// every remaining package's version, source and checksum must stay locked.
fn check_locked_packages(original: &str, remaining: &str) -> Outcome {
    let locked: BTreeSet<&str> = original.lines().collect();
    for package in remaining.lines() {
        if !locked.contains(package) {
            let name = package.split('\t').next().unwrap_or_default();
            return Err(format!("feature check changed locked package: {name}").into());
        }
    }
    Ok(())
}

/// Exact bytes of every file cargo-hack can rewrite. Drop is a fallback for
/// early returns; explicit restoration reports write errors to the caller.
struct Snapshot {
    /// Workspace member manifests, including a virtual workspace's root.
    manifests: Vec<(PathBuf, Vec<u8>)>,
    /// The lockfile whose resolved identities are checked after cargo-hack.
    lock: PathBuf,
    /// Original lock bytes, including comments and formatting.
    original_lock: Vec<u8>,
}

impl Snapshot {
    /// Discover current workspace members without resolving dependencies, then
    /// read every byte before allowing cargo-hack to modify anything.
    fn read(job: &Job) -> Result<Self, Failure> {
        let metadata = Cmd::new("cargo metadata --offline --no-deps --format-version 1")
            .cwd(&job.project)
            .capture()?;
        let paths = Cmd::new("jaq -r")
            .arg(
                ".workspace_members as $m | .workspace_root + \"/Cargo.toml\",
              (.packages[] | select(.id as $i | $m | index($i)) | .manifest_path)",
            )
            .stdin_bytes(metadata.as_bytes())
            .capture()?;
        let mut paths: Vec<PathBuf> = paths.lines().map(PathBuf::from).collect();
        paths.sort_unstable();
        paths.dedup();
        let mut manifests = Vec::new();
        for path in paths {
            let bytes = fs::read(&path)
                .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
            manifests.push((path, bytes));
        }
        let lock = job.project.join("Cargo.lock");
        let original_lock =
            fs::read(&lock).map_err(|error| format!("cannot read {}: {error}", lock.display()))?;
        Ok(Self {
            manifests,
            lock,
            original_lock,
        })
    }

    /// Try every restoration even when one file cannot be written. The first
    /// error fails the step rather than accepting incompletely restored files.
    fn restore(&self) -> Outcome {
        let mut result = Ok(());
        for (path, bytes) in self
            .manifests
            .iter()
            .map(|(path, bytes)| (path, bytes))
            .chain([(&self.lock, &self.original_lock)])
        {
            let restored = fs::write(path, bytes).map_err(|error| {
                Failure::from(format!("cannot restore {}: {error}", path.display()))
            });
            if result.is_ok() {
                result = restored;
            }
        }
        result
    }
}

impl Drop for Snapshot {
    fn drop(&mut self) {
        if let Err(error) = self.restore() {
            eprintln!("{error:?}");
        }
    }
}

/// Run the step.
fn run() -> Outcome {
    let job = Job::current()?;
    let declared = Cmd::new("jaq -r")
        .arg(DECLARED)
        .arg(job.temp.join("metadata.json"))
        .capture()?;
    let mut names: Vec<&str> = declared
        .lines()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .collect();
    names.sort_unstable();
    names.dedup();
    let report = job.report("features.txt")?;
    fs::write(&report, names.join("\n"))
        .map_err(|error| format!("cannot write {}: {error}", report.display()))?;
    if names.is_empty() {
        println!("SKIPPED: the workspace declares no feature");
        return output("applied", "false");
    }
    // `check` rather than `build`: a combination that fails does so in the
    // front end, and a consumer pays for one type check per feature instead of
    // one link. `--each-feature` is linear in the number of features, where a
    // powerset is exponential and would price this gate out of every run.
    let snapshot = Snapshot::read(&job)?;
    let locked = Cmd::new("jaq -r --from toml")
        .arg(LOCKED_PACKAGES)
        .stdin_bytes(&snapshot.original_lock)
        .capture()?;
    // cargo-hack 0.6.45 src/manifest.rs:103-104,176-187 restores the lock
    // under --no-dev-deps, hiding drift. --remove-dev-deps has the same check
    // semantics but leaves restoration to us (src/context.rs:47-48).
    // Earlier locked steps fill a fresh CI Cargo home. Offline resolution can
    // prune dev-only packages without fetching alternatives; reject any drift
    // before accepting a result, including when run with a warmer local cache.
    let checked =
        Cmd::new("cargo hack check --workspace --offline --each-feature --remove-dev-deps")
            .cwd(&job.project)
            .run();
    let validated = Cmd::new("jaq -r --from toml")
        .arg(LOCKED_PACKAGES)
        .arg(&snapshot.lock)
        .capture()
        .and_then(|remaining| check_locked_packages(&locked, &remaining));
    // Do not propagate either failure until every original byte is restored.
    snapshot.restore()?;
    validated?;
    checked?;
    output("applied", "true")
}

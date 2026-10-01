//! The jobs of `ci.yml` as a local run takes them: every step of the `checks`
//! job in its order, each with the gate command the run starts for it, what
//! the run does in its place, or why it is not applied here; then the other
//! jobs, none of which applies. One list: a contract test holds it to
//! `ci.yml`, so a step CI gains is refused until this list says what a local
//! run does with it.

/// What a local run does for one step of the `checks` job.
pub(super) enum Local {
    /// `rust-gate <id>`, the step's own body.
    Gate(&'static str),
    /// The commit a pull request's run checks out, made from the branch here.
    Checkout,
    /// The toolbelt `rust-gate setup` installs, every tool the job installs
    /// at the same pin.
    Toolbelt,
    /// What CI's cache restores: the registry and the build directory the
    /// last local run of this repository kept.
    Restore,
    /// Nothing, for the reason given.
    NotApplied(&'static str),
}

/// Why the job's own installations are not applied: the toolbelt already is.
const IN_THE_TOOLBELT: &str =
    "the toolbelt rust-gate setup installs holds the tool at the pin CI installs";

/// Every step of the `checks` job of `ci.yml`, by its name, in its order.
pub(super) const CHECKS: &[(&str, Local)] = &[
    (
        "Checkout the gate at this workflow's commit",
        Local::NotApplied("the gate running this is the release the repository pins"),
    ),
    (
        "Build the gate",
        Local::NotApplied("the gate running this is already built"),
    ),
    ("Checkout consumer revision", Local::Checkout),
    ("Install the settings reader", Local::Toolbelt),
    ("Validate consumer inputs", Local::Gate("validate")),
    (
        "Configure private Cargo state for direct crates.io access",
        Local::Gate("registry"),
    ),
    ("Restore Cargo registry and build cache", Local::Restore),
    ("Install verified tools", Local::NotApplied(IN_THE_TOOLBELT)),
    (
        "Install the mutation testing tool",
        Local::NotApplied(IN_THE_TOOLBELT),
    ),
    (
        "Install the API compatibility checker",
        Local::NotApplied(IN_THE_TOOLBELT),
    ),
    (
        "Install the SARIF converter",
        Local::NotApplied(IN_THE_TOOLBELT),
    ),
    (
        "Install the unused-dependency tool",
        Local::NotApplied(IN_THE_TOOLBELT),
    ),
    (
        "Install the dependency audit tool",
        Local::NotApplied(IN_THE_TOOLBELT),
    ),
    ("Install the toolchain", Local::Gate("tools")),
    (
        "Source rules ARC, SIZE, NAME, DOC, LIB, TST, WSP and LNT",
        Local::Gate("architecture"),
    ),
    (
        "Repository hygiene HYG and the width of shell scripts",
        Local::Gate("hygiene"),
    ),
    (
        "Managed files as the organization renders them",
        Local::Gate("managed-files"),
    ),
    ("Commit hooks over every file", Local::Gate("hooks")),
    ("Formatting, Clippy and tests", Local::Gate("quality")),
    (
        "Function and file sizes, never blocking",
        Local::Gate("complexity"),
    ),
    (
        "Duplicated functions, pairs reported and three alike refused",
        Local::Gate("duplication"),
    ),
    (
        "Prepare private native cache restore",
        Local::NotApplied(
            "only GitHub provisions transported native entries; local builds use source",
        ),
    ),
    (
        "Restore published native entries",
        Local::NotApplied("only GitHub restores native cache archives"),
    ),
    ("Line coverage gate", Local::Gate("coverage")),
    (
        "Coverage of the lines a pull request adds",
        Local::Gate("changed-coverage"),
    ),
    ("Dependency vulnerability audit", Local::Gate("audit")),
    (
        "Licence, dependency-ban and source policy",
        Local::Gate("licenses"),
    ),
    (
        "Plan mutation shard routing",
        Local::NotApplied(
            "local CI always runs the full mutation suite inline; shard planning is remote-only",
        ),
    ),
    ("Mutation testing", Local::Gate("mutants")),
    (
        "Instruction counts of the declared benchmarks against the base",
        Local::Gate("performance"),
    ),
    ("Public API compatibility", Local::Gate("api")),
    ("Redacted source secret scan", Local::Gate("secrets")),
    (
        "Declared minimum supported Rust version",
        Local::Gate("msrv"),
    ),
    ("Every declared feature compiles", Local::Gate("features")),
    ("Unused declared dependencies", Local::Gate("unused")),
    ("Recorded dependency audits", Local::Gate("vet")),
    (
        "Release build, verified packages and SBOMs",
        Local::Gate("build"),
    ),
    (
        "Reproducible build and binary hardening",
        Local::Gate("hardening"),
    ),
    ("Pull request rules PRL", Local::Gate("pull-request")),
    ("Stage immutable release payload", Local::Gate("stage")),
    (
        "Upload release payload",
        Local::NotApplied("only a GitHub run keeps an artifact; the payload stays in rust-release"),
    ),
    ("Build quality scorecard", Local::Gate("scorecard")),
    (
        "Upload diagnostic reports",
        Local::NotApplied("only a GitHub run keeps an artifact; the reports stay in rust-reports"),
    ),
    (
        "Upload immutable engine mutation plan",
        Local::NotApplied("only GitHub keeps the immutable engine plan artifact"),
    ),
    (
        "Upload inline mutation outcomes for partition aggregation",
        Local::NotApplied("only GitHub harvests the inline default mutation artifacts"),
    ),
    (
        "Check native cache save inventory",
        Local::NotApplied("local runs never publish remote native cache snapshots"),
    ),
    (
        "Save published native entries",
        Local::NotApplied("only a successful default-branch GitHub event saves native entries"),
    ),
];

/// The step CI runs whatever failed before it, once `validate` passed.
pub(super) const ALWAYS: &str = "scorecard";

/// Every other job of `ci.yml`, by its id, and why none applies here.
pub(super) const OTHER_JOBS: &[(&str, &str)] = &[
    (
        "portability",
        "it builds and tests on GitHub's macOS and Windows runners",
    ),
    (
        "mutations",
        "only a GitHub run schedules the remote mutation matrix",
    ),
    (
        "mutation-summary",
        "only a GitHub run aggregates remote mutation shards",
    ),
    (
        "mutation-engine-default",
        "featureless engine-file controls run in separate GitHub workers",
    ),
    (
        "mutation-engine",
        "engine-owned mutation modes run in separate GitHub workers",
    ),
    (
        "mutation-windows",
        "Windows-owned mutation testing runs on GitHub's Windows runner",
    ),
    ("upload", "only a GitHub run uploads SARIF to code scanning"),
    ("coverage", "only a GitHub run uploads coverage to Codecov"),
    (
        "gate",
        "it requires the jobs above, whose steps this run reports",
    ),
];

/// What the scorecard reads of the steps before it, as `ci.yml` hands it:
/// each variable and the step whose outcome it carries.
pub(super) const OUTCOMES: &[(&str, &str)] = &[
    ("OUT_QUALITY", "quality"),
    ("OUT_COVERAGE", "coverage"),
    ("OUT_AUDIT", "audit"),
    ("OUT_SECRETS", "secrets"),
    ("OUT_MSRV", "msrv"),
    ("OUT_FEATURES", "features"),
    ("OUT_LICENCES", "licenses"),
    ("OUT_MUTANTS", "mutants"),
    ("OUT_UNUSED", "unused"),
    ("OUT_STAGE", "stage"),
    ("OUT_API", "api"),
    ("OUT_ARCHITECTURE", "architecture"),
    ("OUT_HYGIENE", "hygiene"),
    ("OUT_MANAGED_FILES", "managed-files"),
    ("OUT_HOOKS", "hooks"),
    ("OUT_DUPLICATION", "duplication"),
    ("OUT_CHANGED_COVERAGE", "changed-coverage"),
    ("OUT_PULL_REQUEST", "pull-request"),
    ("OUT_VET", "vet"),
    ("OUT_PERFORMANCE", "performance"),
];

/// The same for the steps' `applied` outputs.
pub(super) const APPLIED: &[(&str, &str)] = &[
    ("FEATURES_APPLIED", "features"),
    ("MUTANTS_APPLIED", "mutants"),
    ("API_APPLIED", "api"),
    ("HOOKS_APPLIED", "hooks"),
    ("CHANGED_COVERAGE_APPLIED", "changed-coverage"),
    ("PULL_REQUEST_APPLIED", "pull-request"),
    ("PERFORMANCE_APPLIED", "performance"),
];

/// Why the step `id` cannot run on this machine, if it cannot: on `os`, with
/// Valgrind on the PATH or not.
pub(super) fn not_here(id: &str, os: &str, valgrind: bool) -> Option<&'static str> {
    match id {
        "hardening" if os != "linux" => {
            Some("it reads ELF binaries, which only a Linux build writes; CI runs it on Linux")
        }
        "performance" if !valgrind => Some(
            "gungraun counts instructions under Valgrind, which is not on the PATH here; CI \
             runs it on Linux",
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{APPLIED, CHECKS, Local, OUTCOMES, not_here};

    #[test]
    fn only_linux_hardens_and_only_valgrind_counts_instructions() {
        assert!(not_here("hardening", "linux", false).is_none());
        assert!(not_here("hardening", "macos", true).is_some_and(|why| why.contains("ELF")));
        assert!(not_here("performance", "linux", true).is_none());
        assert!(
            not_here("performance", "linux", false).is_some_and(|why| why.contains("Valgrind"))
        );
        assert!(not_here("quality", "windows", false).is_none());
    }

    #[test]
    fn every_step_the_scorecard_reads_is_a_step_the_run_starts() {
        let started: Vec<&str> = CHECKS
            .iter()
            .filter_map(|(_, local)| match local {
                Local::Gate(id) => Some(*id),
                _ => None,
            })
            .collect();
        for (variable, id) in OUTCOMES.iter().chain(APPLIED) {
            assert!(started.contains(id), "{variable} reads {id}");
        }
    }
}

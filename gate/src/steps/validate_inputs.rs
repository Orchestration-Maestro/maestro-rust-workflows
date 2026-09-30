//! `rust-gate validate`: every `ci.yml` input checked before any side effect,
//! then the resolved project, toolchain and gate selectors exported to the
//! rest of the job. A run no workflow called, the one an organization ruleset
//! starts, takes the same values from `maestro-quality.toml` instead: the
//! base commit's, and the head's for mutation ownership.

use crate::checks::checkout_paths::{canonical, committed_file, inside, project_directory};
use crate::checks::digests::sha256_hex;
use crate::checks::inputs::{
    LicensePolicy, artifact_key, clippy_level, coverage_threshold, internal_shard_selftest,
    license_policy, mutation_mutants_per_shard, mutation_shards, unsafe_policy,
};
use crate::checks::mutation_engine;
use crate::checks::quality_config::{FILE, QualityConfig, mutation_windows, read_config};
use crate::checks::rust_versions::{channel_value, is_exact_stable, parse};
use crate::runner::{Cmd, Failure, Outcome, Step, export, flag, input, optional, output};
use std::fs;
use std::path::Path;

/// What this step declares: its inputs, its tools and its reports.
pub(crate) const STEPS: &[Step] = &[Step {
    workflow: "ci",
    id: "validate",
    summary: "Validate consumer inputs",
    inputs: &[
        "API_COMPATIBILITY",
        "ARTIFACT_KEY",
        "CALLED",
        "CLIPPY_LEVEL",
        "COVERAGE",
        "DEPENDENCY_AUDIT",
        "DIRECTORY",
        "GITHUB_RUN_ATTEMPT",
        "GITHUB_RUN_ID",
        "GITHUB_REPOSITORY",
        "GITHUB_SHA",
        "GITHUB_WORKSPACE",
        "INTERNAL_SHARD_SELFTEST",
        "LICENSE_POLICY",
        "MUTATION_MUTANTS_PER_SHARD",
        "MUTATION_ENGINE_POLICY",
        "MUTATION_SHARDS",
        "MUTATION_TEST",
        "MUTATION_WINDOWS",
        "PLATFORMS",
        "REQUESTED_TOOLCHAIN",
        "SARIF_REPORTS",
        "UNSAFE_POLICY",
        "UNUSED_DEPENDENCIES",
    ],
    tools: &["cargo metadata", "git", "jaq", "rust-gate"],
    reports: &[],
    run,
}];

/// The floor: any exact stable release from here up is accepted.
const MSRV: (u64, u64, u64) = (1, 85, 0);

/// Each `ci.yml` input, the variable this step reads it from, and the
/// input's default: what a run no workflow called takes when `[ci]` leaves
/// the input out. Every Rust repository tests macOS and Windows.
const SETTINGS: &[(&str, &str, &str)] = &[
    ("working-directory", "DIRECTORY", "."),
    ("rust-version", "REQUESTED_TOOLCHAIN", ""),
    ("coverage-threshold", "COVERAGE", "90"),
    ("artifact-key", "ARTIFACT_KEY", "ci"),
    ("license-policy", "LICENSE_POLICY", "auto"),
    ("mutation-test", "MUTATION_TEST", "true"),
    ("mutation-shards", "MUTATION_SHARDS", "1"),
    (
        "mutation-mutants-per-shard",
        "MUTATION_MUTANTS_PER_SHARD",
        "50",
    ),
    ("mutation-windows", "MUTATION_WINDOWS", "[]"),
    ("sarif-reports", "SARIF_REPORTS", "true"),
    ("clippy-level", "CLIPPY_LEVEL", "default"),
    ("dependency-audit", "DEPENDENCY_AUDIT", "true"),
    ("unsafe-policy", "UNSAFE_POLICY", "deny"),
    ("unused-dependencies", "UNUSED_DEPENDENCIES", "true"),
    ("platforms", "PLATFORMS", "macos windows"),
    ("api-compatibility", "API_COMPATIBILITY", "true"),
];

/// Encode a list for outputs passed through the workflow.
fn json_strings(values: &[String]) -> Result<String, Failure> {
    let mut command = Cmd::new("jaq -cn").arg("$ARGS.positional").args(["--args"]);
    for value in values {
        command = command.arg(value);
    }
    command.capture().map(|value| value.trim().to_owned())
}

/// Run the step: the checks in the order a consumer sees them fail, then
/// the resolved values exported to the rest of the job.
fn run() -> Outcome {
    if !flag("CALLED")? {
        return from_settings();
    }
    if internal_shard_selftest()?
        && (input("DIRECTORY")? != "examples/workspace"
            || input("MUTATION_TEST")? != "true"
            || mutation_shards()? != 2)
    {
        return Err(concat!(
            "internal-shard-selftest requires mutation-test=true, mutation-shards=2, and ",
            "working-directory=examples/workspace"
        )
        .into());
    }
    let root = canonical(Path::new(&input("GITHUB_WORKSPACE")?))?;
    let project = project_directory()?;
    for name in ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml"] {
        committed_file(&project, name)?;
    }
    let toolchain = selected_toolchain(&project)?;
    coverage_threshold()?;
    let artifact_key = artifact_key()?;
    let license_policy = license_policy()?;
    if license_policy == LicensePolicy::Off {
        return Err(
            "license-policy=off is refused: the organization's licence policy always \
                    applies, and no repository opts out"
                .into(),
        );
    }
    let unsafe_policy = unsafe_policy()?;
    let clippy_level = clippy_level()?;
    let unused_dependencies = flag("UNUSED_DEPENDENCIES")?.to_string();
    let mutation_test = flag("MUTATION_TEST")?.to_string();
    let mutation_shards = mutation_shards()?.to_string();
    let mutation_mutants_per_shard = mutation_mutants_per_shard()?.to_string();
    let mutation_windows = mutation_windows(&project, &input("MUTATION_WINDOWS")?)?;
    let mutation_windows_json = Cmd::new("jaq -cn")
        .arg("$ARGS.positional")
        .args(["--args"])
        .args(mutation_windows.iter())
        .capture()?;
    let api_compatibility = flag("API_COMPATIBILITY")?.to_string();
    let sarif_reports = flag("SARIF_REPORTS")?.to_string();
    let dependency_audit = flag("DEPENDENCY_AUDIT")?.to_string();
    let runners = platform_runners()?;
    let deny_config = deny_configuration(&project, &root)?;
    let directory = relative_directory(&project, &root)?;
    export_engine_policy(&project, &mutation_windows)?;
    output(
        "artifact-name",
        &artifact_name(&directory, &artifact_key, &toolchain)?,
    )?;
    output("directory", &directory)?;
    output("platforms", &runners)?;
    output("toolchain", &toolchain)?;
    output("mutation-test", &mutation_test)?;
    output("mutation-mutants-per-shard", &mutation_mutants_per_shard)?;
    output("mutation-windows", mutation_windows_json.trim())?;
    let temp = input("RUNNER_TEMP")?;
    export(&[
        ("PROJECT", &project.display().to_string()),
        ("RUSTUP_TOOLCHAIN", &toolchain),
        ("REPORTS", &format!("{temp}/rust-reports")),
        ("CARGO_TARGET_DIR", &format!("{temp}/rust-target")),
        ("CARGO_BUILD_TARGET", "x86_64-unknown-linux-gnu"),
        ("COVERAGE", &input("COVERAGE")?),
        ("LICENSE_POLICY", license_policy.as_str()),
        ("DENY_CONFIG", &deny_config),
        ("MUTATION_TEST", &mutation_test),
        ("MUTATION_SHARDS", &mutation_shards),
        ("MUTATION_MUTANTS_PER_SHARD", &mutation_mutants_per_shard),
        ("MUTATION_WINDOWS", mutation_windows_json.trim()),
        ("API_COMPATIBILITY", &api_compatibility),
        ("SARIF_REPORTS", &sarif_reports),
        ("UNSAFE_POLICY", unsafe_policy.as_str()),
        ("DEPENDENCY_AUDIT", &dependency_audit),
        ("CLIPPY_LEVEL", clippy_level.as_str()),
        ("UNUSED_DEPENDENCIES", &unused_dependencies),
    ])
}

/// Validate and export the tested head's engine ownership before other settings.
fn export_engine_policy(project: &Path, windows: &[String]) -> Outcome {
    let explicit = optional("MUTATION_ENGINE_POLICY")?;
    let config = read_config(project)?;
    let value = if explicit.is_empty() {
        config
            .settings
            .iter()
            .find(|(key, _)| key == "mutation-engine")
            .map_or("{}", |(_, value)| value.as_str())
    } else {
        &explicit
    };
    let policy = mutation_engine::engine_policy(project, value, windows, || {
        Cmd::new("cargo metadata --format-version 1 --no-deps --locked")
            .cwd(project)
            .capture()
    })?;
    let features = json_strings(&policy.features)?;
    let files = json_strings(&policy.files)?;
    output("mutation-engine-features", &features)?;
    output("mutation-engine-files", &files)?;
    export(&[
        ("MUTATION_ENGINE_FEATURES", &features),
        ("MUTATION_ENGINE_FILES", &files),
    ])
}

/// A run no workflow called: this step again, with the `[ci]` table of the
/// base commit's `maestro-quality.toml` in place of the inputs, so a pull
/// request cannot loosen its own gate. The checkout holds the merge commit
/// and its first parent, the base branch or the merge queue's base.
///
/// Mutation ownership policies come from the head, the merge commit itself;
/// they only partition mutants into additional required jobs, so they cannot
/// skip one.
fn from_settings() -> Outcome {
    let root = canonical(Path::new(&input("GITHUB_WORKSPACE")?))?;
    let settings = Path::new(&input("RUNNER_TEMP")?).join("ci-settings");
    let base = committed_config(&root, "HEAD^1", &settings.join("base"))?;
    let head = committed_config(&root, "HEAD", &settings.join("head"))?;
    let engine_policy = head
        .settings
        .iter()
        .find(|(key, _)| key == "mutation-engine")
        .map_or("{}", |(_, value)| value.as_str());
    let mut validate = Cmd::new("rust-gate validate")
        .env("CALLED", "true")
        .env("MUTATION_ENGINE_POLICY", engine_policy);
    for (key, variable, default) in SETTINGS {
        let config = if *key == "mutation-windows" {
            &head
        } else {
            &base
        };
        let value = config
            .settings
            .iter()
            .find(|(set, _)| set == key)
            .map_or(*default, |(_, value)| value.as_str());
        validate = validate.env(variable, value);
    }
    validate.run()
}

/// The `maestro-quality.toml` that `revision` of the checkout at `root`
/// commits, copied into `directory` and read there; empty when it has none.
fn committed_config(
    root: &Path,
    revision: &str,
    directory: &Path,
) -> Result<QualityConfig, Failure> {
    fs::create_dir_all(directory).map_err(|error| format!("{}: {error}", directory.display()))?;
    let listed = Cmd::new("git -C")
        .arg(root)
        .args(["ls-tree", "--name-only", revision, "--", FILE])
        .capture()?;
    if !listed.trim().is_empty() {
        let text = Cmd::new("git -C")
            .arg(root)
            .arg("show")
            .arg(format!("{revision}:{FILE}"))
            .capture()?;
        fs::write(directory.join(FILE), text).map_err(|error| format!("{FILE}: {error}"))?;
    }
    read_config(directory)
}

/// The compiler this run uses: the exact stable version the project pins,
/// or the one the caller requested, either way at or above the MSRV.
fn selected_toolchain(project: &Path) -> Result<String, Failure> {
    // One quoted key is all this reads, and the pin check below refuses
    // anything the read got wrong, so no TOML parser is involved.
    let toolchain_file = fs::read_to_string(project.join("rust-toolchain.toml"))
        .map_err(|error| format!("rust-toolchain.toml: {error}"))?;
    let pinned = toolchain_file
        .lines()
        .find_map(channel_value)
        .unwrap_or_default();
    if !is_exact_stable(&pinned) {
        return Err("rust-toolchain.toml must pin an exact stable version".into());
    }
    // Any exact stable release from the MSRV up is accepted: the project
    // declares its compiler. The five pins the consumer matrix runs are what is
    // proven, not a gate a newer patch has to wait behind.
    let requested = optional("REQUESTED_TOOLCHAIN")?;
    let toolchain = if requested.is_empty() {
        pinned
    } else {
        requested
    };
    if !is_exact_stable(&toolchain) {
        return Err("rust-version must be an exact stable version, such as 1.98.1".into());
    }
    if parse(&toolchain, false) < Some(MSRV) {
        return Err("rust-version must be at least the 1.85.0 MSRV".into());
    }
    Ok(toolchain)
}

/// The runners the portability job tests on, one pinned image per platform
/// the caller names, as the JSON array its matrix reads; empty when none is
/// named, which skips the job.
fn platform_runners() -> Result<String, Failure> {
    let mut runners: Vec<&str> = Vec::new();
    for name in input("PLATFORMS")?.split_whitespace() {
        let runner = match name {
            "macos" => "macos-15",
            "windows" => "windows-2025",
            "linux-arm" => "ubuntu-24.04-arm",
            _ => return Err("platforms may name only macos, windows and linux-arm".into()),
        };
        if runners.contains(&runner) {
            return Err(format!("platforms names {name} twice").into());
        }
        runners.push(runner);
    }
    if runners.is_empty() {
        return Ok(String::new());
    }
    Ok(format!("[\"{}\"]", runners.join("\",\"")))
}

/// The consumer's `deny.toml` as a path when one is committed inside the
/// checkout and empty otherwise. `auto` and `enforce` both apply the
/// organization's policy, so neither requires one.
fn deny_configuration(project: &Path, root: &Path) -> Result<String, Failure> {
    let mut deny_config = project.join("deny.toml");
    if deny_config.exists() {
        deny_config = canonical(&deny_config)?;
    }
    if !inside(&deny_config, root) {
        return Err("deny.toml escapes checkout".into());
    }
    if deny_config.is_file() {
        return Ok(deny_config.display().to_string());
    }
    Ok(String::new())
}

/// The project relative to the checkout, `.` for the checkout itself: the
/// working directory the portability job runs in.
fn relative_directory(project: &Path, root: &Path) -> Result<String, Failure> {
    match project.strip_prefix(root) {
        Ok(rest) if rest.as_os_str().is_empty() => Ok(".".to_owned()),
        Ok(rest) => Ok(rest.display().to_string()),
        Err(_) => Err("working-directory escapes checkout".into()),
    }
}

/// The artifact name: the working directory, key and toolchain hashed into
/// one identity, then the revision, run and attempt, so two matrix cases can
/// never share a name.
fn artifact_name(relative: &str, artifact_key: &str, toolchain: &str) -> Result<String, Failure> {
    let digest = sha256_hex(format!("{relative}:{artifact_key}:{toolchain}").as_bytes());
    Ok(format!(
        "rust-{}-x86_64-unknown-linux-gnu-{digest}-{}-{}",
        input("GITHUB_SHA")?,
        input("GITHUB_RUN_ID")?,
        input("GITHUB_RUN_ATTEMPT")?
    ))
}

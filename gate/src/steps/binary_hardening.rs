//! `rust-gate hardening`: every release binary rebuilt cleanly at the same target
//! path must have the same digest, be position independent, carry full
//! RELRO and a non-executable stack, and embed its dependency list.

use crate::checks::cargo_metadata::EXECUTABLES;
use crate::checks::private_directories::private_directory;
use crate::runner::{Cmd, Failure, Job, Outcome, Step, path, write};
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

/// What this step declares: its inputs, its tools and its reports.
pub(crate) const STEPS: &[Step] = &[Step {
    workflow: "ci",
    id: "hardening",
    summary: "Reproducible build and binary hardening",
    inputs: &["CARGO_TARGET_DIR"],
    tools: &["cargo auditable", "jaq", "readelf", "sha256sum"],
    reports: &["hardening.txt"],
    run,
}];

/// Run the step.
fn run() -> Outcome {
    let job = Job::current()?;
    let target = path("CARGO_TARGET_DIR")?;
    // A sibling keeps rename atomic even when RUNNER_TEMP is on another volume.
    let parent = target.parent().unwrap_or(Path::new("."));
    let saved = private_directory(&parent.display().to_string(), "release-original")?;
    let original = saved.join("target");
    fs::rename(&target, &original)
        .map_err(|error| format!("cannot save {}: {error}", target.display()))?;
    // OUT_DIR locations and registry paths may be embedded by rustc. Keep
    // build paths fixed, but move every cached object aside for a fresh build.
    let checked = rebuild_and_check(&job, &target, &original);
    restore_target(&target, &original)?;
    fs::remove_dir(&saved)
        .map_err(|error| format!("cannot remove {}: {error}", saved.display()))?;
    checked
}

/// Rebuild without cached objects and compare to the saved shipped binaries.
fn rebuild_and_check(job: &Job, target: &Path, original: &Path) -> Outcome {
    Cmd::new("cargo auditable build --workspace --release --locked")
        .cwd(&job.project)
        .run()?;
    let executables = Cmd::new("jaq -sr")
        .arg(EXECUTABLES)
        .arg(job.temp.join("build.jsonl"))
        .capture()?;
    let report = job.report("hardening.txt")?;
    write(&report, b"", false)?;
    let target_prefix = format!("{}/", target.display());
    for line in executables.lines() {
        let (name, first) = line.split_once('\t').unwrap_or((line, ""));
        let relative = first.strip_prefix(&target_prefix).unwrap_or(first);
        let saved_binary = original.join(relative);
        if !Path::new(first).is_file() {
            return Err(format!("Rebuilt binary missing for {name}").into());
        }
        if digest(&saved_binary)? != digest(Path::new(first))? {
            return Err(format!(
                "Release binary {name} is not reproducible across build directories"
            )
            .into());
        }
        // Hardening flags the linker must have applied. Rust does not emit C
        // stack canaries, so __stack_chk is deliberately not required here.
        let header = readelf("-h", first)?;
        if !header.lines().any(|line| {
            line.trim_start()
                .strip_prefix("Type:")
                .is_some_and(|rest| rest.trim_start().starts_with("DYN"))
        }) {
            return Err(format!("{name} is not position independent").into());
        }
        let program_headers = readelf("-l", first)?;
        if !program_headers.contains("GNU_RELRO") {
            return Err(format!("{name} lacks RELRO").into());
        }
        let dynamic = readelf("-d", first)?;
        if !dynamic.lines().any(|line| {
            line.contains("BIND_NOW") || (line.contains("FLAGS") && line.contains("NOW"))
        }) {
            return Err(format!("{name} lacks full RELRO").into());
        }
        let lines: Vec<&str> = program_headers.lines().collect();
        let executable_stack = lines.iter().enumerate().any(|(index, line)| {
            line.contains("GNU_STACK")
                && (line.contains("RWE")
                    || lines
                        .get(index + 1)
                        .is_some_and(|next| next.contains("RWE")))
        });
        if executable_stack {
            return Err(format!("{name} has an executable stack").into());
        }
        // Installing cargo-auditable and forgetting to build through it
        // produces a normal binary and no error, so the section it adds is
        // checked rather than assumed.
        if !readelf("-S", first)?.contains(".dep-v0") {
            return Err(format!(
                "{name} carries no embedded dependency list; build through cargo auditable"
            )
            .into());
        }
        write(
            &report,
            format!("{name} reproducible pie relro bind-now noexec-stack auditable\n").as_bytes(),
            true,
        )?;
    }
    Ok(())
}

/// Restore the shipped build and its cache, even after a failed rebuild or check.
fn restore_target(target: &Path, original: &Path) -> Outcome {
    if let Err(error) = fs::remove_dir_all(target)
        && error.kind() != ErrorKind::NotFound
    {
        return Err(format!("cannot remove {}: {error}", target.display()).into());
    }
    fs::rename(original, target)
        .map_err(|error| format!("cannot restore {}: {error}", target.display()).into())
}

/// One readelf query over a binary, captured for the checks that read it.
fn readelf(flag: &str, binary: &str) -> Result<String, Failure> {
    Cmd::new("readelf").arg(flag).arg(binary).capture()
}

/// The digest of a file's bytes, as `sha256sum < file` printed it.
fn digest(file: &Path) -> Result<String, Failure> {
    let bytes = fs::read(file).map_err(|error| format!("{}: {error}", file.display()))?;
    Cmd::new("sha256sum").stdin_bytes(&bytes).capture()
}

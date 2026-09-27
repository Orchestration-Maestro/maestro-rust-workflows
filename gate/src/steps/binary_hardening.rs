//! `rust-gate hardening`: every release binary rebuilt into a second target
//! directory must have the same digest, be position independent, carry full
//! RELRO and a non-executable stack, and embed its dependency list.

use crate::checks::cargo_metadata::EXECUTABLES;
use crate::runner::{Cmd, Failure, Job, Outcome, Step, input, write};
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
    let project = &job.project;
    let temp = &job.temp;
    let verify = temp.join("rust-target-verify");
    // Build a second time into a different target directory and compare the
    // digests. A difference means the release bytes depend on something other
    // than the source, which defeats the point of signing them.
    Cmd::new("cargo auditable build --workspace --release --locked")
        .env("CARGO_TARGET_DIR", &verify.display().to_string())
        .cwd(project)
        .run()?;
    let executables = Cmd::new("jaq -sr")
        .arg(EXECUTABLES)
        .arg(temp.join("build.jsonl"))
        .capture()?;
    let report = job.report("hardening.txt")?;
    write(&report, b"", false)?;
    let target_prefix = format!("{}/", input("CARGO_TARGET_DIR")?);
    for line in executables.lines() {
        let (name, first) = line.split_once('\t').unwrap_or((line, ""));
        let relative = first.strip_prefix(&target_prefix).unwrap_or(first);
        let second = verify.join(relative);
        if !second.is_file() {
            return Err(format!("Rebuilt binary missing for {name}").into());
        }
        if digest(Path::new(first))? != digest(&second)? {
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
    if let Err(error) = fs::remove_dir_all(&verify)
        && error.kind() != ErrorKind::NotFound
    {
        return Err(format!("cannot remove {}: {error}", verify.display()).into());
    }
    Ok(())
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

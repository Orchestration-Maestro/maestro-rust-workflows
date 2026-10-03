//! What a step needs from GitHub Actions: its inputs from the environment,
//! the `GITHUB_ENV` and `GITHUB_OUTPUT` files, and log masking.

use super::outcome::{Failure, Outcome};
use super::step_declaration::{Kind, declared};
use std::env;
use std::env::consts;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

/// An input, exactly as the workflow passed it through `env:`.
pub(crate) fn input(name: &str) -> Result<String, String> {
    declared(Kind::Input, name)?;
    env::var(name).map_err(|_| format!("{name} is not set"))
}

/// An input a workflow may leave unset, read as empty then.
pub(crate) fn optional(name: &str) -> Result<String, String> {
    declared(Kind::Input, name)?;
    Ok(env::var(name).unwrap_or_default())
}

/// An input that names a path.
pub(crate) fn path(name: &str) -> Result<PathBuf, String> {
    input(name).map(PathBuf::from)
}

/// Append to the file a GitHub environment variable names, such as
/// `GITHUB_ENV`.
/// Append to the file a `GITHUB_*` variable names: the environment, the
/// outputs or the PATH of every later step.
fn append(file_variable: &str, text: &str) -> Outcome {
    let path = input(file_variable)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| format!("cannot append to {path}: {error}"))?;
    file.write_all(text.as_bytes())
        .map_err(|error| format!("cannot append to {path}: {error}"))?;
    Ok(())
}

/// One `NAME=value` line of a `GITHUB_*` file. A line break inside the value
/// would start a line the gate never wrote, such as `BASH_ENV=...`, so it is
/// refused whatever validation came before.
fn line(name: &str, value: &str) -> Result<String, Failure> {
    if value.contains(['\n', '\r']) {
        return Err(format!("{name} contains a line break").into());
    }
    Ok(format!("{name}={value}\n"))
}

/// Export variables to every later step of the job.
pub(crate) fn export(pairs: &[(&str, &str)]) -> Outcome {
    let mut text = String::new();
    for (name, value) in pairs {
        text.push_str(&line(name, value)?);
    }
    append("GITHUB_ENV", &text)
}

/// Record a step output.
pub(crate) fn output(name: &str, value: &str) -> Outcome {
    append("GITHUB_OUTPUT", &line(name, value)?)
}

/// Only native Linux x64 is supported: the runner this gate is built for.
pub(crate) fn native_linux() -> Outcome {
    native(consts::OS, consts::ARCH)
}

/// Whether `os` and `arch` name the one platform the gate runs on.
fn native(os: &str, arch: &str) -> Outcome {
    if os != "linux" || arch != "x86_64" {
        return Err("Only native Linux x64 is supported".into());
    }
    Ok(())
}

/// The directories every step after `validate` reads: the consumer's
/// project, the reports directory and the runner's temporary directory,
/// resolved once instead of piecemeal in every step.
pub(crate) struct Job {
    /// The consumer's project, the working directory `validate` resolved.
    pub(crate) project: PathBuf,
    /// Where every report of the run is written.
    pub(crate) reports: PathBuf,
    /// The runner's temporary directory, private to the job.
    pub(crate) temp: PathBuf,
}

impl Job {
    /// Read the three directories `validate` exported, dropping project dot components lexically.
    pub(crate) fn current() -> Result<Self, Failure> {
        Ok(Self {
            project: path("PROJECT")?.components().collect(),
            reports: path("REPORTS")?,
            temp: path("RUNNER_TEMP")?,
        })
    }

    /// A report this step writes: declared, or refused.
    pub(crate) fn report(&self, name: &str) -> Result<PathBuf, Failure> {
        declared(Kind::Report, name)?;
        Ok(self.reports.join(name))
    }

    /// A report an earlier step wrote, read here.
    pub(crate) fn earlier(&self, name: &str) -> PathBuf {
        self.reports.join(name)
    }
}

/// A boolean input: `true` or `false`, the two values a workflow boolean
/// input renders as, and nothing else.
pub(crate) fn flag(name: &str) -> Result<bool, Failure> {
    match input(name)?.as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("{name} must be true or false").into()),
    }
}

/// Append to the job summary.
pub(crate) fn summary(text: &str) -> Outcome {
    append("GITHUB_STEP_SUMMARY", text)
}

/// Put a directory on the PATH of every later step of the job.
pub(crate) fn add_to_path(directory: &Path) -> Outcome {
    append("GITHUB_PATH", &format!("{}\n", directory.display()))
}

#[cfg(test)]
mod tests {
    use super::{line, native};

    #[test]
    fn a_value_with_a_line_break_is_never_written() {
        assert_eq!(line("A", "b").unwrap(), "A=b\n");
        for value in ["b\nBASH_ENV=/tmp/x", "b\rBASH_ENV=/tmp/x"] {
            let error = line("A", value).unwrap_err();
            assert_eq!(error.message.as_deref(), Some("A contains a line break"));
        }
    }

    #[test]
    fn only_native_linux_x64_runs_the_gate() {
        assert!(native("linux", "x86_64").is_ok());
        for (os, arch) in [
            ("linux", "aarch64"),
            ("macos", "x86_64"),
            ("windows", "x86_64"),
        ] {
            let error = native(os, arch).unwrap_err();
            assert_eq!(
                error.message.as_deref(),
                Some("Only native Linux x64 is supported"),
                "{os}/{arch}"
            );
        }
    }
}

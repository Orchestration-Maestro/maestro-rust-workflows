//! Running the pinned tools the way the step bodies did: streamed to the job
//! log, captured into a report, or both, fed from a pipe when the body piped,
//! and with the tool's own exit status as the step's when it fails.

use super::outcome::{Failure, Outcome};
use super::step_declaration::{Kind, declared, tool_key};
use std::env;
use std::ffi::OsStr;
use std::fmt::Write as _;
use std::fs;
use std::fs::OpenOptions;
use std::io;
use std::io::Write;
use std::path::Path;
use std::process::{Command, ExitStatus, Output, Stdio};

/// One tool invocation. The fixed words of the command line are one literal,
/// so a reader, and the North Star test, see `cargo llvm-cov` as written;
/// values that come from inputs or paths are added as separate arguments and
/// never split.
pub(crate) struct Cmd {
    /// The process to spawn, with every argument already attached.
    command: Command,
    /// The command line as a reader sees it, for the message when it cannot
    /// be spawned or fed.
    display: String,
    /// Bytes written to the tool's standard input, when a step pipes some in.
    stdin: Option<Vec<u8>>,
}

impl Cmd {
    /// The fixed words of a command line, whitespace-separated.
    pub(crate) fn new(words: &str) -> Self {
        let mut parts = words.split_whitespace();
        let program = parts.next().unwrap_or_default();
        let mut command = Command::new(program);
        command.args(parts);
        Self {
            command,
            display: words.to_owned(),
            stdin: None,
        }
    }

    /// One more argument, passed verbatim.
    pub(crate) fn arg(mut self, argument: impl AsRef<OsStr>) -> Self {
        self.command.arg(argument);
        self
    }

    /// More arguments, each passed verbatim.
    pub(crate) fn args<I, S>(mut self, arguments: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.command.args(arguments);
        self
    }

    /// Run inside `directory`, the `cd` of the step body.
    pub(crate) fn cwd(mut self, directory: &Path) -> Self {
        self.command.current_dir(directory);
        self
    }

    /// One variable set for this invocation only.
    pub(crate) fn env(mut self, name: &str, value: &(impl AsRef<OsStr> + ?Sized)) -> Self {
        self.command.env(name, value);
        self
    }

    /// Remove an inherited variable from this invocation only, not the job.
    pub(crate) fn env_remove(mut self, name: &str) -> Self {
        self.command.env_remove(name);
        self
    }

    /// Start from an empty environment, as a fresh runner does: only the
    /// variables set afterwards reach the tool.
    pub(crate) fn env_clear(mut self) -> Self {
        self.command.env_clear();
        self
    }

    /// Feed `bytes` on stdin, the right side of a pipe or a `< file`.
    pub(crate) fn stdin_bytes(mut self, bytes: &[u8]) -> Self {
        self.stdin = Some(bytes.to_vec());
        self
    }

    /// Spawn with the given output handling, feed the standard input if any,
    /// and wait for the tool to finish.
    fn execute(&mut self, stdout: Stdio, stderr: Stdio) -> Result<Output, Failure> {
        declared(Kind::Tool, &tool_key(&self.display))?;
        self.trace()?;
        let stdin = if self.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::inherit()
        };
        let mut child = self
            .command
            .stdin(stdin)
            .stdout(stdout)
            .stderr(stderr)
            .spawn()
            .map_err(|error| Failure::spawn(&self.display, &error))?;
        if let (Some(bytes), Some(mut pipe)) = (self.stdin.take(), child.stdin.take()) {
            pipe.write_all(&bytes)
                .map_err(|error| Failure::from(format!("cannot feed {}: {error}", self.display)))?;
        }
        child
            .wait_with_output()
            .map_err(|error| Failure::from(format!("{} did not finish: {error}", self.display)))
    }

    /// Append the command line about to run, environment first, to the file
    /// `RUST_GATE_TRACE` names, when the caller asked for a trace: what the
    /// contract tests read instead of the gate's source. Only reviewed,
    /// non-sensitive environment values are visible; all others are redacted.
    fn trace(&self) -> Outcome {
        let Some(path) = env::var_os("RUST_GATE_TRACE") else {
            return Ok(());
        };
        let quoted = |word: &OsStr| {
            let word = word.to_string_lossy();
            if word.is_empty() || word.chars().any(char::is_whitespace) {
                format!("'{word}'")
            } else {
                word.into_owned()
            }
        };
        let mut line = String::new();
        for (name, value) in self.command.get_envs() {
            if value.is_none() {
                let _ = write!(line, "env -u {} ", quoted(name));
                continue;
            }
            let value = match name.to_str() {
                Some(
                    "RUSTUP_TOOLCHAIN" | "RUSTDOCFLAGS" | "MIRIFLAGS" | "CARGO_TARGET_DIR"
                    | "CLIPPY_CONF_DIR",
                ) => value.map_or_else(String::new, quoted),
                _ => "[REDACTED]".to_owned(),
            };
            let _ = write!(line, "{}={value} ", name.to_string_lossy());
        }
        line.push_str(&quoted(self.command.get_program()));
        for argument in self.command.get_args() {
            line.push(' ');
            line.push_str(&quoted(argument));
        }
        line.push('\n');
        write(Path::new(&path), line.as_bytes(), true)
    }

    /// Run with the job's own stdout and stderr.
    pub(crate) fn run(mut self) -> Outcome {
        let output = self.execute(Stdio::inherit(), Stdio::inherit())?;
        failed(output.status)
    }

    /// Run with stdout captured into `path`, stderr on the log, like `> path`.
    /// The file is written even when the tool fails, so a report of the
    /// failure survives it.
    pub(crate) fn stdout_to(mut self, path: &Path) -> Outcome {
        let output = self.execute(Stdio::piped(), Stdio::inherit())?;
        write(path, &output.stdout, false)?;
        failed(output.status)
    }

    /// Run with both streams captured, retaining partial output and exit status.
    pub(crate) fn capture_output(mut self) -> Result<Output, Failure> {
        self.execute(Stdio::piped(), Stdio::piped())
    }

    /// Run with stdout captured and returned, stderr on the log, like `$(...)`.
    pub(crate) fn capture(self) -> Result<String, Failure> {
        let bytes = self.capture_bytes()?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Run with stdout captured as bytes, stderr on the log: the left side of
    /// a pipe.
    pub(crate) fn capture_bytes(mut self) -> Result<Vec<u8>, Failure> {
        let output = self.execute(Stdio::piped(), Stdio::inherit())?;
        failed(output.status)?;
        Ok(output.stdout)
    }

    /// Run with both streams shown on the log and written to `path`, like
    /// `2>&1 | tee path` or, with `append`, `tee -a path`.
    pub(crate) fn tee(mut self, path: &Path, append: bool) -> Outcome {
        let output = self.execute(Stdio::piped(), Stdio::piped())?;
        let mut combined = output.stdout;
        combined.extend_from_slice(&output.stderr);
        io::stdout()
            .write_all(&combined)
            .map_err(|error| Failure::from(format!("cannot write the log: {error}")))?;
        write(path, &combined, append)?;
        failed(output.status)
    }
}

/// Print a line and write it to a report, like `echo ... | tee path`.
pub(crate) fn tee_line(line: &str, path: &Path, append: bool) -> Outcome {
    println!("{line}");
    write(path, format!("{line}\n").as_bytes(), append)
}

/// Refuse an empty or missing report, like `test -s path`.
pub(crate) fn non_empty(path: &Path) -> Outcome {
    match fs::metadata(path) {
        Ok(metadata) if metadata.len() > 0 => Ok(()),
        _ => Err(Failure::from(format!(
            "{} is missing or empty",
            path.display()
        ))),
    }
}

/// Write a file, truncating or appending, with one message on failure.
pub(crate) fn write(path: &Path, bytes: &[u8], append: bool) -> Outcome {
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(path)
        .map_err(|error| Failure::from(format!("cannot write {}: {error}", path.display())))?;
    file.write_all(bytes)
        .map_err(|error| Failure::from(format!("cannot write {}: {error}", path.display())))
}

/// The outcome of a finished tool: its own exit status when it failed, so
/// the step ends the way the tool did.
fn failed(status: ExitStatus) -> Outcome {
    if status.success() {
        Ok(())
    } else {
        Err(Failure::status(status.code().unwrap_or(1)))
    }
}

#[cfg(test)]
mod tests {
    use super::non_empty;
    use std::path::Path;

    #[test]
    fn a_missing_or_empty_file_is_refused_by_name() {
        let error = non_empty(Path::new("/nonexistent/report.json")).unwrap_err();
        assert_eq!(
            error.message.as_deref(),
            Some("/nonexistent/report.json is missing or empty")
        );
    }
}

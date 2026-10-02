//! Child command execution and allocation evidence for this replay only.

use crate::harness::MEMORY_CAP_WRAPPER;
use std::env;
use std::fs;
use std::io::Write as _;
use std::path::Path;
use std::process::Command;

/// Read one required workflow binding.
pub(super) fn binding(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("missing replay binding: {name}"))
}

/// Run one Cargo phase under a process-group timeout, preserving its full log.
pub(super) fn cargo_phase(
    root: &Path,
    arguments: &[&str],
    seconds: u64,
    log: &Path,
    limit: bool,
) -> i32 {
    let file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .unwrap();
    writeln!(&file, "cargo {} (timeout {seconds}s)", arguments.join(" ")).unwrap();
    let mut command = Command::new("timeout");
    if limit {
        command = Command::new("bash");
        command
            .args(["-c", MEMORY_CAP_WRAPPER, "replay-cap"])
            .arg(binding("REPLAY_TEST_MEMORY_KIB"))
            .arg("timeout");
    }
    command
        .args(["--kill-after=10s", &format!("{seconds}s"), "cargo"])
        .args(arguments)
        .current_dir(root)
        .stdout(file.try_clone().unwrap())
        .stderr(file)
        .status()
        .unwrap()
        .code()
        .unwrap_or(137)
}

/// Preserve allocation-abort evidence separately from assertions or build refusals.
pub(super) fn memory_evidence(log: &Path) -> Option<String> {
    fs::read_to_string(log)
        .unwrap()
        .lines()
        .find(|line| line.contains("memory allocation of ") && line.contains(" failed"))
        .map(str::to_owned)
}

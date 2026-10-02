//! The replay cap refuses setup failure and records its effective value.

use crate::harness::{MEMORY_CAP_WRAPPER, temp_dir, verified_memory_cap};
use std::fs;
use std::process::Command;

#[test]
fn failed_cap_setup_never_launches_the_child() {
    let directory = temp_dir("replay-failed-cap");
    let launched = directory.join("launched");
    let output = Command::new("bash")
        .args([
            "-c",
            "ulimit -d 1048576 && exec bash \"$@\"",
            "inherited-cap",
            "-c",
            MEMORY_CAP_WRAPPER,
            "replay-cap",
            "2097152",
            "touch",
        ])
        .arg(&launched)
        .output()
        .unwrap();
    assert!(!launched.exists(), "failed cap setup launched the child");
    assert_eq!(output.status.code(), Some(125));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn successful_cap_setup_returns_the_effective_value() {
    assert_eq!(verified_memory_cap(262_144), 262_144);
}

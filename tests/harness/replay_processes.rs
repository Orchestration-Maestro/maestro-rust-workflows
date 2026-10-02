//! Memory-limited replay processes and their preserved evidence.

use std::process::Command;

/// Apply the test data limit before launching the replay child.
pub(crate) const MEMORY_CAP_WRAPPER: &str = r#"ulimit -d "$1" || exit 125
verified=$(ulimit -d) || exit 125
[ "$verified" = "$1" ] || exit 125
shift
exec "$@""#;

/// Query the effective limit through the same verified wrapper used by tests.
pub(crate) fn verified_memory_cap(cap_kib: u64) -> u64 {
    let output = Command::new("bash")
        .args(["-c", MEMORY_CAP_WRAPPER, "replay-cap"])
        .arg(cap_kib.to_string())
        .args(["bash", "-c", "ulimit -d"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "replay memory cap setup failed: {output:?}"
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

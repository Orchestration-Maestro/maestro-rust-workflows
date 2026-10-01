//! Hosted native cache transport: real consumer preparation and coverage execution evidence.

#![cfg(test)]
#![forbid(unsafe_code)]

mod harness;

use crate::harness::{prepare_hosted_fixture, verify_hosted_fixture};
#[cfg(unix)]
use std::process::Command;
#[cfg(unix)]
use std::{env, fs, process};

#[test]
fn prepare_the_committed_native_consumer_fixture() {
    prepare_hosted_fixture();
}

#[test]
fn verify_the_executed_native_coverage_fixture() {
    verify_hosted_fixture();
}

#[test]
#[cfg(unix)]
fn compatible_prefix_restore_allows_verified_reuse_without_source_builds() {
    let reports = env::temp_dir().join(format!("native-prefix-{}", process::id()));
    fs::create_dir(&reports).unwrap();
    fs::write(
        reports.join("trace"),
        concat!(
            "FIXTURE_NATIVE_CACHE_DIR=[REDACTED] cargo llvm-cov --workspace --locked ",
            "--no-report --features crate-a/engine\n",
        ),
    )
    .unwrap();
    fs::write(reports.join("build-count"), "").unwrap();
    let output = Command::new(env::current_exe().unwrap())
        .args([
            "--exact",
            "verify_the_executed_native_coverage_fixture",
            "--nocapture",
        ])
        .env("REPORTS", &reports)
        .env("NATIVE_PREPARED", "true")
        .env("NATIVE_RESTORE_HIT", "false")
        .env(
            "NATIVE_RESTORE_MATCHED_KEY",
            "native-v1-compatible-coverage-previous-1",
        )
        .env("GITHUB_REF", "refs/heads/main")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let proof = fs::read_to_string(reports.join("fixture-proof.txt")).unwrap();
    assert!(proof.contains("Native source builds: 0\n"), "{proof}");
    assert!(proof.contains("Restore hit: false\n"), "{proof}");
    assert!(
        proof.contains("Restore matched key: native-v1-compatible-coverage-previous-1\n"),
        "{proof}"
    );
    fs::remove_dir_all(&reports).unwrap();
}

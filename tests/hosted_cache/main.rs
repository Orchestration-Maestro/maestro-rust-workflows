//! Hosted native cache transport: real consumer preparation and coverage execution evidence.

#![cfg(test)]
#![forbid(unsafe_code)]

mod harness;

use crate::harness::{
    assert_clean_checkout, assert_engine_plan, prepare_hosted_fixture,
    prepare_hosted_mutation_fixture, verify_hosted_fixture, verify_mutation_fixture,
};
use std::process::Command;
use std::{env, fs, process};

#[test]
fn prepare_the_committed_native_consumer_fixture() {
    prepare_hosted_fixture();
}

#[test]
fn prepare_the_mutation_native_consumer_fixture() {
    prepare_hosted_mutation_fixture();
}

#[test]
fn assert_the_engine_plan_has_two_nonempty_shards() {
    assert_engine_plan();
}

#[test]
fn verify_the_executed_native_mutation_fixture() {
    verify_mutation_fixture();
}

#[test]
fn assert_the_checkout_is_clean_before_coverage() {
    assert_clean_checkout();
}

#[test]
fn verify_the_executed_native_coverage_fixture() {
    verify_hosted_fixture();
}

#[test]
fn checkout_cleanliness_probe_accepts_ignored_builds_and_refuses_untracked_files() {
    let project = env::temp_dir().join(format!("native-clean-checkout-{}", process::id()));
    fs::create_dir(&project).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(&project)
            .status()
            .unwrap()
            .success()
    );
    fs::write(project.join(".gitignore"), "target/\n").unwrap();
    assert!(
        Command::new("git")
            .args(["add", ".gitignore"])
            .current_dir(&project)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new("git")
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-qm",
                "fixture"
            ])
            .current_dir(&project)
            .status()
            .unwrap()
            .success()
    );
    fs::create_dir(project.join("target")).unwrap();
    fs::write(project.join("target/build-output"), "ignored").unwrap();
    let probe = || {
        Command::new(env::current_exe().unwrap())
            .args([
                "--exact",
                "assert_the_checkout_is_clean_before_coverage",
                "--nocapture",
            ])
            .env("PROJECT", &project)
            .output()
            .unwrap()
    };
    let clean = probe();
    assert!(clean.status.success(), "{clean:?}");
    assert!(String::from_utf8_lossy(&clean.stdout).contains("is empty"));
    fs::write(project.join("untracked-output"), "dirty").unwrap();
    let dirty = probe();
    assert!(!dirty.status.success(), "{dirty:?}");
    assert!(String::from_utf8_lossy(&dirty.stderr).contains("checkout is dirty before coverage"));
    fs::remove_dir_all(project).unwrap();
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

#[test]
fn mutation_fixture_verifier_requires_reuse_only_for_compatible_native_flags() {
    let reports = env::temp_dir().join(format!("native-mutation-verifier-{}", process::id()));
    fs::create_dir(&reports).unwrap();
    let entries = reports.join("entries");
    fs::create_dir(&entries).unwrap();
    fs::write(reports.join("native-entries"), "entry-plain\n").unwrap();
    fs::write(
        reports.join("native-cache-binding.txt"),
        "Key: native-mutation-key\n",
    )
    .unwrap();
    fs::write(
        reports.join("trace"),
        concat!(
            "FIXTURE_NATIVE_CACHE_DIR=[REDACTED] timeout --kill-after=1m 30m ",
            "cargo mutants --features engine --shard 1/2\n"
        ),
    )
    .unwrap();
    for (matched, names, builds, valid) in [
        ("native-mutation-key", "entry-plain", "", true),
        ("", "", "source\n", true),
        ("native-mutation-key", "entry-plain", "source\n", false),
        ("", "", "", false),
        (
            "native-coverage-key",
            "entry-instrumented",
            "source\n",
            true,
        ),
    ] {
        fs::write(
            reports.join("native-cache-before.txt"),
            format!("{}\n{names}", entries.display()),
        )
        .unwrap();
        fs::write(reports.join("build-count"), builds).unwrap();
        let output = Command::new(env::current_exe().unwrap())
            .args([
                "--exact",
                "verify_the_executed_native_mutation_fixture",
                "--nocapture",
            ])
            .env("REPORTS", &reports)
            .env("NATIVE_PREPARED", "true")
            .env("NATIVE_RESTORE_MATCHED_KEY", matched)
            .env("GITHUB_REF", "refs/heads/fixture")
            .env_remove("FIXTURE_NATIVE_CACHE_DIR")
            .output()
            .unwrap();
        assert_eq!(output.status.success(), valid, "{output:?}");
    }
    fs::remove_dir_all(reports).unwrap();
}

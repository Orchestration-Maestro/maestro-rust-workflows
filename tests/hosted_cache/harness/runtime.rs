//! Real hosted fixture setup and evidence, without Bash or command stand-ins.

use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use workflow_contract_tests::engine_workspace;

/// Run a native process and keep its stdout, stderr and exit status visible.
fn checked(program: &str, args: &[&str], project: &Path) -> String {
    let output = Command::new(program)
        .args(args)
        .current_dir(project)
        .output()
        .unwrap();
    assert!(output.status.success(), "{program} {args:?}: {output:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

/// Create and commit the consumer revision; execution profiles and evidence stay outside it.
pub(crate) fn prepare_hosted_fixture() {
    let project = PathBuf::from(env::var("HOSTED_NATIVE_PROJECT").unwrap());
    fs::create_dir(&project).unwrap();
    engine_workspace(&project, true).unwrap();
    fs::write(project.join(".gitignore"), "target/\n").unwrap();
    checked("cargo", &["generate-lockfile", "--offline"], &project);
    checked("git", &["init", "--quiet"], &project);
    checked("git", &["config", "user.name", "Fixture"], &project);
    checked(
        "git",
        &["config", "user.email", "fixture@example.invalid"],
        &project,
    );
    checked("git", &["add", "."], &project);
    checked(
        "git",
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "feat: native cache fixture",
        ],
        &project,
    );
    let sha = checked("git", &["rev-parse", "HEAD"], &project);
    let temp = PathBuf::from(env::var("RUNNER_TEMP").unwrap());
    let reports = temp.join("native-reports");
    fs::create_dir(&reports).unwrap();
    let mut exports = OpenOptions::new()
        .append(true)
        .open(env::var("GITHUB_ENV").unwrap())
        .unwrap();
    for (key, value) in [
        ("PROJECT", project),
        ("REPORTS", reports.clone()),
        ("CARGO_TARGET_DIR", temp.join("native-target")),
        ("FIXTURE_NATIVE_BUILD_LOG", reports.join("build-count")),
        ("RUST_GATE_TRACE", reports.join("trace")),
    ] {
        writeln!(exports, "{key}={}", value.display()).unwrap();
    }
    let mut outputs = OpenOptions::new()
        .append(true)
        .open(env::var("GITHUB_OUTPUT").unwrap())
        .unwrap();
    writeln!(outputs, "sha={sha}").unwrap();
}

/// Assert the executed feature child received the variable only on Unix; retain build counts.
pub(crate) fn verify_hosted_fixture() {
    let reports = PathBuf::from(env::var("REPORTS").unwrap());
    let trace = fs::read_to_string(reports.join("trace")).unwrap();
    let injected: Vec<_> = trace
        .lines()
        .filter(|line| line.contains("FIXTURE_NATIVE_CACHE_DIR="))
        .collect();
    if cfg!(windows) {
        assert!(injected.is_empty(), "{trace}");
        assert_eq!(env::var("NATIVE_PREPARED").unwrap(), "false");
    } else {
        assert_eq!(injected.len(), 1, "{trace}");
        assert!(
            injected
                .first()
                .unwrap()
                .contains("--features crate-a/engine")
        );
        assert_eq!(env::var("NATIVE_PREPARED").unwrap(), "true");
    }
    let builds = fs::read_to_string(reports.join("build-count"))
        .unwrap_or_default()
        .lines()
        .count();
    assert!(builds <= 1, "optional consumer rebuilt {builds} times");
    let matched_key = env::var("NATIVE_RESTORE_MATCHED_KEY").unwrap_or_default();
    if matched_key.is_empty() {
        assert_eq!(builds, 1);
    }
    let proof = format!(
        concat!(
            "OS: {}\nNative source builds: {}\nVariable injections: {}\n",
            "Restore hit: {}\nRestore matched key: {}\nScope: {}\n"
        ),
        env::consts::OS,
        builds,
        injected.len(),
        env::var("NATIVE_RESTORE_HIT").unwrap_or_default(),
        matched_key,
        env::var("GITHUB_REF").unwrap()
    );
    println!("{proof}");
    fs::write(reports.join("fixture-proof.txt"), proof).unwrap();
}

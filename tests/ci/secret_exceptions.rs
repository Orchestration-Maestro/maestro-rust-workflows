//! Reviewed secret exceptions are gate-owned and require all four exact keys.

use crate::harness::{Fixture, refused, succeeds};
use serde_json::{Value, json};
use std::fs;

/// A known vendored xxHash false positive, assembled without flagging this test's source.
fn finding() -> Value {
    json!({
        "File": "lbug-src/third_party/zstd/include/zstd/common/xxhash.h",
        "RuleID": "generic-api-key",
        "StartLine": 17,
        "EndLine": 17,
        "Match": (["data_key_lo = ", "_mm512_srli_epi64 "].concat()),
        "Secret": "synthetic-sensitive-value"
    })
}

/// Supply scanner output while keeping archive extraction and report processing real.
fn scan_fixture(findings: &[Value]) -> Fixture {
    let mut fixture = Fixture::new();
    fixture.set("GITHUB_REPOSITORY", "Orchestration-Maestro/lbug");
    fixture.set("GITHUB_ACTIONS", "true");
    fixture.set("SARIF_REPORTS", "true");
    fixture.stub("git", "tar -cf - --files-from /dev/null");
    fs::write(
        fixture.root.join("scanner.json"),
        serde_json::to_vec(findings).unwrap(),
    )
    .unwrap();
    fixture.stub("gitleaks", "cat scanner.json");
    fixture
}

#[test]
fn an_exact_exception_passes_with_redacted_reports() {
    let fixture = scan_fixture(&[finding()]);
    let output = fixture.run("ci", "secrets");
    succeeds(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("excepted"));
    assert!(stdout.contains("beed276c5f6c5b2dfec67f862565035bffa541d03bb4c5d1d60a1fa6e2949bea"));
    let report = fs::read_to_string(fixture.root.join("reports/secrets.json")).unwrap();
    let findings: Value = serde_json::from_str(&report).unwrap();
    assert_eq!(findings[0]["Status"], "excepted");
    assert_eq!(findings[0]["Exception"]["approved_by"], "owner, 2026-09-30");
    let sarif = fs::read_to_string(fixture.root.join("reports/secrets.sarif")).unwrap();
    let sarif_value: Value = serde_json::from_str(&sarif).unwrap();
    assert_eq!(
        sarif_value["runs"][0]["results"][0]["suppressions"][0]["kind"],
        "external"
    );
    for text in [stdout.as_ref(), &report, &sarif, &fixture.trace()] {
        assert!(!text.contains("synthetic-sensitive-value"));
        assert!(!text.contains(finding()["Match"].as_str().unwrap()));
    }
}

#[test]
fn a_mismatch_in_each_exception_key_still_fails() {
    for key in ["repository", "File", "RuleID", "Match"] {
        let mut changed = finding();
        if key != "repository" {
            changed[key] = json!("different-value");
        }
        let mut fixture = scan_fixture(&[changed]);
        if key == "repository" {
            fixture.set("GITHUB_REPOSITORY", "Other/lbug");
        }
        let output = fixture.run("ci", "secrets");
        assert_eq!(output.status.code(), Some(1), "{key}");
        let report: Value =
            serde_json::from_slice(&fs::read(fixture.root.join("reports/secrets.json")).unwrap())
                .unwrap();
        assert_eq!(report[0]["Status"], "finding", "{key}");
        assert!(report[0]["Exception"].is_null(), "{key}");
    }
}

#[test]
fn unmatched_current_repository_entries_warn_without_failing() {
    let fixture = scan_fixture(&[]);
    fixture.stub("gitleaks", "printf '[]'");
    let output = fixture.run("ci", "secrets");
    succeeds(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.matches("stale secret-scan exception").count(), 8);
    assert!(stdout.contains("::warning::"));
}

#[test]
fn consumer_allowlists_cannot_grant_a_secret_exception() {
    let mut fixture = Fixture::new();
    fixture.set("GITHUB_ACTIONS", "true");
    let project = fixture.root.join("project");
    fixture.set("GITHUB_WORKSPACE", &project.display().to_string());
    let secret = ["aB2cD3eF4gH5", "iJ6kL7mN8pQ9rS0tU"].concat();
    fs::write(
        project.join("src/consumer.rs"),
        format!("api_key = \"{secret}\"\n"),
    )
    .unwrap();
    fs::write(
        project.join(".gitleaks.toml"),
        "[allowlist]\npaths = ['.*']\n",
    )
    .unwrap();
    fs::write(
        project.join(".gitleaksignore"),
        format!(
            "{}:generic-api-key:1\n",
            fixture.root.join("secret-source/src/consumer.rs").display()
        ),
    )
    .unwrap();
    let commit = "cd project && git init -q && git add . && git -c user.name=Fixture \
                  -c user.email=fixture@example.invalid -c commit.gpgsign=false \
                  -c core.hooksPath=/dev/null commit -qm fixture";
    succeeds(&fixture.run_body(commit));
    assert_eq!(
        fixture
            .run_body("cd project && rust-gate secrets")
            .status
            .code(),
        Some(1)
    );
    let report: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("reports/secrets.json")).unwrap())
            .unwrap();
    assert_eq!(report[0]["File"], "src/consumer.rs");
    assert_eq!(report[0]["RuleID"], "generic-api-key");
    fs::create_dir(project.join("policy")).unwrap();
    let consumer_entry = json!([{
        "repository":"Orchestration-Maestro/maestro-rust-workflows", "path":"src/consumer.rs",
        "rule":"generic-api-key", "sha256":report[0]["SHA256"], "reason":"consumer approval",
        "approved_by":"consumer", "approved_on":"2026-09-30"
    }]);
    fs::write(
        project.join("policy/secret-scan-exceptions.json"),
        consumer_entry.to_string(),
    )
    .unwrap();
    succeeds(&fixture.run_body(commit));
    let output = fixture.run_body("cd project && rust-gate secrets");
    assert_eq!(output.status.code(), Some(1));
    let trace = fixture.trace();
    for flag in [
        "--ignore-gitleaks-allow",
        "--gitleaks-ignore-path",
        "--config",
    ] {
        assert!(trace.contains(flag));
    }
    let report = fs::read_to_string(fixture.root.join("reports/secrets.json")).unwrap();
    let findings: Value = serde_json::from_str(&report).unwrap();
    assert_eq!(findings[0]["Status"], "finding");
    assert_eq!(findings[0]["RuleID"], "generic-api-key");
    for text in [
        report.as_str(),
        String::from_utf8_lossy(&output.stdout).as_ref(),
        String::from_utf8_lossy(&output.stderr).as_ref(),
        trace.as_str(),
    ] {
        assert!(!text.contains(&secret));
    }
}

#[test]
fn malformed_scanner_findings_fail_without_disclosing_content() {
    let fixture = scan_fixture(&[json!({"Match":"synthetic-sensitive-value"})]);
    let output = fixture.run("ci", "secrets");
    refused(&output, "Invalid secret-scan data");
    assert!(!String::from_utf8_lossy(&output.stderr).contains("synthetic-sensitive-value"));
}

#[test]
fn locations_outside_the_revision_are_refused() {
    for location in ["../outside.rs", "/outside.rs"] {
        let mut changed = finding();
        changed["File"] = json!(location);
        let fixture = scan_fixture(&[changed]);
        refused(
            &fixture.run("ci", "secrets"),
            "Invalid secret-scan location",
        );
    }
}

#[test]
fn a_mixed_scan_keeps_unreviewed_findings_blocking() {
    let mut changed = finding();
    changed["Match"] = json!("unreviewed-sensitive-match");
    let fixture = scan_fixture(&[finding(), changed]);
    assert_eq!(fixture.run("ci", "secrets").status.code(), Some(1));
    let report: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("reports/secrets.json")).unwrap())
            .unwrap();
    assert_eq!(report[0]["Status"], "excepted");
    assert_eq!(report[1]["Status"], "finding");
    let sarif: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("reports/secrets.sarif")).unwrap())
            .unwrap();
    assert!(sarif["runs"][0]["results"][1].get("suppressions").is_none());
}

#[test]
fn an_absent_scanner_report_cannot_pass_as_clean() {
    let fixture = scan_fixture(&[]);
    fixture.stub("gitleaks", "exit 0");
    refused(
        &fixture.run("ci", "secrets"),
        "Secret scanner produced no report",
    );
}

/// Archive a real consumer revision, with no scanner or Git stand-in.
fn source_fixture(file: &str, content: &str) -> Fixture {
    let mut fixture = Fixture::new();
    fixture.set("GITHUB_ACTIONS", "true");
    let project = fixture.root.join("project");
    fixture.set("GITHUB_WORKSPACE", &project.display().to_string());
    fs::write(project.join(file), content).unwrap();
    succeeds(&fixture.run_body(
        "cd project && git init -q && git add . && git -c user.name=Fixture \
         -c user.email=fixture@example.invalid -c commit.gpgsign=false \
         -c core.hooksPath=/dev/null commit -qm fixture",
    ));
    fixture
}

#[test]
fn inline_allow_markers_are_findings_even_without_a_secret() {
    let source = ["// ", "gitleaks", ":allow"].concat();
    let fixture = source_fixture("src/consumer.rs", &source);
    let output = fixture.run_body("cd project && rust-gate secrets");
    assert_eq!(output.status.code(), Some(1));
    let report: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("reports/secrets.json")).unwrap())
            .unwrap();
    assert_eq!(report.as_array().unwrap().len(), 1);
    assert_eq!(report[0]["RuleID"], "gitleaks-allow");
    assert_eq!(
        report[0]["SHA256"],
        "165b654eb54f67b25e0104e7cef08c742148626c865476d028190b3b7a2d3dc3"
    );
    assert_eq!(report[0]["File"], "src/consumer.rs");
    assert_eq!(report[0]["StartLine"], 1);
    assert_eq!(report[0]["Status"], "finding");
    assert!(!String::from_utf8_lossy(&output.stdout).contains(&source));
}

#[test]
fn consumer_config_environment_cannot_replace_gate_rules() {
    let secret = ["aB2cD3eF4gH5", "iJ6kL7mN8pQ9rS0tU"].concat();
    let mut fixture = source_fixture("src/consumer.rs", &format!("api_key = \"{secret}\"\n"));
    let config = fixture.root.join("project/.gitleaks.toml");
    let config_text = "[allowlist]\npaths = ['.*']\n";
    fs::write(&config, config_text).unwrap();
    succeeds(&fixture.run_body(
        "cd project && git add . && git -c user.name=Fixture \
         -c user.email=fixture@example.invalid -c commit.gpgsign=false \
         -c core.hooksPath=/dev/null commit -qm config",
    ));
    fixture.set("GITLEAKS_CONFIG", &config.display().to_string());
    fixture.set("GITLEAKS_CONFIG_TOML", config_text);
    let output = fixture.run_body("cd project && rust-gate secrets");
    assert_eq!(output.status.code(), Some(1));
    let report: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("reports/secrets.json")).unwrap())
            .unwrap();
    assert_eq!(report[0]["RuleID"], "generic-api-key");
}

#[test]
fn ignored_file_content_is_scanned_under_its_original_path() {
    let secret = ["aB2cD3eF4gH5", "iJ6kL7mN8pQ9rS0tU"].concat();
    let fixture = source_fixture(".gitleaksignore", &format!("api_key = \"{secret}\"\n"));
    let output = fixture.run_body("cd project && rust-gate secrets");
    assert_eq!(output.status.code(), Some(1));
    let report: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("reports/secrets.json")).unwrap())
            .unwrap();
    assert_eq!(report[0]["File"], ".gitleaksignore");
    assert_eq!(report[0]["RuleID"], "generic-api-key");
}

#[test]
fn a_local_clean_scan_needs_no_repository_identity() {
    let mut fixture = scan_fixture(&[]);
    fixture.env.remove("GITHUB_ACTIONS");
    fixture.env.remove("GITHUB_REPOSITORY");
    let output = fixture.run("ci", "secrets");
    succeeds(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("exceptions apply only in hosted CI"));
    assert!(!stdout.contains("stale secret-scan exception"));
    assert!(fixture.calls().contains("gitleaks"));
}

#[test]
fn a_local_approved_finding_without_identity_still_fails() {
    let mut fixture = scan_fixture(&[finding()]);
    fixture.env.remove("GITHUB_ACTIONS");
    fixture.env.remove("GITHUB_REPOSITORY");
    let output = fixture.run("ci", "secrets");
    assert_eq!(output.status.code(), Some(1));
    let report = fixture.root.join("reports/secrets.json");
    assert!(
        report.exists(),
        "local findings must still be scanned and reported"
    );
    let findings: Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(findings[0]["Status"], "finding");
    assert!(findings[0]["Exception"].is_null());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("exceptions apply only in hosted CI"));
    assert!(!stdout.contains("stale secret-scan exception"));
}

#[test]
fn local_repository_variables_cannot_grant_an_exception() {
    let mut fixture = scan_fixture(&[finding()]);
    fixture.env.remove("GITHUB_ACTIONS");
    let output = fixture.run("ci", "secrets");
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("exceptions apply only in hosted CI"));
    assert!(!stdout.contains("stale secret-scan exception"));
}

#[test]
fn hosted_scans_require_a_nonempty_runner_repository() {
    for missing in [true, false] {
        let mut fixture = scan_fixture(&[]);
        if missing {
            fixture.env.remove("GITHUB_REPOSITORY");
        } else {
            fixture.set("GITHUB_REPOSITORY", "");
        }
        let output = fixture.run("ci", "secrets");
        refused(&output, "Hosted secret scan requires GITHUB_REPOSITORY");
        assert!(!fixture.calls().contains("gitleaks"));
    }
}

//! GitHub release writes require observable environment and source protections.

use crate::harness::{Fixture, refused, succeeds, workflow};
use serde_json::json;
use std::fs;

#[test]
fn dry_runs_never_query_release_authorization_or_write_remotely() {
    for name in ["publish-binaries", "publish-crate", "publish-evidence"] {
        let fixture = Fixture::new();
        succeeds(&fixture.run(name, "authorize"));
        assert!(fixture.calls().is_empty());
    }
}

#[test]
fn every_live_publisher_requires_reviewers_and_only_release_tag_deployments() {
    let environment = "release environment must require reviewers and custom tag policies";
    let policies = "release environment must allow only the v* tag policy";
    for name in ["publish-binaries", "publish-crate", "publish-evidence"] {
        let mut fixture = Fixture::new();
        fixture.trusted();
        succeeds(&fixture.run(name, "authorize"));
        for (key, value, message) in [
            ("ENVIRONMENT_JSON", "{}", environment),
            ("ENVIRONMENT_JSON", "not-json", environment),
            (
                "ENVIRONMENT_JSON",
                r#"{"protection_rules":[],"deployment_branch_policy":null}"#,
                environment,
            ),
            (
                "ENVIRONMENT_JSON",
                r#"{"protection_rules":[{"type":"required_reviewers","reviewers":[]}],
                    "deployment_branch_policy":{"custom_branch_policies":true}}"#,
                environment,
            ),
            ("POLICIES_JSON", "{}", policies),
            ("POLICIES_JSON", "not-json", policies),
            (
                "POLICIES_JSON",
                r#"{"total_count":1,"branch_policies":[{"type":"branch","name":"v*"}]}"#,
                policies,
            ),
            (
                "POLICIES_JSON",
                r#"{"total_count":1,"branch_policies":[{"type":"tag","name":"*"}]}"#,
                policies,
            ),
            (
                "POLICIES_JSON",
                r#"{"total_count":2,"branch_policies":[{"type":"tag","name":"v*"}]}"#,
                policies,
            ),
        ] {
            let previous = fixture.env[key].clone();
            fixture.set(key, value);
            refused(&fixture.run(name, "authorize"), message);
            fixture.set(key, &previous);
        }
        fixture.stub("gh", "exit 7");
        refused(
            &fixture.run(name, "authorize"),
            "Cannot verify release environment protection",
        );
    }
}

#[test]
fn live_jobs_enter_release_with_only_the_permissions_they_use() {
    for name in ["publish-binaries", "publish-crate", "publish-evidence"] {
        let data = workflow(name);
        assert_eq!(data["jobs"]["publish"]["environment"], "release");
        assert_eq!(data["jobs"]["publish"]["permissions"]["actions"], "read");
        assert_eq!(
            data["jobs"]["publish"]["permissions"]["contents"],
            if name == "publish-crate" {
                "read"
            } else {
                "write"
            }
        );
        assert!(
            data["jobs"]["publish"]["permissions"]
                .get("id-token")
                .is_none()
        );
        assert_eq!(
            data["on"]["workflow_call"]["inputs"]["dry-run"]["default"],
            true
        );
        assert_eq!(data["jobs"]["preflight"]["permissions"]["actions"], "read");
    }
}

#[test]
fn binary_upload_checks_the_release_revision_and_never_replaces_assets() {
    let mut fixture = Fixture::new();
    fixture.trusted();
    fs::create_dir(fixture.root.join("rust-release")).unwrap();
    for name in ["payload.tar.gz", "provenance.json", "SHA256SUMS"] {
        fs::write(
            fixture.root.join("rust-release").join(name),
            "verified earlier",
        )
        .unwrap();
    }
    succeeds(&fixture.run("publish-binaries", "publish"));
    let calls = fixture.calls();
    assert!(calls.contains("environments/release"));
    assert!(calls.contains("/commits/refs%2Ftags%2Fv0.1.0"));
    assert!(calls.contains("release view v0.1.0"));
    assert!(calls.contains("release upload v0.1.0"));
    assert!(!calls.contains("--clobber") && !calls.contains("release create"));

    for (key, value, message) in [
        (
            "REVISION",
            "main".to_owned(),
            "Publication revision must match this source",
        ),
        (
            "REVISION",
            "b".repeat(40),
            "Publication revision must match this source",
        ),
        (
            "RELEASE_JSON",
            json!({"tagName":"v9.9.9", "assets":[]}).to_string(),
            "Release must exist for this tag with no matching assets",
        ),
        (
            "RELEASE_JSON",
            json!({"tagName":"v0.1.0", "assets":[{"name":"payload.tar.gz"}]}).to_string(),
            "Release must exist for this tag with no matching assets",
        ),
        (
            "RELEASE_JSON",
            "not-json".to_owned(),
            "Release must exist for this tag with no matching assets",
        ),
        (
            "COMMIT_JSON",
            json!({"sha":"b".repeat(40)}).to_string(),
            "Release tag does not match the validated revision",
        ),
        (
            "COMMIT_STATUS",
            "7".to_owned(),
            "Cannot resolve the release tag revision",
        ),
        (
            "VIEW_STATUS",
            "7".to_owned(),
            "Release must already exist for this tag",
        ),
        (
            "POLICIES_STATUS",
            "7".to_owned(),
            "Cannot verify release environment tag policies",
        ),
    ] {
        let previous = fixture.env[key].clone();
        fixture.set(key, &value);
        fs::write(fixture.root.join("calls"), "").unwrap();
        refused(&fixture.run("publish-binaries", "publish"), message);
        assert!(!fixture.calls().contains("release upload"));
        fixture.set(key, &previous);
    }
}

#[test]
fn release_upload_refuses_missing_files_and_propagates_network_failure() {
    let mut fixture = Fixture::new();
    fixture.trusted();
    refused(
        &fixture.run("publish-binaries", "publish"),
        "Release asset is missing or unsafe",
    );
    assert!(!fixture.calls().contains("release upload"));
    fs::create_dir(fixture.root.join("rust-release")).unwrap();
    for name in ["payload.tar.gz", "provenance.json", "SHA256SUMS"] {
        fs::write(
            fixture.root.join("rust-release").join(name),
            "verified earlier",
        )
        .unwrap();
    }
    fixture.set("DRY_RUN", "true");
    fs::write(fixture.root.join("calls"), "").unwrap();
    succeeds(&fixture.run("publish-binaries", "publish"));
    assert!(fixture.calls().is_empty());
    fixture.set("DRY_RUN", "false");
    fixture.set("UPLOAD_STATUS", "7");
    assert_eq!(
        fixture.run("publish-binaries", "publish").status.code(),
        Some(7)
    );
    assert!(!fixture.root.join("summary").exists());
}

#[test]
fn release_upload_refuses_empty_assets_and_directories_before_remote_calls() {
    for directory in [false, true] {
        let mut fixture = Fixture::new();
        fixture.trusted();
        let release = fixture.root.join("rust-release");
        fs::create_dir(&release).unwrap();
        for name in ["provenance.json", "SHA256SUMS"] {
            fs::write(release.join(name), "verified earlier").unwrap();
        }
        let asset = release.join("payload.tar.gz");
        if directory {
            fs::create_dir(&asset).unwrap();
        } else {
            fs::write(&asset, "").unwrap();
        }
        refused(
            &fixture.run("publish-binaries", "publish"),
            "Release asset is missing or unsafe",
        );
        assert!(fixture.calls().is_empty());
    }
}

#[test]
fn crate_publication_rechecks_approval_before_exposing_the_token_to_cargo() {
    let mut fixture = Fixture::new();
    fixture.trusted();
    fixture.set("TOKEN", "synthetic-token-not-a-secret");
    fixture.set("ENVIRONMENT_JSON", "{}");
    fixture.stub("cargo", "exit 99");
    refused(
        &fixture.run("publish-crate", "publish"),
        "release environment must require reviewers and custom tag policies",
    );
    assert!(!fixture.calls().contains("cargo\n"));
    fixture.set("DRY_RUN", "true");
    succeeds(&fixture.run("publish-crate", "publish"));
    assert!(!fixture.calls().contains("cargo\n"));
}

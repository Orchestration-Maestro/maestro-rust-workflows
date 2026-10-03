//! The publishers: dry-run first, and a live path only a protected release
//! can take.

use crate::harness::{Fixture, refused, root, succeeds, workflow};
use serde_json::json;
use std::fs;

#[test]
fn publication_defaults_and_required_dependencies() {
    for name in ["publish-binaries", "publish-crate"] {
        let data = workflow(name);
        assert_eq!(
            data["on"]["workflow_call"]["inputs"]["dry-run"]["default"],
            true
        );
        assert!(
            data["on"]["workflow_call"]["inputs"]
                .get("ci-passed")
                .is_none()
        );
        assert_eq!(data["jobs"]["ci"]["uses"], "./.github/workflows/ci.yml");
        assert!(
            data["jobs"]["stage"]["needs"]
                .as_array()
                .unwrap()
                .contains(&json!("ci"))
        );
        assert!(
            data["jobs"]["publish"]["needs"]
                .as_array()
                .unwrap()
                .contains(&json!("stage"))
        );
        assert!(
            data["jobs"]["publish"]["if"]
                .as_str()
                .unwrap()
                .contains("!inputs.dry-run")
        );
        assert!(
            data["jobs"]["stage"]["permissions"]
                .get("id-token")
                .is_none()
        );
        assert!(
            data["jobs"]["stage"]["permissions"]
                .get("attestations")
                .is_none(),
            "{name}: staging must keep zero write scopes"
        );
        assert!(
            !fs::read_to_string(root().join(format!(".github/workflows/{name}.yml")))
                .unwrap()
                .contains("secrets: inherit")
        );
    }
    let ci = workflow("ci");
    assert_eq!(
        ci["jobs"]["gate"]["needs"],
        json!([
            "mutation-plan",
            "checks",
            "portability",
            "mutations",
            "mutation-summary",
            "mutation-windows",
            "mutation-engine",
            "mutation-engine-default"
        ])
    );
    assert!(
        ci["jobs"]["gate"]["if"]
            .as_str()
            .unwrap()
            .contains("always()")
    );
    assert!(
        ci["on"]["workflow_call"]["outputs"]["artifact-id"]["value"]
            .as_str()
            .unwrap()
            .contains("jobs.gate.outputs")
    );
}

#[test]
fn dry_run_needs_no_credentials_and_rejects_malicious_packages() {
    let mut fixture = Fixture::new();
    for name in ["publish-binaries", "publish-crate"] {
        succeeds(&fixture.run(name, "authorize"));
    }
    for package in ["--registry", "x;touch hacked", "../x", "x\ny"] {
        fixture.set("PACKAGE", package);
        refused(
            &fixture.run("publish-crate", "authorize"),
            "package must be a safe exact Cargo package name",
        );
    }
    assert!(fixture.calls().is_empty());
}

#[test]
fn live_publication_requires_trusted_event_ref_and_configuration() {
    let event = "Live publication requires push or workflow_dispatch";
    let tag = "Use a vMAJOR.MINOR.PATCH release tag";
    let cases = [
        ("EVENT", "pull_request", event),
        ("EVENT", "pull_request_target", event),
        ("REF", "refs/heads/main", tag),
        ("REF", "refs/tags/v0.1.0;touch hacked", tag),
        (
            "PROTECTED",
            "false",
            "Release tag must be protected by a ruleset",
        ),
    ];
    for name in ["publish-binaries", "publish-crate", "publish-evidence"] {
        let mut fixture = Fixture::new();
        fixture.trusted();
        succeeds(&fixture.run(name, "authorize"));
        for &(key, value, message) in &cases {
            let old = fixture.env[key].clone();
            fixture.set(key, value);
            refused(&fixture.run(name, "authorize"), message);
            fixture.set(key, &old);
        }
    }
}

#[test]
fn publishers_forward_every_ci_input_except_the_compiler_override() {
    // A publication reruns CI on the tag; a consumer that needs
    // unsafe-policy: allow or a lower coverage floor in CI needs it there too.
    // The compiler override and repository-only self-test stay out.
    let ci = workflow("ci");
    let ci_inputs = ci["on"]["workflow_call"]["inputs"].as_object().unwrap();
    for input in ["mutation-shards", "mutation-mutants-per-shard"] {
        assert!(ci_inputs.contains_key(input), "ci.yml must expose {input}");
    }
    for name in ["publish-binaries", "publish-crate"] {
        let data = workflow(name);
        let inputs = &data["on"]["workflow_call"]["inputs"];
        let with = &data["jobs"]["ci"]["with"];
        for (input, contract) in ci_inputs {
            if matches!(input.as_str(), "rust-version" | "internal-shard-selftest") {
                assert!(inputs.get(input).is_none(), "{name}/{input}");
                assert!(with.get(input).is_none(), "{name}/{input}");
                continue;
            }
            if input != "artifact-key" {
                assert_eq!(inputs[input]["type"], contract["type"], "{name}/{input}");
                assert_eq!(
                    inputs[input]["default"], contract["default"],
                    "{name}/{input}"
                );
            }
            assert_eq!(
                with[input],
                format!("${{{{ inputs.{input} }}}}"),
                "{name}/{input}"
            );
        }
    }
}

#[test]
fn crates_io_is_the_only_publication_destination() {
    let data = workflow("publish-crate");
    for input in ["registry", "registry-index"] {
        assert!(data["on"]["workflow_call"]["inputs"].get(input).is_none());
    }
    assert_eq!(data["jobs"]["publish"]["environment"], "release");
}

#[test]
fn selected_package_and_tag_are_verified_before_publication() {
    let mut fixture = Fixture::new();
    let manifest = fixture
        .root
        .join("project/Cargo.toml")
        .display()
        .to_string();
    fixture.set("DRY_RUN", "false");
    for (package, tag, publish, manifest, message) in [
        (
            "unknown",
            "refs/tags/v0.1.0",
            json!(null),
            manifest.as_str(),
            "package must select exactly one workspace member",
        ),
        (
            "fixture",
            "refs/tags/v9.0.0",
            json!(null),
            manifest.as_str(),
            "Release tag must equal selected package version",
        ),
        (
            "fixture",
            "refs/tags/v0.1.0",
            json!(["other"]),
            manifest.as_str(),
            "Package does not permit crates.io publication",
        ),
        (
            "fixture",
            "refs/tags/v0.1.0",
            json!(null),
            "/etc/hostname",
            "Selected package escapes checkout",
        ),
    ] {
        let metadata = json!({"workspace_members": ["fixture"], "packages": [{
            "id": "fixture", "name": "fixture", "version": "0.1.0", "publish": publish,
            "manifest_path": manifest}]});
        fixture.stub("cargo", &format!("printf '%s\\n' '{metadata}'"));
        fixture.set("PACKAGE", package);
        fixture.set("REF", tag);
        refused(&fixture.run("publish-crate", "package"), message);
    }
    assert!(!fixture.calls().contains("publish"));
    assert!(!fixture.calls().contains("--package"));
}

#[test]
fn a_malformed_dry_run_value_cannot_skip_the_tag_check() {
    // Read as a literal "false", any other spelling means a live run takes the
    // dry-run branch: the tag no longer has to equal the version and the
    // manifest no longer has to permit the registry. The step refuses instead.
    let mut fixture = Fixture::new();
    let manifest = fixture
        .root
        .join("project/Cargo.toml")
        .display()
        .to_string();
    let metadata = json!({"workspace_members": ["fixture"], "packages": [{
        "id": "fixture", "name": "fixture", "version": "0.1.0", "publish": null,
        "manifest_path": manifest}]});
    fixture.stub("cargo", &format!("printf '%s\\n' '{metadata}'"));
    fixture.set("PACKAGE", "fixture");
    fixture.set("REF", "refs/tags/v9.0.0");
    for value in ["False", "FALSE", "0", "", "no"] {
        fixture.set("DRY_RUN", value);
        refused(
            &fixture.run("publish-crate", "package"),
            "DRY_RUN must be true or false",
        );
    }
    assert!(!fixture.calls().contains("--package"));
}

#[test]
fn missing_live_token_prevents_cargo_invocation() {
    let mut fixture = Fixture::new();
    fixture.trusted();
    fixture.stub("cargo", "exit 0");
    refused(
        &fixture.run("publish-crate", "publish"),
        "CARGO_REGISTRY_TOKEN is required for live publication",
    );
    assert!(!fixture.calls().contains("cargo\n"));
}

#[test]
fn live_cargo_command_scopes_token_and_registry_without_real_publication() {
    let mut fixture = Fixture::new();
    fixture.trusted();
    fixture.set("TOKEN", "test-only-not-a-credential");
    fixture.stub(
        "cargo",
        r#"[[ "$CARGO_REGISTRIES_CRATES_IO_INDEX" == sparse+https://index.crates.io/ ]]
[[ "$CARGO_REGISTRIES_CRATES_IO_TOKEN" == test-only-not-a-credential ]]
[[ "$CARGO_REGISTRIES_CRATES_IO_CREDENTIAL_PROVIDER" == cargo:token ]]
[[ "$*" == 'publish --locked --no-verify --package fixture --registry crates-io' ]]"#,
    );
    succeeds(&fixture.run("publish-crate", "publish"));
    assert!(fixture.calls().contains("environments/release"));
    assert!(!fixture.calls().contains("test-only-not-a-credential"));
    let trace = fixture.trace();
    assert!(!trace.contains("test-only-not-a-credential"), "{trace}");
    for name in ["TOKEN", "INDEX", "CREDENTIAL_PROVIDER"] {
        assert!(trace.contains(&format!("CARGO_REGISTRIES_CRATES_IO_{name}=[REDACTED]")));
    }
}

#[test]
fn publishers_queue_runs_per_ref_and_never_cancel_one() {
    // A publication interrupted by the next push leaves a half-written release;
    // runs for the same ref wait for each other instead.
    for name in ["publish-binaries", "publish-crate"] {
        let concurrency = &workflow(name)["concurrency"];
        assert_eq!(concurrency["cancel-in-progress"], false, "{name}");
        let group = concurrency["group"].as_str().unwrap();
        assert!(
            group.contains("github.ref") && group.contains("inputs.working-directory"),
            "{name}: {group}"
        );
    }
    let defaults = workflow("ci")["on"]["workflow_call"]["inputs"].clone();
    assert_eq!(defaults["mutation-test"]["default"], true);
    assert_eq!(defaults["unused-dependencies"]["default"], true);
    assert_eq!(defaults["unsafe-policy"]["default"], "deny");
}

#[test]
fn semver_check_fails_the_publication_when_cargo_semver_checks_does() {
    // Off by default because a first publication has no baseline; on, the
    // verdict of cargo-semver-checks is the step's own.
    let mut fixture = Fixture::new();
    fixture.stub("cargo", "exit 5");
    fixture.set("SEMVER_CHECK", "false");
    succeeds(&fixture.run("publish-crate", "semver"));
    assert!(
        fixture.calls().is_empty(),
        "semver-check=false must run nothing"
    );
    fixture.set("SEMVER_CHECK", "true");
    assert_eq!(
        fixture.run("publish-crate", "semver").status.code(),
        Some(5)
    );
    assert!(
        fixture
            .calls()
            .contains("semver-checks check-release --package fixture")
    );
}

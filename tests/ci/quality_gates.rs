//! `ci.yml`: the lint, documentation, coverage and analysis gates, each proven
//! to fail when its tool does.

use crate::harness::{Fixture, refused, root, succeeds};
use serde_json::{Value, json};
use std::fs;

#[test]
fn clippy_denies_leftover_scaffolding_at_every_level() {
    // todo!() and dbg!() are scaffolding. Clippy allows both by default, so the
    // gate denies them itself, at every Clippy level a caller can select.
    for level in ["default", "pedantic", "nursery"] {
        let mut fixture = Fixture::new();
        fixture.set("GITHUB_WORKSPACE", &fixture.root.display().to_string());
        fixture.set("CLIPPY_LEVEL", level);
        fixture.set("METADATA", &workspace_metadata(&fixture));
        fixture.stub("cargo", CARGO);
        succeeds(&fixture.run("ci", "quality"));
        let calls = fixture.calls();
        let clippy = calls
            .lines()
            .find(|line| line.starts_with("clippy --workspace") && line.contains(" -- "))
            .unwrap_or_else(|| panic!("no clippy gate at level {level}: {calls}"));
        assert!(
            clippy.contains("-D warnings -D clippy::todo -D clippy::dbg_macro"),
            "level {level}: {clippy}"
        );
        if level != "default" {
            assert!(
                clippy.contains("-D clippy::pedantic"),
                "level {level}: {clippy}"
            );
        }
    }
    // The unsafe ban is one more lint on the same command line, so it reaches
    // the workspace members only: a dependency that uses unsafe stays green.
    for (policy, banned) in [("allow", false), ("deny", true)] {
        let mut fixture = Fixture::new();
        fixture.set("GITHUB_WORKSPACE", &fixture.root.display().to_string());
        fixture.set("UNSAFE_POLICY", policy);
        fixture.set("METADATA", &workspace_metadata(&fixture));
        fixture.stub("cargo", CARGO);
        succeeds(&fixture.run("ci", "quality"));
        let calls = fixture.calls();
        let clippy = calls
            .lines()
            .find(|line| line.starts_with("clippy --workspace") && line.contains(" -- "))
            .unwrap_or_else(|| panic!("no clippy gate with unsafe-policy={policy}: {calls}"));
        assert_eq!(
            clippy.contains("-D unsafe_code"),
            banned,
            "unsafe-policy={policy}: {clippy}"
        );
        assert!(
            !calls.contains("RUSTFLAGS"),
            "the ban must not reach dependencies"
        );
    }
}

/// The metadata a stubbed `cargo metadata` answers: one member, its manifest
/// and one source file, all inside the fixture's checkout.
fn workspace_metadata(fixture: &Fixture) -> String {
    json!({"workspace_members": ["fixture"], "packages": [{
        "id": "fixture", "manifest_path": fixture.root.join("project/Cargo.toml"),
        "targets": [{"src_path": fixture.root.join("project/src/lib.rs")}]}]})
    .to_string()
}

/// The `cargo` a quality test needs: it answers `cargo metadata` with the
/// fixture's own workspace, and under `nextest` leaves the `JUnit` document a
/// real run would have left, which the step copies beside the other reports.
const CARGO: &str = r#"[[ "$1" == metadata ]] && printf '%s' "$METADATA"
[[ "$1" == clippy ]] && printf '{"reason":"build-finished","success":true}\n'
[[ "$1" == nextest ]] && printf '<testsuites tests="1"/>\n' > "$REPORTS/tests.xml"
exit 0"#;

#[test]
fn sarif_reports_are_written_only_when_asked_and_never_empty() {
    // SARIF is an opt-in second pass over the Clippy output. Off, the converter
    // never runs; on, its report must exist and carry bytes, because an empty
    // SARIF uploads as a clean scan.
    let mut fixture = Fixture::new();
    fixture.set("GITHUB_WORKSPACE", &fixture.root.display().to_string());
    fixture.set("METADATA", &workspace_metadata(&fixture));
    fixture.stub("cargo", CARGO);
    fixture.stub(
        "clippy-sarif",
        r#"out=""
while [[ $# -gt 0 ]]; do
  [[ "$1" == --output ]] && { out=$2; shift 2; continue; }
  shift
done
printf '{}' > "$out""#,
    );
    succeeds(&fixture.run("ci", "quality"));
    assert!(
        !fixture.calls().contains("clippy-sarif"),
        "the SARIF converter ran without being asked"
    );
    assert!(!fixture.root.join("reports/clippy.sarif").exists());
    fixture.set("SARIF_REPORTS", "true");
    succeeds(&fixture.run("ci", "quality"));
    assert!(fixture.root.join("reports/clippy.sarif").is_file());
    fs::remove_file(fixture.root.join("reports/clippy.sarif")).unwrap();
    fixture.stub("clippy-sarif", "exit 0");
    assert!(
        !fixture.run("ci", "quality").status.success(),
        "an empty SARIF report must fail the step"
    );
}

#[test]
fn strict_rustdoc_fails_the_run_when_cargo_doc_does() {
    // Strict rustdoc is the last cargo command of the quality step: an
    // undocumented public item or a broken intra-doc link fails cargo doc,
    // and that failure is the step's, with the lint flags cargo doc was given.
    let mut fixture = Fixture::new();
    fixture.set("GITHUB_WORKSPACE", &fixture.root.display().to_string());
    fixture.set("METADATA", &workspace_metadata(&fixture));
    fixture.stub(
        "cargo",
        r#"[[ "$1" == metadata ]] && printf '%s' "$METADATA"
[[ "$1" == nextest ]] && printf '<testsuites tests="1"/>\n' > "$REPORTS/tests.xml"
[[ "$1" == clippy ]] && printf '{"reason":"build-finished","success":true}\n'
[[ "$1" == doc ]] && { printf 'RUSTDOCFLAGS=%s\n' "$RUSTDOCFLAGS" >> "$CALLS"; exit 7; }
exit 0"#,
    );
    assert_eq!(fixture.run("ci", "quality").status.code(), Some(7));
    let calls = fixture.calls();
    assert!(
        calls.contains("RUSTDOCFLAGS=-D warnings -D missing_docs"),
        "{calls}"
    );
    assert_eq!(
        calls
            .lines()
            .filter(|line| line.starts_with("clippy "))
            .count(),
        1
    );
    assert!(
        fs::read_to_string(fixture.root.join("reports/clippy.json"))
            .unwrap()
            .contains("build-finished")
    );
}

#[test]
fn a_broken_private_doc_fails_the_second_strict_rustdoc_build() {
    // The public build leaves private items out, so a broken intra-doc link
    // or a bad doc on one never reaches rustdoc. A second build documents
    // them under the same flags, and its failure is the step's.
    let mut fixture = Fixture::new();
    fixture.set("GITHUB_WORKSPACE", &fixture.root.display().to_string());
    fixture.set("METADATA", &workspace_metadata(&fixture));
    fixture.stub(
        "cargo",
        r#"[[ "$1" == metadata ]] && printf '%s' "$METADATA"
[[ "$1" == nextest ]] && printf '<testsuites tests="1"/>\n' > "$REPORTS/tests.xml"
[[ "$1" == clippy ]] && printf '{"reason":"build-finished","success":true}\n'
[[ "$1" == doc ]] && printf 'RUSTDOCFLAGS=%s %s\n' "$RUSTDOCFLAGS" "$*" >> "$CALLS"
[[ "$*" == *--document-private-items* ]] && exit 9
exit 0"#,
    );
    assert_eq!(fixture.run("ci", "quality").status.code(), Some(9));
    let calls = fixture.calls();
    let builds: Vec<&str> = calls
        .lines()
        .filter(|line| line.starts_with("RUSTDOCFLAGS="))
        .collect();
    assert_eq!(
        builds,
        [
            "RUSTDOCFLAGS=-D warnings -D missing_docs doc --workspace --no-deps --locked",
            "RUSTDOCFLAGS=-D warnings -D missing_docs doc --workspace --no-deps --locked \
             --document-private-items",
        ],
        "{calls}"
    );
}

#[test]
fn line_coverage_below_the_threshold_fails_the_run() {
    // The threshold is handed to cargo-llvm-cov as --fail-under-lines, so a
    // shortfall is the tool's own non-zero exit and the step's. A run that
    // leaves no LCOV report behind fails as well: the report is what the
    // scorecard and a reviewer read.
    let mut fixture = Fixture::new();
    fixture.set("COVERAGE", "90");
    fixture.stub("cargo", "exit 3");
    assert_eq!(fixture.run("ci", "coverage").status.code(), Some(3));
    assert!(fixture.calls().contains("--fail-under-lines 90"));
    fixture.stub("cargo", "exit 0");
    assert!(
        !fixture.run("ci", "coverage").status.success(),
        "a missing coverage report must fail the step"
    );
    fixture.stub(
        "cargo",
        r#"out=""
while [[ $# -gt 0 ]]; do
  [[ "$1" == --output-path ]] && { out=$2; shift 2; continue; }
  shift
done
printf 'TN:\nend_of_record\n' > "$out""#,
    );
    succeeds(&fixture.run("ci", "coverage"));
    assert!(fixture.root.join("reports/coverage.lcov").is_file());
}

#[test]
fn every_declared_feature_is_compiled_and_a_broken_one_fails() {
    // A default build proves one combination. The gate builds each declared
    // feature on its own, so a feature nobody selects cannot quietly stop
    // compiling; a workspace declaring none spends no compile saying so.
    let fixture = Fixture::new();
    let metadata = |features: Value| {
        json!({"workspace_members": ["p"],
               "workspace_root": fixture.root.join("project"),
               "packages": [{"id": "p", "name": "fixture", "features": features,
                             "manifest_path": fixture.root.join("project/Cargo.toml")}]})
        .to_string()
    };
    fs::write(
        fixture.root.join("project/Cargo.lock"),
        "version = 4\n[[package]]\nname = 'fixture'\nversion = '0.1.0'\n",
    )
    .unwrap();
    fixture.stub(
        "cargo",
        "if [[ \"$1\" == metadata ]]; then cat \"$RUNNER_TEMP/metadata.json\"; fi",
    );

    // No feature: nothing to build, and the run says which it is.
    fs::write(fixture.root.join("metadata.json"), metadata(json!({}))).unwrap();
    let outcome = fixture.run("ci", "features");
    succeeds(&outcome);
    assert!(
        fs::read_to_string(fixture.root.join("output"))
            .unwrap()
            .ends_with("applied=false\n")
    );
    assert!(
        String::from_utf8_lossy(&outcome.stdout).contains("SKIPPED: the workspace declares no"),
        "a workspace without features must say so"
    );
    assert!(
        !fixture.calls().contains("hack"),
        "nothing to check must cost no compile: {}",
        fixture.calls()
    );

    // Declared features: each one is built, and the report names them.
    fs::write(
        fixture.root.join("metadata.json"),
        metadata(json!({"default": ["tls"], "tls": [], "vendored": []})),
    )
    .unwrap();
    succeeds(&fixture.run("ci", "features"));
    assert!(
        fs::read_to_string(fixture.root.join("output"))
            .unwrap()
            .ends_with("applied=true\n")
    );
    assert!(
        fixture
            .calls()
            .contains("hack check --workspace --offline --each-feature --remove-dev-deps"),
        "each feature must be built on its own: {}",
        fixture.calls()
    );
    let report = fs::read_to_string(fixture.root.join("reports/features.txt")).unwrap();
    assert_eq!(report, "default\ntls\nvendored", "the report names them");

    // The build is the gate: a combination that does not compile fails here.
    fixture.stub(
        "cargo",
        "if [[ \"$1\" == metadata ]]; then cat \"$RUNNER_TEMP/metadata.json\"; \
         exit 0; fi\nexit 101",
    );
    assert_eq!(fixture.run("ci", "features").status.code(), Some(101));
}

#[test]
fn unused_dependencies_and_recorded_audits_fail_the_run_when_their_tool_does() {
    // Both gates are one tool each. Off, they say SKIPPED in their report and
    // run nothing; on, the tool's own verdict is the step's, and cargo-vet
    // without a committed ledger fails rather than auditing nothing.
    let mut fixture = Fixture::new();
    fixture.stub("cargo", "exit 3");
    succeeds(&fixture.run("ci", "unused"));
    succeeds(&fixture.run("ci", "vet"));
    assert!(
        fixture.calls().is_empty(),
        "a gate that is off must run nothing"
    );
    for report in ["unused-dependencies.txt", "dependency-audit.txt"] {
        assert!(
            fs::read_to_string(fixture.root.join("reports").join(report))
                .unwrap()
                .contains("SKIPPED"),
            "{report} must say the gate was off"
        );
    }
    fixture.set("UNUSED_DEPENDENCIES", "true");
    assert_eq!(fixture.run("ci", "unused").status.code(), Some(3));
    assert!(fixture.calls().contains("machete"));
    fixture.set("DEPENDENCY_AUDIT", "true");
    refused(
        &fixture.run("ci", "vet"),
        "dependency-audit=true requires a committed supply-chain/config.toml; run cargo vet init",
    );
    fs::create_dir_all(fixture.root.join("project/supply-chain")).unwrap();
    fs::write(fixture.root.join("project/supply-chain/config.toml"), "").unwrap();
    refused(
        &fixture.run("ci", "vet"),
        "vet: supply-chain/config.toml does not import orchestration-maestro, mozilla, google, \
         bytecode-alliance, isrg, zcash (VET-001); import the organization's audits and those \
         of Mozilla, Google, the Bytecode Alliance, ISRG and the Zcash Foundation, then run \
         cargo vet regenerate imports",
    );
    // The imports every repository holds, those of an example here.
    fs::copy(
        root().join("examples/binary/supply-chain/config.toml"),
        fixture.root.join("project/supply-chain/config.toml"),
    )
    .unwrap();
    assert_eq!(fixture.run("ci", "vet").status.code(), Some(3));
    assert!(fixture.calls().contains("vet --locked"));
    fixture.stub("cargo", "echo audited");
    succeeds(&fixture.run("ci", "unused"));
    succeeds(&fixture.run("ci", "vet"));
    assert!(
        fs::read_to_string(fixture.root.join("reports/dependency-audit.txt"))
            .unwrap()
            .contains("audited")
    );
    // The organization's audits count under the home's former name too.
    let config = fixture.root.join("project/supply-chain/config.toml");
    let renamed = fs::read_to_string(&config)
        .unwrap()
        .replace("/maestro-rust-workflows/", "/rust-workflows/");
    fs::write(&config, renamed).unwrap();
    succeeds(&fixture.run("ci", "vet"));
}

#[test]
fn the_quality_gate_runs_doctests_and_strict_rustdoc() {
    // Doctests are the examples a reader copies, and rustdoc under
    // `-D missing_docs` is what makes every public item explained. Neither
    // runs by luck: both are commands of the quality step, read from the
    // trace of a run against a cargo that answers metadata and nothing else.
    let mut fixture = Fixture::new();
    fixture.set("METADATA", &workspace_metadata(&fixture));
    fixture.stub(
        "cargo",
        r#"[[ "$1" == metadata ]] && printf '%s' "$METADATA"
[[ "$1" == nextest ]] && printf '<testsuites tests="1"/>\n' > "$REPORTS/tests.xml"
[[ "$*" == *message-format=json* ]] && echo '{}'
exit 0"#,
    );
    succeeds(&fixture.run("ci", "quality"));
    let trace = fixture.trace();
    assert!(
        trace.contains("cargo test --workspace --doc --locked"),
        "doctests must run explicitly: {trace}"
    );
    assert!(
        trace.contains(
            "RUSTDOCFLAGS='-D warnings -D missing_docs' cargo doc --workspace --no-deps --locked"
        ),
        "rustdoc must run strictly: {trace}"
    );
    assert!(
        trace.contains(
            "RUSTDOCFLAGS='-D warnings -D missing_docs' cargo doc --workspace --no-deps --locked \
             --document-private-items"
        ),
        "private items must be documented strictly too: {trace}"
    );
}

//! Contract tests for the sole branch-protection status.

use crate::harness::{Fixture, refused, succeeds};

#[test]
fn the_required_status_fails_unless_every_result_succeeded() {
    // The one status a branch protection can require: green only when every
    // upstream result was a success, in ci.yml and in the consumer matrix alike.
    let mut fixture = Fixture::new();
    for status in ["failure", "cancelled", "skipped", ""] {
        fixture.set("RESULT", status);
        refused(
            &fixture.run("ci", "required"),
            "Required Rust checks failed or were skipped",
        );
    }
    fixture.set("RESULT", "success");
    for (key, value) in [
        ("PORTABILITY", "skipped"),
        ("RUNNERS", ""),
        ("MUTATION_TEST", "false"),
        ("MUTATION_MODE", "disabled"),
        ("MUTATION_COUNT", "0"),
        ("MUTATIONS_RESULT", "skipped"),
        ("MUTATION_SUMMARY_RESULT", "skipped"),
        ("MUTATION_WINDOWS", "[]"),
        ("WINDOWS_MUTATIONS_RESULT", "skipped"),
        ("MUTATION_SHARDS", "0"),
        ("MUTATION_MATRIX", "[]"),
        ("MUTATION_ATTEMPT", "1"),
        ("GITHUB_RUN_ATTEMPT", "1"),
    ] {
        fixture.set(key, value);
    }
    succeeds(&fixture.run("ci", "required"));
    for (mode, count, shards) in [
        ("inline", "", "1"),
        ("inline", "50", "1"),
        ("empty", "0", "0"),
    ] {
        fixture.set("MUTATION_TEST", "true");
        fixture.set("MUTATION_MODE", mode);
        fixture.set("MUTATION_COUNT", count);
        fixture.set("MUTATION_SHARDS", shards);
        succeeds(&fixture.run("ci", "required"));
    }
    fixture.set("MUTATION_MODE", "inline");
    fixture.set("MUTATION_SHARDS", "1");
    for count in ["0", "-1", "not-a-number"] {
        fixture.set("MUTATION_COUNT", count);
        assert!(!fixture.run("ci", "required").status.success(), "{count}");
    }
    for status in ["failure", "cancelled", "skipped", ""] {
        fixture.set("MUTATION_TEST", "true");
        fixture.set("MUTATION_MODE", "sharded");
        fixture.set("MUTATION_COUNT", "3");
        fixture.set("MUTATION_SHARDS", "2");
        fixture.set("MUTATION_MATRIX", "[0,1]");
        fixture.set("MUTATIONS_RESULT", status);
        fixture.set("MUTATION_SUMMARY_RESULT", "success");
        assert!(!fixture.run("ci", "required").status.success(), "{status}");
        fixture.set("MUTATIONS_RESULT", "success");
        fixture.set("MUTATION_SUMMARY_RESULT", status);
        assert!(!fixture.run("ci", "required").status.success(), "{status}");
    }
    fixture.set("MUTATION_SUMMARY_RESULT", "success");
    fixture.set("MUTATIONS_RESULT", "success");
    fixture.set("MUTATION_WINDOWS", "[\"src/windows.rs\"]");
    fixture.set("WINDOWS_MUTATIONS_RESULT", "failure");
    refused(
        &fixture.run("ci", "required"),
        "Windows-owned mutation job failed or was skipped",
    );
    fixture.set("WINDOWS_MUTATIONS_RESULT", "success");
    succeeds(&fixture.run("ci", "required"));
    fixture.set("MUTATION_WINDOWS", "[]");
    fixture.set("WINDOWS_MUTATIONS_RESULT", "success");
    refused(
        &fixture.run("ci", "required"),
        "Windows-owned mutation job ran without configured files",
    );
    fixture.set("WINDOWS_MUTATIONS_RESULT", "skipped");
    fixture.set("MUTATION_ATTEMPT", "1");
    fixture.set("GITHUB_RUN_ATTEMPT", "1");
    succeeds(&fixture.run("ci", "required"));
    for (mode, count, shards, matrix) in [
        ("", "", "", ""),
        ("sharded", "1", "2", "[0,1]"),
        ("sharded", "3", "2", "[1,0]"),
        ("sharded", "3", "3", "[0,1]"),
    ] {
        fixture.set("MUTATION_MODE", mode);
        fixture.set("MUTATION_COUNT", count);
        fixture.set("MUTATION_SHARDS", shards);
        fixture.set("MUTATION_MATRIX", matrix);
        assert!(
            !fixture.run("ci", "required").status.success(),
            "{mode} {count} {shards} {matrix}"
        );
    }
    fixture.set("MUTATION_MODE", "sharded");
    fixture.set("MUTATION_COUNT", "3");
    fixture.set("MUTATION_SHARDS", "2");
    fixture.set("MUTATION_MATRIX", "[0,1]");
    fixture.set("MUTATION_ATTEMPT", "1");
    fixture.set("GITHUB_RUN_ATTEMPT", "2");
    assert!(!fixture.run("ci", "required").status.success());
}

#[test]
fn engine_control_and_feature_jobs_require_complete_aggregation_and_intentional_skips() {
    let mut fixture = Fixture::new();
    for (key, value) in [
        ("RESULT", "success"),
        ("RUNNERS", ""),
        ("MUTATION_TEST", "true"),
        ("MUTATION_MODE", "inline"),
        ("MUTATION_COUNT", "1"),
        ("MUTATION_SHARDS", "1"),
        ("MUTATION_MATRIX", "[]"),
        ("MUTATIONS_RESULT", "skipped"),
        ("MUTATION_ENGINE_FILES", "[\"src/engine.rs\"]"),
        ("MUTATION_ENGINE_COUNT", "1"),
        ("MUTATION_ENGINE_DEFAULT_COUNT", "1"),
        ("ENGINE_MUTATIONS_RESULT", "success"),
        ("ENGINE_DEFAULT_MUTATIONS_RESULT", "success"),
        ("MUTATION_SUMMARY_RESULT", "success"),
    ] {
        fixture.set(key, value);
    }
    succeeds(&fixture.run("ci", "required"));
    for variable in ["ENGINE_MUTATIONS_RESULT", "ENGINE_DEFAULT_MUTATIONS_RESULT"] {
        for state in ["failure", "cancelled", "skipped"] {
            fixture.set(variable, state);
            refused(
                &fixture.run("ci", "required"),
                "Engine-owned mutation job failed or was skipped",
            );
        }
        fixture.set(variable, "success");
    }
    fixture.set("MUTATION_SUMMARY_RESULT", "skipped");
    refused(
        &fixture.run("ci", "required"),
        "Mutation partition aggregation failed or was skipped",
    );
    fixture.set("MUTATION_SUMMARY_RESULT", "success");
    fixture.set("MUTATION_ENGINE_COUNT", "not-a-number");
    refused(
        &fixture.run("ci", "required"),
        "Engine mutation count is invalid",
    );
    fixture.set("MUTATION_ENGINE_COUNT", "0");
    refused(
        &fixture.run("ci", "required"),
        "Engine-owned mutation job ran without planned mutants",
    );
    fixture.set("ENGINE_MUTATIONS_RESULT", "skipped");
    succeeds(&fixture.run("ci", "required"));
}

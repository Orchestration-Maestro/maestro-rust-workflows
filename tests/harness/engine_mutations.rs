//! Shared mode-aware engine planner, worker and aggregate fixtures.

use super::fixture::{Fixture, succeeds};
use super::mutation_shards::{copy_tree, planning_fixture};
use super::repository::tool;
use serde_json::{Value, json};
use std::path::Path;
use std::{fs, process::Output};

/// Shared policy fixture, optionally advanced through planning for worker contracts.
pub(crate) fn engine_fixture(planned: bool) -> Fixture {
    if planned {
        engine_execution_fixture(true)
    } else {
        engine_planning_fixture()
    }
}

/// A feature-owned source with an independent featureless mode obligation.
fn engine_planning_fixture() -> Fixture {
    let mut fixture = planning_fixture(2, 0, 1);
    fixture.set("GITHUB_WORKSPACE", &fixture.root.display().to_string());
    fixture.set("MUTATION_ENGINE_FEATURES", "[\"engine\"]");
    fixture.set("MUTATION_ENGINE_FILES", "[\"src/engine.rs\"]");
    fs::write(fixture.root.join("project/src/engine.rs"), "").unwrap();
    let path = fixture.root.join("listing.json");
    let mut default: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    default[0]["file"] = json!("src/engine.rs");
    fs::write(&path, default.to_string()).unwrap();
    let mut engine = default.clone();
    engine[0]["replacement"] = json!("engine replacement");
    fs::write(fixture.root.join("engine.json"), engine.to_string()).unwrap();
    fixture.set(
        "METADATA",
        &json!({"workspace_root":fixture.root.join("project"), "packages":[{
            "name":"fixture", "version":"0.1.0", "features":{"engine":[]},
        "manifest_path":fixture.root.join("project/Cargo.toml")
        }]})
        .to_string(),
    );
    fixture.stub(
        "cargo",
        r#"if [[ $1 == metadata ]]; then printf '%s' "$METADATA"; exit; fi
[[ $1 == mutants && $2 == --list ]] || exit 88
if [[ " $* " == *" --features "* ]]; then cat "$RUNNER_TEMP/engine.json";
else cat "$RUNNER_TEMP/listing.json"; fi"#,
    );
    fixture
}

/// A dual-mode worker fixture with independent real-shaped outcome documents.
fn engine_execution_fixture(compiled: bool) -> Fixture {
    let mut fixture = engine_planning_fixture();
    if !compiled {
        fs::copy(
            fixture.root.join("listing.json"),
            fixture.root.join("engine.json"),
        )
        .unwrap();
    }
    succeeds(&fixture.run_body("rust-gate mutants-plan"));
    for (key, name) in [
        ("MUTATION_ENGINE_PLAN", "mutation-engine-plan.json"),
        ("MUTATION_ENGINE_LIST", "mutation-engine-list.json"),
        (
            "MUTATION_ENGINE_DEFAULT_LIST",
            "mutation-engine-default-list.json",
        ),
    ] {
        fixture.set(
            key,
            &fixture
                .root
                .join("reports")
                .join(name)
                .display()
                .to_string(),
        );
    }
    fixture.set("MUTATION_SHARDS", "1");
    fixture.set("MUTATION_SHARD", "0/1");
    fixture.set("CONTROL_COMPILED", if compiled { "true" } else { "false" });
    for (name, listing) in [
        ("engine", "mutation-engine-list.json"),
        ("default", "mutation-engine-default-list.json"),
    ] {
        let mutants: Value =
            serde_json::from_slice(&fs::read(fixture.root.join("reports").join(listing)).unwrap())
                .unwrap();
        let outcomes = json!({"outcomes":[
            {"scenario":"Baseline", "summary":"Success", "phase_results":[
                {"phase":"Test", "duration":1.0,
                "argv":["cargo","test"], "process_status":"Success"}]},
            {"scenario":{"Mutant":mutants[0]}, "summary":"CaughtMutant", "phase_results":[{
                "phase":"Test", "duration":1.0,
                "argv":["cargo","test"], "process_status":{"Failure":101}}]}],
            "total_mutants":1, "caught":1,"missed":0,"timeout":0,"unviable":0,"success":0,
            "cargo_mutants_version":"27.1.0", "start_time":"2026-01-01T00:00:00Z",
            "end_time":"2026-01-01T00:01:00Z"});
        fs::write(
            fixture.root.join(format!("{name}-outcomes.json")),
            outcomes.to_string(),
        )
        .unwrap();
    }
    fixture.stub(
        "cargo",
        r#"if [[ $1 == metadata ]]; then printf '%s' "$METADATA"; exit; fi
if [[ $1 == test ]]; then
 printf '%s' "${CARGO_ENCODED_RUSTFLAGS-unset}" > "$RUNNER_TEMP/encoded-flags"
 printf '%s' "${RUSTFLAGS-unset}" > "$RUNNER_TEMP/rust-flags"
 target=''
 while (( $# )); do
  case $1 in --target-dir) target=$2; shift 2;; *) shift;; esac
 done
 mkdir -p "$target/debug/deps"
 sources='src/lib.rs'
 if [[ $CONTROL_COMPILED == true ]]; then sources+=' src/engine.rs'; fi
 printf '%s: %s\n' "$target/debug/deps/fixture.d" "$sources" > "$target/debug/deps/fixture.d"
 printf '{"reason":"compiler-artifact","fresh":false,"target":{"kind":["lib"],'
 printf '"src_path":"%s/src/lib.rs"},"filenames":["%s/debug/deps/libfixture.rlib"]}\n' \
  "$PROJECT" "$target"
 printf '{"reason":"build-finished","success":true}\n'
 exit "${CONTROL_BUILD_STATUS:-0}"
fi
out=''; mode=default
while (( $# )); do
 case $1 in --features) mode=engine; shift 2;; --output) out=$2; shift 2;; *) shift;; esac
done
mkdir -p "$out/mutants.out"
cp "$RUNNER_TEMP/$mode-outcomes.json" "$out/mutants.out/outcomes.json"
if [[ $mode == engine ]]; then list=mutation-engine-list.json;
else list=mutation-engine-default-list.json; fi
cp "$RUNNER_TEMP/reports/$list" "$out/mutants.out/mutants.json"
exit "${CONTROL_MUTANT_STATUS:-0}""#,
    );
    fixture
}

/// Complete inline/default, featureless control and engine evidence for one run.
pub(crate) fn engine_aggregation_fixture() -> Fixture {
    harvest_engine_fixture(engine_execution_fixture(true))
}

/// Non-member controls with identical engine twins, using the same artifact transport.
pub(crate) fn engine_inactive_fixture() -> Fixture {
    harvest_engine_fixture(engine_execution_fixture(false))
}

/// Retain the complete required artifacts for either compile-membership case.
fn harvest_engine_fixture(mut fixture: Fixture) -> Fixture {
    succeeds(&fixture.run_body("rust-gate mutants-engine"));
    succeeds(&fixture.run_body("rust-gate mutants-engine-default"));
    let checks = fixture.root.join("checks");
    copy_tree(&fixture.root.join("reports"), &checks);
    let listing: Value =
        serde_json::from_slice(&fs::read(checks.join("mutants-list.json")).unwrap()).unwrap();
    let mut outcomes: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("default-outcomes.json")).unwrap())
            .unwrap();
    outcomes["outcomes"][1]["scenario"]["Mutant"] = listing[0].clone();
    fs::write(checks.join("mutants.json"), outcomes.to_string()).unwrap();
    fs::create_dir_all(checks.join("mutants/mutants.out")).unwrap();
    fs::write(
        checks.join("mutants/mutants.out/outcomes.json"),
        outcomes.to_string(),
    )
    .unwrap();
    for (prefix, mode, receipt) in [
        ("engine", "mutants-engine", "mutants-engine-shard.json"),
        (
            "engine-default",
            "mutants-engine-default",
            "mutants-engine-default-shard.json",
        ),
    ] {
        let artifacts = fixture.root.join(prefix);
        let worker = artifacts.join(format!("fixture-{prefix}-mutants-0"));
        copy_tree(&fixture.root.join(mode), &worker.join(mode));
        fs::create_dir_all(worker.join("rust-reports")).unwrap();
        fs::copy(
            fixture.root.join("reports").join(receipt),
            worker.join("rust-reports").join(receipt),
        )
        .unwrap();
        fixture.set(
            if prefix == "engine" {
                "MUTATION_ENGINE_ARTIFACTS"
            } else {
                "MUTATION_ENGINE_DEFAULT_ARTIFACTS"
            },
            &artifacts.display().to_string(),
        );
    }
    for (key, value) in [
        ("MUTATION_MODE", "inline"),
        ("MUTATION_SHARDS", "1"),
        ("MUTATION_MATRIX", "[]"),
        ("MUTATION_ARTIFACT_NAME", "fixture"),
        ("MUTATION_ENGINE_COUNT", "1"),
        ("MUTATION_ENGINE_SHARDS", "1"),
        ("MUTATION_ENGINE_MATRIX", "[0]"),
        ("MUTATION_ENGINE_DEFAULT_COUNT", "1"),
        ("MUTATION_ENGINE_DEFAULT_SHARDS", "1"),
        ("MUTATION_ENGINE_DEFAULT_MATRIX", "[0]"),
        ("ENGINE_MUTATIONS_RESULT", "success"),
        ("ENGINE_DEFAULT_MUTATIONS_RESULT", "success"),
        ("CHECKS_RESULT", "success"),
        ("MUTATIONS_RESULT", "skipped"),
    ] {
        fixture.set(key, value);
    }
    fixture.set("MUTATION_PLAN_DIR", &checks.display().to_string());
    fixture.set("MUTATION_ENGINE_PLAN_DIR", &checks.display().to_string());
    fixture
}

/// Each hosted aggregate run starts with a fresh report directory.
pub(crate) fn summarize_engine(fixture: &Fixture) -> Output {
    fs::remove_dir_all(fixture.root.join("reports")).unwrap();
    fs::create_dir_all(fixture.root.join("reports")).unwrap();
    fixture.run_body("rust-gate mutants-aggregate")
}

/// Refresh a digest to exercise semantic refusal rather than only byte-integrity refusal.
pub(crate) fn evidence_hash(path: &Path) -> String {
    let result = tool("sha256sum").arg(path).output().unwrap();
    succeeds(&result);
    String::from_utf8(result.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned()
}

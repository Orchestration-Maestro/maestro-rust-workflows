//! Hosted mutation proof shares production transport rather than a copied contract.

use crate::harness::workflow;
use serde_json::Value;

#[test]
fn hosted_mutation_cache_steps_equal_the_production_transport_contract() {
    let ci = workflow("ci");
    let fixture = workflow("native-cache-fixture");
    let production = ci["jobs"]["mutation-engine"]["steps"].as_array().unwrap();
    let job = &fixture["jobs"]["mutation-engine"];
    assert_eq!(job["if"], "${{ github.event_name != 'pull_request' }}");
    assert_eq!(job["needs"], "checks");
    assert_eq!(
        job["strategy"]["matrix"]["shard"],
        "${{ fromJSON(needs.checks.outputs.mutation-engine-matrix) }}"
    );
    let hosted = job["steps"]
        .as_array()
        .expect("hosted engine workers must exist");
    for id in [
        "native-cache-prepare",
        "native-cache-restore",
        "native-cache-inventory",
        "native-cache-save",
        "engine-mutation-run",
    ] {
        let find = |steps: &[Value]| steps.iter().find(|step| step["id"] == id).unwrap().clone();
        assert_eq!(find(hosted), find(production), "{id}");
    }
    assert_eq!(hosted.last().unwrap()["id"], "native-cache-save");
    let plan = fixture["jobs"]["checks"]["steps"].as_array().unwrap();
    let checkout = plan
        .iter()
        .find(|step| {
            step["uses"]
                .as_str()
                .is_some_and(|uses| uses.starts_with("actions/checkout@"))
        })
        .unwrap();
    assert!(checkout["with"]["fetch-depth"].is_null());
    assert!(
        plan.iter()
            .any(|step| step["run"] == "rust-gate mutants-plan")
    );
    assert!(fixture["on"]["workflow_dispatch"].is_null());
    assert!(
        fixture["on"]
            .as_object()
            .unwrap()
            .contains_key("workflow_dispatch")
    );
}

#[test]
fn each_hosted_verifier_selects_exactly_its_own_mode() {
    let fixture = workflow("native-cache-fixture");
    for (job, mode) in [
        ("native-cache", "coverage"),
        ("mutation-engine", "mutation"),
    ] {
        let runs = fixture["jobs"][job]["steps"].as_array().unwrap();
        let verifier = runs
            .iter()
            .filter_map(|step| step["run"].as_str())
            .find(|run| run.contains("verify_the_executed"))
            .unwrap();
        assert_eq!(
            verifier,
            format!(
                "cargo test --manifest-path tests/Cargo.toml --locked --features \
                 native-cache-hosted --test native_cache_hosted \
                 verify_the_executed_native_{mode}_fixture -- --exact --nocapture"
            )
        );
    }
}

#[test]
fn hosted_workers_consume_the_bound_planners_package_local_selection() {
    let fixture = workflow("native-cache-fixture");
    let checks = &fixture["jobs"]["checks"];
    assert_eq!(checks["env"]["MUTATION_ENGINE_FEATURES"], "[\"engine\"]");
    for name in ["mutation-engine-features", "mutation-engine-files"] {
        assert_eq!(
            checks["outputs"][name],
            format!("${{{{ steps.native-engine-plan.outputs.{name} }}}}")
        );
    }
    let steps = checks["steps"].as_array().unwrap();
    assert!(steps.iter().any(|step| step["id"] == "native-engine-plan"));
    let worker = &fixture["jobs"]["mutation-engine"];
    for name in ["MUTATION_ENGINE_FEATURES", "MUTATION_ENGINE_FILES"] {
        assert!(worker["env"][name].is_null());
    }
}

//! Static workflow wiring contracts for mutation planning and shards.

use crate::harness::{root, tool_rows, workflow};
use serde_json::{Value, json};
use std::fs;

fn workflow_step<'a>(steps: &'a [Value], id: &str) -> &'a Value {
    steps
        .iter()
        .find(|step| step["id"] == id)
        .unwrap_or_else(|| panic!("missing workflow step {id}"))
}

fn assert_required_gate_wiring(ci: &Value) {
    let jobs = &ci["jobs"];
    for name in [
        "checks",
        "portability",
        "mutations",
        "mutation-summary",
        "mutation-windows",
    ] {
        assert!(
            jobs["gate"]["needs"]
                .as_array()
                .unwrap()
                .contains(&json!(name))
        );
    }
    for output in ["artifact-id", "artifact-name", "revision"] {
        assert_eq!(
            jobs["gate"]["outputs"][output],
            format!(
                concat!(
                    "${{{{ steps.required.outcome == 'success' && ",
                    "needs.checks.outputs.{} || '' }}}}"
                ),
                output
            )
        );
    }
}

fn assert_matrix_and_summary_contract(ci: &Value) {
    let jobs = &ci["jobs"];
    assert_worker_matrix_contract(&jobs["mutations"]);
    assert_summary_job_contract(jobs);
}

fn assert_worker_matrix_contract(workers: &Value) {
    assert_eq!(workers["strategy"]["fail-fast"], false);
    assert_eq!(workers["strategy"]["max-parallel"], 32);
    assert!(
        workers["if"]
            .as_str()
            .unwrap()
            .contains("mutation-mode == 'sharded'")
    );
    assert_eq!(
        workers["strategy"]["matrix"]["shard"],
        "${{ fromJSON(needs.checks.outputs.mutation-matrix) }}"
    );
    let steps = workers["steps"].as_array().unwrap();
    let execution = workflow_step(steps, "mutation-worker-run");
    assert_eq!(execution["timeout-minutes"], 35);
    assert_eq!(execution["run"], "rust-gate mutants");
    assert!(steps.iter().any(|step| {
        step["uses"]
            .as_str()
            .unwrap_or_default()
            .starts_with("actions/upload-artifact@")
            && step["if"] == "${{ always() }}"
    }));
}

fn assert_summary_job_contract(jobs: &Value) {
    let summary = &jobs["mutation-summary"];
    assert_eq!(summary["permissions"], json!({}));
    assert!(summary["steps"].as_array().unwrap().iter().all(|step| {
        step["run"]
            .as_str()
            .is_none_or(|run| run.starts_with("rust-gate "))
    }));
    for name in ["checks", "mutations"] {
        assert!(summary["needs"].as_array().unwrap().contains(&json!(name)));
    }
    assert!(summary["if"].as_str().unwrap().contains("always()"));
    let steps = summary["steps"].as_array().unwrap();
    let aggregate = steps
        .iter()
        .position(|step| step["run"] == "rust-gate mutants-aggregate")
        .unwrap();
    let finalize = steps
        .iter()
        .position(|step| step["run"] == "rust-gate scorecard-finalize")
        .unwrap();
    assert!(aggregate < finalize);
    for name in ["upload", "coverage"] {
        assert!(
            jobs[name]["needs"]
                .as_array()
                .unwrap()
                .contains(&json!("mutation-summary"))
        );
        assert!(
            jobs[name]["if"]
                .as_str()
                .unwrap()
                .contains("needs.mutation-summary.result == 'success'")
        );
    }
}

fn assert_planner_and_consumer_contract(ci: &Value) {
    let mutation_steps = ci["jobs"]["checks"]["steps"].as_array().unwrap();
    let planner = mutation_steps
        .iter()
        .position(|step| step["id"] == "mutants-plan")
        .unwrap();
    let runner = mutation_steps
        .iter()
        .position(|step| step["id"] == "mutants")
        .unwrap();
    assert!(planner < runner);
    assert_eq!(mutation_steps[planner]["run"], "rust-gate mutants-plan");

    let internal = workflow("ci-internal");
    let sharded = &internal["jobs"]["sharded-consumer"];
    assert_eq!(sharded["uses"], "./.github/workflows/ci.yml");
    assert_eq!(sharded["with"]["working-directory"], "examples/workspace");
    assert_eq!(sharded["with"]["mutation-shards"], 2);
    assert_eq!(sharded["with"]["artifact-key"], "sharded-consumer");
    assert!(
        internal["jobs"]["required"]["needs"]
            .as_array()
            .unwrap()
            .contains(&json!("sharded-consumer"))
    );
    let required_steps = internal["jobs"]["required"]["steps"].as_array().unwrap();
    assert!(
        required_steps
            .iter()
            .any(|step| step["env"]["SHARDED_RESULT"] == "${{ needs.sharded-consumer.result }}")
    );
}

#[test]
fn windows_mutation_job_is_required_and_uses_its_pinned_asset() {
    let ci = workflow("ci");
    let windows = &ci["jobs"]["mutation-windows"];
    assert_eq!(windows["runs-on"], "windows-2025");
    assert_eq!(windows["needs"], json!(["checks"]));
    assert!(
        windows["if"]
            .as_str()
            .unwrap()
            .contains("mutation-windows != '[]'")
    );
    assert_eq!(windows["env"]["CARGO_MUTANTS_VERSION"], "27.1.0");
    let steps = windows["steps"].as_array().unwrap();
    let registry = steps
        .iter()
        .position(|step| step["id"] == "windows-registry")
        .unwrap();
    let toolchain = steps
        .iter()
        .position(|step| step["run"] == "rust-gate tools")
        .unwrap();
    let install = workflow_step(steps, "windows-mutation-tools");
    let tools_position = steps
        .iter()
        .position(|step| step["id"] == "windows-mutation-tools")
        .unwrap();
    assert!(registry < toolchain && toolchain < tools_position);
    let tools = install["env"]["TOOLS"].as_str().unwrap();
    assert_windows_tool_rows(tools);
    let linux_steps = ci["jobs"]["mutations"]["steps"].as_array().unwrap();
    let linux_tools = tool_rows(workflow_step(linux_steps, "mutation-tools"));
    assert!(linux_tools.iter().any(|row| {
        row.asset
            .contains("cargo-mutants-x86_64-unknown-linux-gnu.tar.gz")
            && row.digest == "dfe6dc37d0342c891d2829b5a695aa57c2d0edecef7e7d0399a30cc6e206411e"
    }));
    assert!(linux_tools.iter().any(|row| {
        row.asset
            .contains("cargo-mutants-x86_64-pc-windows-msvc.zip")
            && row.digest == "2a2f00e47d4b458262a41501b0820aa26015fd35779903d2c8b30b2993f36791"
    }));
    assert_eq!(
        workflow_step(steps, "windows-mutation-run")["run"],
        "rust-gate mutants-windows"
    );
    let upload = workflow_step(steps, "windows-mutation-upload");
    let upload_name = upload["with"]["name"].as_str().unwrap();
    assert!(upload_name.ends_with("-windows-mutants"), "{upload_name}");
    assert!(!upload_name.contains("-mutants-"), "{upload_name}");
    assert_windows_report_paths(upload["with"]["path"].as_str().unwrap());
    let required_steps = ci["jobs"]["gate"]["steps"].as_array().unwrap();
    let required = workflow_step(required_steps, "required");
    assert_eq!(
        required["env"]["WINDOWS_MUTATIONS_RESULT"],
        "${{ needs.mutation-windows.result }}"
    );
}

fn assert_windows_tool_rows(tools: &str) {
    assert!(tools.contains("cargo-mutants-x86_64-pc-windows-msvc.zip"));
    assert!(tools.contains(concat!(
        "cargo-nextest windows nextest-rs/nextest/releases/download/",
        "cargo-nextest-0.9.146/cargo-nextest-0.9.146-x86_64-pc-windows-msvc.zip ",
        "0fa689815c8157e4633225b6b173184b3d546eb6ffb754c3d0e6ea5973284a20 zip cargo-nextest.exe"
    )));
    assert!(tools.contains("2a2f00e47d4b458262a41501b0820aa26015fd35779903d2c8b30b2993f36791"));
}

fn assert_windows_report_paths(artifact: &str) {
    for path in [
        "mutants.out/",
        "rust-reports/mutants.json",
        "rust-reports/mutants.txt",
    ] {
        assert!(artifact.contains(path), "{artifact}");
    }
}

#[test]
fn linux_mutation_workers_exclude_windows_owned_files() {
    let ci = workflow("ci");
    let workers = ci["jobs"]["mutations"]["steps"].as_array().unwrap();
    let execute = workflow_step(workers, "mutation-worker-run");
    assert_eq!(
        execute["env"]["MUTATION_WINDOWS"],
        "${{ needs.checks.outputs.mutation-windows }}"
    );
    let inline = ci["jobs"]["checks"]["steps"].as_array().unwrap();
    let execute = workflow_step(inline, "mutants");
    assert_eq!(
        execute["env"]["MUTATION_WINDOWS"],
        "${{ steps.validate.outputs.mutation-windows }}"
    );
}

#[test]
fn shard_workflow_contract_names_matrix_and_summary_jobs() {
    let ci = workflow("ci");
    assert_required_gate_wiring(&ci);
    assert_matrix_and_summary_contract(&ci);
    assert_planner_and_consumer_contract(&ci);
}

#[test]
fn cargo_mutants_version_environment_matches_the_installed_pin() {
    let ci = workflow("ci");
    let checks = ci["jobs"]["checks"]["steps"].as_array().unwrap();
    let asset = checks
        .iter()
        .flat_map(tool_rows)
        .find(|row| row.name == "cargo-mutants")
        .unwrap()
        .asset;
    let version = asset
        .split("/download/v")
        .nth(1)
        .unwrap()
        .split('/')
        .next()
        .unwrap();
    let windows_tools = workflow_step(
        ci["jobs"]["mutation-windows"]["steps"].as_array().unwrap(),
        "windows-mutation-tools",
    )["env"]["TOOLS"]
        .as_str()
        .unwrap();
    let windows_mutants = windows_tools
        .lines()
        .find(|line| line.starts_with("cargo-mutants "))
        .unwrap();
    assert!(windows_mutants.contains(&format!("/download/v{version}/")));
    for name in [
        "checks",
        "mutations",
        "mutation-summary",
        "mutation-windows",
    ] {
        assert_eq!(
            ci["jobs"][name]["env"]["CARGO_MUTANTS_VERSION"], version,
            "{name} must use the installed cargo-mutants version"
        );
    }
}

#[test]
fn internal_shard_selftest_is_gated_and_runs_in_both_consumers() {
    let ci = workflow("ci");
    let input = &ci["on"]["workflow_call"]["inputs"]["internal-shard-selftest"];
    assert_eq!(input["type"], "boolean");
    assert_eq!(input["default"], false);
    assert!(
        input["description"]
            .as_str()
            .unwrap()
            .contains("Internal to this repository's own CI")
    );
    let caller = &workflow("ci-internal")["jobs"]["sharded-consumer"]["with"];
    assert_eq!(caller["working-directory"], "examples/workspace");
    assert_eq!(caller["mutation-shards"], 2);
    assert_eq!(caller["internal-shard-selftest"], true);
    let planning = workflow_step(
        ci["jobs"]["checks"]["steps"].as_array().unwrap(),
        "mutants-plan",
    );
    assert_eq!(
        planning["env"]["INTERNAL_SHARD_SELFTEST"],
        "${{ inputs.internal-shard-selftest || false }}"
    );
    let inline = workflow_step(ci["jobs"]["checks"]["steps"].as_array().unwrap(), "mutants");
    assert_eq!(
        inline["env"]["INTERNAL_SHARD_SELFTEST"],
        "${{ inputs.internal-shard-selftest || false }}"
    );
    let worker = workflow_step(
        ci["jobs"]["mutations"]["steps"].as_array().unwrap(),
        "mutation-worker-run",
    );
    assert_eq!(
        worker["env"]["INTERNAL_SHARD_SELFTEST"],
        "${{ inputs.internal-shard-selftest || false }}"
    );
    assert_eq!(
        ci["jobs"]["gate"]["steps"][2]["env"]["INTERNAL_SHARD_SELFTEST"],
        "${{ inputs.internal-shard-selftest || false }}"
    );
}

#[test]
fn worker_artifact_contract_matches_the_summary_and_gate_layout() {
    let ci = workflow("ci");
    let workers = ci["jobs"]["mutations"]["steps"].as_array().unwrap();
    let checks_download = workflow_step(workers, "mutation-plan-download");
    assert_eq!(
        checks_download["with"]["name"],
        "${{ needs.checks.outputs.artifact-name }}-checks-reports"
    );
    assert_eq!(
        checks_download["with"]["path"],
        "${{ runner.temp }}/mutation-plan"
    );
    let execute = workflow_step(workers, "mutation-worker-run");
    assert_eq!(
        execute["env"]["MUTATION_PLAN"],
        "${{ runner.temp }}/mutation-plan/mutation-plan.json"
    );
    assert_eq!(
        execute["env"]["MUTATION_LIST"],
        "${{ runner.temp }}/mutation-plan/mutants-list.json"
    );
    let upload = workflow_step(workers, "mutation-worker-upload");
    assert_eq!(
        upload["with"]["name"],
        concat!(
            "${{ needs.checks.outputs.artifact-name }}-mutants-",
            "${{ matrix.shard }}-of-${{ needs.checks.outputs.mutation-shards }}"
        )
    );
    let paths: Vec<_> = upload["with"]["path"]
        .as_str()
        .unwrap()
        .lines()
        .map(str::trim)
        .collect();
    assert_eq!(
        paths,
        [
            "${{ runner.temp }}/mutants/mutants.out/",
            "${{ runner.temp }}/rust-reports/mutants.json",
            "${{ runner.temp }}/rust-reports/mutants.txt",
            "${{ runner.temp }}/rust-reports/mutants-shard.json",
        ]
    );

    let summary = ci["jobs"]["mutation-summary"]["steps"].as_array().unwrap();
    let checks = workflow_step(summary, "mutation-summary-checks-download");
    assert_eq!(
        checks["with"]["name"],
        "${{ needs.checks.outputs.artifact-name }}-checks-reports"
    );
    assert_eq!(checks["with"]["path"], "${{ runner.temp }}/checks-reports");
    let shards = workflow_step(summary, "mutation-summary-shards-download");
    assert_eq!(
        shards["with"]["pattern"],
        "${{ needs.checks.outputs.artifact-name }}-mutants-*"
    );
    assert_eq!(shards["with"]["path"], "${{ runner.temp }}/mutation-shards");
    assert_eq!(shards["with"]["merge-multiple"], false);
    let aggregate = workflow_step(summary, "mutation-aggregate");
    assert_eq!(
        aggregate["env"]["MUTATION_PLAN_DIR"],
        "${{ runner.temp }}/checks-reports"
    );
    assert_eq!(
        aggregate["env"]["MUTATION_ARTIFACTS"],
        "${{ runner.temp }}/mutation-shards"
    );
}

#[test]
fn shard_jobs_build_the_gate_with_its_pinned_toolchain() {
    let ci = workflow("ci");
    for (name, build_id) in [
        ("mutations", "mutation-gate-build"),
        ("mutation-summary", "mutation-summary-gate-build"),
    ] {
        assert!(
            ci["jobs"][name]["env"].get("RUSTUP_TOOLCHAIN").is_none(),
            "{name} must not select the consumer compiler before building the gate"
        );
        let steps = ci["jobs"][name]["steps"].as_array().unwrap();
        let build = steps
            .iter()
            .position(|step| step["id"] == build_id)
            .unwrap();
        if name == "mutations" {
            let validate = steps
                .iter()
                .position(|step| step["id"] == "mutation-validate")
                .unwrap();
            assert!(build < validate);
        } else {
            let aggregate = steps
                .iter()
                .find(|step| step["id"] == "mutation-aggregate")
                .unwrap();
            assert_eq!(
                aggregate["env"]["RUSTUP_TOOLCHAIN"],
                "${{ needs.checks.outputs.toolchain }}"
            );
            let finalize = steps
                .iter()
                .find(|step| step["id"] == "scorecard-finalize")
                .unwrap();
            assert!(finalize["env"].get("RUSTUP_TOOLCHAIN").is_none());
        }
    }
    assert!(
        fs::read_to_string(root().join("gate/Cargo.toml"))
            .unwrap()
            .contains("rust-version = \"1.88\"")
    );
}

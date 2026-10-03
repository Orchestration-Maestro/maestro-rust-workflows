//! Native mutation transport routing, child-only injection and legacy compatibility.

use crate::harness::{Fixture, engine_fixture, output, succeeds, workflow};
use serde_json::{Value, json};
use std::fs;

/// Plan two engine obligations and observe each worker's actual child environment.
fn mutation_fixture(index: usize, policy: bool) -> Fixture {
    let mut fixture = engine_fixture(false);
    if policy {
        fs::write(
            fixture.root.join("project/maestro-quality.toml"),
            concat!(
                "[native-cache]\nenvironment='FIXTURE_NATIVE_CACHE_DIR'\n",
                "platforms=['linux','macos']\nkey-files=['Cargo.lock']\npublished=['entry-*']\n"
            ),
        )
        .unwrap();
    }
    for name in ["engine.json", "listing.json"] {
        let path = fixture.root.join(name);
        let mut listing: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let mut second = listing[0].clone();
        second["replacement"] = json!("second replacement");
        second["name"] = json!("src/engine.rs:2:1: second mutation");
        second["span"]["start"]["line"] = json!(2);
        second["span"]["end"]["line"] = json!(2);
        listing.as_array_mut().unwrap().push(second);
        fs::write(path, listing.to_string()).unwrap();
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
    fixture.set("MUTATION_SHARDS", "2");
    fixture.set("MUTATION_SHARD", &format!("{index}/2"));
    fixture.set("COVERAGE_FEATURES", "");
    fixture.set("NATIVE_CACHE_MODE", "mutation");
    fixture.set("NATIVE_CACHE_OS", "linux");
    fixture.set("NATIVE_CACHE_ARCH", "X64");
    fixture.stub(
        "cargo",
        r#"printf '%s %s\n' "$*" "${FIXTURE_NATIVE_CACHE_DIR:-unset}" >> "$REPORTS/child-env"
if [[ $1 == metadata ]]; then printf '%s' "$METADATA"; exit; fi
if [[ $1 == test ]]; then
 target=''
 while (( $# )); do
  case $1 in --target-dir) target=$2; shift 2;; *) shift;; esac
 done
 mkdir -p "$target/debug/deps"
 printf '%s: src/lib.rs src/engine.rs\n' "$target/debug/deps/fixture.d" \
  > "$target/debug/deps/fixture.d"
 printf '{"reason":"compiler-artifact","fresh":false,"target":{"kind":["lib"],'
 printf '"src_path":"%s/src/lib.rs"},"filenames":["%s/debug/deps/libfixture.rlib"]}\n' \
  "$PROJECT" "$target"
 printf '{"reason":"build-finished","success":true}\n'
 exit
fi
out=''; mode=default; shard=0; control=false
while (( $# )); do
 case $1 in --features) mode=engine; shift 2;; --output) out=$2; shift 2;;
 --shard) shard=${2%/*}; shift 2;; --file) control=true; shift 2;; *) shift;; esac
done
mkdir -p "$out/mutants.out"
if [[ $mode == engine ]]; then list=mutation-engine-list.json;
elif [[ $control == true ]]; then list=mutation-engine-default-list.json;
else list=mutants-list.json; fi
jaq --argjson index "$shard" '{outcomes:[{scenario:"Baseline",summary:"Success",
 phase_results:[{phase:"Test",duration:1.0,argv:["cargo","test"],process_status:"Success"}]},
 {scenario:{Mutant:.[$index]},summary:"CaughtMutant",phase_results:[{phase:"Test",duration:1.0,
 argv:["cargo","test"],process_status:{Failure:101}}]}],
 total_mutants:1,caught:1,missed:0,timeout:0,unviable:0,success:0,
 cargo_mutants_version:"27.1.0",start_time:"2026-01-01T00:00:00Z",
 end_time:"2026-01-01T00:01:00Z"}' \
 "$REPORTS/$list" > "$out/mutants.out/outcomes.json"
jaq --argjson index "$shard" '[.[$index]]' "$REPORTS/$list" > "$out/mutants.out/mutants.json"
if [[ $mode == engine && -d ${FIXTURE_NATIVE_CACHE_DIR:-unset} ]]; then
 mkdir -p "$FIXTURE_NATIVE_CACHE_DIR/entry-worker"; fi"#,
    );
    fixture
}

#[test]
fn every_engine_worker_receives_only_a_verified_private_child_variable() {
    for index in 0..2 {
        let mut fixture = mutation_fixture(index, true);
        fixture.set("FIXTURE_NATIVE_CACHE_DIR", "/inherited-untrusted");
        succeeds(&fixture.run_body("rust-gate native-cache-prepare"));
        assert_eq!(output(&fixture, "enabled"), "true");
        let root = output(&fixture, "root");
        fixture.set("NATIVE_CACHE_ROOT", &root);
        fs::write(fixture.root.join("reports/child-env"), "").unwrap();
        let trace_start = fixture.trace().len();
        succeeds(&fixture.run_body("rust-gate mutants-engine"));
        let trace = fixture.trace();
        assert_eq!(
            trace[trace_start..]
                .matches("if has(\"native-cache\") then")
                .count(),
            1,
            "worker must parse the native policy once: {}",
            &trace[trace_start..]
        );
        let children = fs::read_to_string(fixture.root.join("reports/child-env")).unwrap();
        let metadata: Vec<_> = children
            .lines()
            .filter(|child| child.starts_with("metadata "))
            .collect();
        assert_eq!(
            metadata,
            [
                "metadata --format-version 1 --no-deps /inherited-untrusted",
                "metadata --format-version 1 --no-deps --locked unset",
                "metadata --format-version 1 --no-deps unset",
            ]
        );
        let workers: Vec<_> = children
            .lines()
            .filter(|child| child.starts_with("mutants "))
            .collect();
        assert_eq!(workers.len(), 1, "{children}");
        assert!(workers[0].ends_with(&root), "{}", workers[0]);
        assert!(workers[0].contains("--features engine"), "{}", workers[0]);
        assert_eq!(
            fixture.env["FIXTURE_NATIVE_CACHE_DIR"],
            "/inherited-untrusted"
        );
        fixture.set("EVENT", "push");
        fixture.set("REF", "refs/heads/main");
        fixture.set("DEFAULT_BRANCH", "main");
        fixture.set("JOB_SUCCESS", "true");
        succeeds(&fixture.run_body("rust-gate native-cache-inventory"));
        assert_eq!(output(&fixture, "save"), "true");
    }
}

#[test]
fn featureless_workers_strip_inherited_cache_while_absent_policy_preserves_environments() {
    for (policy, step) in [
        (true, "mutants-engine-default"),
        (true, "mutants"),
        (false, "mutants-engine"),
        (false, "mutants-engine-default"),
        (false, "mutants"),
    ] {
        let mut fixture = mutation_fixture(0, policy);
        fixture.set("FIXTURE_NATIVE_CACHE_DIR", "/legacy-value");
        if step == "mutants" {
            fixture.set("MUTATION_SHARD", "");
        }
        succeeds(&fixture.run_body(&format!("rust-gate {step}")));
        let children = fs::read_to_string(fixture.root.join("reports/child-env")).unwrap();
        for child in children.lines() {
            let isolated = child.starts_with("mutants ")
                || child.starts_with("test ")
                || child.starts_with("metadata --format-version 1 --no-deps --locked ");
            let expected = if policy && isolated {
                " unset"
            } else {
                " /legacy-value"
            };
            assert!(child.ends_with(expected), "{child}");
            if step != "mutants-engine" {
                assert!(!child.contains("--features"), "{child}");
            }
        }
        assert!(!fixture.root.join("native-cache").exists());
        assert!(
            !fixture
                .root
                .join("reports/native-cache-before.txt")
                .exists()
        );
        if !policy {
            assert!(!fixture.trace().contains("env -u"));
            assert!(!fixture.trace().contains("FIXTURE_NATIVE_CACHE_DIR="));
        }
    }
}

#[test]
fn only_engine_jobs_restore_and_only_shard_zero_may_save() {
    let ci = workflow("ci");
    for name in ["mutations", "mutation-engine-default", "mutation-windows"] {
        let job = &ci["jobs"][name];
        assert!(!job.to_string().contains("native-cache"), "{name}");
        assert!(!job.to_string().contains("NATIVE_CACHE_ROOT"), "{name}");
    }
}

#[test]
fn engine_restore_and_save_steps_bind_only_the_selected_worker() {
    let ci = workflow("ci");
    let job = &ci["jobs"]["mutation-engine"];
    assert!(!job["env"].to_string().contains("NATIVE_CACHE_ROOT"));
    let steps = job["steps"].as_array().unwrap();
    let find = |id: &str| {
        let step = steps.iter().find(|step| step["id"] == id);
        assert!(step.is_some(), "engine job must declare {id}");
        step.unwrap()
    };
    let prepare = find("native-cache-prepare");
    assert_eq!(prepare["env"]["NATIVE_CACHE_MODE"], "mutation");
    assert_eq!(
        prepare["env"]["MUTATION_ENGINE_FEATURES"],
        "${{ needs.mutation-plan.outputs.mutation-engine-features }}"
    );
    let restore = find("native-cache-restore");
    assert_eq!(
        restore["if"],
        "${{ steps.native-cache-prepare.outputs.enabled == 'true' }}"
    );
    assert_eq!(
        restore["uses"],
        "actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9"
    );
    assert_eq!(restore["with"]["enableCrossOsArchive"], false);
    assert!(
        restore["with"]["restore-keys"]
            .as_str()
            .unwrap()
            .contains("-mutation-")
    );
    assert_eq!(
        find("engine-mutation-run")["env"]["NATIVE_CACHE_ROOT"],
        "${{ steps.native-cache-prepare.outputs.root }}"
    );
}

#[test]
fn engine_shard_zero_alone_can_inventory_and_save_after_success() {
    let ci = workflow("ci");
    let steps = ci["jobs"]["mutation-engine"]["steps"].as_array().unwrap();
    let find = |id: &str| {
        let step = steps.iter().find(|step| step["id"] == id);
        assert!(step.is_some(), "engine job must declare {id}");
        step.unwrap()
    };
    let inventory = find("native-cache-inventory");
    assert_eq!(
        inventory["if"],
        concat!(
            "${{ success() && matrix.shard == 0 && ",
            "steps.native-cache-prepare.outputs.enabled == 'true' }}"
        )
    );
    assert_eq!(inventory["env"]["EVENT"], "${{ github.event_name }}");
    assert_eq!(
        inventory["env"]["MERGE_GROUP_BASE_REF"],
        "${{ github.event.merge_group.base_ref }}"
    );
    let save = find("native-cache-save");
    assert_eq!(
        save["if"],
        concat!(
            "${{ success() && matrix.shard == 0 && ",
            "steps.native-cache-inventory.outputs.save == 'true' }}"
        )
    );
    assert_eq!(
        save["uses"],
        "actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9"
    );
    assert_eq!(steps.last().unwrap()["id"], "native-cache-save");
    let position = |id: &str| steps.iter().position(|step| step["id"] == id).unwrap();
    assert!(position("native-cache-restore") < position("engine-mutation-run"));
    assert!(position("engine-mutation-run") < position("native-cache-inventory"));
}

#[test]
fn windows_mutation_removes_inherited_policy_without_native_cache_operations() {
    let mut fixture = mutation_fixture(0, true);
    fixture.set("MUTATION_WINDOWS", "[\"src/engine.rs\"]");
    fixture.set("MUTATION_ENGINE_FILES", "[]");
    fixture.set("GITHUB_BASE_REF", "");
    fixture.set("FIXTURE_NATIVE_CACHE_DIR", "/inherited-untrusted");
    fixture.stub("git", "exit 1");
    let listing: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("engine.json")).unwrap()).unwrap();
    fs::write(
        fixture.root.join("windows-list.json"),
        json!([listing[0].clone()]).to_string(),
    )
    .unwrap();
    fixture.stub(
        "cargo",
        r#"printf '%s %s\n' "$*" "${FIXTURE_NATIVE_CACHE_DIR:-unset}" \
 >> "$REPORTS/windows-child-env"
if [[ $2 == --list ]]; then cat "$RUNNER_TEMP/windows-list.json"; exit; fi
out=''
while (( $# )); do
 if [[ $1 == --output ]]; then out=$2; shift 2; else shift; fi
done
mkdir -p "$out/mutants.out"
jaq '{outcomes:[{scenario:"Baseline",summary:"Success",
 phase_results:[{phase:"Test",duration:1.0,argv:["cargo","test"],process_status:"Success"}]},
 {scenario:{Mutant:.[0]},summary:"CaughtMutant",phase_results:[{phase:"Test",duration:1.0,
 argv:["cargo","test"],process_status:{Failure:101}}]}],
 total_mutants:1,caught:1,missed:0,timeout:0,unviable:0,success:0,
 cargo_mutants_version:"27.1.0",start_time:"2026-01-01T00:00:00Z",
 end_time:"2026-01-01T00:01:00Z"}' "$RUNNER_TEMP/windows-list.json" \
 > "$out/mutants.out/outcomes.json""#,
    );
    succeeds(&fixture.run_body("rust-gate mutants-windows"));
    let children = fs::read_to_string(fixture.root.join("reports/windows-child-env")).unwrap();
    let execution = children
        .lines()
        .find(|child| !child.contains("--list"))
        .unwrap();
    assert!(execution.ends_with(" unset"), "{execution}");
    assert!(!children.contains("--features"), "{children}");
    assert!(!fixture.root.join("native-cache").exists());
    assert!(
        !fixture
            .root
            .join("reports/native-cache-before.txt")
            .exists()
    );
}

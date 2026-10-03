//! Independent compiler record shape and failed-process stimuli.

use crate::harness::{
    engine_aggregation_fixture, engine_fallback_fixture, engine_fixture, evidence_hash, refused,
    succeeds, summarize_engine,
};
use serde_json::{Value, json};
use std::fs;

#[test]
fn retained_compiler_logs_require_unique_units_emission_and_complete_quoting() {
    let fixture = engine_aggregation_fixture();
    let root = fixture
        .root
        .join("engine-default/fixture-engine-default-mutants-0/mutants-engine-default/mutants.out");
    let log = root.join("builds/0/cargo-build.log");
    let manifest = root.join("compile-membership.json");
    let original = fs::read_to_string(&log).unwrap();
    let saved: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    fs::write(&log, format!("{original}changed")).unwrap();
    refused(
        &summarize_engine(&fixture),
        "featureless compile membership compiler log digest differs",
    );
    for (changed, message) in [
        (
            String::new(),
            "featureless compiler unit has no unique rustc invocation:",
        ),
        (
            original.repeat(2),
            "featureless compiler unit has no unique rustc invocation:",
        ),
        (
            format!(
                "{original}{}",
                original.replace("--crate-name fixture", "--crate-name foreign")
            ),
            "featureless compiler unit has no unique rustc invocation:",
        ),
        (
            original.replace("--crate-name fixture", "--crate-name foreign"),
            "featureless compiler unit has no unique rustc invocation:",
        ),
        (
            original.replace("--emit", "--test --emit"),
            "featureless compiler unit has no unique rustc invocation:",
        ),
        (
            original.replace("/src/lib.rs", "/src/foreign.rs"),
            "featureless compiler unit has no unique rustc invocation:",
        ),
        (
            original.replace("--emit=dep-info,link", "--emit=link"),
            "featureless rustc unit has no dep-info emission: fixture",
        ),
        (
            original.replace("--out-dir", "--unknown-dir"),
            "featureless rustc unit has no output directory: fixture",
        ),
        (
            original.replace("rustc", "rustc '"),
            "featureless rustc invocation has malformed quoting:",
        ),
        (
            original.replace('`', ""),
            "featureless compiler unit has no unique rustc invocation:",
        ),
        (
            original.trim_end().trim_end_matches('`').to_owned(),
            "featureless rustc invocation has malformed quoting:",
        ),
    ] {
        fs::write(&log, &changed).unwrap();
        let mut binding = saved.clone();
        binding["packages"][0]["log_sha256"] = json!(evidence_hash(&log));
        fs::write(&manifest, binding.to_string()).unwrap();
        refused(&summarize_engine(&fixture), message);
    }
    fs::write(&log, &original).unwrap();
    let cargo = root.join("builds/0/cargo-build.json");
    let cargo_original = fs::read_to_string(&cargo).unwrap();
    let changed = cargo_original.replace("\"name\":\"fixture\"", "\"name\":\"foreign\"");
    assert_ne!(changed, cargo_original);
    fs::write(&cargo, changed).unwrap();
    let mut binding = saved.clone();
    binding["packages"][0]["cargo_sha256"] = json!(evidence_hash(&cargo));
    fs::write(&manifest, binding.to_string()).unwrap();
    refused(
        &summarize_engine(&fixture),
        "featureless compiler unit has no unique rustc invocation:",
    );
    fs::write(&cargo, &cargo_original).unwrap();
    let mut binding = saved.clone();
    binding["packages"][0]
        .as_object_mut()
        .unwrap()
        .remove("log_sha256");
    fs::write(&manifest, binding.to_string()).unwrap();
    refused(
        &summarize_engine(&fixture),
        "featureless compile membership binding differs from its plan",
    );
    fs::write(&manifest, saved.to_string()).unwrap();
    succeeds(&summarize_engine(&fixture));
}

#[test]
fn unterminated_double_quoted_compiler_arguments_are_refused() {
    let fixture = engine_aggregation_fixture();
    let root = fixture
        .root
        .join("engine-default/fixture-engine-default-mutants-0/mutants-engine-default/mutants.out");
    let log = root.join("builds/0/cargo-build.log");
    let original = fs::read_to_string(&log).unwrap();
    let changed = original.replace("`\n", " \"`\n");
    assert_ne!(changed, original);
    fs::write(&log, changed).unwrap();
    let manifest = root.join("compile-membership.json");
    let mut binding: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    binding["packages"][0]["log_sha256"] = json!(evidence_hash(&log));
    fs::write(manifest, binding.to_string()).unwrap();
    refused(
        &summarize_engine(&fixture),
        "featureless rustc invocation has malformed quoting:",
    );
}

#[test]
fn cargo_test_profiles_require_harness_or_explicit_test_configuration() {
    let fixture = engine_aggregation_fixture();
    let root = fixture
        .root
        .join("engine-default/fixture-engine-default-mutants-0/mutants-engine-default/mutants.out");
    let manifest = root.join("compile-membership.json");
    let saved: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    let cargo = root.join("builds/0/cargo-build.json");
    let rows: Vec<Value> = fs::read_to_string(&cargo)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let log = root.join("builds/0/cargo-build.log");
    let original = fs::read_to_string(&log).unwrap();
    for (test, flags, accepted) in [
        (true, "--cfg test", true),
        (true, "--cfg 'feature=\"engine\"' --cfg=test", true),
        (true, "--test", true),
        (false, "--cfg test", true),
        (true, "", false),
        (true, "--cfg 'feature=\"test\"'", false),
        (false, "--test", false),
    ] {
        let mut changed = rows.clone();
        changed[0]["profile"]["test"] = json!(test);
        fs::write(
            &cargo,
            changed
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        fs::write(&log, original.replace("--emit", &format!("{flags} --emit"))).unwrap();
        let mut binding = saved.clone();
        binding["packages"][0]["cargo_sha256"] = json!(evidence_hash(&cargo));
        binding["packages"][0]["log_sha256"] = json!(evidence_hash(&log));
        fs::write(&manifest, binding.to_string()).unwrap();
        if accepted {
            succeeds(&summarize_engine(&fixture));
        } else {
            refused(
                &summarize_engine(&fixture),
                "featureless compiler unit has no unique rustc invocation:",
            );
        }
    }
}

#[test]
fn special_output_matches_require_exact_target_kind_profile_and_directory() {
    let fixture = engine_aggregation_fixture();
    let root = fixture
        .root
        .join("engine-default/fixture-engine-default-mutants-0/mutants-engine-default/mutants.out");
    let manifest = root.join("compile-membership.json");
    let saved: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    let cargo = root.join("builds/0/cargo-build.json");
    let rows: Vec<Value> = fs::read_to_string(&cargo)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let log = root.join("builds/0/cargo-build.log");
    let original = fs::read_to_string(&log).unwrap();
    let directory = format!(
        "{}/debug/deps",
        saved["packages"][0]["target"].as_str().unwrap()
    );
    for (kind, test, output_directory, filename) in [
        (
            "bin",
            false,
            directory.clone(),
            format!("{directory}/build-script-build"),
        ),
        (
            "lib",
            false,
            directory.clone(),
            directory.replace("/deps", "/fixture"),
        ),
        (
            "bin",
            true,
            directory.clone(),
            directory.replace("/deps", "/fixture"),
        ),
        (
            "bin",
            false,
            directory.replace("/deps", "/units"),
            directory.replace("/deps", "/units/fixture"),
        ),
    ] {
        let mut changed = rows.clone();
        changed[0]["target"]["kind"] = json!([kind]);
        changed[0]["profile"]["test"] = json!(test);
        changed[0]["filenames"] = json!([filename]);
        fs::write(
            &cargo,
            changed
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        fs::write(
            &log,
            original.replace(&directory, &output_directory).replace(
                "--emit",
                if test {
                    "--cfg test -C extra-filename=-hash --emit"
                } else {
                    "-C extra-filename=-hash --emit"
                },
            ),
        )
        .unwrap();
        let mut binding = saved.clone();
        binding["packages"][0]["cargo_sha256"] = json!(evidence_hash(&cargo));
        binding["packages"][0]["log_sha256"] = json!(evidence_hash(&log));
        fs::write(&manifest, binding.to_string()).unwrap();
        refused(
            &summarize_engine(&fixture),
            "featureless compiler unit has no unique rustc invocation:",
        );
    }
}

#[test]
fn fallback_packages_refuse_unexpected_compiler_log_bindings() {
    let fixture = engine_fallback_fixture(true);
    let manifest = fixture.root.join(concat!(
        "engine-default/fixture-engine-default-mutants-0/",
        "mutants-engine-default/mutants.out/compile-membership.json",
    ));
    let mut binding: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    binding["packages"][0]["log_sha256"] = json!("unexpected");
    fs::write(&manifest, binding.to_string()).unwrap();
    refused(
        &summarize_engine(&fixture),
        "featureless fallback contains unexpected compile evidence",
    );
}

#[test]
fn missing_emitted_dep_info_fails_closed_with_its_unit_named() {
    let fixture = engine_fixture(true);
    let cargo = fixture.root.join("bin/cargo");
    let original = fs::read_to_string(&cargo).unwrap();
    fs::write(
        &cargo,
        original.replace(
            "exit \"${CONTROL_BUILD_STATUS:-0}\"",
            "rm \"$target/debug/deps/fixture.d\"; exit \"${CONTROL_BUILD_STATUS:-0}\"",
        ),
    )
    .unwrap();
    refused(
        &fixture.run_body("rust-gate mutants-engine-default"),
        &format!(
            concat!(
                "cannot retain featureless dep-info for {}/",
                "engine-control-target/0/debug/deps/fixture.d:",
            ),
            fixture.root.display()
        ),
    );
}

#[test]
fn unverified_control_survivors_fail_without_their_exact_engine_twins() {
    let fixture = engine_fallback_fixture(false);
    refused(
        &summarize_engine(&fixture),
        "featureless control survivor is not caught by its exact engine twin",
    );
}

#[test]
fn aggregation_rejects_verified_compiled_survivors_despite_a_caught_engine_twin() {
    let fixture = engine_aggregation_fixture();
    let root = fixture
        .root
        .join("engine-default/fixture-engine-default-mutants-0")
        .join("mutants-engine-default/mutants.out");
    for name in ["outcomes.json", "tested-outcomes.json"] {
        let path = root.join(name);
        let mut outcomes: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        outcomes["caught"] = json!(0);
        outcomes["missed"] = json!(1);
        outcomes["outcomes"][1]["summary"] = json!("MissedMutant");
        outcomes["outcomes"][1]["phase_results"][0]["process_status"] = json!("Success");
        fs::write(path, outcomes.to_string()).unwrap();
    }
    refused(
        &summarize_engine(&fixture),
        "partition contains a survivor, timeout or untested mutant",
    );
}

#[test]
fn failed_compiler_status_cannot_supply_successful_membership_records() {
    let mut fixture = engine_fixture(true);
    fixture.set("CONTROL_BUILD_STATUS", "7");
    let result = fixture.run_body("rust-gate mutants-engine-default");
    assert_eq!(result.status.code(), Some(7));
}

#[test]
fn failed_mutants_status_cannot_be_excused_by_complete_looking_outcomes() {
    let mut fixture = engine_fixture(true);
    fixture.set("CONTROL_MUTANT_STATUS", "3");
    let result = fixture.run_body("rust-gate mutants-engine-default");
    assert_eq!(result.status.code(), Some(3));
}

#[test]
fn malformed_compiler_records_are_refused_with_refreshed_digests() {
    let fixture = engine_aggregation_fixture();
    let root = fixture
        .root
        .join("engine-default/fixture-engine-default-mutants-0")
        .join("mutants-engine-default/mutants.out");
    let manifest = root.join("compile-membership.json");
    let binding: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    let cargo = root.join("builds/0/cargo-build.json");
    let rows: Vec<Value> = fs::read_to_string(&cargo)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let mut probes = vec![
        vec![rows[0].clone()],
        vec![rows[0].clone(), rows[1].clone(), rows[1].clone()],
        vec![rows[1].clone()],
    ];
    for (pointer, value) in [
        ("/target/src_path", json!("relative")),
        ("/target/src_path", json!(1)),
        ("/target/name", json!(1)),
        ("/profile/test", json!("invalid")),
        ("/filenames", json!([])),
        ("/filenames", json!("invalid")),
        ("/filenames", json!([1])),
    ] {
        let mut changed = rows.clone();
        *changed[0].pointer_mut(pointer).unwrap() = value;
        probes.push(changed);
    }
    for probe in probes {
        fs::write(
            &cargo,
            probe
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let mut changed = binding.clone();
        changed["packages"][0]["cargo_sha256"] = json!(evidence_hash(&cargo));
        fs::write(&manifest, changed.to_string()).unwrap();
        refused(
            &summarize_engine(&fixture),
            "featureless build did not verify fresh compile membership",
        );
    }
}

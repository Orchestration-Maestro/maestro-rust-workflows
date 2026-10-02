//! Real engine execution evidence from the production planner and worker commands.

use serde_json::Value;
use std::collections::BTreeSet;
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

/// Require the existing full-workspace path and two genuinely nonempty engine shards.
pub(crate) fn assert_engine_plan() {
    let reports = PathBuf::from(env::var("REPORTS").unwrap());
    let plan: Value =
        serde_json::from_slice(&fs::read(reports.join("mutation-engine-plan.json")).unwrap())
            .unwrap();
    assert!(env::var("GITHUB_BASE_REF").unwrap_or_default().is_empty());
    assert!(plan["first_parent"].is_null(), "{plan}");
    assert_eq!(plan["sha"], env::var("GITHUB_SHA").unwrap());
    assert_eq!(plan["engine_shards"], 2);
    assert!(plan["engine_count"].as_u64().unwrap() >= 2);
    assert_eq!(plan["features"], serde_json::json!(["engine"]));
    let mut outputs = OpenOptions::new()
        .append(true)
        .open(env::var("GITHUB_OUTPUT").unwrap())
        .unwrap();
    for name in ["features", "files"] {
        writeln!(outputs, "mutation-engine-{name}={}", plan[name]).unwrap();
    }
    println!("Full-workspace engine plan: {plan}");
}

/// Prove child-only injection and real fresh-target native reuse for each engine shard.
pub(crate) fn verify_mutation_fixture() {
    let reports = PathBuf::from(env::var("REPORTS").unwrap());
    let trace = fs::read_to_string(reports.join("trace")).unwrap();
    let injected: Vec<_> = trace
        .lines()
        .filter(|line| line.contains("FIXTURE_NATIVE_CACHE_DIR="))
        .collect();
    assert_eq!(injected.len(), 1, "{trace}");
    let child = injected[0];
    assert!(child.contains("cargo mutants"), "{child}");
    assert!(child.contains("--features engine"), "{child}");
    assert_eq!(env::var("NATIVE_PREPARED").unwrap(), "true");
    assert!(env::var_os("FIXTURE_NATIVE_CACHE_DIR").is_none());
    let builds = fs::read_to_string(reports.join("build-count"))
        .unwrap_or_default()
        .lines()
        .count();
    let matched = env::var("NATIVE_RESTORE_MATCHED_KEY").unwrap_or_default();
    let before = fs::read_to_string(reports.join("native-cache-before.txt")).unwrap();
    assert!(!before.lines().next().unwrap().is_empty());
    let requests = fs::read_to_string(reports.join("native-entries")).unwrap();
    let entries: BTreeSet<_> = requests.lines().collect();
    assert_eq!(
        entries.len(),
        1,
        "one native input key per fixture shard: {requests}"
    );
    let compatible = entries
        .iter()
        .all(|key| before.lines().skip(1).any(|name| name == *key));
    assert_eq!(builds, usize::from(!compatible));
    let binding = fs::read_to_string(reports.join("native-cache-binding.txt")).unwrap();
    let proof = format!(
        "{binding}Native source builds: {builds}\nVariable injections: {}\n\
         Restore matched key: {matched}\nScope: {}\nChild environment: {child}\n\
         Native input entries: {requests}",
        injected.len(),
        env::var("GITHUB_REF").unwrap(),
    );
    println!("{proof}");
    fs::write(reports.join("mutation-fixture-proof.txt"), proof).unwrap();
}

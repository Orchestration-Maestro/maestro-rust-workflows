//! Hosted replay of exact stage-one diffs against gate units and the contract harness.

use crate::harness::{fixture_git as git, root, temp_dir};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io::Write as _;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Read one required workflow binding.
fn binding(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("missing replay binding: {name}"))
}

/// Read a JSON evidence file without accepting a missing or partial receipt.
fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

/// Write a complete receipt, replacing the previous progress snapshot.
fn write_json(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

/// A source file stays strictly inside the gate's tracked source tree.
fn source_file(mutant: &Value) -> String {
    let file = mutant["file"].as_str().unwrap();
    assert!(file.starts_with("src/") && Path::new(file).extension().is_some_and(|ext| ext == "rs"));
    assert!(
        Path::new(file)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    );
    format!("gate/{file}")
}

/// Cargo-mutants labels the new side with prose; Git needs two source paths.
/// Only those labels change. Every recorded hunk remains byte-for-byte intact.
fn apply_diff(root: &Path, mutant: &Value, diff: &str) {
    let file = source_file(mutant);
    let mut lines = diff.splitn(3, '\n');
    assert_eq!(
        lines.next().unwrap(),
        format!("--- {}", mutant["file"].as_str().unwrap())
    );
    assert!(lines.next().unwrap().starts_with("+++ "));
    let patch = format!("--- a/{file}\n+++ b/{file}\n{}", lines.next().unwrap());
    let mut child = Command::new("git")
        .args(["apply", "--whitespace=nowarn", "-"])
        .current_dir(root)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(patch.as_bytes())
        .unwrap();
    assert!(
        child.wait().unwrap().success(),
        "recorded diff must apply exactly"
    );
}

/// Restore on ordinary returns and unwinding, with a fresh modification time.
struct Restore<'a> {
    /// The worker's isolated repository.
    root: &'a Path,
    /// The single tracked source file the recorded diff changes.
    file: String,
}

impl Drop for Restore<'_> {
    fn drop(&mut self) {
        git(self.root, &["checkout", "--", &self.file]);
    }
}

/// Select misses and timeouts only after complete, unique stage-one accounting.
fn selection(report: &Path) -> Vec<Value> {
    let manifest = read_json(&report.join("mutation-plan.json"));
    let listing = read_json(&report.join("mutants-list.json"));
    let outcomes = read_json(&report.join("mutants.json"));
    let expected: BTreeSet<_> = listing
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["name"].as_str().unwrap())
        .collect();
    assert_eq!(expected.len(), listing.as_array().unwrap().len());
    assert_eq!(
        expected.len() as u64,
        manifest["mutant_count"].as_u64().unwrap()
    );
    let mut actual = BTreeSet::new();
    let mut selected = Vec::new();
    for outcome in outcomes["outcomes"].as_array().unwrap() {
        let Some(mutant) = outcome["scenario"].get("Mutant") else {
            assert_eq!(
                outcome["summary"], "Success",
                "stage-one baseline must pass"
            );
            continue;
        };
        assert!(actual.insert(mutant["name"].as_str().unwrap()));
        let summary = outcome["summary"].as_str().unwrap();
        assert!(matches!(
            summary,
            "CaughtMutant" | "MissedMutant" | "Unviable" | "Timeout"
        ));
        if matches!(summary, "MissedMutant" | "Timeout") {
            let relative = Path::new(outcome["diff_path"].as_str().unwrap());
            assert!(
                relative
                    .components()
                    .all(|part| matches!(part, Component::Normal(_)))
            );
            let diff = fs::read_to_string(report.join(relative)).unwrap();
            selected.push(json!({"mutant": mutant, "stage1": summary, "diff": diff}));
        }
    }
    assert_eq!(expected, actual, "stage-one planned must equal tested");
    selected.sort_by_key(|item| {
        (
            item["stage1"] != "Timeout",
            item["mutant"]["name"].as_str().unwrap().to_owned(),
        )
    });
    selected
}

/// Plan deterministic round-robin ownership, recording source revision and run.
fn plan(root: &Path, output: &Path) {
    let report = PathBuf::from(binding("REPLAY_STAGE1"));
    let stage1 = read_json(&report.join("mutation-plan.json"));
    let sha = git(root, &["rev-parse", "HEAD"]).trim().to_owned();
    assert_eq!(stage1["sha"], sha, "stage-one source revision differs");
    let mutants = selection(&report);
    let target: usize = binding("REPLAY_PER_SHARD").parse().unwrap();
    assert!((1..=6).contains(&target));
    let timeouts = mutants
        .iter()
        .filter(|item| item["stage1"] == "Timeout")
        .count();
    let shards = mutants.len().div_ceil(target).max(1).max(timeouts);
    assert!(shards <= 256, "replay matrix exceeds GitHub's limit");
    let plan = json!({"sha": sha, "stage1": stage1, "shards": shards, "mutants": mutants});
    write_json(&output.join("replay-plan.json"), &plan);
    let matrix: Vec<_> = (0..shards).collect();
    writeln!(
        fs::OpenOptions::new()
            .append(true)
            .open(binding("GITHUB_OUTPUT"))
            .unwrap(),
        "shards={shards}\nmatrix={}",
        serde_json::to_string(&matrix).unwrap()
    )
    .unwrap();
    println!(
        "stage 2 planned={} shards={shards}",
        plan["mutants"].as_array().unwrap().len()
    );
}

/// Run one Cargo phase under a process-group timeout, preserving its full log.
fn cargo_phase(root: &Path, arguments: &[&str], seconds: u64, log: &Path, limit: bool) -> i32 {
    let file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .unwrap();
    writeln!(&file, "cargo {} (timeout {seconds}s)", arguments.join(" ")).unwrap();
    let mut command = Command::new("timeout");
    if limit {
        command = Command::new("bash");
        command
            .args(["-c", "ulimit -d \"$1\"; shift; exec \"$@\"", "replay-cap"])
            .arg(binding("REPLAY_TEST_MEMORY_KIB"))
            .arg("timeout");
    }
    command
        .args(["--kill-after=10s", &format!("{seconds}s"), "cargo"])
        .args(arguments)
        .current_dir(root)
        .stdout(file.try_clone().unwrap())
        .stderr(file)
        .status()
        .unwrap()
        .code()
        .unwrap_or(137)
}

/// Prebuild actual gate profiles and workflow contracts without a memory cap.
const BUILDS: [&str; 3] = [
    "test --manifest-path gate/Cargo.toml --locked --offline --no-run",
    "build --manifest-path gate/Cargo.toml --release --locked --offline",
    "test --manifest-path tests/Cargo.toml --locked --test workflows --no-run",
];

/// Normal units/contracts and only ignored examples, never a recursive replay.
const SUITES: [&str; 3] = [
    "test --manifest-path gate/Cargo.toml --locked --offline",
    "test --manifest-path tests/Cargo.toml --locked --test workflows \
     -- --skip replay_stage_one_misses_exactly",
    "test --manifest-path tests/Cargo.toml --locked --test workflows \
     -- --ignored example_gate",
];

/// Distinguish build refusal, failed tests and a process-group timeout.
fn disposition(root: &Path, seconds: u64, log: &Path) -> (String, String, Duration) {
    let started = Instant::now();
    let mut test_duration = Duration::ZERO;
    for (failure, commands) in [("Unviable", BUILDS), ("CaughtMutant", SUITES)] {
        let phase_started = Instant::now();
        for line in commands {
            let arguments: Vec<_> = line.split_whitespace().collect();
            let remaining = seconds.saturating_sub(started.elapsed().as_secs());
            if remaining == 0 {
                return ("Timeout".to_owned(), arguments.join(" "), Duration::ZERO);
            }
            let status = cargo_phase(root, &arguments, remaining, log, failure == "CaughtMutant");
            if status == 124 || status == 137 {
                return ("Timeout".to_owned(), arguments.join(" "), Duration::ZERO);
            }
            if status != 0 {
                let result = match memory_evidence(log) {
                    Some(_) if failure == "CaughtMutant" => "CaughtByMemoryCap",
                    _ => failure,
                };
                return (result.to_owned(), arguments.join(" "), Duration::ZERO);
            }
        }
        if failure == "CaughtMutant" {
            test_duration = phase_started.elapsed();
        }
    }
    (
        "MissedMutant".to_owned(),
        "all suites passed".to_owned(),
        test_duration,
    )
}

/// Preserve allocation-abort evidence separately from assertions or build refusals.
fn memory_evidence(log: &Path) -> Option<String> {
    fs::read_to_string(log)
        .unwrap()
        .lines()
        .find(|line| line.contains("memory allocation of ") && line.contains(" failed"))
        .map(str::to_owned)
}

/// Assigned identities, retained even when a shard cap leaves evidence incomplete.
fn assigned(plan: &Value, shard: usize) -> Vec<Value> {
    let shards = usize::try_from(plan["shards"].as_u64().unwrap()).unwrap();
    assert!(shard < shards);
    plan["mutants"]
        .as_array()
        .unwrap()
        .iter()
        .skip(shard)
        .step_by(shards)
        .cloned()
        .collect()
}

/// Replay once per owned mutant, restoring before recording its disposition.
fn replay(root: &Path, output: &Path) {
    let plan = read_json(&PathBuf::from(binding("REPLAY_PLAN")));
    let cap_kib: u64 = binding("REPLAY_TEST_MEMORY_KIB").parse().unwrap();
    let memory = fs::read_to_string("/proc/meminfo").unwrap();
    let total_kib: u64 = memory
        .lines()
        .find(|line| line.starts_with("MemTotal:"))
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(binding("RUST_TEST_THREADS"), "1");
    // One harness plus its sequential tested-gate child: two capped test processes.
    assert!(cap_kib > 0 && cap_kib * 2 * 4 <= total_kib * 3);
    println!("test data cap={cap_kib} KiB, test processes<=2, MemTotal={total_kib} KiB");
    assert_eq!(plan["sha"], git(root, &["rev-parse", "HEAD"]).trim());
    assert!(
        git(root, &["diff", "--name-only", "HEAD"])
            .trim()
            .is_empty()
    );
    let shard: usize = binding("REPLAY_SHARD").parse().unwrap();
    let owned = assigned(&plan, shard);
    // Avoid a full optimizer rebuild per mutant; release assertions still stay disabled.
    let baseline = disposition(root, 600, &output.join("baseline.log"));
    assert_eq!(
        baseline.0, "MissedMutant",
        "clean full-suite baseline must pass: {baseline:?}"
    );
    assert!(
        baseline.2 <= Duration::from_secs(120),
        "clean baseline test phase exceeds 120s; ordinary-miss budget is unsafe"
    );
    let setup_started: u64 = binding("REPLAY_SETUP_STARTED_AT").parse().unwrap();
    let setup_baseline = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .saturating_sub(setup_started);
    assert!(
        setup_baseline <= 300,
        "setup plus clean baseline exceeds five-minute budget"
    );
    let cap = Duration::from_secs(binding("REPLAY_SHARD_SECONDS").parse().unwrap());
    let started = Instant::now();
    let mut receipt = json!({"sha": plan["sha"], "shard": shard, "planned": owned,
        "baseline": "Success", "test_memory_kib": cap_kib, "max_test_processes": 2,
        "runner_mem_total_kib": total_kib, "setup_baseline_seconds": setup_baseline,
        "baseline_test_seconds": baseline.2.as_secs_f64(), "outcomes": []});
    let receipt_path = output.join(format!("replay-shard-{shard}.json"));
    write_json(&receipt_path, &receipt);
    for (index, item) in owned.iter().enumerate() {
        if started.elapsed() >= cap {
            break;
        }
        let seconds: u64 = binding(if item["stage1"] == "Timeout" {
            "REPLAY_TIMEOUT_TIMEOUT"
        } else {
            "REPLAY_MISSED_TIMEOUT"
        })
        .parse()
        .unwrap();
        assert!(seconds > 0);
        let guard = Restore {
            root,
            file: source_file(&item["mutant"]),
        };
        apply_diff(root, &item["mutant"], item["diff"].as_str().unwrap());
        let (summary, phase, _) =
            disposition(root, seconds, &output.join(format!("mutant-{index}.log")));
        drop(guard);
        assert!(
            git(root, &["diff", "--name-only", "HEAD"])
                .trim()
                .is_empty()
        );
        println!(
            "{}/{} {summary}: {}",
            index + 1,
            owned.len(),
            item["mutant"]["name"]
        );
        receipt["outcomes"].as_array_mut().unwrap().push(json!({
            "mutant": item["mutant"], "stage1": item["stage1"],
            "summary": summary, "phase": phase,
            "log": format!("mutant-{index}.log"),
            "memory_cap_evidence": memory_evidence(&output.join(format!("mutant-{index}.log")))
        }));
        write_json(&receipt_path, &receipt);
    }
    assert_eq!(
        receipt["outcomes"].as_array().unwrap().len(),
        owned.len(),
        "shard cap left untested mutants"
    );
}

/// Count results separately for stage-one misses and the one-time timeout replay.
fn counts(outcomes: &[Value], stage1: &str) -> Value {
    let owned: Vec<_> = outcomes
        .iter()
        .filter(|item| item["stage1"] == stage1)
        .collect();
    let count = |summary| {
        owned
            .iter()
            .filter(|item| item["summary"] == summary)
            .count()
    };
    json!({"tested": owned.len(), "caught": count("CaughtMutant"),
        "missed": count("MissedMutant"), "unviable": count("Unviable"),
        "timeout": count("Timeout"), "caught_by_memory_cap": count("CaughtByMemoryCap")})
}

/// Refuse missing/duplicate/wrong-owner evidence before publishing any final list.
fn aggregate(output: &Path) {
    let plan = read_json(&PathBuf::from(binding("REPLAY_PLAN")));
    let artifacts = PathBuf::from(binding("REPLAY_ARTIFACTS"));
    let shards = usize::try_from(plan["shards"].as_u64().unwrap()).unwrap();
    let mut outcomes = Vec::new();
    for shard in 0..shards {
        let receipt =
            read_json(&artifacts.join(format!("gate-replay-{shard}/replay-shard-{shard}.json")));
        assert_eq!(receipt["sha"], plan["sha"]);
        assert_eq!(receipt["shard"], shard);
        assert_eq!(receipt["baseline"], "Success");
        let owned = assigned(&plan, shard);
        assert_eq!(receipt["planned"], json!(owned));
        let tested = receipt["outcomes"].as_array().unwrap();
        assert_eq!(owned.len(), tested.len(), "planned must equal tested");
        for (expected, actual) in owned.iter().zip(tested) {
            assert_eq!(expected["mutant"], actual["mutant"]);
            assert_eq!(expected["stage1"], actual["stage1"]);
            assert!(matches!(
                actual["summary"].as_str().unwrap(),
                "CaughtMutant" | "MissedMutant" | "Unviable" | "Timeout" | "CaughtByMemoryCap"
            ));
        }
        assert!(
            tested
                .iter()
                .all(|item| item["summary"] != "CaughtByMemoryCap"
                    || item["memory_cap_evidence"].as_str().is_some())
        );
        outcomes.extend_from_slice(tested);
    }
    let report = json!({"sha": plan["sha"], "planned": plan["mutants"].as_array().unwrap().len(),
        "tested": outcomes.len(), "stage1_misses": counts(&outcomes, "MissedMutant"),
        "stage1_timeouts": counts(&outcomes, "Timeout"), "outcomes": outcomes});
    write_json(&output.join("replay-report.json"), &report);
    println!(
        "stage 2 planned={} tested={} misses={} timeouts={}",
        report["planned"], report["tested"], report["stage1_misses"], report["stage1_timeouts"]
    );
    assert!(
        outcomes
            .iter()
            .all(|item| item["summary"] != "MissedMutant" && item["summary"] != "Timeout"),
        "stage 2 has survivors or timeouts; complete report retained"
    );
}

#[test]
#[ignore = "hosted mutation replay only; never run against a developer's checkout"]
fn replay_stage_one_misses_exactly() {
    let root = root();
    let output = PathBuf::from(binding("REPLAY_OUTPUT"));
    fs::create_dir_all(&output).unwrap();
    match binding("REPLAY_MODE").as_str() {
        "plan" => plan(&root, &output),
        "replay" => replay(&root, &output),
        "aggregate" => aggregate(&output),
        mode => panic!("unknown replay mode: {mode}"),
    }
}

#[test]
fn recorded_diff_and_restore_round_trip() {
    let root = temp_dir("gate-replay-roundtrip");
    fs::create_dir_all(root.join("gate/src")).unwrap();
    git(&root, &["init", "--quiet"]);
    fs::write(
        root.join("gate/src/value.rs"),
        "fn value() -> bool { true }\n",
    )
    .unwrap();
    git(&root, &["add", "."]);
    git(
        &root,
        &[
            "-c",
            "user.name=Replay fixture",
            "-c",
            "user.email=replay@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ],
    );
    let mutant = json!({"file": "src/value.rs"});
    let guard = Restore {
        root: &root,
        file: source_file(&mutant),
    };
    apply_diff(
        &root,
        &mutant,
        concat!(
            "--- src/value.rs\n+++ replace value with false\n@@ -1 +1 @@\n",
            "-fn value() -> bool { true }\n+fn value() -> bool { false }\n"
        ),
    );
    assert_eq!(
        fs::read_to_string(root.join("gate/src/value.rs")).unwrap(),
        "fn value() -> bool { false }\n"
    );
    drop(guard);
    assert_eq!(
        fs::read_to_string(root.join("gate/src/value.rs")).unwrap(),
        "fn value() -> bool { true }\n"
    );
    assert!(
        git(&root, &["diff", "--name-only", "HEAD"])
            .trim()
            .is_empty()
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn allocation_aborts_keep_a_separate_count() {
    let directory = temp_dir("replay-allocation");
    let log = directory.join("abort.log");
    let evidence = "memory allocation of 2147483648 bytes failed";
    fs::write(&log, evidence).unwrap();
    assert_eq!(memory_evidence(&log).as_deref(), Some(evidence));
    let outcomes = [json!({"stage1": "Timeout", "summary": "CaughtByMemoryCap"})];
    let report = counts(&outcomes, "Timeout");
    assert_eq!(report["caught_by_memory_cap"], 1);
    assert_eq!(report["caught"], 0);
    assert_eq!(report["tested"], 1);
    fs::remove_dir_all(directory).unwrap();
}

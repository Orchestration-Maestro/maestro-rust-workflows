//! Generated documents: every table between generated markers, and the
//! diagram's control count, is what `just docs` writes from its source, so a
//! reader never has to keep one in step with the other by hand.

use crate::harness::{
    engine_workspace, query, root, succeeds, temp_dir, tool, workflow, write_executable,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::process::Output;

/// Every file `just docs` rewrites, besides the steps document.
fn generated(dir: &Path) -> Vec<PathBuf> {
    let mut files = vec![
        PathBuf::from("README.md"),
        PathBuf::from(".github/assets/how-it-works.svg"),
    ];
    for entry in fs::read_dir(dir.join("docs")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|kind| kind == "md") {
            files.push(path.strip_prefix(dir).unwrap().to_path_buf());
        }
    }
    files
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let path = entry.unwrap().path();
        let target = to.join(path.file_name().unwrap());
        if path.is_dir() {
            copy_tree(&path, &target);
        } else {
            fs::copy(&path, &target).unwrap();
        }
    }
}

/// A copy of the documents and of every source they are generated from.
fn sandbox() -> PathBuf {
    let dir = temp_dir("generated-documents");
    for file in ["justfile", "README.md", "mise.toml"] {
        fs::copy(root().join(file), dir.join(file)).unwrap();
    }
    copy_tree(&root().join("docs"), &dir.join("docs"));
    copy_tree(
        &root().join(".github/workflows"),
        &dir.join(".github/workflows"),
    );
    copy_tree(&root().join(".github/assets"), &dir.join(".github/assets"));
    let scorecard = "gate/src/steps/quality_scorecard";
    copy_tree(&root().join(scorecard), &dir.join(scorecard));
    let rules = "gate/src/checks/gate_rules.tsv";
    fs::create_dir_all(dir.join("gate/src/checks")).unwrap();
    fs::copy(root().join(rules), dir.join(rules)).unwrap();
    dir
}

fn render(dir: &Path) -> Output {
    tool("just")
        .arg("--justfile")
        .arg(dir.join("justfile"))
        .arg("--working-directory")
        .arg(dir)
        .arg("_tables")
        .output()
        .unwrap()
}

#[test]
fn every_generated_table_is_what_its_source_says() {
    let dir = sandbox();
    succeeds(&render(&dir));
    for file in generated(&root()) {
        assert_eq!(
            fs::read_to_string(root().join(&file)).unwrap(),
            fs::read_to_string(dir.join(&file)).unwrap(),
            "{} is stale: run just docs",
            file.display()
        );
    }
}

/// Byte equality and identical file sets keep the hosted consumer tied to its generator.
fn assert_same_tree(generated: &Path, committed: &Path) {
    let names = |directory: &Path| {
        let mut names: Vec<_> = fs::read_dir(directory)
            .unwrap_or_else(|error| panic!("{}: {error}", directory.display()))
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name != "target")
            .collect();
        names.sort();
        names
    };
    assert_eq!(
        names(generated),
        names(committed),
        "{}",
        committed.display()
    );
    for entry in fs::read_dir(generated).unwrap() {
        let path = entry.unwrap().path();
        if path.file_name().unwrap() == "target" {
            continue;
        }
        let counterpart = committed.join(path.file_name().unwrap());
        if path.is_dir() {
            assert_same_tree(&path, &counterpart);
        } else {
            assert_eq!(
                fs::read(&path).unwrap(),
                fs::read(&counterpart).unwrap(),
                "{} is stale: regenerate the native consumer",
                counterpart.display()
            );
        }
    }
}

#[test]
fn committed_native_consumer_matches_its_generated_source() {
    let generated = temp_dir("native-consumer-drift");
    engine_workspace(&generated, true);
    fs::write(generated.join(".gitignore"), "target/\n").unwrap();
    succeeds(
        &tool("cargo")
            .args(["generate-lockfile", "--offline"])
            .current_dir(&generated)
            .output()
            .unwrap(),
    );
    assert_same_tree(
        &generated,
        &root().join("tests/fixtures/native-consumer-project"),
    );
    fs::remove_dir_all(generated).unwrap();
}

#[test]
fn a_changed_source_changes_its_table() {
    let dir = sandbox();
    let fuzz = dir.join(".github/workflows/fuzz.yml");
    let text = fs::read_to_string(&fuzz).unwrap();
    let described = text
        .find("      target:\n        description: >-\n")
        .unwrap();
    let start = described + "      target:\n        description: >-\n".len();
    let end = start + text[start..].find("\n        type:").unwrap();
    fs::write(
        &fuzz,
        format!(
            "{}          Changed at its source{}",
            &text[..start],
            &text[end..]
        ),
    )
    .unwrap();
    succeeds(&render(&dir));
    let ci = fs::read_to_string(dir.join("docs/ci.md")).unwrap();
    assert!(
        ci.contains("| `target` | Empty | Changed at its source |"),
        "{ci}"
    );
}

#[test]
fn a_pinned_tool_without_its_description_is_refused() {
    // A tool mise.toml pins without its `# tool:` line would vanish from the
    // README's toolbelt, so rendering stops instead.
    let dir = sandbox();
    let mise = dir.join("mise.toml");
    let text = fs::read_to_string(&mise).unwrap();
    let kept: Vec<&str> = text
        .lines()
        .filter(|line| !line.starts_with("# tool: jaq |"))
        .collect();
    fs::write(&mise, kept.join("\n")).unwrap();
    let output = render(&dir);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("pins jaq without a '# tool:' line"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_commit_or_a_pull_request_regenerates_the_documents() {
    // Nobody keeps a table in step by hand: the commit hook runs just docs,
    // and on a pull request from this repository the bot commits whatever it
    // rewrote, through the recipe that makes GitHub sign the commit.
    let hooks = query(
        &root().join(".pre-commit-config.yaml"),
        r#".repos[].hooks[] | select(.id == "generated-documents") | .entry"#,
    );
    assert_eq!(hooks.trim(), "just docs");
    let sync = workflow("docs-sync");
    assert!(sync["on"].get("pull_request").is_some());
    let job = &sync["jobs"]["sync"];
    assert!(
        job["if"]
            .as_str()
            .unwrap()
            .contains("github.event.pull_request.head.repo.full_name == github.repository")
    );
    let steps = job["steps"].as_array().unwrap();
    let runs: Vec<&str> = steps
        .iter()
        .filter_map(|step| step["run"].as_str())
        .collect();
    assert!(runs.iter().any(|run| run.contains("just docs")));
    let commit = steps
        .iter()
        .find(|step| {
            step["run"]
                .as_str()
                .is_some_and(|run| run.contains("just _commit-as-bot"))
        })
        .unwrap();
    assert_eq!(
        commit["env"]["BRANCH"],
        "${{ github.event.pull_request.head.ref }}"
    );
    assert_eq!(
        commit["env"]["HEAD"],
        "${{ github.event.pull_request.head.sha }}"
    );
    // Only what just docs writes: installing the toolbelt must never reach a
    // commit.
    assert_eq!(
        commit["env"]["PATHS"],
        "README.md docs .github/assets/how-it-works.svg"
    );
}

#[test]
fn the_bot_commits_exactly_the_changed_files_through_the_api() {
    // A git checkout with one changed file, and a gh that keeps the request.
    let dir = temp_dir("commit-as-bot");
    fs::copy(root().join("justfile"), dir.join("justfile")).unwrap();
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(&dir)
            .output()
            .unwrap();
        succeeds(&output);
    };
    git(&["init", "-q"]);
    fs::write(dir.join("notes.md"), "before\n").unwrap();
    fs::write(dir.join("mise.lock"), "before\n").unwrap();
    git(&["add", "notes.md", "mise.lock", "justfile"]);
    git(&["commit", "-qm", "base"]);
    fs::write(dir.join("notes.md"), "after\n").unwrap();
    // A change outside PATHS, such as a toolbelt install touching the lock,
    // stays out of the commit.
    fs::write(dir.join("mise.lock"), "after\n").unwrap();
    fs::write(dir.join("body"), "Why the bot commits.\n").unwrap();
    let bin = dir.join(".tools/bin");
    fs::create_dir_all(&bin).unwrap();
    write_executable(
        &bin.join("gh"),
        "#!/bin/bash\nset -euo pipefail\n\
         while [[ $# -gt 0 ]]; do [[ $1 == --input ]] && cp \"$2\" request.json; shift; done\n\
         echo 0123abc\n",
    );
    let output = tool("just")
        .arg("--justfile")
        .arg(dir.join("justfile"))
        .arg("--working-directory")
        .arg(&dir)
        .arg("_commit-as-bot")
        .envs([
            ("GH_TOKEN", "token"),
            ("GITHUB_REPOSITORY", "owner/repo"),
            ("BRANCH", "feature"),
            ("HEAD", "abc123"),
            ("TITLE", "docs: regenerate the generated tables"),
            ("BODY", "body"),
            ("PATHS", "notes.md docs"),
        ])
        .output()
        .unwrap();
    succeeds(&output);
    let request: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.join("request.json")).unwrap()).unwrap();
    let input = &request["variables"]["input"];
    assert!(
        request["query"]
            .as_str()
            .unwrap()
            .contains("createCommitOnBranch")
    );
    assert_eq!(input["branch"]["branchName"], "feature");
    assert_eq!(input["branch"]["repositoryNameWithOwner"], "owner/repo");
    assert_eq!(input["expectedHeadOid"], "abc123");
    assert_eq!(
        input["message"]["headline"],
        "docs: regenerate the generated tables"
    );
    assert_eq!(input["message"]["body"], "Why the bot commits.\n");
    let additions = input["fileChanges"]["additions"].as_array().unwrap();
    assert_eq!(additions.len(), 1, "{additions:?}");
    assert_eq!(additions[0]["path"], "notes.md");
    assert_eq!(additions[0]["contents"], "YWZ0ZXIK");
}

//! Repository-wide workflow policy: pins, permissions, runners, trust
//! boundaries and the guards every workflow shares.

use crate::harness::{Fixture, root, step, succeeds, tool, workflow};
use serde_json::Value;
use std::fs;
use std::iter;
use std::os::unix::fs::symlink;
use std::path::Path;

#[test]
fn public_workflow_files_exist() {
    for name in ["ci", "publish-binaries", "publish-crate"] {
        assert!(
            root()
                .join(format!(".github/workflows/{name}.yml"))
                .is_file()
        );
    }
}

#[test]
fn repository_uses_just_and_an_isolated_rust_test_harness() {
    for required in [
        "justfile",
        "tests/Cargo.toml",
        "tests/Cargo.lock",
        "tests/workflows.rs",
    ] {
        assert!(
            root().join(required).is_file(),
            "Missing validation entrypoint: {required}"
        );
    }
    assert!(
        !root().join("Cargo.toml").exists(),
        "No root Rust application"
    );
    for obsolete in ["scripts/check.sh", "scripts/setup-tools.sh"] {
        assert!(
            !root().join(obsolete).exists(),
            "Use Just instead of {obsolete}"
        );
    }
}

#[test]
fn permissions_timeouts_and_shell_policy_hold_in_every_workflow() {
    for name in [
        "ci",
        "publish-binaries",
        "publish-crate",
        "ci-internal",
        "dependabot-auto-merge",
        "scorecard",
    ] {
        let data = workflow(name);
        assert!(data.get("permissions").is_some());
        for job in data["jobs"].as_object().unwrap().values() {
            assert!(job.get("permissions").is_some());
            if job.get("steps").is_some() {
                assert!(job.get("timeout-minutes").is_some());
            }
            let items: Vec<&Value> = iter::once(job)
                .chain(job["steps"].as_array().into_iter().flatten())
                .collect();
            assert!(
                items
                    .iter()
                    .all(|item| item.get("continue-on-error").is_none())
            );
            for command in items.iter().filter_map(|item| item["run"].as_str()) {
                body_follows_the_shell_policy(command);
            }
        }
    }
}

/// Consumer input reaches a step through `env`, never through `${{ }}` inside
/// a body, which is the script-injection path; nothing is silenced; and shell
/// that does not stop on the first failure reports success for work it
/// skipped, so a body is fail-closed Bash, a bare cargo or rustup invocation,
/// or a gate command.
fn body_follows_the_shell_policy(command: &str) {
    assert!(!command.contains("${{"));
    assert!(!command.contains("|| true"));
    let has_commands = command.lines().any(|line| {
        let line = line.trim();
        !line.is_empty() && !line.starts_with('#')
    });
    if has_commands {
        assert!(
            command.starts_with("set -euo pipefail\n")
                || command.starts_with("cargo ")
                || command.starts_with("rustup ")
                || command.starts_with("rust-gate "),
            "shell must fail closed, be a bare cargo invocation, or call the gate: {command}"
        );
    }
}

#[test]
fn all_jobs_use_github_runners_without_caller_overrides() {
    for name in [
        "ci",
        "publish-binaries",
        "publish-crate",
        "ci-internal",
        "attest-binaries",
        "publish-evidence",
        "unsafe-audit",
        "fuzz",
        "dependabot-auto-merge",
    ] {
        let data = workflow(name);
        for input in ["runs-on", "publish-runs-on"] {
            assert!(
                data["on"]["workflow_call"]["inputs"].get(input).is_none(),
                "{name} allows a runner override: {input}"
            );
        }
        for (id, job) in data["jobs"].as_object().unwrap() {
            // The portability matrix and Windows mutation job use pinned
            // platform runners; every other job stays on Ubuntu.
            if name == "ci" && id == "portability" {
                assert_eq!(job["runs-on"], "${{ matrix.runner }}");
            } else if name == "ci" && id == "mutation-windows" {
                assert_eq!(job["runs-on"], "windows-2025");
            } else if job.get("steps").is_some() {
                assert_eq!(job["runs-on"], "ubuntu-24.04", "{name}/{id}");
            }
            if let Some(runner) = job["with"].get("runs-on") {
                assert_eq!(runner, "ubuntu-24.04", "{name}/{id}");
            }
        }
    }
}

#[test]
fn ci_entry_jobs_accept_fork_pull_requests() {
    // CI takes no secret and runs with a read-only token, and the organization
    // holds every external contributor's run for approval: a fork pull request
    // is checked like any other, so an outside contribution can pass the
    // required status. pull_request_target, which runs with the base
    // repository's privileges, stays refused.
    // Inside this repository, ci.yml runs only for a caller: its root holds
    // no Cargo package for a run no workflow called.
    for (name, job, condition) in [
        (
            "ci",
            "checks",
            "${{ github.event_name != 'pull_request_target' && (inputs.artifact-key != '' || \
             github.repository_id != '1382744803') }}",
        ),
        (
            "ci-internal",
            "check",
            "${{ github.event_name != 'pull_request_target' }}",
        ),
    ] {
        assert_eq!(workflow(name)["jobs"][job]["if"], condition, "{name}/{job}");
        assert!(
            workflow(name)["on"]["workflow_call"]["secrets"].is_null(),
            "{name} must take no secret to run fork pull requests"
        );
    }
}

#[test]
fn workflows_find_this_repository_by_its_id_not_its_name() {
    // A rename changes github.repository in this repository's own runs, and
    // a job that compares it with a name would start running here. Its id,
    // 1382744803, is what the organization's rulesets name too, and survives.
    for (name, job) in [("ci", "checks"), ("ci", "gate"), ("hygiene", "hygiene")] {
        let condition = workflow(name)["jobs"][job]["if"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(
            condition.contains("github.repository_id != '1382744803'"),
            "{name}/{job}: {condition}"
        );
    }
    for entry in fs::read_dir(root().join(".github/workflows")).unwrap() {
        let path = entry.unwrap().path();
        let text = fs::read_to_string(&path).unwrap();
        for comparison in ["github.repository != '", "github.repository == '"] {
            assert!(
                !text.contains(comparison),
                "{}: {comparison}",
                path.display()
            );
        }
    }
}

#[test]
fn ruleset_workflows_run_on_every_merge_group() {
    // A merge queue merges a group only once every check its rulesets require
    // has reported on the group's commit, so each workflow an organization
    // ruleset requires runs on `merge_group`, and so does `ci-internal`,
    // whose two checks this repository's own ruleset requires. Only a pull
    // request's run gives way to a newer one: a cancelled merge group run
    // would drop the entry.
    for name in ["ci", "hygiene", "ci-internal"] {
        let data = workflow(name);
        let triggers = data["on"].as_object().unwrap();
        assert!(triggers.contains_key("pull_request"), "{name}");
        assert!(triggers.contains_key("merge_group"), "{name}");
        assert_eq!(
            data["concurrency"]["cancel-in-progress"], "${{ github.event_name == 'pull_request' }}",
            "{name}"
        );
    }
}

#[test]
fn publisher_entry_jobs_reject_untrusted_pull_request_contexts() {
    for (name, job) in [
        ("publish-binaries", "preflight"),
        ("publish-crate", "preflight"),
        ("publish-evidence", "preflight"),
        ("attest-binaries", "attest"),
    ] {
        assert_eq!(
            workflow(name)["jobs"][job]["if"],
            "${{ github.event_name != 'pull_request_target' && \
             (!github.event.pull_request || \
             github.event.pull_request.head.repo.full_name == github.repository) }}",
            "{name}/{job} must reject fork PRs before a runner is allocated"
        );
    }
}

#[test]
fn the_path_guard_is_identical_in_every_workflow_that_takes_a_directory() {
    // A reusable workflow runs inside the consumer's checkout, so the three
    // workflows that take a working directory used to embed the same Bash
    // guard three times. The gate holds it once, and running each validate
    // step against the same bad directories has to produce the same refusal,
    // word for word: not a simple path, a traversal, a symlink out of the
    // checkout, a directory that is not there.
    let cases = [
        ("../escape", "simple relative path"),
        ("project/../escape", "traverse or contain option-like"),
        ("project/out", "escapes checkout"),
        ("project/none", "does not exist inside checkout"),
    ];
    let mut refusals: Vec<Vec<String>> = Vec::new();
    for (workflow, command) in [
        ("ci", "rust-gate validate"),
        ("fuzz", "rust-gate fuzz validate"),
        ("unsafe-audit", "rust-gate unsafe-audit validate"),
    ] {
        assert_eq!(step(workflow, "validate").trim(), command);
        let mut seen = Vec::new();
        for (directory, refusal) in cases {
            let mut fixture = Fixture::new();
            if directory == "project/out" {
                symlink("/", fixture.root.join("project/out")).unwrap();
            }
            fixture.set("DIRECTORY", directory);
            let output = fixture.run(workflow, "validate");
            assert!(!output.status.success(), "{workflow} accepted {directory}");
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            assert!(
                stderr.contains(refusal),
                "{workflow} refused {directory} for another reason: {stderr}"
            );
            seen.push(stderr);
        }
        refusals.push(seen);
    }
    assert!(
        refusals.iter().all(|seen| seen == &refusals[0]),
        "the three workflows refuse the same directory differently: {refusals:?}"
    );
}

#[test]
fn security_policy_documents_how_to_verify_a_release() {
    let text = fs::read_to_string(root().join("SECURITY.md")).unwrap();
    // A reader who cannot verify a release has no way to tell a real asset from
    // a substituted one, whatever the pipeline produced. The bills of materials
    // are named as the payload holds them: the merged CycloneDX document and
    // one SPDX document per binary.
    for step in [
        "sha256sum --check",
        "gh attestation verify",
        "payload.cdx.json",
        "<binary>.spdx.json",
        "cargo audit bin",
    ] {
        assert!(text.contains(step), "SECURITY.md must document: {step}");
    }
    assert!(
        !text.contains("payload.spdx.json"),
        "no such file is released"
    );
    // The procedure was run against a published release; saying which one is
    // part of the instruction, so a reader can repeat it.
    assert!(
        text.contains(
            "https://github.com/Orchestration-Maestro/release-canary/releases/tag/v0.1.0"
        ),
        "SECURITY.md must name the release the procedure was run against"
    );
}

#[test]
fn the_consumer_matrix_calls_every_local_workflow_and_fixture() {
    let matrix = workflow("ci-internal");
    for name in ["ci", "publish-binaries", "publish-crate"] {
        assert!(
            matrix["jobs"]
                .as_object()
                .unwrap()
                .values()
                .any(|job| job["uses"] == format!("./.github/workflows/{name}.yml"))
        );
    }
    for example in ["binary", "library", "workspace"] {
        let base = root().join("examples").join(example);
        assert!(base.join("Cargo.lock").is_file());
        let result = tool("jaq")
            .args(["--from", "toml", "-r", ".toolchain.channel"])
            .arg(base.join("rust-toolchain.toml"))
            .output()
            .unwrap();
        succeeds(&result);
        assert_eq!(String::from_utf8(result.stdout).unwrap().trim(), "1.98.1");
    }
    // The workspace fixture's members inherit their settings and their shared
    // dependencies from its root, the way WSP-001 requires, so a value a member
    // marks `workspace = true` is read there.
    let toml = |path: &Path| -> Value {
        let output = tool("jaq")
            .args(["--from", "toml", "."])
            .arg(path)
            .output()
            .unwrap();
        succeeds(&output);
        serde_json::from_slice(&output.stdout).unwrap()
    };
    let workspace = toml(&root().join("examples/workspace/Cargo.toml"))["workspace"].clone();
    let resolved = |value: &Value, shared: &Value| -> Value {
        if value["workspace"] == true {
            shared.clone()
        } else {
            value.clone()
        }
    };
    for member in ["binary", "library", "workspace/core", "workspace/app"] {
        let manifest = toml(&root().join("examples").join(member).join("Cargo.toml"));
        let setting = |key: &str| resolved(&manifest["package"][key], &workspace["package"][key]);
        assert_eq!(setting("edition"), "2024");
        assert_eq!(setting("rust-version"), "1.85");
        // Fixtures depend on each other by path. The one exception is the
        // binary fixture's single crates.io dependency, there so the dependency
        // policy, the SBOMs and the embedded dependency list are checked against
        // a real lockfile; it also makes the hosted consumer matrix fetch one crate
        // directly from crates.io, as every consumer does.
        for (name, dependency) in manifest["dependencies"]
            .as_object()
            .into_iter()
            .flat_map(|entries| entries.iter())
        {
            let dependency = resolved(dependency, &workspace["dependencies"][name]);
            assert!(
                dependency.get("path").is_some() || (member == "binary" && name == "anyhow"),
                "{member}: unexpected registry dependency {name}"
            );
        }
    }
}

#[test]
fn canonical_document_generation_and_native_status_remain_safe() {
    let recipes = fs::read_to_string(root().join("justfile")).unwrap();
    assert!(
        recipes.contains("[linux]\ndocs:"),
        "canonical docs require Linux"
    );
    assert!(recipes.contains("set windows-shell := [\"cmd.exe\", \"/d\", \"/s\", \"/c\"]"));
}

/// Dependabot's own pull requests queue a squash merge on the organization's
/// bot token, which reaches a Dependabot run only as a Dependabot secret. A merge
/// by `GITHUB_TOKEN` would trigger no workflow on `main`. An update is left to
/// a person when any member of it is major or its type is not recorded. A run a
/// person starts by pushing to Dependabot's branch holds no Dependabot secret,
/// so the job skips it rather than fail to mint the token.
#[test]
fn dependabot_updates_merge_through_the_bot_unless_one_is_major() {
    let data = workflow("dependabot-auto-merge");
    assert_eq!(
        data["on"].as_object().unwrap().keys().collect::<Vec<_>>(),
        ["pull_request"]
    );
    let job = &data["jobs"]["auto-merge"];
    assert_eq!(
        job["if"],
        concat!(
            "${{ github.event.pull_request.user.login == 'dependabot[bot]' ",
            "&& github.actor == 'dependabot[bot]' }}"
        )
    );
    let token = &job["steps"][0];
    assert!(
        token["uses"]
            .as_str()
            .unwrap()
            .starts_with("actions/create-github-app-token@")
    );
    assert_eq!(
        token["with"]["client-id"],
        "${{ secrets.RELEASE_APP_CLIENT_ID }}"
    );
    assert_eq!(
        token["with"]["private-key"],
        "${{ secrets.RELEASE_APP_PRIVATE_KEY }}"
    );
    assert_eq!(
        job["steps"][1]["env"]["GH_TOKEN"],
        "${{ steps.token.outputs.token }}"
    );

    let pull_request = "https://github.com/o/r/pull/7";
    for (types, merges) in [
        (&["patch"][..], true),
        (&["minor", "patch"][..], true),
        (&["minor", "major"][..], false),
        (&[][..], false),
    ] {
        let mut fixture = Fixture::new();
        fixture.set("PR_URL", pull_request);
        let trailer = types
            .iter()
            .map(|kind| {
                format!("- dependency-name: example\n  update-type: version-update:semver-{kind}\n")
            })
            .collect::<Vec<_>>()
            .concat();
        fixture.stub(
            "gh",
            &format!(
                "if [[ $1 == pr && $2 == view ]]; then cat <<'BODY'\n\
                 Bumps example.\n---\nupdated-dependencies:\n{trailer}...\n\n\
                 Signed-off-by: dependabot[bot]\nBODY\nfi"
            ),
        );
        succeeds(&fixture.run("dependabot-auto-merge", "merge"));
        assert_eq!(
            fixture
                .calls()
                .contains(&format!("pr merge --auto --squash {pull_request}")),
            merges,
            "{types:?}"
        );
    }
}

/// The `OpenSSF` Scorecard publishes this repository's score, and its API rejects a
/// run whose workflow breaks its rules: no workflow-level env, defaults or
/// write scope; `id-token: write` only on the scoring job; that job on a hosted
/// Ubuntu runner with no env, container or service, running only approved
/// actions. The findings also land in code scanning.
#[test]
fn the_repository_is_scored_within_the_scorecard_publishing_rules() {
    let data = workflow("scorecard");
    assert!(data.get("env").is_none() && data.get("defaults").is_none());
    assert_eq!(data["permissions"], serde_json::json!({"contents": "read"}));
    let jobs = data["jobs"].as_object().unwrap();
    assert_eq!(jobs.len(), 1);
    let job = &jobs["analysis"];
    assert_eq!(job["runs-on"], "ubuntu-24.04");
    for forbidden in ["env", "defaults", "container", "services"] {
        assert!(job.get(forbidden).is_none(), "{forbidden}");
    }
    assert_eq!(
        job["permissions"],
        serde_json::json!({
            "contents": "read",
            "security-events": "write",
            "id-token": "write"
        })
    );
    let steps = job["steps"].as_array().unwrap();
    let actions: Vec<&str> = steps
        .iter()
        .map(|step| step["uses"].as_str().unwrap().split('@').next().unwrap())
        .collect();
    assert_eq!(
        actions,
        [
            "actions/checkout",
            "ossf/scorecard-action",
            "github/codeql-action/upload-sarif"
        ]
    );
    assert_eq!(steps[0]["with"]["persist-credentials"], false);
    assert_eq!(steps[1]["with"]["publish_results"], true);
    assert_eq!(
        steps[1]["with"]["results_file"],
        steps[2]["with"]["sarif_file"]
    );
}

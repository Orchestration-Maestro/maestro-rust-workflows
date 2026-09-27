//! `ci.yml`'s uploads: the Clippy and secret-scan SARIF to code scanning and
//! the coverage and test results to Codecov, from the organization's check on
//! a pull request and from the merge group that lands on the default branch.

use crate::harness::{root, tool_rows, workflow};
use serde_json::{Value, json};
use std::fs;

/// Where ci.yml's uploads run: once the checks passed, only when no workflow
/// called it, from the organization's ruleset in a repository's own context,
/// and never for a fork pull request, whose token can neither write security
/// events nor log in to Codecov; its reports stay in the artifact. A merge
/// group also waits for the portability legs and uploads nothing when one
/// failed, since its commit then never lands.
const UPLOADS_RUN: &str = concat!(
    "${{ !cancelled() && needs.checks.result == 'success' && ",
    "(needs.checks.outputs.mutation-mode != 'sharded' || ",
    "needs.mutation-summary.result == 'success') && ",
    "(github.event_name != 'merge_group' || needs.portability.result == 'success' || ",
    "needs.portability.result == 'skipped') && inputs.artifact-key == '' && ",
    "github.event_name != 'pull_request_target' && (!github.event.pull_request || ",
    "github.event.pull_request.head.repo.full_name == github.repository) }}"
);

/// The jobs the uploads wait for: the checks that wrote the reports, and the
/// portability legs that decide whether a merge group's commit lands.
const UPLOADS_NEED: [&str; 3] = ["checks", "portability", "mutation-summary"];

#[test]
fn sarif_reports_upload_from_the_organizations_check_alone() {
    // Every run writes SARIF, and the organization's check shows it in code
    // scanning. The job id `upload` and the categories keep the configuration
    // the retired callers recorded on every default branch, so a pull
    // request still compares against it.
    let ci = workflow("ci");
    assert_eq!(
        ci["on"]["workflow_call"]["inputs"]["sarif-reports"]["default"],
        true
    );
    let job = &ci["jobs"]["upload"];
    assert_eq!(job["needs"], json!(UPLOADS_NEED));
    assert_eq!(job["if"], UPLOADS_RUN);
    assert_eq!(
        job["permissions"],
        json!({"contents": "read", "security-events": "write"})
    );
    let steps = job["steps"].as_array().unwrap();
    assert!(
        steps[0]["uses"]
            .as_str()
            .unwrap()
            .starts_with("actions/download-artifact@")
    );
    assert_eq!(
        steps[0]["with"]["name"],
        "${{ needs.checks.outputs.artifact-name }}-reports"
    );
    let uploads: Vec<(&str, &str, &str, &str)> = steps[1..]
        .iter()
        .map(|step| {
            (
                step["uses"].as_str().unwrap().split('@').next().unwrap(),
                step["with"]["sarif_file"].as_str().unwrap(),
                step["with"]["category"].as_str().unwrap(),
                step["if"].as_str().unwrap(),
            )
        })
        .collect();
    let action = "github/codeql-action/upload-sarif";
    assert_eq!(
        uploads,
        [
            (
                action,
                "reports/clippy.sarif",
                "clippy",
                "${{ hashFiles('reports/clippy.sarif') != '' }}"
            ),
            (
                action,
                "reports/secrets.sarif",
                "gitleaks",
                "${{ hashFiles('reports/secrets.sarif') != '' }}"
            ),
        ]
    );
}

#[test]
fn codecov_uploads_run_a_verified_cli_and_never_block() {
    // Codecov takes the run's LCOV and JUnit reports through OIDC, which
    // replaces a stored token. The checks job holds coverage to its floors,
    // so the upload is advisory and a Codecov outage fails nothing. The
    // action runs a CLI it downloads itself even when its signature check
    // fails once errors pass, so it always gets the one this job installed
    // from a release asset verified by digest.
    let ci = workflow("ci");
    let job = &ci["jobs"]["coverage"];
    assert_eq!(job["needs"], json!(UPLOADS_NEED));
    assert_eq!(job["if"], UPLOADS_RUN);
    assert_eq!(
        job["permissions"],
        json!({"contents": "read", "id-token": "write"})
    );
    let steps = job["steps"].as_array().unwrap();
    let uses = |step: &Value, action: &str| {
        step["uses"]
            .as_str()
            .is_some_and(|uses| uses.starts_with(&format!("{action}@")))
    };
    let first_upload = steps
        .iter()
        .position(|step| uses(step, "codecov/codecov-action"))
        .unwrap();
    let installed: Vec<_> = steps[..first_upload].iter().flat_map(tool_rows).collect();
    assert_eq!(installed.len(), 1);
    assert_eq!(installed[0].name, "codecov");
    let asset = &installed[0].asset;
    assert!(
        asset.starts_with("codecov/codecov-cli/releases/download/v")
            && asset.ends_with("/codecovcli_linux"),
        "{asset}"
    );
    assert_eq!(installed[0].digest.len(), 64);
    // Codecov maps the report's paths onto the files of this checkout.
    assert!(steps[..first_upload].iter().any(|step| {
        uses(step, "actions/checkout")
            && step["with"]["ref"] == "${{ github.sha }}"
            && step["with"]["persist-credentials"] == false
    }));
    let uploads: Vec<(&str, &str)> = steps[first_upload..]
        .iter()
        .map(|step| {
            assert!(uses(step, "codecov/codecov-action"));
            let with = &step["with"];
            assert_eq!(with["binary"], "${{ runner.temp }}/rust-tools/bin/codecov");
            assert!(with.get("version").is_none());
            assert_eq!(with["use_oidc"], true);
            assert_eq!(with["disable_search"], true);
            assert_eq!(with["fail_ci_if_error"], false);
            (
                with["files"].as_str().unwrap(),
                with["report_type"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        uploads,
        [
            ("reports/coverage.lcov", "coverage"),
            ("reports/tests.xml", "test_results"),
        ]
    );
}

#[test]
fn merge_group_uploads_land_on_the_default_branch() {
    // A merge group's commit is the one the default branch moves to once its
    // checks pass, so both uploads file it there: code scanning and Codecov
    // hold the baseline each pull request compares against, with no push run.
    // On a pull request every override is empty, and each tool reads the
    // run's own ref and commit.
    let ci = workflow("ci");
    let branch = "${{ github.event.merge_group && github.event.repository.default_branch || '' }}";
    let commit = "${{ github.event.merge_group.head_sha }}";
    let sarif = &ci["jobs"]["upload"]["steps"].as_array().unwrap()[1..];
    assert_eq!(sarif.len(), 2);
    for step in sarif {
        assert_eq!(
            step["with"]["ref"],
            "${{ github.event.merge_group && format('refs/heads/{0}', \
             github.event.repository.default_branch) || '' }}"
        );
        assert_eq!(step["with"]["sha"], commit);
    }
    let codecov: Vec<&Value> = ci["jobs"]["coverage"]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|step| step["with"].get("report_type").is_some())
        .collect();
    assert_eq!(codecov.len(), 2);
    for step in codecov {
        assert_eq!(step["with"]["override_branch"], branch);
        assert_eq!(step["with"]["override_commit"], commit);
    }
    // A merge group has no pull request, so the fork guard lets both run,
    // but only once every portability leg passed: a failed one keeps the
    // group's commit off the default branch, and its results with it. A pull
    // request uploads whatever the legs say, as soon as they end.
    assert!(UPLOADS_RUN.contains("!github.event.pull_request ||"));
    assert!(UPLOADS_RUN.contains(
        "(github.event_name != 'merge_group' || needs.portability.result == 'success' || \
         needs.portability.result == 'skipped')"
    ));
    for job in ["upload", "coverage"] {
        assert_eq!(ci["jobs"][job]["needs"], json!(UPLOADS_NEED), "{job}");
        assert_eq!(ci["jobs"][job]["if"], UPLOADS_RUN, "{job}");
    }
}

#[test]
fn every_caller_of_ci_grants_the_scopes_its_uploads_ask() {
    // GitHub checks a called workflow's scopes when the run starts, a job its
    // `if` skips included, so a caller of ci.yml that withholds the uploads'
    // scopes fails before any job runs; a publisher calls ci.yml, so its
    // callers grant them too.
    let mut callers = 0;
    for entry in fs::read_dir(root().join(".github/workflows")).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let data = workflow(&name);
        for (id, job) in data["jobs"].as_object().unwrap() {
            let called = job["uses"].as_str().unwrap_or_default();
            if ["ci", "publish-binaries", "publish-crate"]
                .iter()
                .any(|workflow| called == format!("./.github/workflows/{workflow}.yml"))
            {
                assert_eq!(
                    job["permissions"]["security-events"], "write",
                    "{name}/{id}"
                );
                assert_eq!(job["permissions"]["id-token"], "write", "{name}/{id}");
                callers += 1;
            }
        }
    }
    assert!(
        callers >= 6,
        "only {callers} callers of ci.yml were checked"
    );
}

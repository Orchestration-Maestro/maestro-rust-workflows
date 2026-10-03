//! API and feature children share only a policy-opted-in, reverified native root.

use crate::harness::{Fixture, cache_fixture, output, succeeds, workflow};
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;

/// Keep Cargo metadata and Git baseline discovery while observing the build child's environment.
fn checks_fixture() -> Fixture {
    let mut fixture = cache_fixture();
    fs::write(
        fixture.root.join("project/Cargo.lock"),
        "version = 4\npackage = []\n",
    )
    .unwrap();
    fixture.set("API_COMPATIBILITY", "true");
    fixture.set("GITHUB_BASE_REF", "main");
    fixture.stub(
        "git",
        r#"case "$1 $2" in
  "rev-parse --show-toplevel") pwd ;;
  "show "*) printf '[package]\nname="fixture"\n' ;;
  *) exit 0 ;;
esac"#,
    );
    let metadata = format!(
        concat!(
            r#"{{"workspace_root":"{0}","workspace_members":["fixture"],"packages":[{{"#,
            r#""id":"fixture","name":"fixture","manifest_path":"{0}/Cargo.toml","#,
            r#""features":{{"engine":[]}},"targets":[{{"kind":["lib"]}}]}}]}}"#
        ),
        fixture.root.join("project").display()
    );
    fs::write(fixture.root.join("metadata.json"), &metadata).unwrap();
    fixture.set("CHECK_METADATA", &metadata);
    fixture.stub(
        "cargo",
        concat!(
            "if [[ $1 == metadata ]]; then printf '%s' \"$CHECK_METADATA\"; fi\n",
            "printf '%s %s\\n' \"$*\" \"${FIXTURE_NATIVE_CACHE_DIR:-unset}\" ",
            ">> \"$REPORTS/check-child-env\""
        ),
    );
    fixture
}

#[test]
fn api_and_features_inject_only_after_policy_and_platform_opt_in() {
    for step in ["api", "features"] {
        for state in ["enabled", "absent", "no-table", "platform", "no-root"] {
            let mut fixture = checks_fixture();
            succeeds(&fixture.run_body("rust-gate native-cache-prepare"));
            let root = output(&fixture, "root");
            fixture.set("NATIVE_CACHE_ROOT", &root);
            fixture.set("FIXTURE_NATIVE_CACHE_DIR", "inherited-root");
            let policy = fixture.root.join("project/maestro-quality.toml");
            if state == "absent" {
                fs::remove_file(&policy).unwrap();
            } else if state == "no-table" {
                fs::write(&policy, "[ci]\nplatforms='macos windows'\n").unwrap();
            } else if state == "platform" {
                let text = fs::read_to_string(&policy).unwrap();
                fs::write(policy, text.replace("['linux','macos']", "['macos']")).unwrap();
            } else if state == "no-root" {
                fixture.set("NATIVE_CACHE_ROOT", "");
            }
            succeeds(&fixture.run("ci", step));
            let children =
                fs::read_to_string(fixture.root.join("reports/check-child-env")).unwrap();
            for child in children.lines() {
                let build = child.starts_with("semver-checks") || child.starts_with("hack");
                let expected = match (state, build) {
                    ("absent" | "no-table", _) => "inherited-root",
                    ("enabled", true) => &root,
                    _ => "unset",
                };
                assert!(
                    child.ends_with(&format!(" {expected}")),
                    "{step}/{state}: {child}"
                );
            }
            assert_eq!(
                fixture
                    .root
                    .join("reports/native-cache-before.txt")
                    .exists(),
                state == "enabled",
                "{step}/{state}"
            );
        }
    }
}

#[test]
fn api_and_features_recheck_private_modes_and_reject_restored_links() {
    for step in ["api", "features"] {
        for unsafe_root in ["parent-mode", "symlink"] {
            let mut fixture = checks_fixture();
            succeeds(&fixture.run_body("rust-gate native-cache-prepare"));
            let root = output(&fixture, "root");
            fixture.set("NATIVE_CACHE_ROOT", &root);
            if unsafe_root == "parent-mode" {
                succeeds(&fixture.run_body("chmod 755 \"$(dirname \"$NATIVE_CACHE_ROOT\")\""));
            } else {
                symlink(
                    fixture.root.join("project"),
                    Path::new(&root).join("entry-bad"),
                )
                .unwrap();
            }
            succeeds(&fixture.run("ci", step));
            let record =
                fs::read_to_string(fixture.root.join("reports/native-cache-before.txt")).unwrap();
            let selected = record.lines().next().unwrap();
            assert_ne!(selected, root, "{step}/{unsafe_root}");
            assert_eq!(
                fs::metadata(selected).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert!(fs::read_dir(selected).unwrap().next().is_none());
            let children =
                fs::read_to_string(fixture.root.join("reports/check-child-env")).unwrap();
            assert!(children.lines().last().unwrap().ends_with(selected));
        }
    }
}

#[test]
fn later_checks_preserve_initial_inventory_but_still_reverify_the_root() {
    let mut fixture = checks_fixture();
    succeeds(&fixture.run_body("rust-gate native-cache-prepare"));
    let root = output(&fixture, "root");
    fixture.set("NATIVE_CACHE_ROOT", &root);
    succeeds(&fixture.run("ci", "api"));
    let before = fs::read(fixture.root.join("reports/native-cache-before.txt")).unwrap();
    fs::create_dir(Path::new(&root).join("entry-one")).unwrap();
    succeeds(&fixture.run("ci", "features"));
    assert_eq!(
        fs::read(fixture.root.join("reports/native-cache-before.txt")).unwrap(),
        before
    );
    symlink(
        fixture.root.join("project"),
        Path::new(&root).join("entry-bad"),
    )
    .unwrap();
    succeeds(&fixture.run("ci", "api"));
    assert_eq!(
        fs::read(fixture.root.join("reports/native-cache-before.txt")).unwrap(),
        before
    );
    let children = fs::read_to_string(fixture.root.join("reports/check-child-env")).unwrap();
    assert!(!children.lines().last().unwrap().ends_with(&root));
}

#[test]
fn checks_workflow_passes_the_same_prepared_root_to_each_build_step() {
    let ci = workflow("ci");
    let steps = ci["jobs"]["checks"]["steps"].as_array().unwrap();
    for id in ["coverage", "api", "features"] {
        let step = steps.iter().find(|step| step["id"] == id).unwrap();
        assert_eq!(
            step["env"]["NATIVE_CACHE_ROOT"],
            "${{ steps.native-cache-prepare.outputs.root }}"
        );
    }
}

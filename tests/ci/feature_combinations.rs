//! Real feature checks supplement the no-feature consumer examples.

use crate::harness::{Fixture, GATE_STEPS, refused, succeeds};
use std::fs;
use std::path::PathBuf;

/// An offline workspace with a normal path dependency and a dev-only dependency
/// that enables the normal dependency's otherwise disabled feature.
fn dev_dependency_workspace() -> Fixture {
    let mut fixture = Fixture::new();
    fixture.set("CARGO_NET_OFFLINE", "true");
    fixture.set("CARGO_BUILD_JOBS", "3");
    fixture.set(
        "CARGO_HOME",
        &fixture.root.join("cargo-home").display().to_string(),
    );
    let cargo = fixture.run_body("command -v cargo");
    succeeds(&cargo);
    fixture.set(
        "REAL_CARGO",
        String::from_utf8(cargo.stdout).unwrap().trim(),
    );
    let project = fixture.root.join("project");
    fs::write(
        project.join("Cargo.toml"),
        "[workspace]\nmembers = ['.', 'member']\nresolver = '3'\n\
         [package]\nname = 'features-fixture'\nversion = '0.1.0'\nedition = '2024'\n\
         [features]\nalpha = []\n[dependencies]\nshared = { path = '../shared' }\n\
         [dev-dependencies]\ndev-only = { path = '../dev-only' }\n",
    )
    .unwrap();
    fs::create_dir_all(project.join("member/src")).unwrap();
    fs::write(
        project.join("member/Cargo.toml"),
        "# Member original bytes.\n[package]\nname = 'feature-member'\nversion = '0.1.0'\n\
         edition = '2024'\n[dev-dependencies]\ndev-only = { path = '../../dev-only' }\n",
    )
    .unwrap();
    fs::write(project.join("member/src/lib.rs"), "//! Workspace member.\n").unwrap();
    for (directory, manifest, source) in [
        (
            "shared",
            "[workspace]\n[package]\nname = 'shared'\nversion = '0.1.0'\nedition = '2024'\n\
             [features]\nunified = []\n",
            "#[cfg(feature = \"unified\")]\npub fn dev_enabled() {}\n",
        ),
        (
            "dev-only",
            "[workspace]\n[package]\nname = 'dev-only'\nversion = '0.1.0'\nedition = '2024'\n\
             [dependencies]\nshared = { path = '../shared', features = ['unified'] }\n",
            "pub fn enable_shared() { shared::dev_enabled(); }\n",
        ),
    ] {
        fs::create_dir_all(fixture.root.join(directory).join("src")).unwrap();
        fs::write(fixture.root.join(directory).join("Cargo.toml"), manifest).unwrap();
        fs::write(fixture.root.join(directory).join("src/lib.rs"), source).unwrap();
    }
    succeeds(&fixture.run_body(
        "cd \"$PROJECT\"; cargo metadata --format-version 1 --offline \
         > \"$RUNNER_TEMP/metadata.json\"",
    ));
    // Commit a deliberately noncanonical comment too: restoration is bytes,
    // not just equivalent package identities or Cargo's rewritten formatting.
    let lock = project.join("Cargo.lock");
    let original = fs::read_to_string(&lock).unwrap();
    fs::write(&lock, format!("# Original fixture bytes.\n{original}")).unwrap();
    succeeds(&fixture.run_body(
        "cd \"$PROJECT\"; git init --quiet; git add .; \
         git -c user.name=Fixture -c user.email=fixture@example.invalid \
         -c commit.gpgsign=false commit --quiet -m fixture",
    ));
    fixture
}

/// Bytes of the lock, the root manifest and every member manifest.
fn original_project_files(fixture: &Fixture) -> Vec<(PathBuf, Vec<u8>)> {
    ["Cargo.lock", "Cargo.toml", "member/Cargo.toml"]
        .iter()
        .map(|path| {
            let path = fixture.root.join("project").join(path);
            let bytes = fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect()
}

/// Restoration must preserve every byte, not just valid Cargo declarations.
fn assert_project_files(original: &[(PathBuf, Vec<u8>)]) {
    for (path, bytes) in original {
        assert_eq!(&fs::read(path).unwrap(), bytes, "{}", path.display());
    }
}

/// A dev-only source that disappears from the resolved graph, unlike path
/// packages whose entries Cargo can retain under --locked.
fn git_dev_dependency_workspace() -> Fixture {
    let fixture = dev_dependency_workspace();
    let dependency = fixture.root.join("dev-only");
    fs::write(
        dependency.join("Cargo.toml"),
        "[package]\nname = 'dev-only'\nversion = '0.1.0'\nedition = '2024'\n",
    )
    .unwrap();
    fs::write(dependency.join("src/lib.rs"), "pub fn dev_only() {}\n").unwrap();
    succeeds(&fixture.run_body(
        "cd dev-only; git init --quiet; git add .; \
         git -c user.name=Fixture -c user.email=fixture@example.invalid \
         -c commit.gpgsign=false commit --quiet -m fixture",
    ));
    let manifest = fixture.root.join("project/Cargo.toml");
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(
        &manifest,
        text.replace(
            "path = '../dev-only'",
            &format!("git = 'file://{}'", dependency.display()),
        ),
    )
    .unwrap();
    // The only fetch is a local file:// repository, never an index or remote.
    succeeds(&fixture.run_body(
        "cd \"$PROJECT\"; cargo metadata --format-version 1 --config net.offline=false \
         > \"$RUNNER_TEMP/metadata.json\"; git add .; \
         git -c user.name=Fixture -c user.email=fixture@example.invalid \
         -c commit.gpgsign=false commit --quiet -m git-dependency",
    ));
    fixture
}

/// Replace only cargo-hack; workspace discovery still uses real Cargo.
fn stub_cargo_hack(fixture: &Fixture, body: &str) {
    fixture.stub(
        "cargo",
        &format!("if [[ \"$1\" == metadata ]]; then exec \"$REAL_CARGO\" \"$@\"; fi\n{body}"),
    );
}

#[test]
fn dev_only_git_dependencies_allow_features_without_changing_lock_bytes() {
    let fixture = git_dev_dependency_workspace();
    let original = original_project_files(&fixture);
    succeeds(&fixture.run("ci", "features"));
    assert_project_files(&original);
    let trace = fixture.trace();
    let hack = trace
        .lines()
        .find(|line| line.starts_with("cargo hack "))
        .unwrap();
    assert!(
        hack.split_whitespace().any(|word| word == "--offline"),
        "{hack}"
    );
    assert!(hack.contains("--remove-dev-deps"), "{hack}");
    assert!(!hack.contains("--locked"), "{hack}");
}

#[test]
fn dev_only_path_dependencies_keep_original_lock_bytes() {
    let fixture = dev_dependency_workspace();
    let original = original_project_files(&fixture);
    succeeds(&fixture.run("ci", "features"));
    assert_project_files(&original);
}

#[test]
fn dev_dependency_feature_unification_cannot_hide_broken_features() {
    let fixture = dev_dependency_workspace();
    fs::write(
        fixture.root.join("project/src/lib.rs"),
        "#[cfg(feature = \"alpha\")]\npub fn needs_dev_feature() { shared::dev_enabled(); }\n",
    )
    .unwrap();
    // Resolver 1 unifies dev features even for the plain check cargo-hack runs.
    let manifest = fixture.root.join("project/Cargo.toml");
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(&manifest, text.replace("resolver = '3'", "resolver = '1'")).unwrap();
    succeeds(&fixture.run_body("cd \"$PROJECT\"; cargo check --all-features --offline --locked"));
    let original = original_project_files(&fixture);
    refused(
        &fixture.run("ci", "features"),
        "cannot find function `dev_enabled`",
    );
    assert_project_files(&original);
}

#[test]
fn a_new_lock_package_is_refused_and_original_bytes_restored() {
    let fixture = dev_dependency_workspace();
    let lock = fixture.root.join("project/Cargo.lock");
    let original = fs::read_to_string(&lock).unwrap();
    let missing = original
        .split("[[package]]")
        .filter(|entry| !entry.contains("name = \"shared\""))
        .collect::<Vec<_>>()
        .join("[[package]]");
    fs::write(&lock, &missing).unwrap();
    let original = original_project_files(&fixture);
    refused(
        &fixture.run("ci", "features"),
        "feature check changed locked package: shared",
    );
    assert_project_files(&original);
}

#[test]
fn cargo_hack_failure_still_restores_original_lock_bytes() {
    let fixture = git_dev_dependency_workspace();
    fs::write(
        fixture.root.join("project/src/lib.rs"),
        "compile_error!(\"feature_fixture_failed\");\n",
    )
    .unwrap();
    let original = original_project_files(&fixture);
    refused(&fixture.run("ci", "features"), "feature_fixture_failed");
    assert_project_files(&original);
    assert!(!fixture.root.join("output").exists());
}

#[test]
fn changed_lock_identity_fields_are_each_refused_with_package_name() {
    let fixture = dev_dependency_workspace();
    let original = original_project_files(&fixture);
    // Stand in only for the external writer to exercise identity changes
    // without registry downloads. Parsing still uses jaq's real TOML parser.
    for identity in [
        "name = 'renamed'\nversion = '0.1.0'",
        "name = 'shared'\nversion = '0.2.0'",
        "name = 'shared'\nversion = '0.1.0'\nsource = 'registry+https://example.invalid/index'",
        "name = 'shared'\nversion = '0.1.0'\nchecksum = 'different'",
    ] {
        stub_cargo_hack(
            &fixture,
            &format!("printf '%s\\n' \"version = 4\n[[package]]\n{identity}\" > Cargo.lock"),
        );
        let name = if identity.contains("renamed") {
            "renamed"
        } else {
            "shared"
        };
        refused(
            &fixture.run("ci", "features"),
            &format!("feature check changed locked package: {name}"),
        );
        assert_project_files(&original);
        assert!(!fixture.root.join("output").exists());
    }
}

#[test]
fn an_unreadable_rewritten_lock_still_restores_original_bytes() {
    let fixture = dev_dependency_workspace();
    let original = original_project_files(&fixture);
    stub_cargo_hack(&fixture, "printf '%s\\n' 'invalid TOML [' > Cargo.lock");
    assert!(!fixture.run("ci", "features").status.success());
    assert_project_files(&original);
}

#[test]
fn a_midway_manifest_edit_failure_restores_every_snapshotted_file() {
    for virtual_root in [false, true] {
        let fixture = dev_dependency_workspace();
        if virtual_root {
            fs::write(
                fixture.root.join("project/Cargo.toml"),
                "# Virtual root original bytes.\n[workspace]\n\
                 members = ['member']\nresolver = '3'\n",
            )
            .unwrap();
        }
        let original = original_project_files(&fixture);
        stub_cargo_hack(
            &fixture,
            "printf '%s\\n' '# Modified root' > Cargo.toml; \
             printf '%s\\n' '# Modified member' > member/Cargo.toml; \
             printf '%s\\n' 'version = 4' 'package = []' > Cargo.lock; exit 73",
        );
        let result = fixture.run("ci", "features");
        assert_eq!(result.status.code(), Some(73));
        assert_project_files(&original);
        assert!(!fixture.root.join("output").exists());
    }
}

#[test]
fn a_member_project_restores_the_workspace_root_lock_bytes() {
    let mut fixture = git_dev_dependency_workspace();
    let project = fixture.root.join("project");
    let member_lock = project.join("member/Cargo.lock");
    fs::copy(project.join("Cargo.lock"), &member_lock).unwrap();
    fixture.set("PROJECT", &project.join("member").display().to_string());
    let original = original_project_files(&fixture);
    let member_bytes = fs::read(&member_lock).unwrap();
    let result = fixture.run("ci", "features");
    succeeds(&result);
    assert_project_files(&original);
    assert_eq!(fs::read(&member_lock).unwrap(), member_bytes);
}

#[test]
fn missing_locks_fail_clearly_except_for_featureless_workspaces() {
    for has_feature in [false, true] {
        let fixture = dev_dependency_workspace();
        let project = fixture.root.join("project");
        if !has_feature {
            let root = project.join("Cargo.toml");
            let text = fs::read_to_string(&root).unwrap();
            fs::write(root, text.replace("[features]\nalpha = []\n", "")).unwrap();
            succeeds(&fixture.run_body(
                "cd \"$PROJECT\"; cargo metadata --format-version 1 --offline \
                 > \"$RUNNER_TEMP/metadata.json\"",
            ));
        }
        let original: Vec<_> = ["Cargo.toml", "member/Cargo.toml"]
            .iter()
            .map(|name| {
                let path = project.join(name);
                let bytes = fs::read(&path).unwrap();
                (path, bytes)
            })
            .collect();
        fs::remove_file(project.join("Cargo.lock")).unwrap();
        let result = fixture.run("ci", "features");
        if has_feature {
            refused(&result, "cannot read ");
            assert!(String::from_utf8_lossy(&result.stderr).contains("Cargo.lock"));
        } else {
            succeeds(&result);
            assert!(
                fs::read_to_string(fixture.root.join("output"))
                    .unwrap()
                    .contains("applied=false")
            );
        }
        assert!(!project.join("Cargo.lock").exists());
        assert_project_files(&original);
    }
}

#[test]
fn lock_formats_and_present_identity_fields_are_validated() {
    let fixture = dev_dependency_workspace();
    let project = fixture.root.join("project");
    let path = project.join("Cargo.lock");
    for (before, after, should_pass) in [
        (
            "name = 'shared'\nversion = '0.1.0'",
            "name = 'shared'\nversion = '0.1.0'",
            true,
        ),
        (
            "name = 'shared'\nversion = '0.1.0'\n\
             source = 'git+file:///fixture#1111111111111111111111111111111111111111'",
            "name = 'shared'\nversion = '0.1.0'\n\
             source = 'git+file:///fixture#2222222222222222222222222222222222222222'",
            false,
        ),
        (
            "name = 'shared'\nversion = '0.1.0'\nchecksum = 'one'",
            "name = 'shared'\nversion = '0.1.0'\nchecksum = 'two'",
            false,
        ),
        (
            "name = 'shared'\nversion = '0.1.0'",
            "name = 'shared'\nversion = '0.1.0'\nchecksum = ''",
            false,
        ),
    ] {
        fs::write(
            &path,
            format!("# original format\nversion = 3\n[[package]]\n{before}\n"),
        )
        .unwrap();
        let original = original_project_files(&fixture);
        stub_cargo_hack(
            &fixture,
            &format!("printf '%s\\n' \"version = 4\n[[package]]\n{after}\" > Cargo.lock"),
        );
        let result = fixture.run("ci", "features");
        if should_pass {
            succeeds(&result);
        } else {
            refused(&result, "feature check changed locked package: shared");
        }
        assert_project_files(&original);
        let output = fixture.root.join("output");
        if output.exists() {
            fs::remove_file(output).unwrap();
        }
    }
}

#[test]
fn restore_errors_still_attempt_all_remaining_workspace_files() {
    let fixture = dev_dependency_workspace();
    let original = original_project_files(&fixture);
    stub_cargo_hack(
        &fixture,
        "chmod a-w Cargo.toml; printf '%s\\n' '# Modified member' > member/Cargo.toml; \
         printf '%s\\n' 'version = 4' 'package = []' > Cargo.lock; exit 73",
    );
    refused(&fixture.run("ci", "features"), "cannot restore ");
    assert_project_files(&original);
}

#[test]
fn the_example_replay_includes_the_feature_gate() {
    assert!(GATE_STEPS.contains(&"features"));
}

#[test]
fn real_features_reject_broken_isolated_and_combined_builds() {
    let mut fixture = Fixture::new();
    fixture.set("CARGO_NET_OFFLINE", "true");
    fs::write(
        fixture.root.join("project/Cargo.toml"),
        "[package]\nname = 'features-fixture'\nversion = '0.1.0'\nedition = '2024'\n\
         rust-version = '1.85'\n[features]\ndefault = ['alpha']\nalpha = []\nbeta = []\n",
    )
    .unwrap();
    succeeds(&fixture.run_body(
        "cd \"$PROJECT\"; cargo metadata --format-version 1 --offline \
         > \"$RUNNER_TEMP/metadata.json\"",
    ));
    let result = fixture.run("ci", "features");
    succeeds(&result);
    let log = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    for selection in [
        "--all-features",
        "--no-default-features",
        "--features alpha",
        "--features beta",
        "--features default",
    ] {
        assert!(log.contains(selection), "{selection}: {log}");
    }
    assert_eq!(
        fs::read_to_string(fixture.root.join("reports/features.txt")).unwrap(),
        "alpha\nbeta\ndefault"
    );
    assert!(
        fs::read_to_string(fixture.root.join("output"))
            .unwrap()
            .contains("applied=true\n")
    );
    fs::remove_file(fixture.root.join("output")).unwrap();
    for condition in [
        "all(feature = \"alpha\", feature = \"beta\")",
        "all(feature = \"beta\", not(feature = \"alpha\"))",
    ] {
        fs::write(
            fixture.root.join("project/src/lib.rs"),
            format!("#[cfg({condition})]\ncompile_error!(\"invalid_feature_selection\");\n"),
        )
        .unwrap();
        let result = fixture.run("ci", "features");
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("invalid_feature_selection"));
        assert!(!fixture.root.join("output").exists());
    }
}

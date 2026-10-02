//! The fixture: one temporary checkout, one environment table, and a step run
//! against stand-ins the way a hosted runner would run it.

use super::gate_declarations::gate_bin;
use super::repository::{root, tool, toolbelt_path, write_executable};
use super::workflow_yaml::step;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Every step outcome the scorecard reads: what a run hands it.
pub(crate) const SCORECARD_OUTCOMES: [&str; 20] = [
    "OUT_QUALITY",
    "OUT_COVERAGE",
    "OUT_AUDIT",
    "OUT_SECRETS",
    "OUT_MSRV",
    "OUT_FEATURES",
    "OUT_LICENCES",
    "OUT_MUTANTS",
    "OUT_UNUSED",
    "OUT_STAGE",
    "OUT_API",
    "OUT_ARCHITECTURE",
    "OUT_HYGIENE",
    "OUT_MANAGED_FILES",
    "OUT_HOOKS",
    "OUT_DUPLICATION",
    "OUT_CHANGED_COVERAGE",
    "OUT_PULL_REQUEST",
    "OUT_VET",
    "OUT_PERFORMANCE",
];

pub(crate) struct Fixture {
    pub(crate) root: PathBuf,
    pub(crate) env: BTreeMap<String, String>,
}

impl Fixture {
    pub(crate) fn new() -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let path = env::temp_dir().join(format!(
            "rust-workflows-{}-{}",
            process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::create_dir(path.join("project")).unwrap();
        fs::create_dir(path.join("bin")).unwrap();
        // Where every step writes its report; the tools step creates it in a run.
        fs::create_dir(path.join("reports")).unwrap();
        fs::write(
            path.join("project/Cargo.toml"),
            "[package]\nname=\"fixture\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
        )
        .unwrap();
        // A real Cargo package always has a target. Without one, `cargo metadata`
        // and any SBOM generator refuse the manifest, and the fixture would be
        // exercising a shape no consumer can actually have.
        fs::create_dir(path.join("project/src")).unwrap();
        fs::write(path.join("project/src/lib.rs"), "//! Fixture crate.\n").unwrap();
        fs::write(path.join("project/Cargo.lock"), "version = 4\n").unwrap();
        fs::write(
            path.join("project/rust-toolchain.toml"),
            "[toolchain]\nchannel=\"1.98.1\"\n",
        )
        .unwrap();
        let mut fixture = Self {
            root: path,
            env: BTreeMap::new(),
        };
        for (key, value) in [
            // A caller's run; a run no workflow called reads `[ci]` instead.
            ("CALLED", "true"),
            ("DIRECTORY", "project"),
            ("RUSTUP_TOOLCHAIN", "1.98.1"),
            // What `install-tools` and `verify-payload` read.
            ("TOOLS", ""),
            ("REQUIRE_BINARIES", "false"),
            ("GITHUB_BASE_REF", ""),
            ("LICENSE_ALLOWLIST", ""),
            ("ARTIFACT_KEY", "test"),
            ("COVERAGE", "90"),
            ("LICENSE_POLICY", "auto"),
            ("MUTATION_TEST", "false"),
            ("INTERNAL_SHARD_SELFTEST", "false"),
            ("MUTATION_SHARDS", "1"),
            ("MUTATION_MUTANTS_PER_SHARD", "50"),
            ("MUTATION_WINDOWS", "[]"),
            ("MUTATION_ENGINE_FEATURES", "[]"),
            ("MUTATION_ENGINE_FILES", "[]"),
            ("MUTATION_ENGINE_COUNT", "0"),
            ("MUTATION_ENGINE_DEFAULT_COUNT", "0"),
            ("ENGINE_DEFAULT_MUTATIONS_RESULT", "skipped"),
            ("ENGINE_MUTATIONS_RESULT", "skipped"),
            ("WINDOWS_MUTATIONS_RESULT", "skipped"),
            ("API_COMPATIBILITY", "false"),
            ("PLATFORMS", ""),
            ("SARIF_REPORTS", "false"),
            ("UNUSED_DEPENDENCIES", "false"),
            ("UNSAFE_POLICY", "allow"),
            ("DEPENDENCY_AUDIT", "false"),
            ("CLIPPY_LEVEL", "default"),
            ("GITHUB_RUN_ID", "123"),
            ("GITHUB_RUN_ATTEMPT", "1"),
            // GitHub always sets this; a step reading it under `set -u` is fine in
            // production but needs it here, or the fixture fails for a reason the
            // workflow does not have.
            ("GITHUB_REF_TYPE", "branch"),
            (
                "GITHUB_REPOSITORY",
                "Orchestration-Maestro/maestro-rust-workflows",
            ),
            ("GITHUB_SERVER_URL", "https://github.com"),
            ("DRY_RUN", "true"),
            ("EVENT", "pull_request"),
            ("REF", "refs/pull/1/merge"),
            ("PROTECTED", "false"),
            ("PACKAGE", "fixture"),
            ("TOKEN", ""),
        ] {
            fixture.set(key, value);
        }
        fixture.set("GITHUB_SHA", &"a".repeat(40));
        for (key, file) in [
            ("GITHUB_WORKSPACE", ""),
            ("RUNNER_TEMP", ""),
            ("GITHUB_OUTPUT", "output"),
            ("GITHUB_ENV", "environment"),
            ("GITHUB_STEP_SUMMARY", "summary"),
            ("GITHUB_PATH", "path"),
            ("PROJECT", "project"),
            ("REPORTS", "reports"),
            ("CALLS", "calls"),
            ("RUST_GATE_TRACE", "trace"),
        ] {
            fixture.set(key, &fixture.root.join(file).display().to_string());
        }
        fixture.set(
            "PATH",
            &format!(
                "{}:{}:{}",
                fixture.root.join("bin").display(),
                gate_bin().display(),
                toolbelt_path()
            ),
        );
        fixture
    }

    /// The real adapter: the same environment table as `new()`, but the project
    /// is a copy of one owned fixture, no command is stubbed, and the owned
    /// floors apply: the owned coverage floor is 90 rather than the public 80,
    /// and every optional gate that can run offline is on.
    pub(crate) fn example(name: &str) -> Self {
        let mut fixture = Self::new();
        let project = fixture.root.join("project");
        fs::remove_dir_all(&project).unwrap();
        copy_tree(&root().join("examples").join(name), &project);
        let run_git = |arguments: &[&str]| {
            let output = tool("git")
                .args(arguments)
                .current_dir(&project)
                .output()
                .unwrap();
            succeeds(&output);
        };
        run_git(&["init", "--quiet"]);
        run_git(&["config", "user.name", "Fixture"]);
        run_git(&["config", "user.email", "fixture@example.invalid"]);
        run_git(&["commit", "--allow-empty", "--quiet", "-m", "base"]);
        run_git(&["add", "--all"]);
        run_git(&["commit", "--quiet", "-m", "fixture sources"]);
        for (key, value) in [
            ("CARGO_BUILD_TARGET", "x86_64-unknown-linux-gnu"),
            ("COVERAGE", "90"),
            ("UNSAFE_POLICY", "deny"),
            ("MUTATION_TEST", "true"),
            ("UNUSED_DEPENDENCIES", "true"),
        ] {
            fixture.set(key, value);
        }
        if name == "workspace" {
            fixture.set("MUTATION_WINDOWS", "[\"core/src/windows.rs\"]");
        }
        fixture.set(
            "CARGO_TARGET_DIR",
            &fixture.root.join("target").display().to_string(),
        );
        fixture
    }

    /// A fixture whose project is a Cargo package on the organization's
    /// edition and not published, holding `files`: each a path under the
    /// project and its source. What the source rules read, with only the rule
    /// under test left to speak.
    pub(crate) fn with_sources(files: &[(&str, &str)]) -> Self {
        let fixture = Self::new();
        let project = fixture.root.join("project");
        fs::write(
            project.join("Cargo.toml"),
            concat!(
                "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
                "edition = \"2024\"\npublish = false\n",
            ),
        )
        .unwrap();
        // The organization's lints, LNT-001, written the way a repository gets
        // them: by the gate's own command. The gate hands Clippy the
        // organization's thresholds itself.
        let written = tool("rust-gate")
            .args(["lints", "--write"])
            .env(
                "PATH",
                format!("{}:{}", gate_bin().display(), toolbelt_path()),
            )
            .current_dir(&project)
            .output()
            .unwrap();
        succeeds(&written);
        for (path, source) in files {
            let file = project.join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, source).unwrap();
        }
        fixture
    }

    /// Point the toolbelt's cache into the fixture and stand in for the mise
    /// `scripts/bootstrap.sh` pins, already fetched there: it records each call
    /// and `mise bin-paths` prints `bin_paths`, the directories mise installed
    /// the tools in. The cache it returns is what `rust-gate setup` writes.
    pub(crate) fn cached_mise(&mut self, bin_paths: &str) -> PathBuf {
        let cache = self.root.join("cache");
        self.set("XDG_CACHE_HOME", &cache.display().to_string());
        let bootstrap = fs::read_to_string(root().join("scripts/bootstrap.sh")).unwrap();
        let version = bootstrap
            .lines()
            .find_map(|line| line.strip_prefix("readonly MISE_VERSION=v"))
            .unwrap();
        let tools = cache.join("maestro/tools");
        let mise = tools.join("mise").join(version);
        fs::create_dir_all(&mise).unwrap();
        write_executable(
            &mise.join("mise"),
            &format!(
                "#!/bin/bash\nset -euo pipefail\nprintf '%s\\n' mise \"$*\" >> \"$CALLS\"\n\
                 if [[ \"$1\" == bin-paths ]]; then printf '%s\\n' {bin_paths}; fi\n"
            ),
        );
        tools
    }

    /// Write the organization's lints into the project's manifest, as a
    /// repository does after writing a manifest of its own.
    pub(crate) fn write_lints(&self) {
        succeeds(&self.run_body("cd project && rust-gate lints --write"));
    }

    pub(crate) fn set(&mut self, key: &str, value: &str) {
        self.env.insert(key.to_owned(), value.to_owned());
    }

    /// Run one step body with exactly the fixture's environment table on top of
    /// a bare login environment, the way a hosted runner does. The test process
    /// itself is a child of `cargo test`, which injects `CARGO_PKG_*` and other
    /// variables of its own; leaked into a body, `CARGO_PKG_NAME` alone makes
    /// cargo-machete mistake its subcommand name for a path.
    pub(crate) fn run(&self, name: &str, id: &str) -> Output {
        self.run_body(&step(name, id))
    }

    pub(crate) fn run_body(&self, body: &str) -> Output {
        let mut command = Command::new("bash");
        command.env_clear();
        for key in [
            "HOME",
            "USER",
            "LANG",
            "TERM",
            "TMPDIR",
            "RUSTUP_HOME",
            "CARGO_HOME",
        ] {
            if let Ok(value) = env::var(key) {
                command.env(key, value);
            }
        }
        command
            .args(["-c", body])
            .current_dir(&self.root)
            .envs(&self.env)
            .output()
            .unwrap()
    }

    pub(crate) fn stub(&self, command: &str, body: &str) {
        write_executable(
            &self.root.join("bin").join(command),
            &format!(
                "#!/bin/bash\nset -euo pipefail\n\
                 printf '%s\\n' '{command}' \"$*\" >> \"$CALLS\"\n{body}\n"
            ),
        );
    }

    /// Every command the gate ran in this fixture, one line each, environment
    /// first: the plan the tests read instead of the gate's source.
    pub(crate) fn trace(&self) -> String {
        fs::read_to_string(self.root.join("trace")).unwrap_or_default()
    }

    pub(crate) fn calls(&self) -> String {
        fs::read_to_string(self.root.join("calls")).unwrap_or_default()
    }

    pub(crate) fn trusted(&mut self) {
        for (key, value) in [
            ("DRY_RUN", "false"),
            ("EVENT", "push"),
            ("REF", "refs/tags/v0.1.0"),
            ("PROTECTED", "true"),
            (
                "ENVIRONMENT_JSON",
                r#"{"protection_rules":[{"type":"required_reviewers",
                    "reviewers":[{"type":"User","reviewer":{"id":42}}]}],
                    "deployment_branch_policy":{"protected_branches":false,
                    "custom_branch_policies":true}}"#,
            ),
            (
                "POLICIES_JSON",
                r#"{"total_count":1,"branch_policies":[{"type":"tag","name":"v*"}]}"#,
            ),
            ("RELEASE_JSON", r#"{"tagName":"v0.1.0","assets":[]}"#),
            ("UPLOAD_STATUS", "0"),
            ("VIEW_STATUS", "0"),
            ("POLICIES_STATUS", "0"),
            ("COMMIT_STATUS", "0"),
        ] {
            self.set(key, value);
        }
        self.set("REVISION", &"a".repeat(40));
        self.set(
            "COMMIT_JSON",
            &format!("{{\"sha\":\"{}\"}}", "a".repeat(40)),
        );
        self.stub(
            "gh",
            r#"case "$1 $2" in
  'api '*'/environments/release') printf '%s\n' "$ENVIRONMENT_JSON" ;;
  'api '*'/deployment-branch-policies?per_page=100')
    printf '%s\n' "$POLICIES_JSON"; exit "$POLICIES_STATUS" ;;
  'api '*'/commits/refs%2Ftags%2Fv0.1.0') printf '%s\n' "$COMMIT_JSON"; exit "$COMMIT_STATUS" ;;
  'release view') printf '%s\n' "$RELEASE_JSON"; exit "$VIEW_STATUS" ;;
  'release upload') exit "$UPLOAD_STATUS" ;;
  *) exit 88 ;;
esac"#,
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

/// Copy a fixture into the temporary root, leaving local build output and
/// generated SBOMs behind.
fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "target" || name.ends_with(".cdx.json") || name.contains(":Zone.Identifier") {
            continue;
        }
        let target = to.join(&name);
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A step body that pipes through `tee` reports on stdout, so a failure shows
/// both streams.
/// The step refused, and said why with `message` on stderr.
pub(crate) fn refused(output: &Output, message: &str) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "the step did not refuse: {stderr}"
    );
    assert!(
        stderr.contains(message),
        "expected {message:?} in: {stderr}"
    );
}

pub(crate) fn succeeds(output: &Output) {
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

pub(crate) fn checksums(release: &Path) {
    let output = Command::new("sha256sum")
        .args(["payload.tar.gz", "provenance.json"])
        .current_dir(release)
        .output()
        .unwrap();
    succeeds(&output);
    fs::write(release.join("SHA256SUMS"), output.stdout).unwrap();
}

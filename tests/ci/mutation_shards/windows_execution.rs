//! The Windows mutation step guards configured files and runs exact diff scopes.

use crate::harness::{Fixture, refused, succeeds};
use std::fs;

fn windows_fixture(touched: &str) -> Fixture {
    let mut fixture = Fixture::new();
    let project = fixture.root.join("project/examples/workspace");
    fs::create_dir_all(project.join("core/src")).unwrap();
    fs::write(
        project.join("Cargo.toml"),
        "[package]\nname=\"fixture\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    fs::write(project.join("Cargo.lock"), "version = 4\n").unwrap();
    fs::write(
        project.join("core/src/windows.rs"),
        "pub fn value() -> bool { true }\n",
    )
    .unwrap();
    fixture.set("PROJECT", &project.display().to_string());
    fixture.set("MUTATION_WINDOWS", "[\"core/src/windows.rs\"]");
    fixture.set("MUTATION_TEST", "true");
    fixture.set("CARGO_MUTANTS_VERSION", "27.1.0");
    fixture.set("GITHUB_BASE_REF", "main");
    fixture.stub(
        "git",
        &format!(
            concat!(
                "case \"$*\" in\n",
                "  'rev-parse --verify -q HEAD^1') echo {} ;;\n",
                "  'diff --relative HEAD^1 HEAD -- .') ;;\n",
                "  *'-c core.quotePath=false diff --relative --name-only ",
                "HEAD^1 HEAD -- .') printf '%s\\n' '{}' ;;\n",
                "  *) exit 1 ;;\n",
                "esac"
            ),
            "a".repeat(40),
            touched
        ),
    );
    fixture
}

#[test]
fn windows_mutation_refuses_a_touched_file_with_no_mutants() {
    let fixture = windows_fixture("core/src/windows.rs");
    fixture.stub("cargo", "printf '\\n'");
    refused(
        &fixture.run_body("rust-gate mutants-windows"),
        "core/src/windows.rs produced no mutants",
    );
}

#[test]
fn windows_mutation_refuses_an_invalid_listing_count_by_name() {
    let fixture = windows_fixture("core/src/windows.rs");
    fixture.stub("cargo", "printf '[{}]\\n'");
    fixture.stub(
        "jaq",
        concat!(
            "if [[ \"$1\" == -nr ]]; then ",
            "printf 'core/src/windows.rs\\n'; else printf 'invalid\\n'; fi"
        ),
    );
    refused(
        &fixture.run_body("rust-gate mutants-windows"),
        "cargo-mutants listing count is invalid",
    );
}

#[test]
fn windows_mutation_accepts_no_changed_owned_files() {
    let fixture = windows_fixture("");
    fixture.stub("cargo", "exit 1");
    succeeds(&fixture.run_body("rust-gate mutants-windows"));
    let calls = fixture.calls();
    assert!(
        calls.contains("core.quotePath=false diff --relative --name-only"),
        "{calls}"
    );
    assert!(!calls.contains("cargo"), "{calls}");
}

#[test]
fn windows_mutation_runs_subdirectory_files_with_the_changed_line_scope() {
    let fixture = windows_fixture("core/src/windows.rs");
    fixture.stub(
        "cargo",
        concat!(
            "if [[ \"$*\" == *--list* ]]; then\n",
            "  count=$(grep -c -- '--list' \"$CALLS\")\n",
            "  if [[ $count == 1 ]]; then printf '[{}]\\n'; ",
            "else printf '\\n'; fi\n",
            "else\n",
            "  while [[ $# -gt 0 ]]; do ",
            "if [[ $1 == --output ]]; then out=$2; shift; fi; shift; done\n",
            "  mkdir -p \"$out/mutants.out\"\n",
            "  printf '%s' '{\"caught\":0,\"missed\":0,",
            "\"timeout\":0,\"unviable\":0}' > ",
            "\"$out/mutants.out/outcomes.json\"\n",
            "fi"
        ),
    );
    let output = fixture.run_body("rust-gate mutants-windows");
    succeeds(&output);
    let calls = fixture.calls();
    assert!(
        calls.contains("core.quotePath=false diff --relative --name-only HEAD^1 HEAD -- ."),
        "{calls}"
    );
    assert!(calls.contains("--file core/src/windows.rs --in-diff"));
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("SKIPPED: no mutants in the changed lines of core/src/windows.rs")
    );
}

//! Shared opted-in native cache and observed coverage child fixtures.

use super::fixture::Fixture;
use std::fs;

/// Opted-in policy with stable key inputs.
pub(crate) fn cache_fixture() -> Fixture {
    let mut fixture = Fixture::new();
    fs::write(
        fixture.root.join("project/maestro-quality.toml"),
        concat!(
            "[native-cache]\nenvironment='FIXTURE_NATIVE_CACHE_DIR'\n",
            "platforms=['linux','macos']\nkey-files=['Cargo.lock']\npublished=['entry-*']\n"
        ),
    )
    .unwrap();
    fixture.set("COVERAGE_FEATURES", "[\"fixture/engine\"]");
    fixture.set("NATIVE_CACHE_MODE", "coverage");
    fixture.set("NATIVE_CACHE_OS", "linux");
    fixture.set("NATIVE_CACHE_ARCH", "X64");
    fixture
}

/// Stub Cargo output while keeping selection, root checks and child environments real.
pub(crate) fn coverage_child_fixture() -> Fixture {
    let fixture = cache_fixture();
    fixture.stub(
        "git",
        "[[ $1 != rev-parse ]] || printf '%s' \"$GITHUB_SHA\"; exit 0",
    );
    fixture.stub(
        "cargo",
        concat!(
            "printf '%s %s\\n' \"$*\" \"${FIXTURE_NATIVE_CACHE_DIR:-unset}\" ",
            ">> \"$REPORTS/all-child-env\"\n",
            "if [[ $1 == metadata ]]; then printf '%s' '",
            "{\"workspace_members\":[\"a\"],\"packages\":[",
            "{\"id\":\"a\",\"name\":\"fixture\",\"features\":{\"engine\":[]}}]}'; fi\n",
            "if [[ $* == *--no-report* ]]; then printf '%s %s\\n' \"$*\" ",
            "\"${FIXTURE_NATIVE_CACHE_DIR:-unset}\" >> \"$RUNNER_TEMP/child-env\"; fi\n",
            "while [[ $# -gt 0 ]]; do ",
            "if [[ $1 == --output-path ]]; then echo LCOV > \"$2\"; fi; shift; done"
        ),
    );
    fixture
}

//! Shared real A/B/C source and Git setup for mode and input-only ownership regressions.

use super::fixture::succeeds;
use super::repository::tool;
use std::fs;
use std::path::Path;

/// Run real Git, never a fixture stand-in, and return its trimmed output.
pub(crate) fn fixture_git(project: &Path, args: &[&str]) -> String {
    let result = tool("git")
        .args(args)
        .current_dir(project)
        .output()
        .unwrap();
    succeeds(&result);
    String::from_utf8(result.stdout).unwrap().trim().to_owned()
}

/// A and B declare/forward engine; C is unrelated and declares no features.
pub(crate) fn engine_workspace(project: &Path, killing: bool) {
    fs::write(
        project.join("Cargo.toml"),
        "[workspace]\nmembers=['crates/a','crates/b','crates/c']\nresolver='3'\n",
    )
    .unwrap();
    for (package, extras) in [
        ("a", "[features]\nengine=[]\n"),
        (
            "b",
            "[features]\nengine=['crate-a/engine']\n[dependencies]\ncrate-a={path='../a'}\n",
        ),
        ("c", ""),
    ] {
        let root = project.join("crates").join(package);
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            format!("[package]\nname='crate-{package}'\nversion='0.1.0'\nedition='2024'\n{extras}"),
        )
        .unwrap();
        let module = if package == "a" {
            "#[cfg(feature=\"engine\")] pub mod engine;\n"
        } else {
            ""
        };
        fs::write(
            root.join("src/lib.rs"),
            format!(
                concat!(
                    "//! Feature-partition regression.\n{}pub fn answer() -> u8 {{ 7 }}\n",
                    "#[cfg(test)] mod tests {{ #[test] fn answer_is_seven() {{ ",
                    "assert_eq!(super::answer(), 7); }} }}\n"
                ),
                module
            ),
        )
        .unwrap();
    }
    let assertion = if killing {
        "assert_eq!(super::engine_answer(), 9);"
    } else {
        ""
    };
    fs::write(
        project.join("crates/a/src/engine.rs"),
        format!(
            concat!(
                "//! Engine-only regression.\npub fn engine_answer() -> u8 {{ 9 }}\n",
                "#[cfg(test)] mod tests {{ #[test] fn engine_answer_is_nine() {{ {} }} }}\n"
            ),
            assertion
        ),
    )
    .unwrap();
    fs::write(
        project.join("maestro-quality.toml"),
        "[ci.mutation-engine]\nfeatures=['engine']\nfiles=['crates/a/src/engine.rs']\n",
    )
    .unwrap();
}

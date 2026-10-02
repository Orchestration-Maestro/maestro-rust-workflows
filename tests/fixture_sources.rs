//! Shared three-package workspace with an optional native publication consumer.

use std::fs;
use std::io;
use std::path::Path;

/// A and B declare/forward engine; C is unrelated and declares no features.
///
/// # Errors
/// Returns the filesystem error if a fixture file or directory cannot be written.
pub fn engine_workspace(project: &Path, killing: bool) -> io::Result<()> {
    fs::write(
        project.join("Cargo.toml"),
        concat!(
            "[workspace]\nmembers = ['crates/a', 'crates/b', 'crates/c']\n",
            "exclude = ['native']\nresolver = '3'\n"
        ),
    )?;
    for (package, extras) in [
        (
            "a",
            concat!(
                "[features]\nengine = ['dep:fixture-native']\n[dependencies]\n",
                "fixture-native = { path = '../../native', optional = true }\n"
            ),
        ),
        (
            "b",
            concat!(
                "[features]\nengine = ['crate-a/engine']\n[dependencies]\n",
                "crate-a = { path = '../a' }\n"
            ),
        ),
        ("c", ""),
    ] {
        let root = project.join("crates").join(package);
        fs::create_dir_all(root.join("src"))?;
        fs::write(
            root.join("Cargo.toml"),
            format!(
                "[package]\nname = 'crate-{package}'\nversion = '0.1.0'\nedition = '2024'\n{extras}"
            ),
        )?;
        let module = if package == "a" {
            "#[cfg(feature = \"engine\")]\npub mod engine;\n"
        } else {
            ""
        };
        fs::write(
            root.join("src/lib.rs"),
            format!(
                concat!(
                    "//! Feature-partition regression.\n{}pub fn answer() -> u8 {{\n    7\n}}\n",
                    "#[cfg(test)]\nmod tests {{\n    #[test]\n    fn answer_is_seven() {{\n",
                    "        assert_eq!(super::answer(), 7);\n    }}\n}}\n"
                ),
                module
            ),
        )?;
    }
    let native = project.join("native");
    fs::create_dir_all(native.join("src"))?;
    fs::write(
        native.join("Cargo.toml"),
        "[package]\nname = 'fixture-native'\nversion = '0.1.0'\nedition = '2024'\n",
    )?;
    fs::write(
        native.join("src/lib.rs"),
        "//! Native fixture.\ninclude!(concat!(env!(\"OUT_DIR\"), \"/native.rs\"));\n",
    )?;
    fs::write(
        native.join("build.rs"),
        include_str!("fixtures/native-consumer/build.rs.in"),
    )?;
    let assertion = if killing {
        "        assert_eq!(super::engine_answer(), 9);\n"
    } else {
        ""
    };
    fs::write(
        project.join("crates/a/src/engine.rs"),
        format!(
            concat!(
                "//! Engine-only regression.\npub fn engine_answer() -> u8 {{\n    9\n}}\n",
                "#[cfg(test)]\nmod tests {{\n    #[test]\n    fn engine_answer_is_nine() {{\n",
                "{}    }}\n}}\n"
            ),
            assertion
        ),
    )?;
    fs::write(
        project.join("maestro-quality.toml"),
        concat!(
            "[ci.mutation-engine]\nfeatures = ['engine']\nfiles = ['crates/a/src/engine.rs']\n",
            "[native-cache]\nenvironment = 'FIXTURE_NATIVE_CACHE_DIR'\n",
            "platforms = ['linux', 'macos']\nkey-files = ['Cargo.lock']\npublished = ['entry-*']\n"
        ),
    )?;
    Ok(())
}

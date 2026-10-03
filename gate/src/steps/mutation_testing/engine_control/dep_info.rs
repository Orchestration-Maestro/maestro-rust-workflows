//! Verify raw compiler records and translate rustc dependency paths.

use crate::runner::{Cmd, Failure};
use std::collections::BTreeSet;
use std::mem;
use std::path::{Component, Path, PathBuf};

/// Verify raw successful Cargo output and require dep-info for every emitted non-build-script unit.
pub(super) fn dep_paths(cargo: &Path) -> Result<Vec<String>, Failure> {
    Cmd::new("jaq -se")
        .arg(concat!(
            "([.[] | select(.reason == \"build-finished\")] | length == 1) and ",
            "([.[] | select(.reason == \"build-finished\")][0].success == true) and ",
            "any(.[]; .reason == \"compiler-artifact\") and ",
            "all(.[] | select(.reason == \"compiler-artifact\"); .fresh == false and ",
            "(.target.src_path | type == \"string\" and startswith(\"/\")) and ",
            "(.filenames | type == \"array\" and length > 0 and all(.[]; type == \"string\")))"
        ))
        .arg(cargo)
        .capture()
        .map_err(|_| "featureless build did not verify fresh compile membership")?;
    let paths = Cmd::new("jaq -sr")
        .arg(concat!(
            "[.[] | select(.reason == \"compiler-artifact\") | ",
            ".filenames[]] | unique | .[]"
        ))
        .arg(cargo)
        .capture()?;
    let mut deps = BTreeSet::new();
    for path in paths.lines() {
        let path = Path::new(path);
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or("featureless artifact filename is invalid")?;
        let script_stem;
        let stem = if stem == "build-script-build" {
            let hash = path
                .parent()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                .and_then(|name| name.rsplit_once('-'))
                .map(|(_, hash)| hash)
                .ok_or("featureless build-script artifact filename is invalid")?;
            script_stem = format!("build_script_build-{hash}");
            script_stem.as_str()
        } else {
            stem
        };
        let stem = match path.extension().and_then(|extension| extension.to_str()) {
            Some("rlib" | "rmeta" | "so" | "dylib" | "a") => {
                stem.strip_prefix("lib").unwrap_or(stem)
            }
            _ => stem,
        };
        deps.insert(
            path.with_file_name(format!("{stem}.d"))
                .to_string_lossy()
                .into_owned(),
        );
    }
    Ok(deps.into_iter().collect())
}

/// Parse rustc's first Make rule, including escaped spaces and line continuations.
pub(super) fn dependencies(text: &str) -> Result<Vec<String>, Failure> {
    let joined = text.replace("\\\n", "");
    let line = joined
        .lines()
        .next()
        .ok_or("featureless dep-info is empty")?;
    let (_, dependencies) = line
        .split_once(": ")
        .ok_or("featureless dep-info has no dependency rule")?;
    let mut words = Vec::new();
    let mut word = String::new();
    let mut escaped = false;
    for character in dependencies.chars() {
        if escaped {
            word.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character.is_whitespace() {
            if !word.is_empty() {
                words.push(mem::take(&mut word));
            }
        } else {
            word.push(character);
        }
    }
    if escaped {
        return Err("featureless dep-info has an incomplete escape".into());
    }
    if !word.is_empty() {
        words.push(word);
    }
    if words.is_empty() {
        return Err("featureless dep-info has no source dependencies".into());
    }
    Ok(words)
}

/// Rust path attributes can contain parent components; normalize before matching plan paths.
pub(super) fn normalized(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::{dep_paths, dependencies, normalized};
    use std::path::Path;
    use std::{env, fs, process};

    #[test]
    fn dep_info_escaping_and_phony_rules_preserve_exact_source_paths() {
        assert_eq!(
            dependencies("a: src/lib.rs src/a\\ b.rs \\\n src/c.rs\n\nsrc/lib.rs:\n").unwrap(),
            ["src/lib.rs", "src/a b.rs", "src/c.rs"]
        );
        for text in ["", "bad", "a: ", "a: src/unfinished\\"] {
            assert!(dependencies(text).is_err());
        }
    }

    #[test]
    fn cargo_artifact_names_select_library_test_and_build_script_dep_info() {
        let path = env::temp_dir().join(format!("control-artifacts-{}.json", process::id()));
        fs::write(&path, concat!(
            r#"{"reason":"compiler-artifact","fresh":false,"target":{"src_path":"/p/build.rs"},"#,
            r#""filenames":["/target/debug/build/probe-1234/build-script-build"]}"#, "\n",
            r#"{"reason":"compiler-artifact","fresh":false,"target":{"src_path":"/p/src/lib.rs"},"#,
            r#""filenames":["/target/debug/deps/libprobe-5678.rlib","#,
            r#""/target/debug/deps/libprobe-5678.rmeta","/target/debug/deps/probe-9012"]}"#, "\n",
            r#"{"reason":"build-finished","success":true}"#, "\n"
        )).unwrap();
        assert_eq!(
            dep_paths(&path).unwrap(),
            [
                "/target/debug/build/probe-1234/build_script_build-1234.d",
                "/target/debug/deps/probe-5678.d",
                "/target/debug/deps/probe-9012.d"
            ]
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn source_parent_components_do_not_hide_compiled_plan_paths() {
        assert_eq!(
            normalized(Path::new("/workspace/crates/a/src/../src/./engine.rs")),
            Path::new("/workspace/crates/a/src/engine.rs")
        );
    }
}

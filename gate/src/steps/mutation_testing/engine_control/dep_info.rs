//! Verify raw compiler records and translate rustc dependency paths.

use super::rustc_invocation::{dep_path, option, words};
use crate::runner::{Cmd, Failure};
use std::path::{Component, Path, PathBuf};
use std::{fs, mem};

/// Require a bijection between fresh Cargo units and exact verbose compiler invocations.
pub(super) fn dep_paths(root: &Path, workspace: &Path) -> Result<Vec<String>, Failure> {
    let cargo = root.join("cargo-build.json");
    Cmd::new("jaq -se")
        .arg(concat!(
            "([.[] | select(.reason == \"build-finished\")] | length == 1) and ",
            "([.[] | select(.reason == \"build-finished\")][0].success == true) and ",
            "any(.[]; .reason == \"compiler-artifact\") and ",
            "all(.[] | select(.reason == \"compiler-artifact\"); .fresh == false and ",
            "(.target.src_path | type == \"string\" and startswith(\"/\")) and ",
            "(.target.name | type == \"string\") and (.profile.test | type == \"boolean\") and ",
            "(.filenames | type == \"array\" and length > 0 and all(.[]; type == \"string\")))"
        ))
        .arg(&cargo)
        .capture()
        .map_err(|_| "featureless build did not verify fresh compile membership")?;
    let log = fs::read_to_string(root.join("cargo-build.log"))
        .map_err(|error| format!("cannot read featureless compiler log: {error}"))?;
    let mut units = Vec::new();
    for line in log.lines() {
        let Some(command) = line.trim().strip_prefix("Running `") else {
            continue;
        };
        let command = command.strip_suffix('`').ok_or_else(|| {
            format!("featureless rustc invocation has malformed quoting: {command}")
        })?;
        let argv = words(command)?;
        // Build-script executions do not have rustc's crate-name option.
        if option(&argv, "--crate-name").is_none() {
            continue;
        }
        units.push(unit(&argv, workspace)?);
    }
    let value = Cmd::new("jaq -c")
        .args(["--slurpfile", "cargo"])
        .arg(&cargo)
        .stdin_bytes(format!("[{}]", units.join(",")).as_bytes())
        .arg(concat!(
            "def matches($a; $u): ",
            "($u.sources | index($a.target.src_path)) != null and ",
            "$u.name == ($a.target.name | gsub(\"-\";\"_\")) and $u.test == $a.profile.test and ",
            "any($a.filenames[]; . as $f | ",
            "$f == $u.base or ($f | startswith($u.base + \".\")) or ",
            "$f == $u.libbase or ($f | startswith($u.libbase + \".\")) or ",
            "($a.target.kind == [\"custom-build\"] and ",
            "$f == $u.directory + \"/build-script-build\") or ",
            "($a.target.kind == [\"bin\"] and $u.test == false and ",
            "($u.directory | endswith(\"/deps\")) and ",
            "$f == ($u.directory | rtrimstr(\"/deps\")) + \"/\" + $a.target.name)); ",
            ". as $units | [$cargo[] | select(.reason == \"compiler-artifact\")] as $artifacts | ",
            "{invalid:([$artifacts[] | . as $a | ",
            "select([$units[] | select(matches($a; .))] | length != 1) | .target.src_path] + ",
            "[$units[] | . as $u | select([$artifacts[] | select(matches(.; $u))] | ",
            "length != 1) | .path]), ",
            "paths:([$units[].path] | unique)}"
        ))
        .capture()?;
    let invalid = Cmd::new("jaq -r")
        .arg(".invalid[]")
        .stdin_bytes(value.as_bytes())
        .capture()?;
    if !invalid.trim().is_empty() {
        return Err(format!(
            "featureless compiler unit has no unique rustc invocation: {}",
            invalid.trim()
        )
        .into());
    }
    let paths = Cmd::new("jaq -r")
        .arg(".paths[]")
        .stdin_bytes(value.as_bytes())
        .capture()?;
    Ok(paths.lines().map(str::to_owned).collect())
}

/// Serialize the compiler's identity and filename alongside normalized source arguments.
fn unit(argv: &[String], workspace: &Path) -> Result<String, Failure> {
    let path = normalized(&workspace.join(dep_path(argv)?));
    let path = path.as_path();
    let directory = path.parent().unwrap_or(Path::new(""));
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let sources: Vec<_> = argv
        .iter()
        .map(|word| {
            normalized(&workspace.join(word))
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    Cmd::new("jaq -cn")
        .args([
            "--arg",
            "name",
            option(argv, "--crate-name").unwrap_or_default(),
        ])
        .args([
            "--argjson",
            "test",
            if argv.iter().any(|word| word == "--test") {
                "true"
            } else {
                "false"
            },
        ])
        .args(["--arg", "path"])
        .arg(path)
        .args(["--arg", "directory"])
        .arg(directory)
        .args(["--arg", "base"])
        .arg(directory.join(stem.as_ref()))
        .args(["--arg", "libbase"])
        .arg(directory.join(format!("lib{stem}")))
        .arg(concat!(
            "{name:$name,test:$test,path:$path,directory:$directory,",
            "base:$base,libbase:$libbase,sources:$ARGS.positional}",
        ))
        .args(["--args", "--"])
        .args(&sources)
        .capture()
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
    use super::{dependencies, normalized};
    use std::path::Path;

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
    fn source_parent_components_do_not_hide_compiled_plan_paths() {
        assert_eq!(
            normalized(Path::new("/workspace/crates/a/src/../src/./engine.rs")),
            Path::new("/workspace/crates/a/src/engine.rs")
        );
    }
}

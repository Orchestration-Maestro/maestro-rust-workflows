//! Validate the configured Windows-owned mutation paths before use.

use crate::runner::{Cmd, Failure};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path};

/// Read Windows-owned mutation paths from the validated JSON input.
pub(crate) fn mutation_windows(project: &Path, value: &str) -> Result<Vec<String>, Failure> {
    let listed = Cmd::new("jaq -nr")
        .env("MUTATION_WINDOWS", value)
        .arg(concat!(
            "$ENV.MUTATION_WINDOWS | fromjson | if type == \"array\" ",
            "and all(.[]; type == \"string\") then .[] else ",
            "error(\"expected an array of strings\") end"
        ))
        .capture()
        .map_err(|_| "MUTATION_WINDOWS must be a JSON array of file paths")?;
    let root =
        fs::canonicalize(project).map_err(|error| format!("{}: {error}", project.display()))?;
    let mut files = BTreeSet::new();
    for name in listed.lines() {
        let path = Path::new(name);
        if name.is_empty()
            || path.is_absolute()
            || path
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
            || !name.contains('/')
            || !matches!(
                path.extension().and_then(|extension| extension.to_str()),
                Some("rs")
            )
            || name.contains(['*', '?', '[', ']', '{', '}', '\\', '!'])
        {
            return Err(format!(
                "MUTATION_WINDOWS path `{name}` must be an exact relative file path"
            )
            .into());
        }
        let canonical = fs::canonicalize(project.join(path))
            .map_err(|_| format!("MUTATION_WINDOWS file `{name}` does not exist"))?;
        if !canonical.starts_with(&root) || !canonical.is_file() {
            return Err(format!(
                "MUTATION_WINDOWS file `{name}` must stay inside the Cargo workspace"
            )
            .into());
        }
        if !files.insert(name.to_owned()) {
            return Err(format!("MUTATION_WINDOWS file `{name}` is listed more than once").into());
        }
    }
    Ok(files.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::mutation_windows;
    #[cfg(unix)]
    use std::os::unix::fs as unix_fs;
    use std::{env, fs, process};

    #[test]
    fn mutation_windows_accepts_existing_exact_files_and_refuses_unsafe_paths() {
        let project = env::temp_dir().join(format!("mutation-windows-{}", process::id()));
        fs::remove_dir_all(&project).ok();
        fs::create_dir_all(project.join("src/windows-dir.rs")).unwrap();
        fs::create_dir_all(project.join("src")).unwrap();
        fs::write(project.join("src/windows.rs"), "").unwrap();
        assert_eq!(
            mutation_windows(&project, r#"["src/windows.rs"]"#).unwrap(),
            ["src/windows.rs"]
        );
        for (value, message) in [
            (
                "not-json",
                "MUTATION_WINDOWS must be a JSON array of file paths",
            ),
            (
                r#"["src/windows.rs","src/windows.rs"]"#,
                "MUTATION_WINDOWS file `src/windows.rs` is listed more than once",
            ),
            (
                r#"["src/windows-dir.rs"]"#,
                "MUTATION_WINDOWS file `src/windows-dir.rs` must stay inside the Cargo workspace",
            ),
            (
                r#"["src/*.rs"]"#,
                "MUTATION_WINDOWS path `src/*.rs` must be an exact relative file path",
            ),
            (
                r#"["windows.rs"]"#,
                "MUTATION_WINDOWS path `windows.rs` must be an exact relative file path",
            ),
            (
                r#"["src/{windows}.rs"]"#,
                "MUTATION_WINDOWS path `src/{windows}.rs` must be an exact relative file path",
            ),
            (
                r#"["src/a{b.rs"]"#,
                "MUTATION_WINDOWS path `src/a{b.rs` must be an exact relative file path",
            ),
            (
                r#"["src/a}b.rs"]"#,
                "MUTATION_WINDOWS path `src/a}b.rs` must be an exact relative file path",
            ),
            (
                r#"["src/windows.txt"]"#,
                "MUTATION_WINDOWS path `src/windows.txt` must be an exact relative file path",
            ),
            (
                r#"["src/!windows.rs"]"#,
                "MUTATION_WINDOWS path `src/!windows.rs` must be an exact relative file path",
            ),
            (
                r#"["src/windows\\file.rs"]"#,
                "MUTATION_WINDOWS path `src/windows\\file.rs` must be an exact relative file path",
            ),
            (
                r#"["missing/windows.rs"]"#,
                "MUTATION_WINDOWS file `missing/windows.rs` does not exist",
            ),
            (
                r#"["../outside.rs"]"#,
                "MUTATION_WINDOWS path `../outside.rs` must be an exact relative file path",
            ),
        ] {
            assert_eq!(
                mutation_windows(&project, value)
                    .unwrap_err()
                    .message
                    .as_deref(),
                Some(message)
            );
        }
        #[cfg(unix)]
        {
            let outside = project.with_extension("outside.rs");
            fs::write(&outside, "").unwrap();
            unix_fs::symlink(&outside, project.join("src/outside.rs")).unwrap();
            assert_eq!(
                mutation_windows(&project, r#"["src/outside.rs"]"#)
                    .unwrap_err()
                    .message
                    .as_deref(),
                Some("MUTATION_WINDOWS file `src/outside.rs` must stay inside the Cargo workspace")
            );
            fs::remove_file(outside).unwrap();
        }
        fs::remove_dir_all(project).unwrap();
    }
}

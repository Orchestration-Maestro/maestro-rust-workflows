//! Validate package-local features and exact engine mutation ownership.

use crate::checks::mutation_windows::mutation_windows;
use crate::runner::{Cmd, Failure};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// One validated, package-local engine mutation policy.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct EnginePolicy {
    /// Cargo features enabled only in engine mutation workers.
    pub(crate) features: Vec<String>,
    /// Exact source files owned by engine mutation workers.
    pub(crate) files: Vec<String>,
}

/// Parse the policy JSON, validate ownership, and ensure each owner declares every feature.
pub(crate) fn engine_policy(
    project: &Path,
    value: &str,
    windows: &[String],
    metadata: impl FnOnce() -> Result<String, Failure>,
) -> Result<EnginePolicy, Failure> {
    refuse_global_mutation_features(project)?;
    if value.trim() == "{}" {
        return Ok(EnginePolicy {
            features: Vec::new(),
            files: Vec::new(),
        });
    }
    let exact_shape = Cmd::new("jaq -en")
        .env("MUTATION_ENGINE", value)
        .arg("$ENV.MUTATION_ENGINE | fromjson | keys | sort == [\"features\", \"files\"]")
        .capture();
    if exact_shape.is_err() {
        return Err("[ci.mutation-engine] must contain only features and files".into());
    }
    let features = strings(value, "features")?;
    if features.is_empty() {
        return Err("[ci.mutation-engine] features must not be empty".into());
    }
    let mut unique_features = BTreeSet::new();
    for feature in &features {
        if feature.is_empty()
            || !feature
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "_-".contains(character))
            || !unique_features.insert(feature)
        {
            return Err(format!(
                "[ci.mutation-engine] feature `{feature}` is invalid or duplicated"
            )
            .into());
        }
    }
    let files = strings(value, "files")?;
    if files.is_empty() {
        return Err("[ci.mutation-engine] files must not be empty".into());
    }
    let serialized = Cmd::new("jaq -cn")
        .arg("$ARGS.positional")
        .args(["--args"])
        .args(files.iter())
        .capture()?;
    let validated = mutation_windows(project, &serialized)?;
    let windows: BTreeSet<&str> = windows.iter().map(String::as_str).collect();
    for file in &validated {
        if windows.contains(file.as_str()) {
            return Err(
                format!("engine mutation file `{file}` also belongs to mutation-windows").into(),
            );
        }
    }
    declared_features(project, &validated, &features, metadata)?;
    Ok(EnginePolicy {
        features,
        files: validated,
    })
}

/// Refuse cargo-mutants config that enables features or workspace-wide tests globally.
fn refuse_global_mutation_features(project: &Path) -> Result<(), Failure> {
    let config = project.join(".cargo/mutants.toml");
    if !config.is_file() {
        return Ok(());
    }
    let forbidden = Cmd::new("jaq --from toml -r")
        .arg("has(\"features\") or has(\"test_workspace\")")
        .arg(&config)
        .capture()?;
    if forbidden.trim() == "true" {
        return Err(".cargo/mutants.toml must not set global features or test_workspace".into());
    }
    Ok(())
}

/// Preserve exact entries: the raw line transport cannot carry embedded CR or LF.
fn strings(value: &str, key: &str) -> Result<Vec<String>, Failure> {
    let listed = Cmd::new("jaq -nr")
        .env("MUTATION_ENGINE", value)
        .arg(concat!(
            "$ENV.MUTATION_ENGINE | fromjson | .[$ENV.MUTATION_KEY] | if type == \"array\" ",
            "and all(.[]; type == \"string\" and (contains(\"\\n\") | not) ",
            "and (contains(\"\\r\") | not)) then .[] else ",
            "error(\"expected an array of strings\") end"
        ))
        .env("MUTATION_KEY", key)
        .capture()
        .map_err(|_| format!("[ci.mutation-engine] {key} must be an array of strings"))?;
    Ok(listed.lines().map(str::to_owned).collect())
}

/// Require every package containing an owned file to declare all selected features.
fn declared_features(
    project: &Path,
    files: &[String],
    features: &[String],
    metadata: impl FnOnce() -> Result<String, Failure>,
) -> Result<(), Failure> {
    let metadata = metadata()?;
    let package_rows = Cmd::new("jaq -r")
        .arg(".packages[] | [.manifest_path, (.features | keys | join(\" \"))] | @tsv")
        .stdin_bytes(metadata.as_bytes())
        .capture()?;
    let packages = package_rows
        .lines()
        .filter_map(|row| {
            let (manifest, names) = row.split_once('\t')?;
            Some((
                PathBuf::from(manifest).parent()?.to_path_buf(),
                names.split_whitespace().collect::<BTreeSet<_>>(),
            ))
        })
        .collect::<Vec<_>>();
    for file in files {
        let path = project.join(file);
        let owner = packages
            .iter()
            .filter(|(root, _)| path.starts_with(root))
            .max_by_key(|(root, _)| root.components().count());
        let Some((_, declared)) = owner else {
            return Err(
                format!("engine mutation file `{file}` is outside every Cargo package").into(),
            );
        };
        for feature in features {
            if !declared.contains(feature.as_str()) {
                return Err(format!(
                    "Cargo package for `{file}` does not declare engine feature `{feature}`"
                )
                .into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::engine_policy as parse_policy;
    use crate::runner::{Cmd, Failure};
    use std::{env, fs, path::Path, process};

    /// Parse using the fixture workspace metadata.
    fn parse(
        project: &Path,
        value: &str,
        windows: &[String],
    ) -> Result<super::EnginePolicy, Failure> {
        parse_policy(project, value, windows, || {
            Cmd::new("cargo metadata --format-version 1 --no-deps")
                .cwd(project)
                .capture()
        })
    }

    #[test]
    fn engine_policy_refuses_empty_and_invalid_feature_selections() {
        let root = env::temp_dir().join(format!("engine-policy-{}", process::id()));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/engine.rs"), "").unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname='fixture'\nversion='0.1.0'\nedition='2024'\n[features]\nengine=[]\n",
        )
        .unwrap();
        for (policy, message) in [
            (
                r#"{"features":["engine"],"files":[]}"#,
                "[ci.mutation-engine] files must not be empty",
            ),
            (
                r#"{"features":["engine"],"files":null}"#,
                "[ci.mutation-engine] files must be an array of strings",
            ),
            (
                r#"{"features":null,"files":["src/engine.rs"]}"#,
                "[ci.mutation-engine] features must be an array of strings",
            ),
            (
                r#"{"features":["engine"]}"#,
                "[ci.mutation-engine] must contain only features and files",
            ),
            (
                r#"{"features":[],"files":["src/engine.rs"]}"#,
                "[ci.mutation-engine] features must not be empty",
            ),
            (
                r#"{"features":["pkg/engine"],"files":["src/engine.rs"]}"#,
                "[ci.mutation-engine] feature `pkg/engine` is invalid or duplicated",
            ),
        ] {
            assert_eq!(
                parse(&root, policy, &[]).unwrap_err().message.as_deref(),
                Some(message)
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn engine_policy_validates_exact_files_features_and_windows_ownership() {
        let root = env::temp_dir().join(format!("engine-policy-owner-{}", process::id()));
        fs::create_dir_all(root.join("crates/a/src")).unwrap();
        fs::write(root.join("crates/a/src/engine.rs"), "").unwrap();
        fs::write(root.join("crates/a/src/default.rs"), "").unwrap();
        fs::write(root.join("crates/a/src/lib.rs"), "").unwrap();
        fs::write(
            root.join("crates/a/Cargo.toml"),
            "[package]\nname='fixture-a'\nversion='0.1.0'\nedition='2024'\n[features]\nengine=[]\n",
        )
        .unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers=['crates/a']\nresolver='3'\n",
        )
        .unwrap();
        let policy = r#"{"features":["engine"],"files":["crates/a/src/engine.rs"]}"#;
        assert_eq!(
            parse(&root, policy, &[]).unwrap(),
            super::EnginePolicy {
                features: vec!["engine".into()],
                files: vec!["crates/a/src/engine.rs".into()]
            }
        );
        for invalid in [
            policy.replace(r#""engine""#, r#""engine\n""#),
            policy.replace(r#""engine""#, r#""engine\r""#),
            policy.replace("engine.rs", r"engine.rs\n"),
            policy.replace("engine.rs", r"engine.rs\r"),
            policy.replace("engine.rs", r"engine.rs\ncrates/a/src/default.rs"),
        ] {
            assert!(parse(&root, &invalid, &[]).is_err(), "{invalid}");
        }
        assert_eq!(
            parse(&root, policy, &["crates/a/src/engine.rs".into()])
                .unwrap_err()
                .message
                .as_deref(),
            Some("engine mutation file `crates/a/src/engine.rs` also belongs to mutation-windows")
        );
        let outside = parse_policy(&root, policy, &[], || Ok("{\"packages\":[]}".into()));
        assert!(
            outside
                .unwrap_err()
                .message
                .unwrap()
                .contains("` is outside every Cargo package")
        );
        let unsupported = policy.replace("\"engine\"", "\"unknown\"");
        assert!(
            parse(&root, &unsupported, &[])
                .unwrap_err()
                .message
                .unwrap()
                .contains("` does not declare engine feature `unknown`")
        );
        let missing = policy.replace("engine.rs", "missing.rs");
        assert!(
            parse(&root, &missing, &[])
                .unwrap_err()
                .message
                .unwrap()
                .contains("does not exist")
        );
        let duplicate = concat!(
            r#"{"features":["engine"],"files":["crates/a/src/engine.rs", "#,
            r#""crates/a/src/engine.rs"]}"#
        );
        assert!(
            parse(&root, duplicate, &[])
                .unwrap_err()
                .message
                .unwrap()
                .contains("listed more than once")
        );
        fs::create_dir_all(root.join(".cargo")).unwrap();
        fs::write(
            root.join(".cargo/mutants.toml"),
            "features = [\"engine\"]\n",
        )
        .unwrap();
        assert_eq!(
            parse(&root, policy, &[]).unwrap_err().message.as_deref(),
            Some(".cargo/mutants.toml must not set global features or test_workspace")
        );
        fs::remove_dir_all(root).unwrap();
    }
}

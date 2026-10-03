//! Tested-head policy for exact provisioned-host mutation ownership.

use crate::checks::coverage_features::coverage_features;
use crate::checks::digests::sha256_hex;
use crate::runner::{Cmd, Failure};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path};

/// A nonempty ownership transfer and its bound provisioner.
#[derive(Debug)]
pub(crate) struct HostPolicy {
    /// Exact workspace-relative Rust files.
    pub(crate) files: Vec<String>,
    /// Qualified workspace features.
    pub(crate) features: Vec<String>,
    /// Tracked script, passed to bash as a path, not command text.
    pub(crate) provisioner: String,
    /// Normalized policy hash.
    pub(crate) digest: String,
    /// Tested-head script hash.
    pub(crate) provisioner_digest: String,
}

/// Read ownership from the tested checkout, including in called workflows.
pub(crate) fn host_policy(project: &Path) -> Result<Option<HostPolicy>, Failure> {
    let config = project.join("maestro-quality.toml");
    let text = if project.ancestors().any(|path| path.join(".git").exists()) {
        let listed = Cmd::new("git ls-tree --name-only HEAD -- maestro-quality.toml")
            .cwd(project)
            .capture()?;
        if listed.trim().is_empty() {
            return Ok(None);
        }
        let prefix = Cmd::new("git rev-parse --show-prefix")
            .cwd(project)
            .capture()?;
        Cmd::new("git show")
            .arg(format!("HEAD:{}maestro-quality.toml", prefix.trim()))
            .cwd(project)
            .capture()?
    } else {
        if !config.is_file() {
            return Ok(None);
        }
        fs::read_to_string(&config).map_err(|error| format!("cannot read host policy: {error}"))?
    };
    let present = Cmd::new("jaq --from toml -r")
        .arg(".ci // {} | has(\"mutation-provisioned-host\")")
        .stdin_bytes(text.as_bytes())
        .capture()?;
    if present.trim() != "true" {
        return Ok(None);
    }
    tracked_file(project, "maestro-quality.toml")?;
    let value = Cmd::new("jaq --from toml -c")
        .arg(".ci[\"mutation-provisioned-host\"]")
        .stdin_bytes(text.as_bytes())
        .capture()?;
    parse_policy(project, value.trim()).map(Some)
}

/// Validate the strict table, safe tracked paths and declared features once.
fn parse_policy(project: &Path, value: &str) -> Result<HostPolicy, Failure> {
    Cmd::new("jaq -en")
        .args(["--argjson", "policy", value])
        .arg(concat!(
            "$policy | type == \"object\" and (keys | sort) == ",
            "[\"features\",\"files\",\"provisioner\"] and ",
            "all(.files,.features; type == \"array\" and length > 0 and ",
            "all(.[]; type == \"string\" and (test(\"[\\r\\n\\t]\") | not))) and ",
            "(.provisioner | type == \"string\" and length > 0)"
        ))
        .capture()
        .map_err(|_| {
            "[ci.mutation-provisioned-host] requires only nonempty files, features \
        and provisioner"
        })?;
    let strings = |key: &str| -> Result<Vec<String>, Failure> {
        Ok(Cmd::new("jaq -nr")
            .args(["--argjson", "policy", value])
            .arg(format!("$policy.{key}[]"))
            .capture()?
            .lines()
            .map(str::to_owned)
            .collect())
    };
    let files = strings("files")?;
    let mut unique = BTreeSet::new();
    for file in &files {
        if Path::new(file).extension().and_then(|value| value.to_str()) != Some("rs")
            || !unique.insert(file)
        {
            return Err(format!("host mutation file `{file}` must be unique Rust source").into());
        }
        tracked_file(project, file)?;
        let source =
            fs::read_to_string(project.join(file)).map_err(|error| format!("{file}: {error}"))?;
        let attributes = source.split_whitespace().collect::<String>();
        if attributes.contains("mutants::skip") || attributes.contains("coverage(off)") {
            return Err(
                format!("host mutation file `{file}` disables mutation or coverage").into(),
            );
        }
    }
    let features_json = Cmd::new("jaq -cn")
        .args(["--argjson", "policy", value])
        .arg("$policy.features")
        .capture()?;
    let features = coverage_features(features_json.trim(), || {
        Cmd::new("cargo metadata --format-version 1 --no-deps --locked")
            .cwd(project)
            .capture()
    })?;
    let provisioner = Cmd::new("jaq -nr")
        .args(["--argjson", "policy", value])
        .arg("$policy.provisioner")
        .capture()?;
    let provisioner = provisioner.trim().to_owned();
    tracked_file(project, &provisioner)?;
    let bytes =
        fs::read(project.join(&provisioner)).map_err(|error| format!("{provisioner}: {error}"))?;
    Ok(HostPolicy {
        files,
        features,
        provisioner,
        digest: sha256_hex(value.as_bytes()),
        provisioner_digest: sha256_hex(&bytes),
    })
}

/// A lexical exact path, with no symlink component, must equal its tracked HEAD blob.
fn tracked_file(project: &Path, name: &str) -> Result<(), Failure> {
    let path = Path::new(name);
    if !exact_path(name) {
        return Err(
            format!("host policy path `{name}` must be an exact relative tracked file").into(),
        );
    }
    let mut current = project.to_path_buf();
    for component in path.components() {
        current.push(component);
        let metadata = fs::symlink_metadata(&current)
            .map_err(|error| format!("host policy path `{name}`: {error}"))?;
        if metadata.file_type().is_symlink() {
            return Err(format!("host policy path `{name}` must not contain symlinks").into());
        }
    }
    if !current.is_file() {
        return Err(format!("host policy path `{name}` must be a regular file").into());
    }
    let prefix = Cmd::new("git rev-parse --show-prefix")
        .cwd(project)
        .capture()?;
    let blob = Cmd::new("git show")
        .arg(format!("HEAD:{}{name}", prefix.trim()))
        .cwd(project)
        .capture_output()?;
    let bytes = fs::read(current).map_err(|error| format!("{name}: {error}"))?;
    if !blob.status.success() || blob.stdout != bytes {
        return Err(
            format!("host policy path `{name}` must match its tracked tested HEAD bytes").into(),
        );
    }
    Ok(())
}

/// Exact paths exclude shell/options syntax, escapes and control characters.
fn exact_path(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.contains(['*', '?', '[', ']', '{', '}', '\\', '!'])
        && !name.chars().any(char::is_control)
        && Path::new(name)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

/// Refuse disabled mutation and overlapping owners before any partition is removed.
pub(crate) fn validate_owners(
    policy: &HostPolicy,
    enabled: bool,
    windows: &[String],
    engine: &[String],
) -> Result<(), Failure> {
    if !enabled {
        return Err("[ci.mutation-provisioned-host] requires mutation-test=true".into());
    }
    for file in &policy.files {
        if windows.contains(file) || engine.contains(file) {
            return Err(format!(
                "host mutation file `{file}` overlaps Windows or engine ownership"
            )
            .into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::exact_path;

    #[test]
    fn host_paths_reject_globs_options_and_escape_components() {
        assert!(exact_path(".github/scripts/host.sh"));
        for path in [
            "",
            "../a.rs",
            "src/*.rs",
            "-src/a.rs",
            "src/a\n.rs",
            "src/../a.rs",
        ] {
            assert!(!exact_path(path), "{path}");
        }
    }
}

//! Strict policy-file-only native cache contract, independent of engine ownership.

use crate::checks::checkout_paths::{canonical, strictly_inside};
use crate::checks::digests::sha256_hex;
use crate::runner::{Cmd, Failure};
use std::fs;
use std::path::{Component, Path};

/// Validated transport settings. Absence disables transport.
pub(crate) struct NativeCache {
    /// Variable attached only to selected child commands.
    pub(crate) environment: String,
    /// Unix platforms allowed by the consumer.
    pub(crate) platforms: Vec<String>,
    /// Published root-child selectors, never staging or Cargo targets.
    pub(crate) published: Vec<String>,
    /// Digest of policy data, not unrelated source.
    pub(crate) policy_digest: String,
    /// Digest of key file names and bytes, including Cargo.lock.
    pub(crate) files_digest: String,
}

/// Read and validate the policy from the consumer project.
pub(crate) fn native_cache(project: &Path) -> Result<Option<NativeCache>, Failure> {
    let file = project.join("maestro-quality.toml");
    if !file.is_file() {
        return Ok(None);
    }
    let value = Cmd::new("jaq --from toml -c")
        .arg(".[\"native-cache\"] // null")
        .arg(&file)
        .capture()?;
    if value.trim() == "null" {
        return Ok(None);
    }
    let query = |program: &str| {
        Cmd::new("jaq -nr")
            .env("NATIVE_POLICY", value.trim())
            .arg(format!("$ENV.NATIVE_POLICY | fromjson | {program}"))
            .capture()
    };
    if query(concat!(
        "keys | sort | if . == [\"environment\",\"key-files\",\"platforms\",\"published\"] ",
        "then true else error(\"shape\") end"
    ))
    .is_err()
    {
        return Err(concat!(
            "[native-cache] must contain only environment, platforms, ",
            "key-files and published"
        )
        .into());
    }
    let environment = query(concat!(
        ".environment | if type == \"string\" and (contains(\"\\n\") | not) ",
        "and (contains(\"\\r\") | not) then . else error(\"name\") end"
    ))
    .map_err(|_| "[native-cache] environment is malformed or reserved")?;
    let environment = environment.trim_end_matches('\n').to_owned();
    if !environment_name(&environment) {
        return Err("[native-cache] environment is malformed or reserved".into());
    }
    let strings = |key: &str| -> Result<Vec<String>, Failure> {
        let listed = query(&format!(
            concat!(
                ".[\"{}\"] | if type == \"array\" and length > 0 and all(.[]; ",
                "type == \"string\" and length > 0 and (contains(\"\\n\") | not) ",
                "and (contains(\"\\r\") | not)) then .[] else error(\"strings\") end"
            ),
            key
        ))
        .map_err(|_| "[native-cache] lists must be nonempty arrays of strings")?;
        Ok(listed.lines().map(str::to_owned).collect())
    };
    let platforms = strings("platforms")?;
    if platforms.iter().any(|os| os != "linux" && os != "macos") {
        return Err("[native-cache] platforms must name linux or macos".into());
    }
    let published = strings("published")?;
    if published.iter().any(|name| !selector(name)) {
        return Err("[native-cache] published selectors must name root children".into());
    }
    let mut files = strings("key-files")?;
    if !files.iter().any(|name| name == "Cargo.lock") {
        files.push("Cargo.lock".to_owned());
    }
    files.sort();
    files.dedup();
    let mut bytes = Vec::new();
    for name in files {
        let path = Path::new(&name);
        if path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err("[native-cache] key file must exist inside the project".into());
        }
        let real = canonical(&project.join(path))
            .map_err(|_| "[native-cache] key file must exist inside the project")?;
        if !strictly_inside(&real, &canonical(project)?) || !real.is_file() {
            return Err("[native-cache] key file must exist inside the project".into());
        }
        bytes.extend_from_slice(name.as_bytes());
        bytes.push(0);
        let content = fs::read(real).map_err(|error| format!("native cache key file: {error}"))?;
        bytes.extend_from_slice(&(content.len() as u64).to_be_bytes());
        bytes.extend(content);
    }
    Ok(Some(NativeCache {
        environment,
        platforms,
        published,
        policy_digest: sha256_hex(value.trim().as_bytes()),
        files_digest: sha256_hex(&bytes),
    }))
}

/// Windows cannot opt into native caching, even with a selected engine feature.
pub(crate) fn cache_platform(os: &str, platforms: &[String]) -> bool {
    os != "windows" && platforms.iter().any(|platform| platform == os)
}

/// Refuse process controls, runner controls and gate inputs, not just shell injection.
fn environment_name(name: &str) -> bool {
    let reserved = [
        "RUSTC",
        "RUSTDOC",
        "RUSTFLAGS",
        "RUSTDOCFLAGS",
        "CC",
        "CXX",
        "AR",
        "PATH",
        "HOME",
        "SHELL",
        "ENV",
        "BASH_ENV",
        "IFS",
        "CDPATH",
        "TMPDIR",
        "TEMP",
        "TMP",
        "PROJECT",
        "REPORTS",
        "DIRECTORY",
        "COVERAGE",
        "COVERAGE_FEATURES",
    ];
    !name.is_empty()
        && name
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        && !reserved.contains(&name)
        && ![
            "GITHUB_",
            "RUNNER_",
            "RUST_",
            "RUSTC_",
            "RUSTDOC_",
            "RUSTUP_",
            "CARGO_",
            "LD_",
            "DYLD_",
            "NATIVE_CACHE_",
            "MUTATION_",
            "OUT_",
        ]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

/// A single child name or glob; traversal, separators and recursive globs are refused.
fn selector(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains("**")
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "_-.*?".contains(ch))
}

#[cfg(test)]
mod tests {
    use super::{cache_platform, environment_name, selector};

    #[test]
    fn environment_names_cannot_override_process_or_gate_controls() {
        assert!(environment_name("FIXTURE_NATIVE_CACHE_DIR"));
        for name in [
            "",
            "bad-name",
            "PATH",
            "BASH_ENV",
            "RUSTC_WRAPPER",
            "CARGO_HOME",
            "GITHUB_ENV",
            "RUNNER_TEMP",
            "LD_PRELOAD",
            "COVERAGE_FEATURES",
        ] {
            assert!(!environment_name(name), "{name}");
        }
    }

    #[test]
    fn published_selectors_stay_at_one_root_child() {
        for name in ["entry-*", "entry-one", "entry-?", ".cache"] {
            assert!(selector(name));
        }
        for name in [
            "",
            ".",
            "../entry",
            "/entry",
            "entry/child",
            "entry\\child",
            "**",
            "entry-[0-9]",
            "entry-{one,two}",
            "a/b",
        ] {
            assert!(!selector(name));
        }
    }
    #[test]
    fn windows_cannot_enable_caching_even_in_a_selected_platform_list() {
        let platforms = vec!["linux".to_owned(), "macos".to_owned(), "windows".to_owned()];
        assert!(cache_platform("linux", &platforms));
        assert!(cache_platform("macos", &platforms));
        assert!(!cache_platform("windows", &platforms));
        assert!(!cache_platform("linux", &["macos".to_owned()]));
    }
}

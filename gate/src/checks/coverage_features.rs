//! Validate the qualified workspace features selected for merged coverage.

use crate::runner::{Cmd, Failure};
use std::collections::BTreeSet;

/// Parse a nonempty selection, requiring every package and feature in workspace metadata.
/// An absent value keeps coverage's legacy command and does not read metadata.
pub(crate) fn coverage_features(
    value: &str,
    metadata: impl FnOnce() -> Result<String, Failure>,
) -> Result<Vec<String>, Failure> {
    if value.is_empty() {
        return Ok(Vec::new());
    }
    let listed = Cmd::new("jaq -nr")
        .env("COVERAGE_SELECTION", value)
        .arg(concat!(
            "$ENV.COVERAGE_SELECTION | fromjson | if type == \"array\" ",
            "and all(.[]; type == \"string\" and (contains(\"\\n\") | not) ",
            "and (contains(\"\\r\") | not)) then .[] else ",
            "error(\"expected an array of strings\") end"
        ))
        .capture()
        .map_err(|_| "coverage-features must be a JSON array of strings")?;
    let features: Vec<String> = listed.lines().map(str::to_owned).collect();
    if features.is_empty() {
        return Err("coverage-features must not be empty".into());
    }
    let mut unique = BTreeSet::new();
    for entry in &features {
        if !unique.insert(entry) {
            return Err(format!("coverage-features entry `{entry}` is duplicated").into());
        }
    }
    let metadata = metadata()?;
    let rows = Cmd::new("jaq -r")
        .arg(concat!(
            ".workspace_members as $members | .packages[] | ",
            "select(.id as $id | $members | index($id)) | ",
            "[.name, (.features | keys | join(\" \"))] | @tsv"
        ))
        .stdin_bytes(metadata.as_bytes())
        .capture()?;
    for entry in &features {
        let (package, feature) = qualified(entry)?;
        let declared = rows.lines().find_map(|row| {
            let (name, declared) = row.split_once('\t')?;
            (name == package).then_some(declared)
        });
        let Some(declared) = declared else {
            return Err(
                format!("coverage-features package `{package}` is not a workspace member").into(),
            );
        };
        if !declared.split_whitespace().any(|name| name == feature) {
            return Err(format!(
                "coverage-features package `{package}` does not declare feature `{feature}`"
            )
            .into());
        }
    }
    Ok(features)
}

/// Two plain Cargo names, never options, whitespace or additional slashes.
fn qualified(entry: &str) -> Result<(&str, &str), Failure> {
    let names = entry.split_once('/');
    if let Some((package, feature)) = names
        && plain_name(package)
        && plain_name(feature)
    {
        return Ok((package, feature));
    }
    Err(format!("coverage-features entry `{entry}` must be package/feature").into())
}

/// Cargo package and feature identifiers accepted by the selector.
fn plain_name(name: &str) -> bool {
    name.starts_with(|character: char| character.is_ascii_alphanumeric() || character == '_')
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "_-".contains(character))
}

#[cfg(test)]
mod tests {
    use super::{coverage_features, qualified};

    #[test]
    fn absent_selection_never_reads_workspace_metadata() {
        assert!(
            coverage_features("", || panic!("unexpected metadata"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn entries_have_exactly_two_plain_cargo_names() {
        assert_eq!(qualified("crate-a/engine").unwrap(), ("crate-a", "engine"));
        for entry in [
            "", "engine", "/engine", "a/", "a/b/c", "-a/b", "a/-b", "a/b c",
        ] {
            assert!(qualified(entry).is_err(), "{entry}");
        }
    }

    #[test]
    fn selection_accepts_only_declared_workspace_features() {
        let metadata = || {
            Ok(concat!(
                "{\"workspace_members\":[\"a\"],\"packages\":[{\"id\":\"a\",\"name\":\"crate-a\",",
                "\"features\":{\"engine\":[]}}]}"
            )
            .into())
        };
        assert_eq!(
            coverage_features("[\"crate-a/engine\"]", metadata).unwrap(),
            ["crate-a/engine"]
        );
        assert!(coverage_features("[\"other/engine\"]", metadata).is_err());
        assert!(coverage_features("[\"crate-a/other\"]", metadata).is_err());
    }
}

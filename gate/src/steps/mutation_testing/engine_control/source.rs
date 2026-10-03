//! Bind the compiler coordinate frame and package selection to retained Cargo metadata.

use crate::runner::{Cmd, Failure, Job, Outcome, input, write};
use std::path::Path;

/// Retain the independently obtained source metadata with its checkout coordinate frame.
pub(super) fn retain(job: &Job, root: &Path, metadata: &str) -> Outcome {
    let value = Cmd::new("jaq -cn")
        .args(["--argjson", "metadata", metadata])
        .args(["--arg", "checkout", &input("GITHUB_WORKSPACE")?])
        .args(["--arg", "project"])
        .arg(&job.project)
        .arg("{metadata:$metadata,checkout:$checkout,project:$project}")
        .capture()?;
    write(&root.join("source-metadata.json"), value.as_bytes(), false)
}

/// Verify the retained frame, every planned owner, and the manifest's coordinates.
pub(super) fn verify(root: &Path, receipt: &Path, assigned: &Path, manifest: &Path) -> Outcome {
    Cmd::new("jaq -e")
        .args(["--slurpfile", "receipt"])
        .arg(receipt)
        .args(["--slurpfile", "assigned"])
        .arg(assigned)
        .args(["--slurpfile", "manifest"])
        .arg(manifest)
        .arg(concat!(
            ". as $source | $manifest[0] as $m | ",
            ".project == $m.project and .metadata.workspace_root == $m.workspace and ",
            "(.metadata.workspace_root | type == \"string\" and startswith(\"/\")) and ",
            ".project == (if $receipt[0].directory == \".\" then .checkout ",
            "else .checkout + \"/\" + $receipt[0].directory end) and ",
            "all($assigned[0][]; . as $mutant | ",
            "$source.metadata.packages | map(. + {root:(.manifest_path | ",
            "split(\"/\") | .[:-1] | join(\"/\"))}) | ",
            "map(.root as $root | select(($source.project + \"/\" + $mutant.file) | ",
            "startswith($root + \"/\"))) | sort_by(.root | length) | last | ",
            ".name == $mutant.package)"
        ))
        .arg(root.join("source-metadata.json"))
        .capture()
        .map_err(|_| "featureless compile membership binding differs from its plan")?;
    Ok(())
}

/// Only one owning package can be compiled without unifying distinct owners' features.
pub(super) fn package(root: &Path, assigned: &Path) -> Result<Option<String>, Failure> {
    let package = Cmd::new("jaq -r")
        .args(["--slurpfile", "assigned"])
        .arg(assigned)
        .arg(concat!(
            "($assigned[0] | map(.package) | unique) as $owners | ",
            "if ($owners | length) == 1 then ",
            "[.metadata.packages[] | select(.name == $owners[0])] | ",
            "if length == 1 and (.[0].version | type == \"string\") then ",
            ".[0] | .name + \"@\" + .version else empty end else empty end"
        ))
        .arg(root.join("source-metadata.json"))
        .capture()?;
    Ok(if package.trim().is_empty() {
        None
    } else {
        Some(package.trim().into())
    })
}

/// Remove only inherited inclusion regexes; every other bound setting is unchanged.
pub(super) fn selection_config(root: &Path) -> Result<String, Failure> {
    let value = Cmd::new("jaq --from toml --to toml")
        .arg("del(.examine_re)")
        .arg(root.join("compile-config.toml"))
        .capture()?;
    let retained = root.join("selection-config.toml");
    write(&retained, value.as_bytes(), false)?;
    Ok(retained.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::package;
    use std::{env, fs, process};

    #[test]
    fn only_one_version_qualified_planned_package_can_establish_compile_membership() {
        let root = env::temp_dir().join(format!("control-owner-{}", process::id()));
        fs::create_dir_all(&root).unwrap();
        let listing = root.join("assigned.json");
        fs::write(&listing, r#"[{"package":"a"}]"#).unwrap();
        let source = root.join("source-metadata.json");
        fs::write(
            &source,
            r#"{"metadata":{"packages":[{"name":"a","version":"1.2.3"}]}}"#,
        )
        .unwrap();
        assert_eq!(
            package(&root, &listing).unwrap().as_deref(),
            Some("a@1.2.3")
        );
        for metadata in [
            r#"{"metadata":{"packages":[]}}"#,
            r#"{"metadata":{"packages":[{"name":"a"}]}}"#,
            r#"{"metadata":{"packages":[{"name":"a","version":1}]}}"#,
            r#"{"metadata":{"packages":[{"name":"a","version":"1"},{"name":"a","version":"2"}]}}"#,
        ] {
            fs::write(&source, metadata).unwrap();
            assert_eq!(package(&root, &listing).unwrap(), None);
        }
        fs::write(
            &source,
            r#"{"metadata":{"packages":[{"name":"a","version":"1"},{"name":"b","version":"1"}]}}"#,
        )
        .unwrap();
        for assigned in ["[]", r#"[{"package":"a"},{"package":"b"}]"#] {
            fs::write(&listing, assigned).unwrap();
            assert_eq!(package(&root, &listing).unwrap(), None);
        }
        fs::remove_dir_all(root).unwrap();
    }
}

//! Compare mode-aware identities against their same-source featureless control.

use crate::runner::{Cmd, Failure, Outcome, tee_line};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One completed mutant's package and viability classification.
#[derive(Debug)]
struct MutantResult {
    /// Package scope used by cargo-mutants.
    package: String,
    /// The validated pinned-tool outcome summary.
    summary: String,
}

/// Identity checks prevent both offsetting regressions and newly unviable feature-only code.
pub(super) fn compare(control: &[PathBuf], engine: &[PathBuf], report: &Path) -> Outcome {
    let control = results(control)?;
    let engine = results(engine)?;
    unchanged_viability(&control, &engine)?;
    let control_counts = package_counts(&control);
    let engine_counts = package_counts(&engine);
    tee_line(
        &format!(
            "Unviable comparison by package: default={control_counts:?}; engine={engine_counts:?}"
        ),
        report,
        true,
    )?;
    tee_line(
        &format!(
            "Unviable comparison by partition: default={}; engine={}",
            control_counts.values().sum::<usize>(),
            engine_counts.values().sum::<usize>()
        ),
        report,
        true,
    )
}

/// Join complete outcomes by the same stable identity used in planning.
fn results(paths: &[PathBuf]) -> Result<BTreeMap<String, MutantResult>, Failure> {
    let mut results = BTreeMap::new();
    for path in paths {
        let rows = Cmd::new("jaq -r")
            .arg(concat!(
                ".outcomes[] | select((.scenario | type) == \"object\") | ",
                ".summary as $summary | .scenario.Mutant | ",
                "[.package,.name,.file,.span.start.line,.span.start.column,",
                ".span.end.line,.span.end.column,.replacement,$summary] | @tsv"
            ))
            .arg(path)
            .capture()?;
        for row in rows.lines() {
            let Some((identity, summary)) = row.rsplit_once('\t') else {
                continue;
            };
            let package = identity.split('\t').next().unwrap_or_default();
            if results
                .insert(
                    identity.to_owned(),
                    MutantResult {
                        package: package.to_owned(),
                        summary: summary.to_owned(),
                    },
                )
                .is_some()
            {
                return Err("duplicate execution within a mutation mode".into());
            }
        }
    }
    Ok(results)
}

/// Every allowed engine unviable has the identical unviable control, including its package.
fn unchanged_viability(
    control: &BTreeMap<String, MutantResult>,
    engine: &BTreeMap<String, MutantResult>,
) -> Outcome {
    for (identity, result) in engine {
        if result.summary != "Unviable" {
            continue;
        }
        match control.get(identity) {
            Some(default) if default.summary == "Unviable" => {}
            Some(_) => {
                return Err("default-caught mutant became unviable in the engine mode".into());
            }
            None => return Err("feature-only mutant became unviable in the engine mode".into()),
        }
    }
    Ok(())
}

/// Record per-package counts; identity inclusion above enforces each count and global total.
fn package_counts(results: &BTreeMap<String, MutantResult>) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for result in results.values() {
        if result.summary == "Unviable" {
            *counts.entry(result.package.clone()).or_default() += 1;
        }
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::{MutantResult, package_counts, results, unchanged_viability};
    use std::{collections::BTreeMap, env, fs, process};

    /// A stable identity with a pinned-tool outcome and explicit package scope.
    fn result(package: &str, summary: &str) -> MutantResult {
        MutantResult {
            package: package.into(),
            summary: summary.into(),
        }
    }

    #[test]
    fn shared_unviable_is_allowed_but_offsets_never_hide_regression() {
        let control = BTreeMap::from([
            ("a".into(), result("A", "CaughtMutant")),
            ("b".into(), result("B", "Unviable")),
        ]);
        let unchanged = BTreeMap::from([("b".into(), result("B", "Unviable"))]);
        assert!(unchanged_viability(&control, &unchanged).is_ok());
        assert_eq!(
            package_counts(&unchanged),
            BTreeMap::from([("B".into(), 1)])
        );
        let offset = BTreeMap::from([("a".into(), result("A", "Unviable"))]);
        assert_eq!(
            unchanged_viability(&control, &offset)
                .unwrap_err()
                .message
                .as_deref(),
            Some("default-caught mutant became unviable in the engine mode")
        );
        let new = BTreeMap::from([("new".into(), result("A", "Unviable"))]);
        assert_eq!(
            unchanged_viability(&control, &new)
                .unwrap_err()
                .message
                .as_deref(),
            Some("feature-only mutant became unviable in the engine mode")
        );
    }
    #[test]
    fn duplicate_mode_execution_is_not_collapsed_during_identity_join() {
        let path = env::temp_dir().join(format!("engine-duplicate-{}.json", process::id()));
        fs::write(
            &path,
            r#"{"outcomes":[{"summary":"Unviable","scenario":{"Mutant":{"package":"a"}}}]}"#,
        )
        .unwrap();
        assert_eq!(
            results(&[path.clone(), path.clone()])
                .unwrap_err()
                .message
                .as_deref(),
            Some("duplicate execution within a mutation mode")
        );
        fs::remove_file(path).unwrap();
    }
}

//! Preserve raw tested results and account explicitly for every verified non-member mutant.

use super::membership;
use crate::runner::{Cmd, Failure, Job, Outcome, write};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Project only the assigned mutants whose files the default build compiles.
pub(super) fn tested_listing(
    listing: &Path,
    members: Option<&BTreeSet<String>>,
) -> Result<String, Failure> {
    query(members)?
        .arg(concat!(
            "[.[] | select(.file as $f | $members == null or ",
            "($members | any(.[]; . == $f)))]"
        ))
        .arg(listing)
        .capture()
}

/// Compose the new versioned control document without relabelling any tested mutant.
pub(super) fn compose(
    listing: &Path,
    raw: &Path,
    members: Option<&BTreeSet<String>>,
) -> Result<String, Failure> {
    query(members)?
        .args(["--slurpfile", "raw"])
        .arg(raw)
        .arg(concat!(
            "[.[] | select(.file as $f | $members != null and ",
            "($members | any(.[]; . == $f) | not))] as $inactive | ",
            "$raw[0] + {engine_control_schema:1, ",
            "not_compiled_without_features:($inactive | length), ",
            "total_mutants:($raw[0].total_mutants + ($inactive | length)), ",
            "outcomes:($raw[0].outcomes + [$inactive[] | ",
            "{scenario:{Mutant:.}, summary:\"NotCompiledWithoutFeatures\", phase_results:[]}])}"
        ))
        .arg(listing)
        .capture()
}

/// Validate classification before the ordinary evidence validator sees the tested subset.
pub(in super::super) fn validate(
    job: &Job,
    listing: &Path,
    outcomes: &Path,
    receipt: &Path,
) -> Result<(PathBuf, PathBuf), Failure> {
    let root = outcomes
        .parent()
        .ok_or("featureless outcomes have no directory")?;
    let members = membership::verify(root, receipt)?;
    let tested = root.join("tested-outcomes.json");
    let expected = compose(listing, &tested, members.as_ref())?;
    Cmd::new("jaq -e")
        .args(["--argjson", "expected", &expected])
        .arg(". == $expected")
        .arg(outcomes)
        .capture()
        .map_err(|_| "featureless compile membership differs from its outcomes")?;
    let projection = job.temp.join("engine-control-tested-list.json");
    let compiled = tested_listing(listing, members.as_ref())?;
    write(&projection, compiled.as_bytes(), false)?;
    let discovered = root.join("tested-mutants.json");
    Cmd::new("jaq -e")
        .args(["--argjson", "compiled", &compiled])
        .arg(". == $compiled")
        .arg(&discovered)
        .capture()
        .map_err(|_| "featureless tested discovery differs from compile membership")?;
    Ok((projection, tested))
}

/// Use Cargo's verified build as the baseline when no mutant needs a test invocation.
pub(super) fn build_baseline(root: &Path, version: &str) -> Outcome {
    let value = Cmd::new("jaq -c")
        .args(["--arg", "version", version])
        .arg(concat!(
            ". as $build | {outcomes:[{scenario:\"Baseline\",summary:\"Success\",phase_results:[{",
            "phase:\"Build\",duration:$build.duration,process_status:\"Success\",",
            "argv:$build.argv}],",
            "log_path:\"cargo-build.log\"}],total_mutants:0,caught:0,missed:0,timeout:0,",
            "unviable:0,success:0,cargo_mutants_version:$version,",
            "start_time:$build.start_time,end_time:$build.end_time}"
        ))
        .arg(root.join("build-record.json"))
        .capture()?;
    write(&root.join("tested-outcomes.json"), value.as_bytes(), false)
}

/// Serialize an optional exact source set using the existing pinned JSON reader.
fn query(members: Option<&BTreeSet<String>>) -> Result<Cmd, Failure> {
    let members = if let Some(members) = members {
        Cmd::new("jaq -cn")
            .arg("$ARGS.positional")
            .arg("--args")
            .args(members)
            .capture()?
    } else {
        "null".into()
    };
    Ok(Cmd::new("jaq -c").args(["--argjson", "members", &members]))
}

#[cfg(test)]
mod tests {
    use super::{query, tested_listing, validate};
    use crate::runner::Job;
    use std::path::{Path, PathBuf};
    use std::{collections::BTreeSet, env, fs, process};

    #[test]
    fn parentless_outcomes_cannot_supply_compile_membership_evidence() {
        let job = Job {
            project: PathBuf::new(),
            reports: PathBuf::new(),
            temp: PathBuf::new(),
        };
        assert_eq!(
            validate(&job, Path::new("list"), Path::new(""), Path::new("receipt"))
                .unwrap_err()
                .message
                .as_deref(),
            Some("featureless outcomes have no directory")
        );
    }

    #[test]
    fn absent_membership_falls_back_to_every_assigned_mutant() {
        assert!(query(None).is_ok());
        let path = env::temp_dir().join(format!("control-subset-{}.json", process::id()));
        fs::write(&path, "[{\"file\":\"src/a.rs\"},{\"file\":\"src/b.rs\"}]").unwrap();
        assert_eq!(
            tested_listing(&path, None).unwrap().trim(),
            "[{\"file\":\"src/a.rs\"},{\"file\":\"src/b.rs\"}]"
        );
        let members = BTreeSet::from(["src/a.rs".into()]);
        assert_eq!(
            tested_listing(&path, Some(&members)).unwrap().trim(),
            "[{\"file\":\"src/a.rs\"}]"
        );
        fs::remove_file(path).unwrap();
    }
}

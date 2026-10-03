//! Preserve raw tested results and account explicitly for every verified non-member mutant.

use super::membership::{self, Membership};
use crate::runner::{Cmd, Failure, Job, Outcome, write};
use std::fs;
use std::path::{Path, PathBuf};

/// Project only the assigned mutants whose files the default build compiles.
pub(super) fn tested_listing(listing: &Path, members: &Membership) -> Result<String, Failure> {
    query(members)?
        .arg(concat!(
            "[.[] | select(.file as $f | $members[.package] as $compiled | ",
            "$compiled == null or ($compiled | any(.[]; . == $f)))]"
        ))
        .arg(listing)
        .capture()
}

/// Compose the new versioned control document without relabelling any tested mutant.
pub(super) fn compose(listing: &Path, raw: &Path, members: &Membership) -> Result<String, Failure> {
    query(members)?
        .args(["--slurpfile", "raw"])
        .arg(raw)
        .arg(concat!(
            "[.[] | select(.file as $f | $members[.package] as $compiled | ",
            "$compiled != null and ($compiled | any(.[]; . == $f) | not))] as $inactive | ",
            "$raw[0] + {engine_control_schema:1, ",
            "not_compiled_without_features:($inactive | length), ",
            "total_mutants:($raw[0].total_mutants + ($inactive | length)), ",
            "outcomes:($raw[0].outcomes + [$inactive[] | ",
            "{scenario:{Mutant:.}, summary:\"NotCompiledWithoutFeatures\", phase_results:[]}])}"
        ))
        .arg(listing)
        .capture()
}

/// Validate the tested subset and reject survivors only for independently verified owners.
pub(in super::super) fn validate(
    job: &Job,
    listing: &Path,
    outcomes: &Path,
    receipt: &Path,
) -> Result<(PathBuf, PathBuf), Failure> {
    let root = outcomes
        .parent()
        .ok_or("featureless outcomes have no directory")?;
    let members = membership::verify(root, receipt, listing)?;
    let tested = root.join("tested-outcomes.json");
    let expected = compose(listing, &tested, &members)?;
    Cmd::new("jaq -e")
        .args(["--argjson", "expected", &expected])
        .arg(". == $expected")
        .arg(outcomes)
        .capture()
        .map_err(|_| "featureless compile membership differs from its outcomes")?;
    let projection = job.temp.join("engine-control-tested-list.json");
    let compiled = tested_listing(listing, &members)?;
    write(&projection, compiled.as_bytes(), false)?;
    let discovered = root.join("tested-mutants.json");
    Cmd::new("jaq -e")
        .args(["--argjson", "compiled", &compiled])
        .arg(". == $compiled")
        .arg(&discovered)
        .capture()
        .map_err(|_| "featureless tested discovery differs from compile membership")?;
    query(&members)?
        .arg("-e")
        .arg(concat!(
            "all(.outcomes[] | select(.summary == \"MissedMutant\"); ",
            "$members[.scenario.Mutant.package] == null)"
        ))
        .arg(&tested)
        .capture()
        .map_err(|_| "partition contains a survivor, timeout or untested mutant")?;
    Ok((projection, tested))
}

/// Use every owning package's verified build as the baseline when no mutant needs testing.
pub(super) fn build_baseline(root: &Path, version: &str, packages: usize) -> Outcome {
    let mut documents = Vec::new();
    for index in 0..packages {
        let path = root.join(format!("builds/{index}/build-record.json"));
        documents.extend(
            fs::read(&path)
                .map_err(|error| format!("cannot read baseline build record: {error}"))?,
        );
        documents.push(b'\n');
    }
    let value = Cmd::new("jaq -sc").args(["--arg", "version", version]).arg(concat!(
        ". as $builds | {outcomes:[{scenario:\"Baseline\",summary:\"Success\",phase_results:",
        "[$builds[] | {phase:\"Build\",duration:.duration,process_status:\"Success\",argv:.argv}],",
        "log_path:\"builds/0/cargo-build.log\"}],total_mutants:0,caught:0,missed:0,timeout:0,",
        "unviable:0,success:0,cargo_mutants_version:$version,",
        "start_time:$builds[0].start_time,end_time:$builds[-1].end_time}"
    )).stdin_bytes(&documents).capture()?;
    write(&root.join("tested-outcomes.json"), value.as_bytes(), false)
}

/// Serialize the package-local sets using the existing pinned JSON reader.
fn query(members: &Membership) -> Result<Cmd, Failure> {
    let mut entries = Vec::new();
    for (owner, sources) in members {
        let sources = if let Some(sources) = sources {
            Cmd::new("jaq -cn")
                .arg("$ARGS.positional")
                .arg("--args")
                .args(sources)
                .capture()?
        } else {
            "null".into()
        };
        entries.push(
            Cmd::new("jaq -cn")
                .args(["--arg", "owner", owner])
                .args(["--argjson", "sources", &sources])
                .arg("{key:$owner,value:$sources}")
                .capture()?,
        );
    }
    let value = Cmd::new("jaq -cn")
        .args(["--argjson", "entries", &format!("[{}]", entries.join(","))])
        .arg("$entries | from_entries")
        .capture()?;
    Ok(Cmd::new("jaq -c").args(["--argjson", "members", &value]))
}

#[cfg(test)]
mod tests {
    use super::{Membership, query, tested_listing, validate};
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
        let fallback = Membership::from([("a".into(), None)]);
        assert!(query(&fallback).is_ok());
        let path = env::temp_dir().join(format!("control-subset-{}.json", process::id()));
        fs::write(
            &path,
            "[{\"package\":\"a\",\"file\":\"src/a.rs\"},{\"package\":\"a\",\"file\":\"src/b.rs\"}]",
        )
        .unwrap();
        assert_eq!(
            tested_listing(&path, &fallback).unwrap().trim(),
            "[{\"package\":\"a\",\"file\":\"src/a.rs\"},{\"package\":\"a\",\"file\":\"src/b.rs\"}]"
        );
        let members = Membership::from([("a".into(), Some(BTreeSet::from(["src/a.rs".into()])))]);
        assert_eq!(
            tested_listing(&path, &members).unwrap().trim(),
            "[{\"package\":\"a\",\"file\":\"src/a.rs\"}]"
        );
        fs::remove_file(path).unwrap();
    }
}

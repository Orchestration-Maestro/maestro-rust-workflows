//! Join typed host outcomes with pending coverage, without editing LCOV.

use super::artifacts::{copy_artifact, safe_directory, safe_file, safe_relative, validate_tree};
use crate::checks::digests::sha256_hex;
use crate::runner::{Cmd, Failure, Job, Outcome, input, optional, output, tee_line, write};
use std::fs;
use std::path::Path;

/// A host partition always needs its actual executor result, even with no ordinary work.
pub(super) fn required() -> Result<bool, Failure> {
    let count = optional("MUTATION_HOST_COUNT")?;
    if count.is_empty() || count == "0" {
        return Ok(false);
    }
    if count.parse::<usize>().is_err() {
        return Err("provisioned-host mutation count is invalid".into());
    }
    if optional("HOST_MUTATIONS_RESULT")?.is_empty() {
        return Err("provisioned-host mutation result is missing; no host executor ran".into());
    }
    if input("HOST_MUTATIONS_RESULT")? != "success" {
        return Err("provisioned-host mutation job failed or was skipped".into());
    }
    Ok(true)
}

/// Validate the complete same-run plan, every phase and each enclosing-function obligation.
pub(super) fn join(job: &Job, report: &Path) -> Outcome {
    let plan = safe_file(&job.reports.join("mutation-host-plan.json"))?;
    let pending = safe_file(&job.reports.join("changed-coverage-pending.json"))?;
    let root = safe_directory(job, Path::new(&input("MUTATION_HOST_ARTIFACTS")?))?;
    validate_tree(&root)?;
    let outcomes = safe_file(&root.join("host-outcomes.json"))?;
    let plan_bytes = fs::read(&plan).map_err(|error| format!("cannot read host plan: {error}"))?;
    let mut documents = plan_bytes.clone();
    let default = safe_file(&job.reports.join("mutation-plan.json"))?;
    for path in [&pending, &outcomes, &default] {
        documents.push(b'\n');
        documents
            .extend(fs::read(path).map_err(|error| format!("cannot read host evidence: {error}"))?);
    }
    let command = || -> Result<Cmd, Failure> {
        Ok(Cmd::new("jaq -se")
            .args(["--arg", "sha", &input("GITHUB_SHA")?])
            .args(["--arg", "run", &input("GITHUB_RUN_ID")?])
            .args(["--arg", "attempt", &input("GITHUB_RUN_ATTEMPT")?])
            .args(["--arg", "digest", &sha256_hex(&plan_bytes)])
            .args(["--argjson", "count", &input("MUTATION_HOST_COUNT")?])
            .args(["--arg", "compiler", &input("RUSTUP_TOOLCHAIN")?])
            .args(["--arg", "tool", &input("CARGO_MUTANTS_VERSION")?])
            .stdin_bytes(&documents))
    };
    command()?.arg(SHAPE).capture().map_err(
        |_| "host plan, pending coverage or outcomes belong to another source, policy or run",
    )?;
    command()?.arg(IDENTITY).capture().map_err(
        |_| "host plan, pending coverage or outcomes belong to another source, policy or run",
    )?;
    command()?.arg(POPULATION).capture().map_err(
        |_| "host outcomes do not equal the complete nonempty planned mutant population",
    )?;
    command()?.arg(PHASES).capture().map_err(|_| {
        "host evidence requires caught mutants, successful baselines, build, \
        provisioning and cleanup"
    })?;
    validate_logs(&root, &outcomes)?;
    command()?.arg(COVERAGE).capture().map_err(|_| {
        "host coverage partition, ordinary allowance or enclosing-function \
        evidence is incomplete"
    })?;
    copy_artifact(&root, &job.report("mutation-partitions")?.join("host"))?;
    let text = command()?.arg("-r").arg(concat!(
        ".[1] as $c | \"\\($c.coverable|length) coverable new lines; ",
        "\\($c.ordinary|length) ordinary, \\($c.ordinary_uncovered|length) ordinary uncovered, ",
        "\\($c.ordinary_allowed) ordinary allowed; \\($c.host|length) HOST_BEHAVIOUR_VERIFIED; ",
        "\\([$c.host[] | select(.hits > 0)]|length) raw host LLVM_EXECUTED; ",
        "\\(.[0].mutants|length) host mutants caught; missed=0 timeout=0 unviable=0 untested=0\""
    )).capture()?;
    write(&job.report("changed-coverage.txt")?, text.as_bytes(), false)?;
    tee_line(text.trim_matches('"'), report, true)?;
    let resolved = command()?
        .arg(".[1] + {state:\"passed\",host_verified:(.[1].host|length)}")
        .capture()?;
    write(
        &job.report("changed-coverage-final.json")?,
        resolved.as_bytes(),
        false,
    )?;
    output("changed-coverage-state", "passed")
}

/// Exact versioned envelopes and records; native cargo-mutants identities stay unchanged.
const SHAPE: &str = concat!(
    "def keys_are($names): type == \"object\" and (keys|sort) == ($names|split(\" \")|sort); ",
    "def receipt: keys_are(\"build provision test cleanup selected_tests passed failed ignored ",
    "phases test_sha256 bootstrap_sha256 logs\"); ",
    ".[0] as $p | .[1] as $c | .[2] as $o | ",
    "($p | keys_are(\"schema identity mutants files features packages source_sha256 policy_sha256 ",
    "provisioner provisioner_sha256 workflow_revision\")) and ",
    "($p.identity | keys_are(\"sha first_parent directory toolchain cargo_mutants_version run_id ",
    "attempt config_sha256 diff_sha256 mutant_count shard_count\")) and ",
    "($c | keys_are(\"schema state sha run_id attempt plan_sha256 policy_sha256 target coverable ",
    "ordinary host ordinary_uncovered ordinary_allowed full_allowed\")) and ",
    "all([$c.coverable,$c.ordinary,$c.host,$c.ordinary_uncovered][]; ",
    "type == \"array\" and all(.[]; keys_are(\"file line hits\"))) and ",
    "($o | keys_are(\"schema sha run_id attempt plan_sha256 policy_sha256 provisioner_sha256 ",
    "source_sha256 baseline_before baseline_after outcomes\")) and ",
    "all([$o.baseline_before,$o.baseline_after][]; receipt) and ",
    "($o.outcomes|type == \"array\" and all(.[]; ",
    "keys_are(\"mutant outcome patched_source_sha256 receipt\") and ",
    "(.receipt | has(\"test_failure\") and (del(.test_failure)|receipt))))"
);

/// All documents bind to the same plan digest and current attempt, not an old qualification.
const IDENTITY: &str = concat!(
    ".[0] as $p | .[1] as $c | .[2] as $o | ",
    "$p.schema == 1 and $c.schema == 1 and $o.schema == 1 and ",
    "$p.identity == .[3] and $p.identity.toolchain == $compiler and ",
    "$p.identity.cargo_mutants_version == $tool and ",
    "$c.state == \"pending-host\" and $p.identity.sha == $sha and ",
    "$p.identity.run_id == $run and $p.identity.attempt == $attempt and ",
    "all([$c,$o][]; .sha == $sha and .run_id == $run and .attempt == $attempt ",
    "and .plan_sha256 == $digest and .policy_sha256 == $p.policy_sha256) and ",
    "$o.provisioner_sha256 == $p.provisioner_sha256 and ",
    "$o.source_sha256 == $p.source_sha256"
);

/// Exact stable identities include patches and enclosing functions; no duplicates or partial runs.
const POPULATION: &str = concat!(
    ".[0] as $p | .[2] as $o | ($p.mutants|length) == $count and $count > 0 and ",
    "($p.mutants|length) == ($p.mutants|unique_by([.package,.name])|length) and ",
    "([$p.mutants[].file]|unique|sort) == ($p.files|sort) and ",
    "($o.outcomes|length) == $count and ",
    "([$o.outcomes[].mutant]|sort_by([.package,.name])) == ",
    "($p.mutants|sort_by([.package,.name]))"
);

/// Infrastructure, unviable and timeout results never prove behaviour.
/// Integral selection and named-failure array lengths imply integral test counters.
const PHASES: &str = concat!(
    "def positive: (.selected_tests|type == \"array\" and length > 0) and ",
    "(.selected_tests|length) == (.selected_tests|unique|length) and ",
    "all(.selected_tests[]; type == \"string\" and length > 0) and ",
    "(.passed|type == \"number\" and . >= 0) and ",
    "(.failed|type == \"number\" and . >= 0) and .ignored == 0 and ",
    "(.selected_tests|length) == (.passed + .failed); ",
    "def setup: .build == \"passed\" and .provision == \"passed\" and ",
    ".cleanup == \"passed\" and (.test_sha256|test(\"^[0-9a-f]{64}$\")) and ",
    "(.bootstrap_sha256|test(\"^[0-9a-f]{64}$\")); ",
    "def phases: (.phases|type == \"object\") and (.phases|keys|sort) == ",
    "[\"abandon\",\"normal\",\"preparing\",\"recover_abandon\",\"recover_preparing\"] and ",
    "all(.phases[]; . == \"passed\" or . == \"failed\"); ",
    "def named: . as $r | .test_failure as $names | ",
    "($names|type == \"array\" and length > 0) and ",
    "($names|length) == ($names|unique|length) and ",
    "all($names[]; type == \"string\" and length > 0) and ",
    "all($r.phases|to_entries[]; .value == \"passed\" or ",
    "(.key as $name | $names|index($name))) and ",
    "([$names[] | select(. as $name | $r.phases|has($name)|not)]|length) == $r.failed and ",
    "all($names[]; . as $name | if $r.phases|has($name) then ",
    "$r.phases[$name] == \"failed\" else ($r.selected_tests|index($name)) and ",
    "$r.failed > 0 end); ",
    ".[2] as $o | all([$o.baseline_before,$o.baseline_after][]; setup and positive and ",
    "phases and all(.phases[]; . == \"passed\") and ",
    ".test == \"passed\" and .passed > 0 and .failed == 0) and ",
    "all($o.outcomes[]; .outcome == \"caught\" and ",
    "(.patched_source_sha256|test(\"^[0-9a-f]{64}$\")) and ",
    "(.receipt | setup and positive and phases and named and .test == \"failed\"))"
);

/// Recompute both allowances and the disjoint partition; credit only caught enclosing functions.
const COVERAGE: &str = concat!(
    ".[0] as $p | .[1] as $c | ",
    "$c.target as $t | ($t == 95 or $t == 90) and ",
    "($c.coverable|length) == ($c.coverable|unique_by([.file,.line])|length) and ",
    "all($c.coverable[]; (.line|type == \"number\" and floor == . and . > 0) and ",
    "(.hits|type == \"number\" and floor == . and . >= 0)) and ",
    "($c.coverable|sort_by([.file,.line])) == (($c.ordinary+$c.host)|sort_by([.file,.line])) and ",
    "all($c.ordinary[]; .file as $f | $p.files | index($f) | not) and ",
    "all($c.host[]; . as $line | ($p.files|index($line.file)) and ",
    "any($p.mutants[]; .file == $line.file and .function != null and ",
    ".function.span.start.line <= $line.line and .function.span.end.line >= $line.line)) and ",
    "$c.ordinary_uncovered == [$c.ordinary[]|select(.hits == 0)] and ",
    "$c.ordinary_allowed == ([1,((100-$t)*($c.ordinary|length)/100|floor)]|max) and ",
    "$c.full_allowed == ([1,((100-$t)*($c.coverable|length)/100|floor)]|max) and ",
    "($c.ordinary_uncovered|length) <= $c.ordinary_allowed"
);

/// Every retained log is a nonempty safe file bound by its digest, not just a claimed status.
fn validate_logs(root: &Path, outcomes: &Path) -> Outcome {
    let logs = Cmd::new("jaq -r")
        .arg(concat!(
            "[.baseline_before,.baseline_after,(.outcomes[].receipt)][] | ",
            "if (.logs|type == \"object\" and length > 0) then ",
            ".logs | to_entries[] | [.key,.value] | @tsv else error(\"missing logs\") end"
        ))
        .arg(outcomes)
        .capture()
        .map_err(|_| "host evidence is missing raw phase logs")?;
    for row in logs.lines() {
        let (name, digest) = row.split_once('\t').unwrap_or_default();
        if !safe_relative(name) {
            return Err("host evidence log path escapes its artifact".into());
        }
        let path = safe_file(&root.join(name))?;
        if !path.starts_with(root) {
            return Err("host evidence log path escapes its artifact".into());
        }
        let bytes = fs::read(path).map_err(|error| format!("cannot read host log: {error}"))?;
        if bytes.is_empty() || sha256_hex(&bytes) != digest {
            return Err("host evidence raw log digest is missing or mismatched".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{COVERAGE, IDENTITY, PHASES, POPULATION};

    #[test]
    fn host_join_keeps_identity_population_phases_and_coverage_independent() {
        assert!(IDENTITY.contains("plan_sha256"));
        assert!(POPULATION.contains("$count > 0"));
        assert!(PHASES.contains(".cleanup == \"passed\""));
        assert!(COVERAGE.contains("ordinary_allowed"));
    }
}

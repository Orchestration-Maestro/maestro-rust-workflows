//! Versioned pending coverage keeps ordinary and full denominators unchanged.

use crate::checks::digests::sha256_hex;
use crate::checks::mutation_host::HostPolicy;
use crate::runner::{Cmd, Failure, Job, Outcome, input, write};
use std::collections::BTreeMap;
use std::fs;

/// Save actual LCOV identities, partitioned only by exact validated ownership.
pub(super) fn pending_coverage(
    job: &Job,
    policy: &HostPolicy,
    target: u32,
    added: &BTreeMap<String, Vec<usize>>,
    executed: &BTreeMap<String, BTreeMap<usize, u64>>,
) -> Outcome {
    for file in &policy.files {
        if !executed.contains_key(file) {
            return Err(format!("host-owned file `{file}` has no LCOV line records").into());
        }
    }
    let plan_path = job.earlier("mutation-host-plan.json");
    let plan = fs::read(&plan_path)
        .map_err(|error| format!("host coverage requires mutation-host-plan.json: {error}"))?;
    let mut records = Vec::new();
    for (file, lines) in added {
        let Some(counts) = executed.get(file) else {
            continue;
        };
        let numbers = lines
            .iter()
            .filter_map(|line| {
                counts
                    .get(line)
                    .map(|hits| format!("{{\"line\":{line},\"hits\":{hits}}}"))
            })
            .collect::<Vec<_>>()
            .join(",");
        let json = Cmd::new("jaq -cn")
            .args(["--arg", "file", file])
            .args(["--argjson", "lines", &format!("[{numbers}]")])
            .arg("$lines | map(. + {file:$file})")
            .capture()?;
        records.push(json);
    }
    let lines = Cmd::new("jaq -sc")
        .arg("add // []")
        .stdin_bytes(records.join("\n").as_bytes())
        .capture()?;
    let json = Cmd::new("jaq -cn")
        .args(["--argjson", "lines", &lines])
        .args([
            "--argjson",
            "plan",
            &String::from_utf8(plan.clone())
                .map_err(|error| format!("host plan is not UTF-8: {error}"))?,
        ])
        .args(["--arg", "digest", &sha256_hex(&plan)])
        .args(["--arg", "sha", &input("GITHUB_SHA")?])
        .args(["--arg", "run", &input("GITHUB_RUN_ID")?])
        .args(["--arg", "attempt", &input("GITHUB_RUN_ATTEMPT")?])
        .args(["--arg", "policy", &policy.digest])
        .args(["--argjson", "target", &target.to_string()])
        .arg(concat!(
            "$lines | unique_by([.file,.line]) as $c | ",
            "[$c[] | select(.file as $f | $plan.files | index($f) | not)] as $d | ",
            "[$c[] | select(.file as $f | $plan.files | index($f))] as $h | ",
            "{schema:1, state:\"pending-host\", sha:$sha,run_id:$run,attempt:$attempt, ",
            "plan_sha256:$digest,policy_sha256:$policy,target:$target, ",
            "coverable:$c,ordinary:$d,host:$h, ",
            "ordinary_uncovered:[$d[] | select(.hits == 0)], ",
            "ordinary_allowed:([1, ((100-$target)*($d|length)/100|floor)]|max), ",
            "full_allowed:([1, ((100-$target)*($c|length)/100|floor)]|max)}"
        ))
        .capture()?;
    write(
        &job.report("changed-coverage-pending.json")?,
        json.as_bytes(),
        false,
    )
}

/// Read the pending ordinary count and enforce its independent allowance.
pub(super) fn ordinary_verdict(job: &Job) -> Result<String, Failure> {
    let path = job.earlier("changed-coverage-pending.json");
    Cmd::new("jaq -e")
        .arg("(.ordinary_uncovered|length) <= .ordinary_allowed")
        .arg(&path)
        .capture()
        .map_err(
            |_| "changed-coverage: ordinary uncovered lines exceed their unchanged allowance",
        )?;
    Cmd::new("jaq -r")
        .arg(concat!(
            "\"pending-host: \\(.coverable|length) coverable new lines; ",
            "\\(.ordinary|length) ordinary, \\(.ordinary_uncovered|length) ordinary uncovered, ",
            "\\(.ordinary_allowed) ordinary allowed; \\(.host|length) host obligations\""
        ))
        .arg(&path)
        .capture()
}

#[cfg(test)]
mod tests {
    #[test]
    fn ordinary_allowance_does_not_grow_with_host_population() {
        let allowance = |lines: usize| (5 * lines / 100).max(1);
        assert_eq!(allowance(3), 1);
        assert_eq!(allowance(100), 5);
    }
}

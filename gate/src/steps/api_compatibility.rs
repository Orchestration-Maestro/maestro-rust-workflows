//! `rust-gate api`: a pull request that breaks a library's public API fails
//! unless its title declares the break. cargo-semver-checks compares the
//! workspace's libraries with the base branch as a minor release, so an
//! addition passes and a removal or an incompatible signature fails. A title
//! with `!` after its type, which release-please turns into a major release,
//! declares the break and skips the comparison.

use crate::checks::native_cache::{native_cache, native_cache_command, native_command};
use crate::checks::rust_versions::parse;
use crate::runner::{Cmd, Failure, Job, Outcome, Step, flag, input, optional, output, tee_line};

/// What this step declares: its inputs, its tools and its reports.
pub(crate) const STEPS: &[Step] = &[Step {
    workflow: "ci",
    id: "api",
    summary: "Public API compatibility",
    inputs: &[
        "API_COMPATIBILITY",
        "GITHUB_BASE_REF",
        "PULL_REQUEST_TITLE",
        "RUSTUP_TOOLCHAIN",
        "NATIVE_CACHE_ROOT",
    ],
    tools: &["cargo metadata", "cargo semver-checks", "git", "jaq"],
    reports: &["api-compatibility.txt", "native-cache-before.txt"],
    run,
}];

/// Each workspace library, one per line: its name, then its manifest path.
const LIBRARIES: &str = concat!(
    r#".packages[] | select([.targets[].kind[]] | "#,
    r#"any(. == "lib" or . == "rlib" or . == "proc-macro")) | "\(.name) \(.manifest_path)""#
);

/// The package a manifest names, or nothing for a workspace root.
const BASE_NAME: &str = ".package.name // \"\"";

/// The oldest compiler cargo-semver-checks 0.50 runs on.
const OLDEST_RUST: (u64, u64, u64) = (1, 93, 0);

/// Run the step. Every case that compares nothing says why and records that
/// the check did not apply, so the scorecard never counts it as a pass.
fn run() -> Outcome {
    let job = Job::current()?;
    let report = job.report("api-compatibility.txt")?;
    let skip = |reason: &str| -> Outcome {
        tee_line(reason, &report, false)?;
        output("applied", "false")
    };
    if !flag("API_COMPATIBILITY")? {
        return skip("SKIPPED: api-compatibility=false");
    }
    let base = optional("GITHUB_BASE_REF")?;
    if base.is_empty() {
        return skip("NOT APPLICABLE: only a pull request is compared with its base");
    }
    if parse(&input("RUSTUP_TOOLCHAIN")?, false).is_none_or(|version| version < OLDEST_RUST) {
        return skip("NOT APPLICABLE: cargo-semver-checks needs Rust 1.93 or newer");
    }
    if declares_break(&optional("PULL_REQUEST_TITLE")?) {
        return skip("NOT APPLICABLE: the title declares a breaking change");
    }
    // The checkout fetched the merge commit and its first parent, the base
    // branch; cargo-semver-checks builds that parent as the baseline.
    let parent = Cmd::new("git rev-parse --verify -q HEAD^1")
        .cwd(&job.project)
        .capture()
        .is_ok();
    if !parent {
        return Err("pull request checkout must include the base parent".into());
    }
    let policy = native_cache(&job.project)?;
    let metadata = native_cache_command(
        policy.as_ref(),
        "cargo metadata --format-version 1 --no-deps --locked",
    )
    .cwd(&job.project)
    .capture()?;
    let libraries = Cmd::new("jaq -r")
        .arg(LIBRARIES)
        .stdin_bytes(metadata.as_bytes())
        .capture()?;
    if libraries.trim().is_empty() {
        return skip("NOT APPLICABLE: no library target, so no public API");
    }
    let (compared, new) = split_by_base(&job, &libraries)?;
    tee_line(&format!("baseline: {base}"), &report, false)?;
    if !new.is_empty() {
        let names = new.join(", ");
        tee_line(&format!("new in this pull request: {names}"), &report, true)?;
    }
    if compared.is_empty() {
        tee_line(
            "NOT APPLICABLE: every library is new in this pull request",
            &report,
            true,
        )?;
        return output("applied", "false");
    }
    let mut check = native_cache_command(
        policy.as_ref(),
        "cargo semver-checks --baseline-rev HEAD^1 --release-type minor",
    );
    for name in &compared {
        check = check.arg("--package").arg(name);
    }
    native_command(&job, check.cwd(&job.project), policy.as_ref())?.tee(&report, true)?;
    output("applied", "true")
}

/// The libraries the base branch already has, which can be compared, and
/// the ones this pull request adds or renames, which have no baseline: the
/// base commit's manifest at the same path, naming the same package, decides.
fn split_by_base(job: &Job, libraries: &str) -> Result<(Vec<String>, Vec<String>), Failure> {
    let top = Cmd::new("git rev-parse --show-toplevel")
        .cwd(&job.project)
        .capture()?;
    let top = format!("{}/", top.trim());
    let (mut compared, mut new) = (Vec::new(), Vec::new());
    for line in libraries.lines() {
        let Some((name, manifest)) = line.split_once(' ') else {
            continue;
        };
        let relative = manifest.strip_prefix(&top).unwrap_or(manifest);
        let base_manifest = Cmd::new("git show")
            .arg(format!("HEAD^1:{relative}"))
            .cwd(&job.project)
            .capture()
            .ok();
        let base_name = base_manifest.and_then(|text| {
            Cmd::new("jaq --from toml -r")
                .arg(BASE_NAME)
                .stdin_bytes(text.as_bytes())
                .capture()
                .ok()
        });
        let in_base = base_name.is_some_and(|base| base.trim() == name);
        if in_base {
            compared.push(name.to_owned());
        } else {
            new.push(name.to_owned());
        }
    }
    Ok((compared, new))
}

/// Whether a conventional title marks a breaking change: `!` right after its
/// type and optional scope, as in `feat!:` or `fix(api)!:`.
fn declares_break(title: &str) -> bool {
    title.split_once(':').is_some_and(|(header, _)| {
        header.strip_suffix('!').is_some_and(|kind| {
            let name = kind.split_once('(').map_or(kind, |(name, _)| name);
            !name.is_empty() && name.chars().all(|character| character.is_ascii_lowercase())
        })
    })
}

#[cfg(test)]
mod tests {
    use super::declares_break;

    #[test]
    fn only_a_bang_after_the_type_declares_a_break() {
        assert!(declares_break("feat!: drop the old API"));
        assert!(declares_break("fix(api)!: rename the error"));
        assert!(!declares_break("feat: add a product"));
        assert!(!declares_break("docs: mention feat!: in the guide"));
        assert!(!declares_break("Revert \"feat!: drop the old API\""));
        assert!(!declares_break("!: nothing"));
    }
}

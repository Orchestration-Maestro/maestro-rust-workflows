//! `rust-gate ci --local`: the `checks` job of `ci.yml` on a developer's
//! machine, before a push, so CI confirms what the push already passed. Every
//! step in the job's order, each the same gate command CI runs, over the
//! commit CI would check, in the environment a runner gives it, with the
//! tools at CI's pins from the toolbelt `rust-gate setup` installs. The first
//! failing step ends the run, as it ends the job, and the scorecard still
//! says what ran; `--keep-going` runs every step whatever failed before it.

use super::cache;
use super::checkout::{Checkout, check_out, repository_root};
use super::environment::{Environment, INHERITED};
use super::job::{ALWAYS, APPLIED, CHECKS, Local, OTHER_JOBS, OUTCOMES, not_here};
use crate::checks::toolbelt::{install_toolbelt, local_ci_directory};
use crate::checks::workflow_home::is_workflow_home;
use crate::runner::{Failure, Outcome, Step, write};
use std::collections::BTreeMap;
use std::env;
use std::env::consts;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// The programs a local run starts itself: git for the checkout, the gate for
/// each step, and what installing the toolbelt runs.
const TOOLS: &[&str] = if cfg!(windows) {
    &[
        "cargo install",
        "curl",
        "git",
        "mise",
        "powershell.exe",
        "rust-gate",
        "tar",
    ]
} else {
    &["cargo install", "curl", "git", "mise", "rust-gate", "tar"]
};

/// What these commands declare: their inputs, their tools and their reports.
pub(crate) const STEPS: &[Step] = &[
    Step {
        workflow: "local",
        id: "ci --local",
        summary: "The checks job of ci.yml here, step by step, over the commit a push sends",
        inputs: INHERITED,
        tools: TOOLS,
        reports: &[],
        run: stop_at_first_failure,
    },
    Step {
        workflow: "local",
        id: "ci --local --keep-going",
        summary: "The same, every step run whatever failed before it",
        inputs: INHERITED,
        tools: TOOLS,
        reports: &[],
        run: keep_going,
    },
];

/// Run the job, ending at the first failing step.
fn stop_at_first_failure() -> Outcome {
    run_job(false)
}

/// Run the job, every step whatever failed before it.
fn keep_going() -> Outcome {
    run_job(true)
}

/// How one step of the job ended here.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ended {
    /// It ran and passed.
    Passed,
    /// It ran and failed.
    Failed,
    /// It did not run: an earlier step failed.
    Skipped,
    /// It does not apply on this machine.
    NotApplied,
}

impl Ended {
    /// The step's outcome as GitHub writes it, what the scorecard reads.
    fn outcome(self) -> &'static str {
        match self {
            Self::Passed => "success",
            Self::Failed => "failure",
            Self::Skipped | Self::NotApplied => "skipped",
        }
    }

    /// The word the summary shows.
    fn word(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "FAILED",
            Self::Skipped => "skipped",
            Self::NotApplied => "not applied",
        }
    }
}

/// One local run of the job.
struct Run {
    /// The environment of the next step.
    environment: Environment,
    /// Where this repository's runs keep their checkout, job and cache.
    state: PathBuf,
    /// The commit checked out, once it is.
    checkout: Option<Checkout>,
    /// Whether a failing step leaves the rest to run.
    keep_going: bool,
    /// How each gate step ended, by id.
    ended: BTreeMap<&'static str, Ended>,
    /// Each step's `applied` output, by id.
    applied: BTreeMap<&'static str, String>,
    /// Every step of the job in order: its name, how it ended, its seconds.
    rows: Vec<(&'static str, Ended, Option<f64>)>,
}

/// Run the `checks` job of `ci.yml` here.
fn run_job(keep_going: bool) -> Outcome {
    let environment = Environment::inherited()?;
    let root = PathBuf::from(repository_root(&environment)?);
    if is_workflow_home(&root) {
        return Err(Failure::from(
            "ci --local: maestro-rust-workflows runs its checks in just check, not through ci.yml",
        ));
    }
    let state = local_ci_directory(&root)?;
    let temp = fresh_job(&state)?;
    let mut run = Run {
        environment,
        state,
        checkout: None,
        keep_going,
        ended: BTreeMap::new(),
        applied: BTreeMap::new(),
        rows: Vec::new(),
    };
    let steps = run.steps(&root, &temp);
    let saved = cache::save(&run.state.join("cache"), &|name| {
        run.environment.get(name).to_owned()
    });
    steps?;
    saved?;
    run.report()
}

/// The job's temporary directory under `state`, made afresh as a runner's
/// is. A run stopped midway left its build directory there: it is kept first.
fn fresh_job(state: &Path) -> Result<PathBuf, Failure> {
    let temp = state.join("temp");
    if temp.exists() {
        let job = temp.display().to_string();
        let left = |name: &str| {
            if name == "RUNNER_TEMP" {
                job.clone()
            } else {
                String::new()
            }
        };
        cache::save(&state.join("cache"), &left)?;
        fs::remove_dir_all(&temp).map_err(|error| format!("{}: {error}", temp.display()))?;
    }
    fs::create_dir_all(temp.join("github"))
        .map_err(|error| format!("{}: {error}", temp.display()))?;
    Ok(temp)
}

impl Run {
    /// Every step of the job, in order.
    fn steps(&mut self, root: &Path, temp: &Path) -> Outcome {
        for (name, local) in CHECKS {
            println!("\n==> {name}");
            let started = Instant::now();
            let ended = match local {
                Local::Gate(id) => self.gate(id, temp)?,
                Local::Checkout => self.check_out(root, temp)?,
                Local::Toolbelt => {
                    let toolbelt = install_toolbelt()?;
                    self.environment.add_path(toolbelt.bin);
                    Ended::Passed
                }
                Local::Restore => {
                    let job = |variable: &str| self.environment.get(variable).to_owned();
                    let moved = cache::restore(&self.state.join("cache"), &job)?;
                    println!("restored {moved} kept directories of the last local run");
                    Ended::Passed
                }
                Local::NotApplied(reason) => {
                    println!("not applied locally: {reason}");
                    Ended::NotApplied
                }
            };
            let seconds = matches!(ended, Ended::Passed | Ended::Failed)
                .then(|| started.elapsed().as_secs_f64());
            self.rows.push((name, ended, seconds));
        }
        Ok(())
    }

    /// Check the commit out and reproduce CI inputs without hosted identity.
    fn check_out(&mut self, root: &Path, temp: &Path) -> Result<Ended, Failure> {
        let directory = self.state.join("checkout");
        let checkout = check_out(&self.environment, root, &directory)?;
        let workspace = fs::canonicalize(&directory)
            .map_err(|error| format!("{}: {error}", directory.display()))?;
        let event = if checkout.base.is_empty() {
            "push"
        } else {
            "pull_request"
        };
        let defaults = [
            ("CI", "true"),
            // Replay CI checks without claiming hosted runner identity.
            ("GITHUB_ACTIONS", "false"),
            ("GITHUB_EVENT_NAME", event),
            ("GITHUB_RUN_ID", "1"),
            ("GITHUB_RUN_ATTEMPT", "1"),
            // A run no workflow called: `validate` reads `[ci]` of the base
            // commit's maestro-quality.toml, as the ruleset's run does.
            ("CALLED", "false"),
        ];
        for (name, value) in defaults {
            self.environment.set(name, value);
        }
        let set = [
            ("GITHUB_WORKSPACE", workspace.display().to_string()),
            ("RUNNER_TEMP", temp.display().to_string()),
            ("GITHUB_SHA", checkout.revision.clone()),
            ("GITHUB_BASE_REF", checkout.base.clone()),
            ("GITHUB_HEAD_REF", checkout.branch.clone()),
            ("PULL_REQUEST_TITLE", checkout.title.clone()),
            // The organization's variable, which a run always passes, empty
            // when unset; here the developer's, if any.
            (
                "LICENSE_ALLOWLIST",
                self.environment.get("LICENSE_ALLOWLIST").to_owned(),
            ),
            (
                "GITHUB_STEP_SUMMARY",
                temp.join("summary.md").display().to_string(),
            ),
        ];
        for (name, value) in set {
            self.environment.set(name, &value);
        }
        println!("checked out {}", checkout.describe());
        if !checkout.title.is_empty() {
            println!("pull request title: {}", checkout.title);
        }
        self.checkout = Some(checkout);
        Ok(Ended::Passed)
    }

    /// Run `rust-gate <id>` as the step does, unless an earlier failure or
    /// this machine rules it out.
    fn gate(&mut self, id: &'static str, temp: &Path) -> Result<Ended, Failure> {
        let failed = self.ended.values().any(|ended| *ended == Ended::Failed);
        let validated = self.ended.get("validate") == Some(&Ended::Passed);
        let ended = if (failed && !self.keep_going && id != ALWAYS) || (id == ALWAYS && !validated)
        {
            println!("skipped: an earlier step failed");
            Ended::Skipped
        } else if let Some(reason) =
            not_here(id, consts::OS, on_path(&self.environment, "valgrind"))
        {
            println!("not applied locally: {reason}");
            Ended::NotApplied
        } else {
            self.start(id, temp)?
        };
        self.ended.insert(id, ended);
        Ok(ended)
    }

    /// Start the gate for `id` with the files GitHub gives a step, then take
    /// what it wrote to them.
    fn start(&mut self, id: &'static str, temp: &Path) -> Result<Ended, Failure> {
        let github = temp.join("github");
        let files = [
            ("GITHUB_ENV", github.join("env")),
            ("GITHUB_PATH", github.join("path")),
            ("GITHUB_OUTPUT", github.join("output")),
        ];
        for (name, file) in &files {
            write(file, b"", false)?;
            self.environment.set(name, &file.display().to_string());
        }
        if id == ALWAYS {
            self.hand_the_scorecard_its_steps();
        }
        let workspace = PathBuf::from(self.environment.get("GITHUB_WORKSPACE"));
        let outcome = self
            .environment
            .command("rust-gate")?
            .arg(id)
            .cwd(&workspace)
            .run();
        if let Err(Failure {
            message: Some(message),
            ..
        }) = &outcome
        {
            eprintln!("{message}");
        }
        let [(_, exports), (_, paths), (_, outputs)] =
            files.map(|(name, file)| (name, fs::read_to_string(file).unwrap_or_default()));
        if let Some(change) = self.environment.take(&exports, &paths) {
            println!("{change}");
        }
        if let Some(applied) = outputs
            .lines()
            .find_map(|line| line.strip_prefix("applied="))
        {
            self.applied.insert(id, applied.to_owned());
        }
        Ok(if outcome.is_ok() {
            Ended::Passed
        } else {
            Ended::Failed
        })
    }

    /// Set what `ci.yml` hands the scorecard: each step's outcome and whether
    /// it applied.
    fn hand_the_scorecard_its_steps(&mut self) {
        for (variable, id) in OUTCOMES {
            let ended = self.ended.get(id).copied().unwrap_or(Ended::Skipped);
            self.environment.set(variable, ended.outcome());
        }
        for (variable, id) in APPLIED {
            let applied = self.applied.get(id).cloned().unwrap_or_default();
            self.environment.set(variable, &applied);
        }
    }

    /// Print the summary, and fail when a step did.
    fn report(&self) -> Outcome {
        let described = self
            .checkout
            .as_ref()
            .map_or_else(String::new, Checkout::describe);
        println!("\n## ci --local over {described}\n");
        for (name, ended, seconds) in &self.rows {
            let time = seconds.map_or_else(String::new, |seconds| format!("{seconds:.1} s"));
            println!("{:<12} {time:>9}  {name}", ended.word());
        }
        for (job, reason) in OTHER_JOBS {
            println!(
                "{:<12} {:>9}  job {job}: {reason}",
                Ended::NotApplied.word(),
                ""
            );
        }
        let failed: Vec<&str> = self
            .rows
            .iter()
            .filter(|(_, ended, _)| *ended == Ended::Failed)
            .map(|(name, _, _)| *name)
            .collect();
        verdict(&failed)
    }
}

/// How the run ends: passed, or failed naming each step that failed.
fn verdict(failed: &[&str]) -> Outcome {
    match failed {
        [] => {
            println!("\nci --local: every step CI runs here passed");
            Ok(())
        }
        [one] => Err(Failure::from(format!("ci --local: one step failed: {one}"))),
        many => Err(Failure::from(format!(
            "ci --local: {} steps failed: {}",
            many.len(),
            many.join("; ")
        ))),
    }
}

/// Whether `program` is on the PATH the steps get.
fn on_path(environment: &Environment, program: &str) -> bool {
    let executable = format!("{program}{}", consts::EXE_SUFFIX);
    env::split_paths(environment.get("PATH")).any(|directory| directory.join(&executable).is_file())
}

#[cfg(test)]
mod tests {
    use super::{Ended, verdict};

    #[test]
    fn a_step_s_ending_reads_as_github_writes_its_outcome() {
        let outcomes: Vec<&str> = [
            Ended::Passed,
            Ended::Failed,
            Ended::Skipped,
            Ended::NotApplied,
        ]
        .iter()
        .map(|ended| ended.outcome())
        .collect();
        assert_eq!(outcomes, ["success", "failure", "skipped", "skipped"]);
        assert_eq!(Ended::NotApplied.word(), "not applied");
    }

    #[test]
    fn the_verdict_names_every_step_that_failed() {
        assert!(verdict(&[]).is_ok());
        let message = |failed: &[&str]| verdict(failed).unwrap_err().message.unwrap_or_default();
        assert_eq!(
            message(&["Mutation testing"]),
            "ci --local: one step failed: Mutation testing"
        );
        assert_eq!(
            message(&["Line coverage gate", "Mutation testing"]),
            "ci --local: 2 steps failed: Line coverage gate; Mutation testing"
        );
    }
}

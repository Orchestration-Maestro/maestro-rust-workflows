//! The step registry: every step declares what it reads, runs and writes,
//! the gate describes itself from those declarations, the workflows run
//! nothing the gate does not register, and the source uses nothing a step
//! did not declare.

use crate::harness::{
    Described, describe_text, described, described_step, root, rust_files, workflow_steps,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

#[test]
fn the_steps_document_is_the_one_the_gate_describes() {
    // docs/steps.md is generated, so a reader and the gate cannot disagree;
    // a stale copy is a failed test, and `just docs` is the fix.
    let committed = fs::read_to_string(root().join("docs/steps.md")).unwrap();
    assert!(
        committed == describe_text(),
        "docs/steps.md differs from rust-gate describe; run just docs"
    );
}

#[test]
fn every_workflow_body_runs_a_registered_step_and_every_step_is_run() {
    // A body naming a step the gate does not register fails on the runner;
    // a registered step no workflow runs is dead code with a document row.
    // A local command runs where a developer, a hook or the organization's
    // sync runs it, never in a workflow here; a contract test runs each one.
    let steps = described();
    let mut run: BTreeSet<(String, String)> = BTreeSet::new();
    for (name, _, step) in workflow_steps() {
        let Some(body) = step["run"].as_str() else {
            continue;
        };
        if !body.trim().starts_with("rust-gate ") {
            continue;
        }
        let found = described_step(&steps, body).unwrap_or_else(|| {
            panic!("{name}.yml runs {body:?}, which the gate does not register")
        });
        run.insert((found.workflow.clone(), found.id.clone()));
    }
    for step in steps.iter().filter(|step| step.workflow != "local") {
        assert!(
            run.contains(&(step.workflow.clone(), step.id.clone())),
            "no workflow runs rust-gate {} {}",
            step.workflow,
            step.id
        );
    }
    assert!(run.len() > 30, "only {} steps run", run.len());
}

#[test]
fn a_workflow_names_a_step_what_the_gate_says_it_does() {
    // The runner's log shows the step's `name:`; the document shows its
    // summary. One text, or a reader sees two descriptions of one step. The
    // shared commands carry the name of each call site instead.
    let steps = described();
    let mut checked = 0;
    for (name, _, step) in workflow_steps() {
        let Some(found) = step["run"]
            .as_str()
            .and_then(|body| described_step(&steps, body))
        else {
            continue;
        };
        if found.workflow == "shared" {
            continue;
        }
        assert_eq!(
            step["name"].as_str().unwrap_or_default(),
            found.summary,
            "{name}.yml names rust-gate {} {} differently from the gate",
            found.workflow,
            found.id
        );
        checked += 1;
    }
    assert!(checked > 30, "only {checked} step names checked");
}

/// The quoted names `pattern` introduces in `text`, such as `input("X")`.
fn quoted_after(text: &str, pattern: &str) -> BTreeSet<String> {
    text.match_indices(pattern)
        .filter_map(|(index, _)| {
            let rest = &text[index + pattern.len()..];
            let rest = rest.trim_start();
            let rest = rest.strip_prefix('"')?;
            Some(rest.split('"').next().unwrap_or_default().to_owned())
        })
        .collect()
}

/// Each check function body, so a step is charged with its reachable inputs,
/// tools and reports, including those of helpers it calls transitively.
fn reads_of_the_checks() -> BTreeMap<String, String> {
    let mut reads = BTreeMap::new();
    for entry in fs::read_dir(root().join("gate/src/checks")).unwrap() {
        let text = fs::read_to_string(entry.unwrap().path()).unwrap();
        reads.extend(reads_of_check_source(&text));
    }
    reads
}

/// The same scanner over a supplied check module, so synthetic regressions can
/// exercise its call propagation independently of the repository's current layout.
fn reads_of_check_source(text: &str) -> BTreeMap<String, String> {
    let text = text.split("#[cfg(test)]").next().unwrap_or_default();
    let mut reads = BTreeMap::new();
    for (index, _) in text.match_indices("fn ") {
        if index > 0 && !text[..index].ends_with('\n') && !text[..index].ends_with(' ') {
            continue;
        }
        let attributes = text[..index]
            .rsplit_once('\n')
            .map_or("", |(before, _)| before);
        if (attributes.trim_end().ends_with("#[cfg(windows)]") && !cfg!(windows))
            || (attributes.trim_end().ends_with("#[cfg(unix)]") && !cfg!(unix))
        {
            continue;
        }
        let body = &text[index..];
        let name = body[3..].split('(').next().unwrap_or_default().to_owned();
        if !name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
        {
            continue;
        }
        let end = body.find("\n}\n").unwrap_or(body.len());
        reads.insert(name, body[..end].to_owned());
    }
    reads
}

#[test]
fn called_checks_contribute_direct_and_transitive_tools_and_reports() {
    let checks = reads_of_check_source(
        r#"
fn shared_check() {
    nested_check();
}
fn nested_check() {
    input("CHECK_INPUT");
    Cmd::new("gh api");
    context.report("approval.json");
}
"#,
    );
    let [inputs, tools, reports] = names_used(
        "shared_check(); Cmd::new(\"jaq -e\"); context.report(\"direct.json\");",
        &checks,
    );
    assert!(inputs.contains("CHECK_INPUT"));
    assert_eq!(tools, BTreeSet::from(["gh".to_owned(), "jaq".to_owned()]));
    assert_eq!(
        reports,
        BTreeSet::from(["approval.json".to_owned(), "direct.json".to_owned()])
    );
}

#[test]
fn undeclared_transitive_usage_and_uncalled_declarations_are_not_hidden() {
    let checks = reads_of_check_source(
        r#"
fn shared_check() {
    nested_check();
}
fn nested_check() {
    Cmd::new("gh api");
    context.report("approval.json");
}
fn unused_check() {
    Cmd::new("curl");
    context.report("unused.json");
}
"#,
    );
    let [_, tools, reports] = names_used("shared_check(); Cmd::new(\"jaq -e\");", &checks);
    let declared = BTreeSet::from(["jaq".to_owned()]);
    assert_eq!(
        tools.difference(&declared).cloned().collect::<Vec<_>>(),
        ["gh"]
    );
    assert_eq!(
        reports.iter().cloned().collect::<Vec<_>>(),
        ["approval.json"]
    );
    assert!(!tools.contains("curl"));
    assert!(!reports.contains("unused.json"));
}

#[test]
fn checks_use_only_the_current_platform_function_body() {
    let checks = reads_of_check_source(
        r#"
#[cfg(unix)]
fn private_check() {
    Cmd::new("sh");
}
#[cfg(windows)]
fn private_check() {
    Cmd::new("powershell.exe");
}
"#,
    );
    let [_, tools, _] = names_used("private_check();", &checks);
    assert_eq!(tools, BTreeSet::from(["sh".to_owned()]));
}

/// The tool a command line runs, the way the gate keys it.
fn tool_key(words: &str) -> String {
    let mut parts = words
        .split_whitespace()
        .filter(|word| !word.starts_with('+'));
    let program = parts.next().unwrap_or_default();
    match (program, parts.next()) {
        ("cargo", Some(subcommand)) => format!("cargo {subcommand}"),
        _ => program.to_owned(),
    }
}

/// The inputs, tools and reports a module's source uses, outside its tests
/// and outside the declaration under test.
fn names_used(text: &str, checks: &BTreeMap<String, String>) -> [BTreeSet<String>; 3] {
    let runner_own = [
        "GITHUB_ENV",
        "GITHUB_OUTPUT",
        "GITHUB_PATH",
        "GITHUB_STEP_SUMMARY",
        "PROJECT",
        "REPORTS",
        "RUNNER_TEMP",
        "RUST_GATE_TRACE",
    ];
    let body = text.split("#[cfg(test)]").next().unwrap_or_default();
    // The declaration is the claim under test, not evidence for itself.
    let start = body.find("const STEPS: &[Step]").unwrap_or(body.len());
    let end = body[start..]
        .find("];")
        .map_or(body.len(), |offset| start + offset + 2);
    let mut body = format!("{}{}", &body[..start], &body[end..]);
    let mut reached = BTreeSet::new();
    while let Some((name, source)) = checks
        .iter()
        .find(|(name, _)| !reached.contains(*name) && body.contains(&format!("{name}(")))
    {
        reached.insert(name.clone());
        body.push('\n');
        body.push_str(source);
    }
    let mut inputs = BTreeSet::new();
    for pattern in ["input(", "optional(", "flag(", "path("] {
        inputs.extend(quoted_after(&body, pattern));
    }
    inputs.retain(|name| !runner_own.contains(&name.as_str()));
    let tools = quoted_after(&body, "Cmd::new(")
        .iter()
        .map(|words| tool_key(words))
        .collect();
    let mut reports = quoted_after(&body, ".report(");
    for (index, _) in body.match_indices(".report(&format!(\"") {
        let name = body[index + 18..].split('"').next().unwrap_or_default();
        reports.insert(format!(
            "*{}",
            name.trim_start_matches(|character: char| character != '.')
        ));
    }
    [inputs, tools, reports]
}

/// One step module's whole source: the file, or every file of its directory
/// when the step keeps an internal seam beside it. A name used in that seam is
/// used by the step, so the declaration has to cover it too.
fn step_source(path: &Path) -> String {
    if path.is_file() {
        return fs::read_to_string(path).unwrap();
    }
    let mut text = String::new();
    let files = rust_files(path);
    for child in files {
        // Each file loses its own test module first: concatenating whole files
        // would cut the step at the first `#[cfg(test)]` a seam happens to
        // carry, and the declaration below it would never be read.
        let child = fs::read_to_string(child).unwrap();
        text.push_str(child.split("#[cfg(test)]").next().unwrap_or_default());
        text.push('\n');
    }
    text
}

#[test]
fn windows_mutation_commands_are_declared_by_their_windows_steps() {
    let install_tools = fs::read_to_string(root().join("gate/src/steps/install_tools.rs")).unwrap();
    let windows_tools = install_tools
        .split("tools: if cfg!(windows) {")
        .nth(1)
        .unwrap()
        .split("} else {")
        .next()
        .unwrap();
    let declared_install_tools: BTreeSet<_> = windows_tools
        .split('\"')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect();
    let mut install_commands = BTreeSet::new();
    for path in [
        "gate/src/steps/install_tools.rs",
        "gate/src/checks/private_directories.rs",
    ] {
        let source = fs::read_to_string(root().join(path)).unwrap();
        let source = source.split("#[cfg(test)]").next().unwrap_or_default();
        install_commands.extend(
            quoted_after(source, "Cmd::new(")
                .iter()
                .map(|command| tool_key(command)),
        );
    }
    install_commands.remove("install"); // Unix-only executable installation.
    assert_eq!(
        install_commands, declared_install_tools,
        "Windows install-tools command set differs from its platform declaration"
    );

    let mutation_source =
        fs::read_to_string(root().join("gate/src/steps/mutation_testing/windows.rs")).unwrap();
    let mutation_commands: BTreeSet<_> = quoted_after(&mutation_source, "Cmd::new(")
        .iter()
        .map(|command| tool_key(command))
        .collect();
    let declarations =
        fs::read_to_string(root().join("gate/src/steps/mutation_testing/step.rs")).unwrap();
    let windows_step = declarations
        .split("id: \"mutants-windows\"")
        .nth(1)
        .unwrap()
        .split("run: super::windows::run")
        .next()
        .unwrap();
    let declared_mutation_tools: BTreeSet<_> = windows_step
        .split("tools: &[")
        .nth(1)
        .unwrap()
        .split(']')
        .next()
        .unwrap()
        .split('\"')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect();
    assert_eq!(
        mutation_commands, declared_mutation_tools,
        "Windows mutation command set differs from its step declaration"
    );
}

#[test]
fn every_step_module_declares_what_its_source_uses_and_nothing_else() {
    // The gate refuses an undeclared input, tool or report while a step runs,
    // which proves the declaration on the paths the contract tests exercise.
    // This reads the source for the rest: every name a module uses is
    // declared by one of its steps, and every declared name is used.
    let steps = described();
    let checks = reads_of_the_checks();
    let mut checked = 0;
    let mut visited: Vec<String> = Vec::new();
    for entry in fs::read_dir(root().join("gate/src/steps")).unwrap() {
        let path = entry.unwrap().path();
        let module = path.file_name().unwrap().to_string_lossy().into_owned();
        if module == "mod.rs" || module == "registry.rs" {
            continue;
        }
        let text = step_source(&path);
        // A module is named after what it does; its declaration names the
        // steps, so the described steps of this module are the ones whose
        // workflow and id the declaration spells.
        let start = text.find("const STEPS: &[Step]").unwrap_or(text.len());
        let end = text[start..]
            .find("];")
            .map_or(text.len(), |offset| start + offset + 2);
        let declaration = &text[start..end];
        let workflows = quoted_after(declaration, "workflow:");
        let ids = quoted_after(declaration, "id:");
        visited.extend(ids.iter().cloned());
        let declared: Vec<&Described> = steps
            .iter()
            .filter(|step| workflows.contains(&step.workflow) && ids.contains(&step.id))
            .collect();
        assert_eq!(
            declared.len(),
            ids.len(),
            "{module}: the gate describes {} of its {} declared steps",
            declared.len(),
            ids.len()
        );
        let [inputs, tools, reports] = names_used(&text, &checks);
        let body = text.split("#[cfg(test)]").next().unwrap_or_default();
        let start = body.find("const STEPS: &[Step]").unwrap_or(body.len());
        let end = body[start..]
            .find("];")
            .map_or(body.len(), |offset| start + offset + 2);
        let body = format!("{}{}", &body[..start], &body[end..]);
        for (kind, used, declared_names) in [
            (
                "input",
                &inputs,
                declared
                    .iter()
                    .flat_map(|step| step.inputs.iter().cloned())
                    .collect::<BTreeSet<_>>(),
            ),
            (
                "tool",
                &tools,
                declared
                    .iter()
                    .flat_map(|step| step.tools.iter().cloned())
                    .collect(),
            ),
            (
                "report",
                &reports,
                declared
                    .iter()
                    .flat_map(|step| step.reports.iter().cloned())
                    .collect(),
            ),
        ] {
            for name in used {
                assert!(
                    declared_names.contains(name),
                    "{module} uses {kind} {name} that no step of it declares"
                );
                checked += 1;
            }
            for name in &declared_names {
                assert!(
                    used.contains(name) || body.contains(&format!("\"{name}")),
                    "{module} declares {kind} {name} that its source never uses"
                );
            }
        }
    }
    assert!(checked > 0, "the walk checked no name at all");
    let seen: BTreeSet<&str> = visited.iter().map(String::as_str).collect();
    let registered: BTreeSet<&str> = steps.iter().map(|step| step.id.as_str()).collect();
    assert_eq!(
        seen, registered,
        "the walk did not reach every registered step"
    );
}

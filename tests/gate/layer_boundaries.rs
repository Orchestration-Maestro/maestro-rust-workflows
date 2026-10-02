//! This crate's own shape, beside the organization's rules: the layers are
//! ARC-004 and the cycles ARC-001, declared in `maestro-quality.toml` and held
//! by `rust-gate architecture` in `just check`. What stays here is particular
//! to the gate: a step's only door is its declaration, the checks door offers
//! modules, every seam with a function has unit tests, and no test module
//! imports the whole harness.

use crate::harness::{Fixture, root, test_sources};
use std::fs;
use std::path::{Path, PathBuf};

/// Every Rust file of a layer, keyed by its path below that layer, so a
/// module nested inside a step is held to the same rules as one beside it.
fn layer_files(layer: &Path) -> Vec<(String, PathBuf)> {
    let mut found = Vec::new();
    let mut queue = vec![layer.to_path_buf()];
    while let Some(current) = queue.pop() {
        for entry in fs::read_dir(&current).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                queue.push(path);
            } else if path.extension().is_some_and(|kind| kind == "rs") {
                found.push((
                    path.strip_prefix(layer).unwrap().display().to_string(),
                    path,
                ));
            }
        }
    }
    found.sort();
    found
}

#[test]
fn a_step_offers_only_its_declaration_and_reaches_no_sibling() {
    // A step's only door is its declaration, `STEPS`; a file inside a step's
    // directory reaches no further than the step; main.rs reaches into no
    // layer. The registry is the steps door's implementation, the one module
    // that names every step.
    let src = root().join("gate/src");
    let mut checked = 0;
    for (name, path) in layer_files(&src.join("steps")) {
        if name == "mod.rs" || name == "registry.rs" || name.ends_with("/mod.rs") {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        let body = text.split("#[cfg(test)]").next().unwrap_or_default();
        checked += 1;
        if name.contains('/') {
            let offered = body.lines().find(|line| {
                line.starts_with("pub(crate) ") && !line.starts_with("pub(crate) const STEPS:")
            });
            assert!(
                offered.is_none(),
                "{name} is a step's internal seam; it reaches no further than the step"
            );
            continue;
        }
        assert!(
            !body.contains("super::"),
            "{name} reaches a sibling step through super::"
        );
        for line in body.lines().filter(|line| line.starts_with("pub(crate) ")) {
            assert!(
                line.starts_with("pub(crate) const STEPS:"),
                "{name} exposes {line:?}; a step's only door is STEPS"
            );
        }
    }
    let main = fs::read_to_string(src.join("main.rs")).unwrap();
    assert!(
        !main.contains(concat!("crate", "::")),
        "main.rs reaches into a layer"
    );
    assert!(
        checked > 25,
        "only {checked} step modules checked; the walk drifted"
    );
}

#[test]
fn no_test_module_imports_the_whole_harness() {
    // A test module that imported the whole harness would hide which readers
    // it depends on; ARC-004 holds the rest of the harness door.
    let glob = concat!("use crate::harness::", "*");
    let mut checked = 0;
    for path in test_sources() {
        let text = fs::read_to_string(&path).unwrap();
        assert!(
            !text.contains(glob),
            "{} imports the whole harness; name what it uses",
            path.display()
        );
        checked += 1;
    }
    assert!(
        checked > 40,
        "only {checked} test files read; the walk drifted"
    );
}

/// Whether a seam has inline tests or a declared, test-only cases sibling.
fn has_unit_tests(path: &Path, text: &str) -> bool {
    if text
        .split_once("#[cfg(test)]")
        .is_some_and(|(_, tests)| tests.contains("#[test]"))
    {
        return true;
    }
    let stem = path.file_stem().unwrap().to_str().unwrap();
    let cases = if stem.ends_with("_cases") {
        stem.to_owned()
    } else {
        format!("{stem}_cases")
    };
    let directory = path.parent().unwrap();
    let door = fs::read_to_string(directory.join("mod.rs")).unwrap_or_default();
    let tokens: Vec<&str> = door.split_whitespace().collect();
    let declaration = format!("{cases};");
    let declared = tokens
        .windows(3)
        .any(|parts| parts == ["#[cfg(test)]", "mod", &declaration]);
    declared
        && fs::read_to_string(directory.join(format!("{cases}.rs")))
            .is_ok_and(|tests| tests.contains("#[test]"))
}

#[test]
fn siblings_require_a_test_declaration_and_real_test_items() {
    let fixture = Fixture::new();
    let seam = fixture.root.join("seam.rs");
    let cases = fixture.root.join("seam_cases.rs");
    let door = fixture.root.join("mod.rs");
    let source = "fn untested() {}\n";
    fs::write(&seam, source).unwrap();
    fs::write(&door, "mod seam;\n").unwrap();
    assert!(!has_unit_tests(&seam, source));
    fs::write(&cases, "#[test]\nfn a_real_test_is_present() {}\n").unwrap();
    assert!(!has_unit_tests(&seam, source));
    fs::write(&door, "mod seam;\nmod seam_cases;\n").unwrap();
    assert!(!has_unit_tests(&seam, source));
    fs::write(&door, "mod seam;\n#[cfg(test)]\nmod seam_cases;\n").unwrap();
    assert!(has_unit_tests(&seam, source));
    assert!(has_unit_tests(&cases, &fs::read_to_string(&cases).unwrap()));
    fs::write(&cases, "fn no_tests_are_present() {}\n").unwrap();
    assert!(!has_unit_tests(&seam, source));
    assert!(!has_unit_tests(
        &cases,
        &fs::read_to_string(&cases).unwrap()
    ));
}

#[test]
fn every_check_and_step_seam_with_a_function_has_unit_tests() {
    // A check is a rule several steps trust; a rule without a unit test is
    // held only by whichever contract test happens to cross it.
    let mut checked = 0;
    let mut modules = layer_files(&root().join("gate/src/checks"));
    modules.extend(
        layer_files(&root().join("gate/src/steps"))
            .into_iter()
            .filter(|(name, _)| name.contains('/') && !name.ends_with("/mod.rs")),
    );
    for (name, path) in modules {
        if name == "mod.rs" {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        let body = text.split("#[cfg(test)]").next().unwrap_or_default();
        if !body.contains("fn ") {
            continue;
        }
        assert!(
            has_unit_tests(&path, &text),
            "{name} defines functions and has no unit test"
        );
        checked += 1;
    }
    assert!(
        checked >= 4,
        "only {checked} checks modules scanned; the walk drifted"
    );
}

#[test]
fn the_checks_door_offers_modules_and_not_their_names() {
    // The runner's door names a vocabulary every step speaks, each name used
    // by many of them. The checks door held twenty-four names drawn from seven
    // unrelated concerns, so a call site named a rule without saying what kind
    // of rule it was. It offers the modules instead, and the import says it.
    let door = fs::read_to_string(root().join("gate/src/checks/mod.rs")).unwrap();
    let offered: Vec<&str> = door
        .lines()
        .filter(|line| line.starts_with("pub(crate) "))
        .collect();
    assert!(
        offered.len() >= 5,
        "only {} names offered; the door drifted",
        offered.len()
    );
    for line in offered {
        assert!(
            line.starts_with("pub(crate) mod "),
            "the checks door offers {line:?}; it offers modules, not their names"
        );
    }
}

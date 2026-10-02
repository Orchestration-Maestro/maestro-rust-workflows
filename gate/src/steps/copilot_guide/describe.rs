//! What a file is for, from what it says of itself: a Rust module's doc, a
//! Python docstring, a Markdown title or first sentence, a workflow's name
//! and the workflows it calls, a JSON ruleset or description, a manifest's
//! description, a first comment, or else its name and kind.

use super::text::{MANAGED, first_comment, module_doc, paragraph, sentence, title_of, value_of};
use crate::runner::Cmd;
use std::fs;
use std::path::Path;

/// What the guide adds to the explanation of a file `rust-gate sync` writes.
const RENDERED: &str = "; rendered by rust-gate sync";

/// The most characters of a file read to explain it.
const READ: usize = 20_000;

/// Files whose role their name fixes, whatever they contain.
const FIXED: [(&str, &str); 7] = [
    (
        "LICENSE",
        "The licence this repository is distributed under",
    ),
    (
        "Cargo.lock",
        "Exact dependency versions, committed so every build resolves the same",
    ),
    ("mise.lock", "The checksum of every pinned tool download"),
    ("CODEOWNERS", "Who reviews each path"),
    (
        "AGENTS.md",
        "Rules for coding agents: what to read, what never to weaken, how to verify",
    ),
    (
        "CONTEXT.md",
        "The words this repository uses, and the ones it avoids",
    ),
    (
        "copilot-instructions.md",
        "This guide, written by rust-gate guide at every commit",
    ),
];

/// What a file is for when it says nothing of itself.
const FALLBACK: [(&str, &str); 8] = [
    (".gitignore", "Paths git never tracks"),
    (
        ".pre-commit-config.yaml",
        "The commit hooks prek runs locally and CI runs over every file",
    ),
    (".yamlfmt.yml", "How yamlfmt formats every YAML file"),
    ("rust-toolchain.toml", "The pinned Rust toolchain"),
    (
        "justfile",
        "The local gate, just check, and every developer recipe",
    ),
    (
        "mise.toml",
        "The pinned toolbelt, installed by scripts/bootstrap.sh",
    ),
    (
        "version.txt",
        "The released version, which release-please moves",
    ),
    ("imports.lock", "The audits cargo-vet imports, locked"),
];

/// A file's kind, by extension, when nothing else explains it.
const KIND: [(&str, &str); 15] = [
    ("json", "JSON data"),
    ("svg", "SVG image"),
    ("png", "PNG image"),
    ("jpg", "JPEG image"),
    ("jpeg", "JPEG image"),
    ("webp", "WebP image"),
    ("gif", "GIF image"),
    ("txt", "Text"),
    ("sh", "Shell script"),
    ("py", "Python script"),
    ("rs", "Rust source"),
    ("md", "Document"),
    ("yml", "YAML settings"),
    ("yaml", "YAML settings"),
    ("toml", "TOML settings"),
];

/// Path parts under which a Markdown file is sample content, not
/// documentation.
const SAMPLES: [&str; 4] = ["examples", "expected", "fixtures", "testdata"];

/// The jaq program that explains a JSON file: a ruleset's name and rule
/// kinds, else its description, else its name, else nothing.
const JSON: &str = r#"if type != "object" then ""
elif (.rules | type) == "array" and (.name | type) == "string"
then "Ruleset \(.name): \([.rules[] | objects | .type // "?"] | unique | join(", "))"
else ([.description, .name] | map(strings | select(explode | map(select(. > 32)) | length > 0))
      | first // "")
end"#;

/// What the file at `relative`, under `root`, is for; a file `rust-gate
/// sync` writes says so.
pub(super) fn describe_file(root: &Path, relative: &str) -> String {
    let path = Path::new(relative);
    let text = read_start(&root.join(relative));
    let name = path.file_name().map_or_else(
        || relative.to_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    let found = said(root, path, &name, &text).unwrap_or_else(|| fallback(path, &name));
    let managed = text.lines().take(5).any(|line| line.contains(MANAGED));
    if managed && !found.contains(RENDERED) {
        format!("{found}{RENDERED}")
    } else {
        found
    }
}

/// The first `READ` characters of the file at `path`, empty when it cannot
/// be read.
fn read_start(path: &Path) -> String {
    fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).chars().take(READ).collect())
        .unwrap_or_default()
}

/// What a file says of itself, by its name and kind; None when it says
/// nothing.
fn said(root: &Path, path: &Path, name: &str, text: &str) -> Option<String> {
    if let Some((_, fixed)) = FIXED.iter().find(|(file, _)| *file == name) {
        return Some((*fixed).to_owned());
    }
    let found = match extension(path).as_str() {
        "rs" => module_doc(text).map(sentence),
        "py" => python_doc(text)
            .map(sentence)
            .or_else(|| first_comment(text)),
        "md" | "mdx" => markdown(text, path),
        "yml" | "yaml" => yaml(text, path),
        "json" => json(&root.join(path)),
        _ if name == "Cargo.toml" => Some(cargo(text)),
        "toml" | "sh" | "" => first_comment(text),
        _ if name.starts_with('.') => first_comment(text),
        _ => None,
    };
    found.filter(|found| !found.is_empty())
}

/// The extension of `path`, empty when it has none.
fn extension(path: &Path) -> String {
    path.extension()
        .map(|extension| extension.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// What a file is for when it says nothing of itself: its usual role, else
/// its kind and the words of its name.
fn fallback(path: &Path, name: &str) -> String {
    if let Some((_, role)) = FALLBACK.iter().find(|(file, _)| *file == name) {
        return (*role).to_owned();
    }
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().replace(['-', '_'], " "))
        .unwrap_or_default();
    let words = stem.trim_matches(['.', ' ']);
    let extension = extension(path);
    let kind = KIND
        .iter()
        .find(|(suffix, _)| *suffix == extension)
        .map_or("File", |(_, kind)| *kind);
    format!("{kind}: {}", if words.is_empty() { name } else { words })
}

/// The first paragraph of a Python module's docstring, past a shebang or
/// other comment lines.
fn python_doc(text: &str) -> Option<&str> {
    let mut rest = text.trim_start();
    while rest.starts_with('#') {
        rest = rest.split_once('\n')?.1;
    }
    let rest = rest.trim_start();
    let rest = rest
        .strip_prefix(['r', 'R'])
        .unwrap_or(rest)
        .strip_prefix("\"\"\"")?
        .trim_start();
    let quote = rest.get(1..)?.find("\"\"\"").map(|at| at + 1);
    let end = quote.into_iter().chain(blank_line(rest)).min()?;
    rest.get(..end)
}

/// Where the first blank line of `text` starts: a newline after its first
/// character whose following whitespace holds another newline.
fn blank_line(text: &str) -> Option<usize> {
    text.match_indices('\n')
        .map(|(at, _)| at)
        .filter(|at| *at >= 1)
        .find(|at| {
            text.get(at + 1..).is_some_and(|after| {
                after
                    .chars()
                    .take_while(|character| character.is_whitespace())
                    .any(|character| character == '\n')
            })
        })
}

/// A Markdown file's front-matter description, its title, or its first
/// sentence; a sample document says so.
fn markdown(text: &str, path: &Path) -> Option<String> {
    let body = match front_matter(text) {
        Some((front, body)) => {
            if let Some(described) = value_of(front, "description") {
                return Some(sentence(described));
            }
            body
        }
        None => text,
    };
    let title = title_of(body);
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let readme = path.file_name().is_some_and(|name| name == "README.md");
    if !readme && is_sample(path) {
        let named = title.map_or_else(|| stem.clone(), sentence);
        return Some(format!("Sample document: {named}"));
    }
    let titled = title.filter(|title| {
        !readme && title.trim_matches(['`', ' ']).to_lowercase() != stem.to_lowercase()
    });
    if let Some(title) = titled {
        return Some(sentence(title));
    }
    paragraph(body).or_else(|| title.map(sentence))
}

/// Whether `path` lies under a directory of sample content.
fn is_sample(path: &Path) -> bool {
    path.components()
        .any(|part| SAMPLES.contains(&part.as_os_str().to_string_lossy().as_ref()))
}

/// The front matter of `text`, between its opening `---` line and the next,
/// and what follows it.
fn front_matter(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix("---\n")?;
    let end = rest.find("\n---\n")?;
    Some((rest.get(..end)?, rest.get(end + 5..)?))
}

/// A workflow's name and the reusable workflows it calls, an issue form's
/// description, else a first comment, else a name.
fn yaml(text: &str, path: &Path) -> Option<String> {
    let name = value_of(text, "name");
    let workflow = text.lines().any(is_on) && text.lines().any(|line| line.starts_with("jobs:"));
    if let Some(name) = name.filter(|_| workflow) {
        let called = called_workflows(text);
        if called.is_empty() {
            return Some(name.to_owned());
        }
        return Some(format!("{name}: calls {}", called.join(", ")));
    }
    let form = path
        .components()
        .any(|part| part.as_os_str() == "ISSUE_TEMPLATE");
    if let Some(described) = value_of(text, "description").filter(|_| form) {
        return Some(sentence(described));
    }
    first_comment(text).or_else(|| name.map(str::to_owned))
}

/// Whether `line` opens a workflow's triggers: `on:`, quoted or not.
fn is_on(line: &str) -> bool {
    let rest = line.strip_prefix('"').unwrap_or(line);
    rest.strip_prefix("on")
        .is_some_and(|rest| rest.strip_prefix('"').unwrap_or(rest).starts_with(':'))
}

/// The reusable workflows `text` calls through `uses:`, by file name, sorted
/// and each once.
fn called_workflows(text: &str) -> Vec<String> {
    let mut called: Vec<String> = text
        .match_indices("uses:")
        .filter_map(|(at, _)| {
            let token = text.get(at + 5..)?.split_whitespace().next()?;
            let (target, _) = token.split_once('@')?;
            let (_, file) = target.rsplit_once("/.github/workflows/")?;
            let named = !file.is_empty()
                && file.chars().all(|character| {
                    character.is_alphanumeric() || matches!(character, '_' | '.' | '-')
                });
            named.then(|| file.to_owned())
        })
        .collect();
    called.sort();
    called.dedup();
    called
}

/// A manifest's package description, or what a workspace manifest holds.
fn cargo(text: &str) -> String {
    let described = text.lines().find_map(|line| {
        let rest = line
            .strip_prefix("description")?
            .trim_start()
            .strip_prefix('=')?
            .trim_start()
            .strip_prefix('"')?;
        let (value, _) = rest.rsplit_once('"')?;
        (!value.is_empty()).then_some(value)
    });
    if let Some(value) = described {
        return format!("Crate manifest: {}", sentence(value));
    }
    if text.lines().any(|line| line.starts_with("[workspace]")) {
        return "Workspace manifest: its members and the lints every member inherits".to_owned();
    }
    "Crate manifest".to_owned()
}

/// A JSON file's ruleset, description or name, read through the pinned jaq.
fn json(file: &Path) -> Option<String> {
    let found = Cmd::new("jaq -r").arg(JSON).arg(file).capture().ok()?;
    let found = sentence(found.trim_end_matches('\n'));
    (!found.is_empty()).then_some(found)
}

#[cfg(test)]
mod tests {
    use super::{called_workflows, cargo, fallback, front_matter, markdown, python_doc, yaml};
    use std::path::Path;

    #[test]
    fn a_python_module_is_explained_by_its_docstring() {
        let text = "#!/usr/bin/env python3\n\"\"\"Write the guide.\n\nMore.\n\"\"\"\n";
        assert_eq!(python_doc(text), Some("Write the guide."));
        assert_eq!(python_doc("r\"\"\"One line.\"\"\"\n"), Some("One line."));
        assert_eq!(python_doc("import os\n"), None);
    }

    #[test]
    fn markdown_prefers_front_matter_then_a_title_then_prose() {
        let front = "---\ndescription: \"From the front.\"\n---\n# Title\n";
        assert_eq!(
            front_matter(front).unwrap().0,
            "description: \"From the front.\""
        );
        assert_eq!(
            markdown(front, Path::new("a.md")).unwrap(),
            "From the front"
        );
        let titled = "# The design notes\n\nWhy.\n";
        assert_eq!(
            markdown(titled, Path::new("docs/notes.md")).unwrap(),
            "The design notes"
        );
        assert_eq!(
            markdown("# Notes\n\nWhy it is so.\n", Path::new("notes.md")).unwrap(),
            "Why it is so"
        );
        assert_eq!(
            markdown("# Input\n", Path::new("examples/in.md")).unwrap(),
            "Sample document: Input"
        );
    }

    #[test]
    fn a_workflow_names_what_it_calls_and_a_form_its_description() {
        let workflow = concat!(
            "name: Release\n\"on\": push\njobs:\n  a:\n",
            "    uses: org/repo/.github/workflows/b.yml@x\n",
            "  c:\n    uses: org/repo/.github/workflows/a.yml@y\n",
        );
        assert_eq!(called_workflows(workflow), ["a.yml", "b.yml"]);
        assert_eq!(
            yaml(workflow, Path::new("x.yml")).unwrap(),
            "Release: calls a.yml, b.yml"
        );
        let form = "name: Bug\ndescription: Report a defect. Please.\n";
        assert_eq!(
            yaml(form, Path::new(".github/ISSUE_TEMPLATE/bug.yml")).unwrap(),
            "Report a defect"
        );
        assert_eq!(yaml(form, Path::new("other.yml")).unwrap(), "Bug");
    }

    #[test]
    fn a_manifest_and_a_silent_file_are_explained_all_the_same() {
        assert_eq!(
            cargo("[package]\ndescription = \"Adds numbers.\"\n"),
            "Crate manifest: Adds numbers"
        );
        assert_eq!(
            cargo("[workspace]\nmembers = []\n"),
            "Workspace manifest: its members and the lints every member inherits"
        );
        assert_eq!(cargo("[package]\n"), "Crate manifest");
        assert_eq!(
            fallback(Path::new("assets/brand-mark.svg"), "brand-mark.svg"),
            "SVG image: brand mark"
        );
        assert_eq!(
            fallback(Path::new("a/justfile"), "justfile"),
            "The local gate, just check, and every developer recipe"
        );
        assert_eq!(fallback(Path::new("x.bin"), "x.bin"), "File: x");
    }
    use super::describe_file;
    use crate::checks::private_directories::private_directory;
    use std::{env, fs};

    #[test]
    fn file_kinds_choose_only_their_own_descriptions() {
        let root = private_directory(env::temp_dir().to_str().unwrap(), "guide-kinds").unwrap();
        for (name, text, expected) in [
            ("module.py", "\"\"\"Python purpose.\"\"\"", "Python purpose"),
            ("settings.toml", "# TOML purpose.\n", "TOML purpose"),
            ("run.sh", "# Shell purpose.\n", "Shell purpose"),
            ("command", "# Command purpose.\n", "Command purpose"),
            (".settings.ini", "# Hidden purpose.\n", "Hidden purpose"),
            ("settings.ini", "# Not a description.\n", "File: settings"),
            (
                "Cargo.toml",
                "description = \"Package purpose.\"\n",
                "Crate manifest: Package purpose",
            ),
            ("blank.py", "\"\"\"Intro\n\nMore.\"\"\"", "Intro"),
            (
                "multi.py",
                "\"\"\"First line\ncontinued\n\nMore.\"\"\"",
                "First line continued",
            ),
            (
                "front.md",
                "---\ntitle: ignored\n---\nBody purpose.\n",
                "Body purpose",
            ),
            (
                "managed.sh",
                "# generated by rust-gate\n# Task; rendered by rust-gate sync\n",
                "Task; rendered by rust-gate sync",
            ),
        ] {
            fs::write(root.join(name), text).unwrap();
            assert_eq!(describe_file(&root, name), expected, "{name}");
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn workflow_triggers_jobs_and_called_names_are_required() {
        let root = private_directory(env::temp_dir().to_str().unwrap(), "guide-yaml").unwrap();
        for (name, text, expected) in [
            (
                "no-jobs.yml",
                "name: Named\non: push\n    uses: org/r/.github/workflows/a.yml@x\n",
                "Named",
            ),
            (
                "no-on.yml",
                "name: Named\njobs:\n    uses: org/r/.github/workflows/a.yml@x\n",
                "Named",
            ),
            (
                "invalid.yml",
                concat!(
                    "name: Named\non: push\njobs:\n",
                    "    uses: org/r/.github/workflows/@x\n",
                    "    uses: org/r/.github/workflows/bad/name.yml@x\n",
                ),
                "Named",
            ),
            (
                "valid.yml",
                "name: Named\non: push\njobs:\n    uses: org/r/.github/workflows/a.yml@x\n",
                "Named: calls a.yml",
            ),
        ] {
            fs::write(root.join(name), text).unwrap();
            assert_eq!(describe_file(&root, name), expected, "{name}");
        }
        fs::remove_dir_all(root).unwrap();
    }
}

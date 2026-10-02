//! The module trees of a Cargo project, the way the compiler builds them:
//! every target `cargo metadata` reports, and for each the files its `mod`
//! declarations reach from its root, with the items, paths and re-exports of
//! each.

use super::cargo_metadata::tsv_fields;
use super::manifests::LIBRARY_KINDS;
use super::rust_code::{Item, blanked, items, without_tests};
use super::rust_paths::{NamedPath, paths, use_leaves};
use crate::runner::{Cmd, Failure};
use std::fs;
use std::path::{Path, PathBuf};

/// Cargo's targets, one line each: their package, their kinds and their root
/// file.
const TARGETS: &str = ".packages[] | .name as $package | .targets[] \
    | [$package, (.kind | join(\",\")), .src_path] | @tsv";

/// One target and the modules its root reaches.
pub(crate) struct Tree {
    /// The package the target belongs to.
    pub(crate) package: String,
    /// The target's kinds, as Cargo names them: `lib`, `bin`, `test`...
    pub(crate) kinds: Vec<String>,
    /// Every module, the root first.
    pub(crate) modules: Vec<Module>,
}

/// One module of a tree.
pub(crate) struct Module {
    /// Its path from the crate root: `[]` for the root, `["runner"]`.
    pub(crate) path: Vec<String>,
    /// The file holding it.
    pub(crate) file: PathBuf,
    /// The file as written.
    pub(crate) source: String,
    /// Its code with comments and literals blanked, test items included.
    pub(crate) code: String,
    /// Its top-level items.
    pub(crate) items: Vec<Item>,
    /// The paths it names outside its test items.
    pub(crate) paths: Vec<NamedPath>,
    /// Every name it re-exports with a `pub` visibility.
    pub(crate) exports: Vec<Export>,
}

/// One name a module re-exports.
pub(crate) struct Export {
    /// The line of the `use` declaring it.
    pub(crate) line: usize,
    /// The path it re-exports, as written from the module.
    pub(crate) segments: Vec<String>,
    /// Whether it is offered past the module's parent.
    pub(crate) offered: bool,
}

impl Module {
    /// The module at `path`, held in `file`, read from its source.
    pub(crate) fn read(path: Vec<String>, file: PathBuf, source: &str) -> Self {
        let code = blanked(source);
        let items = items(&code);
        let paths = paths(&without_tests(&code));
        let exports = items
            .iter()
            .filter(|item| item.kind == "use" && item.visibility.starts_with("pub"))
            .filter(|item| !item.is_test())
            .flat_map(|item| {
                use_leaves(&item.text).into_iter().map(|segments| Export {
                    line: item.line,
                    segments,
                    offered: item.is_offered(),
                })
            })
            .collect();
        Self {
            path,
            file,
            source: source.to_owned(),
            code,
            items,
            paths,
            exports,
        }
    }

    /// Whether the module is a directory's door, a `mod.rs`.
    pub(crate) fn is_mod_rs(&self) -> bool {
        self.file.file_name().is_some_and(|name| name == "mod.rs")
    }

    /// The modules it declares in files of their own, test modules left out.
    pub(crate) fn children(&self) -> impl Iterator<Item = &Item> {
        self.items
            .iter()
            .filter(|item| item.is_module_file() && !item.is_test())
    }
}

impl Tree {
    /// Whether the target is a library: its root is a crate's public door.
    pub(crate) fn is_library(&self) -> bool {
        self.kinds
            .iter()
            .any(|kind| LIBRARY_KINDS.contains(&kind.as_str()))
    }

    /// The index of the module at `path`.
    pub(crate) fn find(&self, path: &[String]) -> Option<usize> {
        self.modules.iter().position(|module| module.path == path)
    }

    /// The absolute path a named path stands for, seen from the module at
    /// `from`: `crate` starts at the root, `self` and a child's name where
    /// `from` is, and each `super` climbs one level. `None` for a path into
    /// another crate or above the root.
    pub(crate) fn absolute(&self, from: &[String], segments: &[String]) -> Option<Vec<String>> {
        let (first, tail) = segments.split_first()?;
        let (mut base, mut rest) = match first.as_str() {
            "crate" => (Vec::new(), tail),
            "self" => (from.to_vec(), tail),
            "super" => (from.to_vec(), segments),
            _ => {
                let mut child = from.to_vec();
                child.push(first.clone());
                self.find(&child)?;
                (from.to_vec(), segments)
            }
        };
        while rest.first().is_some_and(|segment| segment == "super") {
            base.pop()?;
            rest = rest.get(1..).unwrap_or_default();
        }
        base.extend(rest.iter().cloned());
        Some(base)
    }

    /// The module an absolute path lands in: its longest prefix that is a
    /// module.
    pub(crate) fn landing(&self, absolute: &[String]) -> Option<usize> {
        (0..=absolute.len())
            .rev()
            .find_map(|length| self.find(absolute.get(..length)?))
    }
}

/// Every target the Cargo metadata at `metadata` lists, and its module tree.
pub(crate) fn module_trees(metadata: &Path) -> Result<Vec<Tree>, Failure> {
    let listing = Cmd::new("jaq -r").arg(TARGETS).arg(metadata).capture()?;
    listing.lines().map(target_tree).collect()
}

/// The tree of the target one line of the listing names.
fn target_tree(line: &str) -> Result<Tree, Failure> {
    let fields = tsv_fields(line);
    let [package, kinds, root] = fields.as_slice() else {
        return Err(format!("cargo metadata listed a target the gate cannot read: {line}").into());
    };
    Ok(Tree {
        package: package.to_owned(),
        kinds: kinds.split(',').map(str::to_owned).collect(),
        modules: walk(Path::new(root))?,
    })
}

/// The modules a root file reaches through its `mod` declarations, the root
/// first. A declaration whose file is missing is left to the compiler.
fn walk(root: &Path) -> Result<Vec<Module>, Failure> {
    let mut modules = Vec::new();
    let mut pending = vec![(Vec::new(), root.to_path_buf())];
    while let Some((path, file)) = pending.pop() {
        let source =
            fs::read_to_string(&file).map_err(|error| format!("{}: {error}", file.display()))?;
        let module = Module::read(path, file, &source);
        let directory = children_directory(&module);
        for child in module.children() {
            if let Some(found) = child_file(&directory, &child.name) {
                let mut path = module.path.clone();
                path.push(child.name.clone());
                pending.push((path, found));
            }
        }
        modules.push(module);
    }
    modules.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(modules)
}

/// Where a module's children live: beside a root or a `mod.rs`, and in the
/// directory named after any other file.
fn children_directory(module: &Module) -> PathBuf {
    let parent = module
        .file
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    if module.path.is_empty() || module.is_mod_rs() {
        return parent;
    }
    parent.join(module.file.file_stem().unwrap_or_default())
}

/// The file of child `name` in `directory`: `name.rs`, else `name/mod.rs`.
fn child_file(directory: &Path, name: &str) -> Option<PathBuf> {
    [
        directory.join(format!("{name}.rs")),
        directory.join(name).join("mod.rs"),
    ]
    .into_iter()
    .find(|candidate| candidate.is_file())
}

/// A tree of in-memory modules for unit tests: each a file under `/w/src`
/// and its source, the module path read from the file name, the root first.
#[cfg(test)]
pub(crate) fn sample(kinds: &[&str], files: &[(&str, &str)]) -> Tree {
    let mut modules: Vec<Module> = files
        .iter()
        .map(|(file, source)| {
            let stem = file.trim_end_matches(".rs");
            let stem = stem.strip_suffix("/mod").unwrap_or(stem);
            let path = if matches!(stem, "lib" | "main") {
                Vec::new()
            } else {
                stem.split('/').map(str::to_owned).collect()
            };
            Module::read(path, PathBuf::from("/w/src").join(file), source)
        })
        .collect();
    modules.sort_by(|left, right| left.path.cmp(&right.path));
    Tree {
        package: "sample".to_owned(),
        kinds: kinds.iter().map(|kind| (*kind).to_owned()).collect(),
        modules,
    }
}

#[cfg(test)]
mod tests {
    use super::{Module, sample, target_tree, walk};
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use std::process;

    /// Owned segments.
    fn owned(path: &[&str]) -> Vec<String> {
        path.iter().map(|segment| (*segment).to_owned()).collect()
    }

    #[test]
    fn named_paths_become_absolute_from_the_root_a_parent_or_a_child() {
        let tree = sample(
            &["lib"],
            &[("lib.rs", "mod a;"), ("a.rs", "mod b;"), ("a/b.rs", "")],
        );
        let from = owned(&["a"]);
        let absolute = |from: &[String], path: &[&str]| tree.absolute(from, &owned(path));
        assert_eq!(
            absolute(&from, &["crate", "x", "Y"]),
            Some(owned(&["x", "Y"]))
        );
        assert_eq!(absolute(&from, &["self", "f"]), Some(owned(&["a", "f"])));
        assert_eq!(absolute(&from, &["b", "f"]), Some(owned(&["a", "b", "f"])));
        assert_eq!(
            absolute(&owned(&["a", "b"]), &["super", "super", "g"]),
            Some(owned(&["g"]))
        );
        assert_eq!(absolute(&from, &["super", "super", "g"]), None);
        assert_eq!(absolute(&from, &["std", "fs"]), None);
        assert_eq!(tree.landing(&owned(&["a", "b", "f"])), Some(2));
        assert_eq!(tree.landing(&owned(&["a", "f"])), Some(1));
    }

    #[test]
    fn a_door_records_what_it_re_exports_and_how_far() {
        let door = Module::read(
            owned(&["runner"]),
            PathBuf::from("/w/runner/mod.rs"),
            concat!(
                "mod commands;\npub(crate) use commands::{Cmd, write};\n",
                "pub(super) use commands::Job;\nuse private::Secret;\n",
                "pub(crate) fn not_an_export() {}\n#[cfg(test)]\nmod tests;\n",
            ),
        );
        let exports: Vec<(String, bool)> = door
            .exports
            .iter()
            .map(|export| (export.segments.join("::"), export.offered))
            .collect();
        assert_eq!(
            exports,
            [
                ("commands::Cmd".to_owned(), true),
                ("commands::write".to_owned(), true),
                ("commands::Job".to_owned(), false),
            ]
        );
        assert!(door.is_mod_rs());
        let children: Vec<&str> = door.children().map(|child| child.name.as_str()).collect();
        assert_eq!(children, ["commands"]);
    }

    #[test]
    fn a_walk_follows_declarations_into_both_file_layouts() {
        let root = env::temp_dir().join(format!("module-tree-{}", process::id()));
        fs::remove_dir_all(&root).ok();
        for directory in ["a", "b"] {
            fs::create_dir_all(root.join(directory)).unwrap();
        }
        for (file, source) in [
            (
                "lib.rs",
                "mod a;\nmod b;\nmod missing;\n#[cfg(test)]\nmod tests;\n",
            ),
            ("a.rs", "mod c;\n"),
            ("a/c.rs", ""),
            ("b/mod.rs", ""),
            ("tests.rs", ""),
        ] {
            fs::write(root.join(file), source).unwrap();
        }
        let modules = walk(&root.join("lib.rs")).unwrap();
        let paths: Vec<String> = modules
            .iter()
            .map(|module| module.path.join("::"))
            .collect();
        assert_eq!(paths, ["", "a", "a::c", "b"]);
        let listed = format!("maestro-core\tlib,rlib\t{}", root.join("lib.rs").display());
        let tree = target_tree(&listed).unwrap();
        assert_eq!(
            (tree.package.as_str(), tree.kinds.len()),
            ("maestro-core", 2)
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_listing_line_without_a_root_is_refused_by_name() {
        let error = target_tree("lib").err().unwrap();
        assert_eq!(
            error.message.as_deref(),
            Some("cargo metadata listed a target the gate cannot read: lib")
        );
        let error = target_tree("maestro-core\tlib").err().unwrap();
        assert_eq!(
            error.message.as_deref(),
            Some("cargo metadata listed a target the gate cannot read: maestro-core\tlib")
        );
    }
}

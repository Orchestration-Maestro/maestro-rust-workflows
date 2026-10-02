//! Paths as every step checks them: canonical forms, containment in the
//! checkout, the consumer's project directory, symlinks, and the Rust
//! sources of a tree.

use crate::runner::{Failure, Outcome, input};
use std::fs;
use std::path::{Path, PathBuf};

/// The real path of something that must exist, symlinks resolved.
pub(crate) fn canonical(path: &Path) -> Result<PathBuf, String> {
    fs::canonicalize(path).map_err(|error| format!("{}: {error}", path.display()))
}

/// Whether `path` is `root` itself or lies under it, compared by components.
pub(crate) fn inside(path: &Path, root: &Path) -> bool {
    path.starts_with(root)
}

/// Whether `path` lies under `root` without being `root` itself.
pub(crate) fn strictly_inside(path: &Path, root: &Path) -> bool {
    path.starts_with(root) && path != root
}

/// Whether the path itself is a symbolic link, without following it.
pub(crate) fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink())
}

/// The consumer's project directory, once `DIRECTORY` is confirmed to be a
/// simple relative path that stays inside the checkout.
pub(crate) fn project_directory() -> Result<PathBuf, Failure> {
    let root = canonical(Path::new(&input("GITHUB_WORKSPACE")?))?;
    let directory = input("DIRECTORY")?;
    let simple = directory == "."
        || (directory
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphanumeric() || character == '_')
            && directory
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "_./-".contains(character)));
    if !simple {
        return Err("working-directory must be a simple relative path".into());
    }
    if directory
        .split('/')
        .any(|part| part == ".." || part.starts_with('-'))
    {
        return Err("working-directory must not traverse or contain option-like components".into());
    }
    let project = canonical(&root.join(&directory))
        .map_err(|_| "working-directory does not exist inside checkout".to_owned())?;
    if !inside(&project, &root) {
        return Err("working-directory escapes checkout".into());
    }
    Ok(project)
}

/// A file the project must commit, confirmed to lie inside the checkout.
pub(crate) fn committed_file(project: &Path, name: &str) -> Outcome {
    let root = canonical(Path::new(&input("GITHUB_WORKSPACE")?))?;
    let file = canonical(&project.join(name))
        .map_err(|_| format!("{name} must be a file inside checkout"))?;
    if !file.is_file() || !inside(&file, &root) {
        return Err(format!("{name} must be a file inside checkout").into());
    }
    Ok(())
}

/// Every `.rs` file under `root`, build output and hidden directories left
/// out, in no particular order.
pub(crate) fn rust_sources(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = fs::read_dir(&directory)
            .map_err(|error| format!("{}: {error}", directory.display()))?;
        for entry in entries {
            let path = entry
                .map_err(|error| format!("{}: {error}", directory.display()))?
                .path();
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let rust = path.extension().is_some_and(|extension| extension == "rs");
            if path.is_dir() && name != "target" && !name.starts_with('.') {
                pending.push(path);
            } else if !path.is_dir() && rust {
                files.push(path);
            }
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::{canonical, inside, is_symlink, rust_sources, strictly_inside};
    use std::env;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    #[cfg(windows)]
    use std::os::windows::fs::symlink_file;
    use std::path::{Path, PathBuf};
    use std::process;

    /// A fresh tree: `src/a.rs`, `target/b.rs`, `.hidden/c.rs` and a link.
    fn tree(name: &str) -> PathBuf {
        let root = env::temp_dir().join(format!("checkout-paths-{}-{name}", process::id()));
        fs::remove_dir_all(&root).ok();
        for directory in ["src", "target", ".hidden"] {
            fs::create_dir_all(root.join(directory)).unwrap();
        }
        for file in ["src/a.rs", "target/b.rs", ".hidden/c.rs"] {
            fs::write(root.join(file), "").unwrap();
        }
        #[cfg(unix)]
        symlink("src/a.rs", root.join("link")).unwrap();
        #[cfg(windows)]
        symlink_file("src/a.rs", root.join("link")).unwrap();
        root
    }

    #[test]
    fn containment_compares_components_and_strictness_excludes_the_root() {
        let root = Path::new("/tmp/project");
        assert!(inside(&root.join("src"), root) && inside(root, root));
        assert!(!inside(Path::new("/tmp/project-two"), root));
        assert!(strictly_inside(&root.join("src"), root) && !strictly_inside(root, root));
    }

    #[test]
    fn rust_sources_skip_build_output_hidden_directories_and_links() {
        let root = tree("sources");
        assert_eq!(rust_sources(&root).unwrap(), [root.join("src/a.rs")]);
        assert!(is_symlink(&root.join("link")) && !is_symlink(&root.join("src/a.rs")));
        assert!(
            canonical(&root.join("missing"))
                .unwrap_err()
                .contains("missing")
        );
        fs::remove_dir_all(&root).unwrap();
    }
}

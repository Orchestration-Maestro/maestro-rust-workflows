//! Published root-child inventories exclude publication staging and build targets.

use std::fs;
use std::path::Path;

/// Sorted names, compared before and after execution, not contents of immutable entries.
pub(crate) fn published_inventory(root: &Path, selectors: &[String]) -> Result<String, String> {
    let mut names = Vec::new();
    for entry in fs::read_dir(root).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "native cache entry name is not UTF-8")?;
        if name.contains(['\n', '\r']) {
            return Err("native cache entry name contains a line break".to_owned());
        }
        if selectors
            .iter()
            .any(|selector| glob(selector.as_bytes(), name.as_bytes()))
        {
            names.push(name);
        }
    }
    names.sort();
    Ok(names.join("\n"))
}

/// Simple child globs: star matches any suffix and question mark matches one byte.
fn glob(pattern: &[u8], name: &[u8]) -> bool {
    match pattern.split_first() {
        None => name.is_empty(),
        Some((&b'*', tail)) => {
            (0..=name.len()).any(|offset| glob(tail, name.get(offset..).unwrap_or_default()))
        }
        Some((&head, tail)) => name
            .split_first()
            .is_some_and(|(&byte, rest)| (head == b'?' || head == byte) && glob(tail, rest)),
    }
}

#[cfg(test)]
mod tests {
    use super::glob;

    #[test]
    fn child_globs_match_only_selected_published_names() {
        for name in ["entry-one", "entry-two", "entry-"] {
            assert!(glob(b"entry-*", name.as_bytes()));
        }
        for name in ["staging-entry-one", "target", ".entry-one"] {
            assert!(!glob(b"entry-*", name.as_bytes()));
        }
        assert!(glob(b"entry-?", b"entry-a"));
        assert!(!glob(b"entry-?", b"entry-ab"));
        assert!(glob(b"entry-one", b"entry-one"));
        assert!(!glob(b"entry-one", b"entry-two"));
    }
    #[test]
    #[cfg(unix)]
    fn invalid_entry_names_cannot_enter_the_inventory_report() {
        use super::published_inventory;
        use crate::checks::private_directories::private_directory;
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        use std::{env, fs};
        let root = private_directory(
            &env::temp_dir().display().to_string(),
            "native-inventory-test",
        )
        .unwrap();
        fs::write(root.join("entry-\n"), "bytes").unwrap();
        assert_eq!(
            published_inventory(&root, &["entry-*".to_owned()]).unwrap_err(),
            "native cache entry name contains a line break"
        );
        fs::remove_file(root.join("entry-\n")).unwrap();
        fs::write(root.join(OsStr::from_bytes(b"entry-\xff")), "bytes").unwrap();
        assert_eq!(
            published_inventory(&root, &["entry-*".to_owned()]).unwrap_err(),
            "native cache entry name is not UTF-8"
        );
        fs::remove_dir_all(root).unwrap();
    }
}

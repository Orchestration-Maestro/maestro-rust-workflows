//! Owner-only restore roots. Never normalize links, foreign objects or special files.

use crate::checks::private_directories::{create_private, private_directory};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// Create the stable, fresh parent once; a collision selects only a random fallback.
pub(crate) fn prepare_root(temp: &Path) -> Result<(PathBuf, bool), String> {
    let parent = temp.join("native-cache");
    let attempt = || -> Result<PathBuf, String> {
        create_private(&parent).map_err(|error| format!("native cache parent: {error}"))?;
        let probe = private_directory(&temp.display().to_string(), "native-owner")?;
        let checked = private_parent(&parent, &probe);
        fs::remove_dir(&probe).map_err(|error| format!("native cache owner probe: {error}"))?;
        checked?;
        let root = parent.join("entries");
        create_private(&root).map_err(|error| format!("native cache root: {error}"))?;
        Ok(root)
    };
    match attempt() {
        Ok(root) => Ok((root, true)),
        Err(error) => {
            eprintln!("Native cache fallback: {error}");
            Ok((fallback_root(temp)?, false))
        }
    }
}

/// Empty safe root for source builds; no rejected bytes are moved into it.
pub(crate) fn fallback_root(temp: &Path) -> Result<PathBuf, String> {
    let root = private_directory(&temp.display().to_string(), "native-fallback")?;
    let probe = private_directory(&temp.display().to_string(), "native-owner")?;
    let checked = private_parent(&root, &probe);
    fs::remove_dir(&probe).map_err(|error| format!("native cache owner probe: {error}"))?;
    checked?;
    Ok(root)
}

/// Verify the fresh parent independently of umask and of any restored child.
#[cfg(unix)]
fn private_parent(parent: &Path, owner: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(parent).map_err(|error| error.to_string())?;
    let uid = fs::symlink_metadata(owner)
        .map_err(|error| error.to_string())?
        .uid();
    owned_directory(&metadata, uid)
}

/// The parent must be a directory owned by this process with exactly owner-only access.
#[cfg(unix)]
fn owned_directory(metadata: &fs::Metadata, uid: u32) -> Result<(), String> {
    if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o7777 != 0o700 {
        return Err("native cache parent is not an owned private directory".to_owned());
    }
    Ok(())
}

/// Recheck both object safety and the requested permissions after normalization.
#[cfg(unix)]
fn normalized_object(metadata: &fs::Metadata, uid: u32, mode: u32) -> Result<(), String> {
    safe_object(metadata, uid)?;
    if metadata.mode() & 0o7777 != mode {
        return Err("native cache permissions did not normalize".to_owned());
    }
    Ok(())
}

/// Windows never reaches root preparation or restoration.
#[cfg(windows)]
fn private_parent(_parent: &Path, _owner: &Path) -> Result<(), String> {
    Err("native cache is disabled on Windows".to_owned())
}

/// Validate every object before chmod, normalize owned objects, then recheck.
#[cfg(unix)]
pub(crate) fn normalize_restore(root: &Path, temp: &Path) -> Result<(), String> {
    let probe = fallback_root(temp)?;
    let uid = fs::symlink_metadata(&probe)
        .map_err(|error| error.to_string())?
        .uid();
    fs::remove_dir(&probe).map_err(|error| error.to_string())?;
    let parent = root.parent().ok_or("native cache root has no parent")?;
    let parent_meta = fs::symlink_metadata(parent).map_err(|error| error.to_string())?;
    owned_directory(&parent_meta, uid)?;
    let mut pending = vec![root.to_path_buf()];
    let mut objects = Vec::new();
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        safe_object(&metadata, uid)?;
        if metadata.is_dir() {
            for child in fs::read_dir(&path).map_err(|error| error.to_string())? {
                pending.push(child.map_err(|error| error.to_string())?.path());
            }
        }
        let mode = if metadata.is_dir() {
            0o700
        } else {
            0o600 | (metadata.mode() & 0o100)
        };
        objects.push((path, mode));
    }
    for (path, mode) in objects {
        fs::set_permissions(&path, fs::Permissions::from_mode(mode))
            .map_err(|error| error.to_string())?;
        let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
        normalized_object(&metadata, uid, mode)?;
    }
    Ok(())
}

/// Object checks are separate so foreign ownership can be tested without privileged chown.
#[cfg(unix)]
fn safe_object(metadata: &fs::Metadata, uid: u32) -> Result<(), String> {
    if metadata.uid() != uid {
        return Err("native cache object has a foreign owner".to_owned());
    }
    if !(metadata.is_dir() || metadata.is_file()) {
        return Err("native cache object is not a directory or regular file".to_owned());
    }
    if metadata.is_file() && metadata.nlink() != 1 {
        return Err("native cache file has multiple links".to_owned());
    }
    Ok(())
}

/// Fail closed even if a future caller forgets to disable Windows.
#[cfg(windows)]
pub(crate) fn normalize_restore(_root: &Path, _temp: &Path) -> Result<(), String> {
    Err("native cache is disabled on Windows".to_owned())
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::{
        normalize_restore, normalized_object, owned_directory, prepare_root, private_parent,
        safe_object,
    };
    #[cfg(unix)]
    use crate::checks::private_directories::private_directory;
    #[cfg(unix)]
    use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
    #[cfg(unix)]
    use std::path::Path;
    #[cfg(unix)]
    use std::{env, fs};

    #[test]
    #[cfg(unix)]
    fn existing_wrong_mode_and_symlink_parents_are_never_adopted() {
        for kind in ["existing", "mode", "symlink"] {
            let temp =
                private_directory(&env::temp_dir().display().to_string(), "native-test").unwrap();
            let parent = temp.join("native-cache");
            let mode = if kind == "mode" { 0o755 } else { 0o700 };
            if kind == "symlink" {
                symlink(&temp, &parent).unwrap();
            } else {
                fs::create_dir(&parent).unwrap();
                fs::set_permissions(&parent, fs::Permissions::from_mode(mode)).unwrap();
            }
            let (root, restore) = prepare_root(&temp).unwrap();
            assert!(!restore);
            assert!(!root.starts_with(parent));
            assert!(root.read_dir().unwrap().next().is_none());
            fs::remove_dir_all(temp).unwrap();
        }
    }

    #[test]
    #[cfg(unix)]
    fn foreign_ownership_and_nonprivate_modes_are_refused() {
        let temp =
            private_directory(&env::temp_dir().display().to_string(), "native-owner-test").unwrap();
        let metadata = fs::metadata(&temp).unwrap();
        assert_eq!(
            safe_object(&metadata, metadata.uid().wrapping_add(1)).unwrap_err(),
            "native cache object has a foreign owner"
        );
        fs::set_permissions(&temp, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            private_parent(&temp, &temp).unwrap_err(),
            "native cache parent is not an owned private directory"
        );
        fs::remove_dir_all(temp).unwrap();
    }
    #[test]
    #[cfg(unix)]
    fn restore_object_refusals_cover_links_owners_and_permission_rechecks() {
        let temp =
            private_directory(&env::temp_dir().display().to_string(), "native-check-test").unwrap();
        let parent = fs::metadata(&temp).unwrap();
        assert_eq!(
            owned_directory(&parent, parent.uid().wrapping_add(1)).unwrap_err(),
            "native cache parent is not an owned private directory"
        );
        assert_eq!(
            normalized_object(&parent, parent.uid(), 0o600).unwrap_err(),
            "native cache permissions did not normalize"
        );
        assert_eq!(
            normalize_restore(Path::new("/"), &temp).unwrap_err(),
            "native cache root has no parent"
        );
        let file = temp.join("file");
        fs::write(&file, "bytes").unwrap();
        fs::hard_link(&file, temp.join("other")).unwrap();
        assert_eq!(
            safe_object(&fs::symlink_metadata(&file).unwrap(), parent.uid()).unwrap_err(),
            "native cache file has multiple links"
        );
        symlink(&file, temp.join("link")).unwrap();
        assert_eq!(
            safe_object(
                &fs::symlink_metadata(temp.join("link")).unwrap(),
                parent.uid()
            )
            .unwrap_err(),
            "native cache object is not a directory or regular file"
        );
        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    #[cfg(windows)]
    fn windows_restore_helpers_stay_disabled_without_any_bytes() {
        use super::{normalize_restore, private_parent};
        use std::path::Path;
        assert_eq!(
            normalize_restore(Path::new("unused"), Path::new("unused")).unwrap_err(),
            "native cache is disabled on Windows"
        );
        assert_eq!(
            private_parent(Path::new("unused"), Path::new("unused")).unwrap_err(),
            "native cache is disabled on Windows"
        );
    }
    #[test]
    #[cfg(unix)]
    fn owned_restores_normalize_modes_while_preserving_owner_execute_bits() {
        let temp =
            private_directory(&env::temp_dir().display().to_string(), "native-modes-test").unwrap();
        let (root, enabled) = prepare_root(&temp).unwrap();
        assert!(enabled);
        let entry = root.join("entry-one");
        fs::create_dir(&entry).unwrap();
        fs::set_permissions(&entry, fs::Permissions::from_mode(0o755)).unwrap();
        let file = entry.join("library");
        fs::write(&file, "native bytes").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o777)).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o700)).unwrap();
        let metadata = fs::metadata(&file).unwrap();
        assert_eq!(
            owned_directory(&metadata, metadata.uid()).unwrap_err(),
            "native cache parent is not an owned private directory"
        );
        fs::set_permissions(&file, fs::Permissions::from_mode(0o777)).unwrap();
        normalize_restore(&root, &temp).unwrap();
        assert_eq!(fs::metadata(entry).unwrap().mode() & 0o7777, 0o700);
        assert_eq!(fs::metadata(&file).unwrap().mode() & 0o7777, 0o700);
        assert_eq!(fs::read_to_string(file).unwrap(), "native bytes");
        fs::remove_dir_all(temp).unwrap();
    }
}

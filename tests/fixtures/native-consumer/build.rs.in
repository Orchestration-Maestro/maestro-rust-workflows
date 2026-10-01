//! Tiny native publication contract. No engine, downloads or external compiler.

use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process;

/// Bind the fixture entry to its native target and profile and verify its exact bytes.
fn main() -> io::Result<()> {
    println!("cargo:rerun-if-env-changed=FIXTURE_NATIVE_CACHE_DIR");
    println!("cargo:rerun-if-env-changed=FIXTURE_NATIVE_BUILD_LOG");
    let target = env::var("TARGET").map_err(io::Error::other)?;
    let profile = env::var("PROFILE").map_err(io::Error::other)?;
    let key = format!("entry-v1-{target}-{profile}");
    let manifest = format!("native-fixture-v1\ntarget={target}\nprofile={profile}\npayload=9\n");
    let root = cache_root();
    let entry = root.as_ref().map(|root| root.join(&key));
    let reusable = entry
        .as_ref()
        .is_some_and(|entry| verified(entry, &manifest));
    if reusable {
        println!("cargo:warning=native fixture verified reuse");
    } else {
        if let Some(log) = env::var_os("FIXTURE_NATIVE_BUILD_LOG") {
            writeln!(
                OpenOptions::new().create(true).append(true).open(log)?,
                "source"
            )?;
        }
        println!("cargo:warning=native fixture source build");
        if let Some(root) = root {
            publish(&root, &key, &manifest)?;
        }
    }
    let out =
        PathBuf::from(env::var_os("OUT_DIR").ok_or_else(|| io::Error::other("OUT_DIR missing"))?);
    fs::write(
        out.join("native.rs"),
        "pub fn native_answer() -> u8 { 9 }\n",
    )
}

/// Windows builds from source even when a caller tries to supply the variable.
fn cache_root() -> Option<PathBuf> {
    if cfg!(windows) {
        return None;
    }
    let root = PathBuf::from(env::var_os("FIXTURE_NATIVE_CACHE_DIR")?);
    private_object(&root, true).then_some(root)
}

/// Restore transport is not inner verification: every fixture object is checked again.
#[cfg(unix)]
fn private_object(path: &Path, directory: bool) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    // A fresh OUT_DIR is owned by the build user, without requiring libc or id.
    let Some(out) = env::var_os("OUT_DIR") else {
        return false;
    };
    let Ok(owner) = fs::metadata(out) else {
        return false;
    };
    metadata.uid() == owner.uid()
        && metadata.mode() & 0o077 == 0
        && if directory {
            metadata.is_dir()
        } else {
            metadata.is_file() && metadata.nlink() == 1
        }
}

/// Windows never opts into cache reads or writes.
#[cfg(windows)]
fn private_object(_path: &Path, _directory: bool) -> bool {
    false
}

/// Complete manifests are immutable and byte-bound; missing or corrupt entries rebuild.
fn verified(entry: &Path, manifest: &str) -> bool {
    private_object(entry, true)
        && private_object(&entry.join("manifest"), false)
        && fs::read_to_string(entry.join("manifest")).is_ok_and(|bytes| bytes == manifest)
}

/// Publish a complete private entry by rename; a concurrent completed publisher wins safely.
fn publish(root: &Path, key: &str, manifest: &str) -> io::Result<()> {
    let staging = root.join(format!("staging-{}", process::id()));
    fs::create_dir(&staging)?;
    #[cfg(unix)]
    fs::set_permissions(&staging, fs::Permissions::from_mode(0o700))?;
    fs::write(staging.join("manifest"), manifest)?;
    #[cfg(unix)]
    fs::set_permissions(staging.join("manifest"), fs::Permissions::from_mode(0o600))?;
    if let Err(error) = fs::rename(&staging, root.join(key)) {
        fs::remove_dir_all(&staging)?;
        if !root.join(key).exists() {
            return Err(error);
        }
    }
    Ok(())
}

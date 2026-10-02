//! The organization's toolbelt, on a developer's machine and on a runner:
//! every tool the home's `mise.toml` pins, at the checksums its `mise.lock`
//! records for each platform, both read into the gate when it is built, so no
//! other repository keeps a toolbelt of its own. mise itself is fetched once,
//! its digest verified before it is unpacked; mise then installs the rest
//! under `--locked` into a per-user cache keyed by the gate's version,
//! reading no configuration but the gate's. A tool that publishes no build for
//! the platform is built from crates.io at its pinned version, as the
//! `# source:` line above its pin declares, and a `# skip:` line names the one
//! that cannot run there at all. Every tool is then linked into one directory
//! whose path never changes: the one entry a PATH needs. `rust-gate setup`
//! and the `hooks` step install it the same way.

use crate::checks::digests::sha256_hex;
use crate::checks::private_directories::private_directory;
use crate::runner::{Cmd, Failure, optional, write};
use std::env;
use std::env::consts;
use std::ffi::OsString;
use std::fs;
use std::io;
#[cfg(unix)]
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

/// The tools and their versions, read in from the home when the gate is built.
const MISE_TOML: &str = include_str!("../../../mise.toml");

/// The URL and checksum of every download on every platform, read in beside
/// them.
const MISE_LOCK: &str = include_str!("../../../mise.lock");

/// The release of the gate, which keys the cache.
const VERSION: &str = include_str!("../../../version.txt");

/// The mise release `scripts/bootstrap.sh` pins.
const MISE_VERSION: &str = "2026.9.9";

/// One platform the toolbelt is pinned for.
struct Platform {
    /// The operating system, as Rust names it.
    os: &'static str,
    /// The architecture, as Rust names it.
    arch: &'static str,
    /// The platform, as mise and `mise.lock` name it.
    name: &'static str,
    /// The end of the name of mise's own archive for it.
    archive: &'static str,
    /// The executable inside that archive.
    member: &'static str,
    /// The archive's SHA-256, as the release publishes it.
    sha256: &'static str,
}

/// Every platform the toolbelt is pinned for, with mise's build for each.
const PLATFORMS: [Platform; 5] = [
    Platform {
        os: "linux",
        arch: "x86_64",
        name: "linux-x64",
        archive: "linux-x64.tar.gz",
        member: "mise/bin/mise",
        sha256: "e4767e4854af5daeff2191b2bbdc94f834742a23efad591dbd33187861d41604",
    },
    Platform {
        os: "linux",
        arch: "aarch64",
        name: "linux-arm64",
        archive: "linux-arm64.tar.gz",
        member: "mise/bin/mise",
        sha256: "5f72efaa1265c9c3562ecf8d22e6623b5278700bd7ef1014a785d0ce1d1a90b2",
    },
    Platform {
        os: "macos",
        arch: "x86_64",
        name: "macos-x64",
        archive: "macos-x64.tar.gz",
        member: "mise/bin/mise",
        sha256: "e22ecf26ebdfbbf0d3ba2deaa3866699c555e50ad6d900894da0d7f206a4c95a",
    },
    Platform {
        os: "macos",
        arch: "aarch64",
        name: "macos-arm64",
        archive: "macos-arm64.tar.gz",
        member: "mise/bin/mise",
        sha256: "0f13937fb7c548c4f39e3faca914f8c591c5b43a150b80db8a178afc60d4f581",
    },
    Platform {
        os: "windows",
        arch: "x86_64",
        name: "windows-x64",
        archive: "windows-x64.zip",
        member: "mise/bin/mise.exe",
        sha256: "f758ee4afe061cccd4587c0108c147209a7cb2372704909a8b9d5e230203ec07",
    },
];

/// The toolbelt as installed: the directory every tool is linked into, and
/// each pinned tool this platform cannot run, with the reason.
pub(crate) struct Toolbelt {
    /// The one directory a PATH needs.
    pub(crate) bin: PathBuf,
    /// `tool: reason` for each tool a `# skip:` line leaves out here.
    pub(crate) skipped: Vec<String>,
}

/// The release of the gate whose toolbelt this is.
pub(crate) fn toolbelt_version() -> &'static str {
    VERSION.trim()
}

/// Install the toolbelt, or find it installed.
pub(crate) fn install_toolbelt() -> Result<Toolbelt, Failure> {
    let platform = toolbelt_platform(consts::OS, consts::ARCH)?;
    let root = toolbelt_cache(
        consts::OS,
        &optional("XDG_CACHE_HOME")?,
        &optional("HOME")?,
        &optional("LOCALAPPDATA")?,
    )?;
    let store = root.join(toolbelt_version());
    fs::create_dir_all(&store).map_err(|error| format!("{}: {error}", store.display()))?;
    for (name, text) in [("mise.toml", MISE_TOML), ("mise.lock", MISE_LOCK)] {
        write(&store.join(name), text.as_bytes(), false)?;
    }
    let mise = fetched_mise(&root, platform)?;
    let path = toolbelt_path(&mise)?;
    // --locked installs exactly the URLs the lock records and never rewrites it.
    mise_isolated(Cmd::new("mise install --locked"), &root, &store, &path).run()?;
    let listed = mise_isolated(Cmd::new("mise bin-paths"), &root, &store, &path).capture()?;
    let mut directories = vec![mise];
    directories.extend(listed.lines().map(PathBuf::from));
    let built = store.join("source");
    for (tool, _) in declared_gaps(MISE_TOML, "source", platform.name) {
        let version = pinned_version(MISE_TOML, &tool)?;
        Cmd::new("cargo install --locked --root")
            .arg(&built)
            .arg("--version")
            .arg(format!("={version}"))
            .arg(&tool)
            .cwd(&store)
            .run()?;
    }
    directories.push(built.join("bin"));
    let bin = root.join("bin");
    link_toolbelt(&bin, &directories)?;
    let skipped = declared_gaps(MISE_TOML, "skip", platform.name)
        .into_iter()
        .map(|(tool, reason)| format!("{tool}: {reason}"))
        .collect();
    Ok(Toolbelt { bin, skipped })
}

/// Where `rust-gate ci --local` keeps what one repository's runs share:
/// `maestro/ci` beside the toolbelts, one directory per repository, named
/// after `root` and the start of its path's digest. Outside the repository,
/// so Cargo finds neither its manifests nor its configuration above the
/// checkout a run makes there.
pub(crate) fn local_ci_directory(root: &Path) -> Result<PathBuf, Failure> {
    let tools = toolbelt_cache(
        consts::OS,
        &optional("XDG_CACHE_HOME")?,
        &optional("HOME")?,
        &optional("LOCALAPPDATA")?,
    )?;
    Ok(tools.with_file_name("ci").join(repository_key(root)))
}

/// The name of `root`'s directory under `maestro/ci`: its last component and
/// twelve digits of the digest of its whole path, so two clones never share.
fn repository_key(root: &Path) -> String {
    let name = root
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let digest: String = sha256_hex(root.display().to_string().as_bytes())
        .chars()
        .take(12)
        .collect();
    format!("{name}-{digest}")
}

/// `directory` ahead of the PATH this process was given.
pub(crate) fn toolbelt_path(directory: &Path) -> Result<OsString, Failure> {
    let inherited = optional("PATH")?;
    let directories = [directory.to_path_buf()]
        .into_iter()
        .chain(env::split_paths(&inherited));
    env::join_paths(directories).map_err(|error| Failure::from(format!("PATH: {error}")))
}

/// The platform `os` and `arch` name, or a refusal naming the ones pinned.
fn toolbelt_platform(os: &str, arch: &str) -> Result<&'static Platform, Failure> {
    PLATFORMS
        .iter()
        .find(|platform| platform.os == os && platform.arch == arch)
        .ok_or_else(|| {
            Failure::from(format!(
                "toolbelt: no toolbelt is pinned for {os} {arch}; it is pinned for linux-x64, \
                 linux-arm64, macos-x64, macos-arm64 and windows-x64"
            ))
        })
}

/// Where the toolbelts are cached: `maestro\tools` under `LOCALAPPDATA` on
/// Windows; elsewhere `maestro/tools` under an absolute `XDG_CACHE_HOME`, or
/// under `~/.cache`.
fn toolbelt_cache(
    os: &str,
    xdg_cache_home: &str,
    home: &str,
    local_app_data: &str,
) -> Result<PathBuf, Failure> {
    let cache = if os == "windows" {
        PathBuf::from(local_app_data)
    } else if Path::new(xdg_cache_home).is_absolute() {
        PathBuf::from(xdg_cache_home)
    } else {
        PathBuf::from(home).join(".cache")
    };
    if !cache.is_absolute() {
        return Err(Failure::from(
            "toolbelt: set HOME, XDG_CACHE_HOME or on Windows LOCALAPPDATA to an absolute \
             path, where the toolbelt is cached",
        ));
    }
    Ok(cache.join("maestro").join("tools"))
}

/// The tools a `# {kind}:` line of `toml` declares a gap for on `platform`,
/// each with its reason: `# source: tool | platform ... | reason`.
fn declared_gaps(toml: &str, kind: &str, platform: &str) -> Vec<(String, String)> {
    let prefix = format!("# {kind}: ");
    toml.lines()
        .filter_map(|line| line.strip_prefix(&prefix))
        .filter_map(|line| {
            let mut fields = line.splitn(3, '|').map(str::trim);
            let (tool, platforms, reason) = (fields.next()?, fields.next()?, fields.next()?);
            platforms
                .split_whitespace()
                .any(|named| named == platform)
                .then(|| (tool.to_owned(), reason.to_owned()))
        })
        .collect()
}

/// The version the `[tools]` table of `toml` pins `tool` at: `tool =
/// "1.0.0"` or `tool = { version = "1.0.0", ... }`. The `[tool_alias]` table
/// above it names the same tool after its backend, never a version.
fn pinned_version(toml: &str, tool: &str) -> Result<String, Failure> {
    toml.lines()
        .skip_while(|line| line.trim() != "[tools]")
        .find_map(|line| line.strip_prefix(&format!("{tool} = ")))
        .map(|pin| pin.strip_prefix("{ version = ").unwrap_or(pin))
        .and_then(|quoted| quoted.strip_prefix('"')?.split('"').next())
        .map(str::to_owned)
        .ok_or_else(|| Failure::from(format!("toolbelt: mise.toml pins no version of {tool}")))
}

/// The directory holding mise at [`MISE_VERSION`] for `platform` under
/// `root`, fetched and verified the first time.
fn fetched_mise(root: &Path, platform: &Platform) -> Result<PathBuf, Failure> {
    let directory = root.join("mise").join(MISE_VERSION);
    let executable = platform.member.rsplit('/').next().unwrap_or_default();
    if directory.join(executable).is_file() {
        return Ok(directory);
    }
    let downloads = private_directory(&root.display().to_string(), "mise-download")?;
    let outcome = unpacked_mise(&downloads, platform).and_then(|unpacked| {
        fs::create_dir_all(&directory)
            .and_then(|()| fs::rename(unpacked, directory.join(executable)))
            .map_err(|error| Failure::from(format!("{}: {error}", directory.display())))
    });
    fs::remove_dir_all(&downloads).ok();
    outcome.map(|()| directory)
}

/// Download mise's archive for `platform` into `downloads`, refuse it unless
/// its SHA-256 is the pinned one, and unpack the executable: its path. The
/// digest is checked before any parser reads a byte of it.
fn unpacked_mise(downloads: &Path, platform: &Platform) -> Result<PathBuf, Failure> {
    let name = format!("mise-v{MISE_VERSION}-{}", platform.archive);
    let archive = downloads.join(&name);
    // Seven retries, one second apart and doubling: two minutes of server
    // errors before the download fails, as `install-tools` waits.
    Cmd::new("curl --retry 7 --retry-all-errors --fail --silent --show-error --location --output")
        .arg(&archive)
        .arg(format!(
            "https://github.com/jdx/mise/releases/download/v{MISE_VERSION}/{name}"
        ))
        .run()?;
    let bytes = fs::read(&archive).map_err(|error| format!("{name}: {error}"))?;
    let digest = sha256_hex(&bytes);
    if digest != platform.sha256 {
        return Err(Failure::from(format!(
            "toolbelt: {name} has SHA-256 {digest}, not the pinned {}; nothing was unpacked",
            platform.sha256
        )));
    }
    // Windows' own tar reads a zip; a Git for Windows tar ahead of it on the
    // PATH does not.
    let mut unpack = Cmd::new("tar -xf")
        .arg(&archive)
        .arg("-C")
        .arg(downloads)
        .arg(platform.member);
    if let Some(path) = native_tar_path(platform.os)? {
        unpack = unpack.env("PATH", &path);
    }
    unpack.run()?;
    Ok(downloads.join(platform.member))
}

/// Windows' native tar directory, excluding a Git tar that cannot read ZIP archives.
fn native_tar_path(os: &str) -> Result<Option<PathBuf>, String> {
    if os == "windows" {
        Ok(Some(Path::new(&optional("SystemRoot")?).join("System32")))
    } else {
        Ok(None)
    }
}

/// `mise` run in `store` on the gate's configuration alone: its own data,
/// cache, state and global configuration, the store's `mise.toml` trusted,
/// and no configuration read above `root`, so a repository's own pins and
/// the user's never reach the toolbelt. Each download retries a server
/// error eight times, its waits growing, where mise's own three give up in
/// seconds.
fn mise_isolated(mise: Cmd, root: &Path, store: &Path, path: &OsString) -> Cmd {
    mise.cwd(store)
        .env("PATH", path)
        .env("MISE_DATA_DIR", &store.join("data"))
        .env("MISE_CACHE_DIR", &store.join("cache"))
        .env("MISE_STATE_DIR", &store.join("state"))
        .env("MISE_CONFIG_DIR", &store.join("config"))
        .env(
            "MISE_GLOBAL_CONFIG_FILE",
            &store.join("config").join("config.toml"),
        )
        .env("MISE_TRUSTED_CONFIG_PATHS", store)
        .env("MISE_CEILING_PATHS", root)
        .env("MISE_YES", "1")
        .env("MISE_HTTP_RETRIES", "8")
}

/// Empty `bin`, then link into it every executable of `directories`, the
/// first of one name winning.
fn link_toolbelt(bin: &Path, directories: &[PathBuf]) -> Result<(), Failure> {
    let failed = |error: io::Error| Failure::from(format!("{}: {error}", bin.display()));
    if fs::symlink_metadata(bin).is_ok() {
        fs::remove_dir_all(bin).map_err(failed)?;
    }
    fs::create_dir_all(bin).map_err(failed)?;
    for directory in directories {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let linked = bin.join(entry.file_name());
            if is_tool(&entry.path()) && fs::symlink_metadata(&linked).is_err() {
                link_tool(&entry.path(), &linked).map_err(failed)?;
            }
        }
    }
    Ok(())
}

/// Whether `file` is a regular file anyone may execute, links followed.
#[cfg(unix)]
fn is_tool(file: &Path) -> bool {
    fs::metadata(file).is_ok_and(|found| found.is_file() && found.permissions().mode() & 0o111 != 0)
}

/// Whether `file` is a Windows executable, links followed.
#[cfg(not(unix))]
fn is_tool(file: &Path) -> bool {
    fs::metadata(file).is_ok_and(|found| found.is_file())
        && file
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
}

/// A symbolic link at `linked` to `target`.
#[cfg(unix)]
fn link_tool(target: &Path, linked: &Path) -> io::Result<()> {
    symlink(target, linked)
}

/// A hard link at `linked` to what `target` resolves to: Windows lets any
/// user make one on the same volume, where a symbolic link needs a privilege.
#[cfg(not(unix))]
fn link_tool(target: &Path, linked: &Path) -> io::Result<()> {
    fs::hard_link(fs::canonicalize(target)?, linked)
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::link_toolbelt;
    use super::{
        MISE_LOCK, MISE_TOML, PLATFORMS, declared_gaps, pinned_version, repository_key,
        toolbelt_cache, toolbelt_platform, toolbelt_version,
    };
    use std::path::Path;
    #[cfg(unix)]
    use std::{env, fs, os::unix::fs::PermissionsExt, process};

    #[test]
    fn windows_archives_select_only_the_native_tar_directory() {
        use std::env;
        let system = env::var("SystemRoot").unwrap_or_default();
        assert_eq!(
            super::native_tar_path("windows").unwrap(),
            Some(Path::new(&system).join("System32"))
        );
        assert_eq!(super::native_tar_path("linux").unwrap(), None);
        assert_eq!(super::native_tar_path("macos").unwrap(), None);
    }

    #[test]
    #[cfg(windows)]
    fn windows_tool_links_filter_extensions_and_preserve_file_bytes() {
        use super::{is_tool, link_tool};
        use std::{env, fs, process};
        let root = env::temp_dir().join(format!("windows-tool-links-{}", process::id()));
        fs::create_dir(&root).unwrap();
        let target = root.join("tool.EXE");
        fs::write(&target, "tool bytes").unwrap();
        fs::write(root.join("readme"), "not executable").unwrap();
        fs::create_dir(root.join("directory.exe")).unwrap();
        assert!(is_tool(&target));
        assert!(!is_tool(&root.join("readme")));
        assert!(!is_tool(&root.join("directory.exe")));
        assert!(!is_tool(&root.join("missing.exe")));
        let linked = root.join("linked.exe");
        link_tool(&target, &linked).unwrap();
        assert_eq!(fs::read(&linked).unwrap(), b"tool bytes");
        fs::write(&target, "updated bytes").unwrap();
        assert_eq!(fs::read(&linked).unwrap(), b"updated bytes");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn five_platforms_are_pinned_and_the_refusal_names_them() {
        for platform in &PLATFORMS {
            let found = toolbelt_platform(platform.os, platform.arch).unwrap();
            assert_eq!(found.name, platform.name);
            assert!(MISE_LOCK.contains(&format!("\"platforms.{}\"", platform.name)));
        }
        assert_eq!(
            toolbelt_platform("freebsd", "x86_64")
                .err()
                .and_then(|refused| refused.message)
                .unwrap_or_default(),
            "toolbelt: no toolbelt is pinned for freebsd x86_64; it is pinned for linux-x64, \
             linux-arm64, macos-x64, macos-arm64 and windows-x64"
        );
    }

    #[test]
    fn the_cache_follows_the_platform_s_own_cache_directory() {
        let cache = |os, xdg, home, local| toolbelt_cache(os, xdg, home, local).unwrap();
        assert_eq!(
            cache("linux", "/x/cache", "/home/a", ""),
            Path::new("/x/cache/maestro/tools")
        );
        assert_eq!(
            cache("macos", "relative", "/home/a", ""),
            Path::new("/home/a/.cache/maestro/tools")
        );
        assert_eq!(
            cache("linux", "", "/home/a", "/ignored"),
            Path::new("/home/a/.cache/maestro/tools")
        );
        assert_eq!(
            toolbelt_cache("linux", "", "", "")
                .unwrap_err()
                .message
                .unwrap_or_default(),
            "toolbelt: set HOME, XDG_CACHE_HOME or on Windows LOCALAPPDATA to an absolute \
             path, where the toolbelt is cached"
        );
        assert!(!toolbelt_version().is_empty() && !toolbelt_version().contains('\n'));
    }

    #[test]
    fn each_clone_keeps_its_local_runs_apart() {
        let first = repository_key(Path::new("/work/core"));
        let second = repository_key(Path::new("/other/core"));
        assert!(first.starts_with("core-") && second.starts_with("core-"));
        assert_eq!(first.len(), "core-".len() + 12);
        assert_ne!(first, second);
        assert_eq!(first, repository_key(Path::new("/work/core")));
    }

    #[test]
    fn a_gap_line_names_its_tools_platforms_and_reason() {
        let toml = concat!(
            "[tool_alias]\n",
            "cargo-vet = \"github:mozilla/cargo-vet\"\n",
            "[tools]\n",
            "# source: cargo-vet | linux-arm64 macos-x64 | no build\n",
            "cargo-vet = { version = \"0.10.0\", os = [\"linux/x64\"] }\n",
            "# skip: gungraun-runner | windows-x64 | Valgrind\n",
            "just = \"1.58.0\"\n",
        );
        assert_eq!(
            declared_gaps(toml, "source", "macos-x64"),
            [("cargo-vet".to_owned(), "no build".to_owned())]
        );
        assert!(declared_gaps(toml, "source", "linux-x64").is_empty());
        assert_eq!(declared_gaps(toml, "skip", "windows-x64").len(), 1);
        assert_eq!(pinned_version(toml, "cargo-vet").unwrap(), "0.10.0");
        assert_eq!(pinned_version(toml, "just").unwrap(), "1.58.0");
        assert_eq!(
            pinned_version(toml, "typos")
                .unwrap_err()
                .message
                .unwrap_or_default(),
            "toolbelt: mise.toml pins no version of typos"
        );
        // Every source build the home's pins declare names a pinned version.
        for platform in &PLATFORMS {
            for (tool, _) in declared_gaps(MISE_TOML, "source", platform.name) {
                let version = pinned_version(MISE_TOML, &tool).unwrap();
                assert!(
                    version.split('.').all(|part| part.parse::<u32>().is_ok()),
                    "{tool} = {version}"
                );
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn every_executable_is_linked_once_and_a_stale_link_is_dropped() {
        let root = env::temp_dir().join(format!("toolbelt-links-{}", process::id()));
        fs::remove_dir_all(&root).ok();
        let (first, second, bin) = (root.join("first"), root.join("second"), root.join("bin"));
        for directory in [&first, &second, &bin] {
            fs::create_dir_all(directory).unwrap();
        }
        let executable = |path: &Path| {
            fs::write(path, "#!/bin/sh\n").unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        };
        executable(&first.join("typos"));
        executable(&second.join("typos"));
        executable(&second.join("taplo"));
        fs::write(second.join("README"), "not a tool\n").unwrap();
        fs::write(bin.join("stale"), "").unwrap();
        link_toolbelt(&bin, &[first.clone(), second, root.join("missing")]).unwrap();
        let mut names: Vec<String> = fs::read_dir(&bin)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["taplo", "typos"]);
        assert_eq!(
            fs::read_link(bin.join("typos")).unwrap(),
            first.join("typos")
        );
        fs::remove_dir_all(&root).unwrap();
    }
}

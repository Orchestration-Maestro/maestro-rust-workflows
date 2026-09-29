//! `rust-gate install-tools`: GitHub release assets downloaded directly,
//! every digest verified before anything is extracted, the executables
//! installed under the runner's temporary directory and put on the PATH of
//! every later step. This is how untrusted bytes become executables on a
//! runner, so it lives in one place.

use crate::checks::digests::sha256_hex;
use crate::checks::private_directories::private_directory;
use crate::checks::simple_names::{is_hex, simple};
use crate::runner::{Cmd, Outcome, Step, add_to_path, input, optional, path};
use std::fs;
use std::path::Path;

/// What this step declares: its inputs, its tools and its reports.
pub(crate) const STEPS: &[Step] = &[Step {
    workflow: "shared",
    id: "install-tools",
    summary: "Install pinned tools, each download verified by digest before extraction",
    inputs: &["RUNNER_OS", "TOOLS"],
    tools: &["curl", "install", "tar"],
    reports: &[],
    run,
}];

/// One verified tool asset and the file it contributes to PATH.
struct Tool {
    /// The installed command's name.
    name: String,
    /// The immutable release asset relative to GitHub.
    asset: String,
    /// The expected digest of the complete downloaded asset.
    digest: String,
    /// The release archive format.
    archive_kind: String,
    /// The executable path within the archive, or the asset itself.
    member: String,
}

/// Where every release asset is downloaded from.
const RELEASES: &str = "https://github.com";

/// Run the step. `TOOLS` holds one legacy Linux row or platform rows of
/// `<name> <platform> <asset> <sha256> <archive-kind> <member>`.
fn run() -> Outcome {
    let bin = path("RUNNER_TEMP")?.join("rust-tools/bin");
    fs::create_dir_all(&bin)
        .map_err(|error| format!("cannot create {}: {error}", bin.display()))?;
    let downloads = private_directory(&input("RUNNER_TEMP")?, "tool-downloads")?;
    let runner_os_input = optional("RUNNER_OS")?;
    let os = runner_os(if runner_os_input.is_empty() {
        "Linux"
    } else {
        &runner_os_input
    })?;
    let mut installed = 0;
    for line in input("TOOLS")?.lines() {
        let Some(tool) = parse_tool(line, os)? else {
            continue;
        };
        let Tool {
            name,
            asset,
            digest,
            archive_kind,
            member,
        } = tool;
        // An immutable release asset, never a branch or tag archive whose
        // bytes can change under a digest that then no longer matches.
        if !is_release_asset(&asset) {
            return Err(format!("Not a release asset path: {asset}").into());
        }
        let file_name = asset.rsplit('/').next().unwrap_or(&asset);
        let archive = downloads.join(file_name);
        // A reset connection or a run of server errors is transient: seven
        // retries, one second apart and doubling, wait two minutes in all.
        // Retrying every error also retries a missing asset, which only
        // delays the same refusal.
        Cmd::new(
            "curl --retry 7 --retry-all-errors --fail --silent --show-error --location --output",
        )
        .arg(&archive)
        .arg(format!("{RELEASES}/{asset}"))
        .run()?;
        // Verified before extraction: a tarball is parsed by tar, and bytes
        // nobody vouched for must not reach a parser.
        let bytes =
            fs::read(&archive).map_err(|error| format!("{}: {error}", archive.display()))?;
        verify_digest(&bytes, &digest, &name)?;
        extract(&archive, &downloads, &member, &archive_kind)?;
        let executable = if os == "windows" && !name.to_ascii_lowercase().ends_with(".exe") {
            format!("{name}.exe")
        } else {
            name.clone()
        };
        if os == "windows" {
            fs::copy(downloads.join(&member), bin.join(executable))
                .map_err(|error| format!("cannot install {name}: {error}"))?;
        } else {
            Cmd::new("install -m755")
                .arg(downloads.join(&member))
                .arg(bin.join(executable))
                .run()?;
        }
        installed += 1;
    }
    if installed == 0 {
        return Err("tools lists nothing to install".into());
    }
    fs::remove_dir_all(&downloads)
        .map_err(|error| format!("cannot remove {}: {error}", downloads.display()))?;
    add_to_path(&bin)
}

/// Resolve the runner's release-asset platform.
fn runner_os(value: &str) -> Result<&'static str, String> {
    match value {
        "Linux" => Ok("linux"),
        "Windows" => Ok("windows"),
        "macOS" => Ok("macos"),
        value => Err(format!("Unsupported runner OS: {value}")),
    }
}

/// Parse a legacy Linux row or an explicit platform row, skipping other platforms.
fn parse_tool(line: &str, platform: &str) -> Result<Option<Tool>, String> {
    let words: Vec<_> = line.split_whitespace().collect();
    let Some(name) = words.first().copied() else {
        return Ok(None);
    };
    if name.starts_with('#') {
        return Ok(None);
    }
    if !simple(name, "", "._-") {
        return Err(format!("Invalid tool name: {name}"));
    }
    let explicit = words
        .get(1)
        .is_some_and(|word| ["linux", "windows", "macos"].contains(word));
    let (asset_index, kind_index, member_index) = if explicit {
        if words.get(1).copied() != Some(platform) {
            return Ok(None);
        }
        (2, 4, 5)
    } else {
        if platform != "linux" {
            return Ok(None);
        }
        (1, usize::MAX, 3)
    };
    let asset = words.get(asset_index).copied().unwrap_or_default();
    if !is_release_asset(asset) {
        return Err(format!("Not a release asset path: {asset}"));
    }
    let digest_index = asset_index + 1;
    let digest = words.get(digest_index).copied().unwrap_or_default();
    if !is_hex(digest, 64) {
        return Err(format!("Invalid sha256 for {name}"));
    }
    let file_name = asset.rsplit('/').next().unwrap_or(asset);
    let archive_kind = if kind_index == usize::MAX {
        if file_name.to_ascii_lowercase().ends_with(".tar.gz")
            || file_name.to_ascii_lowercase().ends_with(".tgz")
        {
            "tar.gz"
        } else if file_name.to_ascii_lowercase().ends_with(".tar.xz") {
            "tar.xz"
        } else {
            "file"
        }
    } else {
        words.get(kind_index).copied().unwrap_or_default()
    };
    if !["tar.gz", "tar.xz", "zip", "file"].contains(&archive_kind) {
        return Err(format!("Invalid archive kind for {name}: {archive_kind}"));
    }
    let member = words
        .get(member_index..)
        .filter(|rest| !rest.is_empty())
        .map_or_else(|| name.to_owned(), |rest| rest.join(" "));
    if !is_member(&member) {
        return Err(format!("Invalid archive member for {name}"));
    }
    Ok(Some(Tool {
        name: name.to_owned(),
        asset: asset.to_owned(),
        digest: digest.to_owned(),
        archive_kind: archive_kind.to_owned(),
        member,
    }))
}

/// Verify bytes before any archive parser can inspect them.
fn verify_digest(bytes: &[u8], expected: &str, name: &str) -> Result<(), String> {
    if sha256_hex(bytes) == expected {
        Ok(())
    } else {
        Err(format!("sha256 mismatch for {name}; refusing extraction"))
    }
}

/// Extract only the named file, after the complete archive digest passed.
fn extract(archive: &Path, directory: &Path, member: &str, kind: &str) -> Outcome {
    if kind == "file" {
        fs::copy(archive, directory.join(member))
            .map_err(|error| format!("cannot copy {}: {error}", archive.display()))?;
        return Ok(());
    }
    let (command, arguments) = extraction_arguments(kind, archive, directory, member)?;
    Cmd::new(command).args(arguments).run()
}

/// Select the native tar arguments for one archive kind.
fn extraction_arguments(
    kind: &str,
    archive: &Path,
    directory: &Path,
    member: &str,
) -> Result<(&'static str, Vec<String>), String> {
    let flags = match kind {
        "tar.gz" => "-xzf",
        "tar.xz" => "-xJf",
        "zip" => "-xf",
        _ => return Err(format!("Unsupported archive kind: {kind}")),
    };
    Ok((
        "tar",
        vec![
            flags.to_owned(),
            archive.display().to_string(),
            "-C".to_owned(),
            directory.display().to_string(),
            member.to_owned(),
        ],
    ))
}

/// An immutable release asset path, `owner/repo/releases/download/tag/asset`,
/// with no traversal anywhere in it.
fn is_release_asset(value: &str) -> bool {
    let parts: Vec<&str> = value.split('/').collect();
    let label = |part: &str, extra: &str| {
        !part.is_empty()
            && part.chars().all(|character| {
                character.is_ascii_alphanumeric()
                    || "._-".contains(character)
                    || extra.contains(character)
            })
    };
    matches!(parts.as_slice(), [owner, repository, "releases", "download", tag, file]
        if label(owner, "") && label(repository, "") && label(tag, "%") && label(file, ""))
        && !value.contains("..")
}

/// A path inside an archive: relative, no traversal, no empty component.
fn is_member(value: &str) -> bool {
    !value.contains("..")
        && value.split('/').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
        })
        && !value.starts_with('/')
}

#[cfg(test)]
mod tests {
    use super::{
        extraction_arguments, is_member, is_release_asset, parse_tool, runner_os, simple,
        verify_digest,
    };
    use std::path::Path;

    #[test]
    fn runner_os_names_the_supported_asset_platforms() {
        assert_eq!(runner_os("Windows"), Ok("windows"));
        assert_eq!(
            runner_os("FreeBSD").unwrap_err(),
            "Unsupported runner OS: FreeBSD"
        );
    }

    #[test]
    fn platform_rows_carry_their_assets_digests_and_archive_kind() {
        let linux = parse_tool(
            concat!(
                "cargo-mutants linux sourcefrog/cargo-mutants/releases/download/v27.1.0/",
                "cargo-mutants-x86_64-unknown-linux-gnu.tar.gz ",
                "dfe6dc37d0342c891d2829b5a695aa57c2d0edecef7e7d0399a30cc6e206411e ",
                "tar.gz cargo-mutants"
            ),
            "linux",
        )
        .unwrap()
        .unwrap();
        let windows = parse_tool(
            concat!(
                "cargo-mutants windows sourcefrog/cargo-mutants/releases/download/v27.1.0/",
                "cargo-mutants-x86_64-pc-windows-msvc.zip ",
                "2a2f00e47d4b458262a41501b0820aa26015fd35779903d2c8b30b2993f36791 ",
                "zip cargo-mutants.exe"
            ),
            "windows",
        )
        .unwrap()
        .unwrap();
        assert_eq!(linux.archive_kind, "tar.gz");
        assert_eq!(windows.archive_kind, "zip");
        assert_eq!(windows.member, "cargo-mutants.exe");
        assert!(
            parse_tool("cargo-mutants windows asset bad zip file", "linux")
                .unwrap()
                .is_none()
        );
        let invalid_kind = concat!(
            "cargo-mutants windows sourcefrog/cargo-mutants/releases/download/v27.1.0/asset.zip ",
            "2a2f00e47d4b458262a41501b0820aa26015fd35779903d2c8b30b2993f36791 ",
            "rar cargo-mutants.exe"
        );
        assert_eq!(
            parse_tool(invalid_kind, "windows").err().as_deref(),
            Some("Invalid archive kind for cargo-mutants: rar")
        );
    }

    #[test]
    fn zip_extraction_uses_bsd_tar_with_the_named_windows_executable() {
        let (command, arguments) = extraction_arguments(
            "zip",
            Path::new("cargo-mutants.zip"),
            Path::new("downloads"),
            "cargo-mutants.exe",
        )
        .unwrap();
        assert_eq!(command, "tar");
        assert_eq!(
            arguments,
            [
                "-xf",
                "cargo-mutants.zip",
                "-C",
                "downloads",
                "cargo-mutants.exe"
            ]
        );
        assert_eq!(
            extraction_arguments("rar", Path::new("unknown.rar"), Path::new("downloads"), "x")
                .unwrap_err(),
            "Unsupported archive kind: rar"
        );
    }

    #[test]
    fn a_digest_mismatch_refuses_the_asset_before_extraction() {
        assert_eq!(
            verify_digest(b"tampered", "00", "cargo-mutants").unwrap_err(),
            "sha256 mismatch for cargo-mutants; refusing extraction"
        );
    }

    #[test]
    fn only_release_assets_are_installed() {
        assert!(is_release_asset(
            "01mf02/jaq/releases/download/v3.1.1/jaq-x86_64-unknown-linux-gnu"
        ));
        assert!(is_release_asset(
            "rustsec/rustsec/releases/download/cargo-audit%2Fv0.22.2/cargo-audit.tgz"
        ));
        for bad in [
            "https://evil.example/jaq",
            "01mf02/jaq/archive/refs/tags/v3.1.1.tar.gz",
            "../jaq/releases/download/t/a",
        ] {
            assert!(!is_release_asset(bad), "{bad}");
        }
        assert!(
            is_member("cargo-vet-x86_64-unknown-linux-gnu/cargo-vet")
                && !is_member("../../bin/jaq")
        );
        assert!(simple("cargo-vet", "", "._-") && !simple("../jaq", "", "._-"));
    }
}

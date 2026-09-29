//! Tool pins stay current: mise.lock is the one record of every downloaded
//! tool, each workflow installs exactly what it locked, and `just update-tools`
//! moves a pin everywhere at once.

use crate::harness::{root, succeeds, temp_dir, tool, workflow, write_executable};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// Every `rust-gate install-tools` row across the workflows: name, asset,
/// digest, and the archive member when there is one.
fn install_rows(dir: &Path) -> Vec<(String, String, String, Option<String>)> {
    let mut rows = Vec::new();
    for entry in fs::read_dir(dir.join(".github/workflows")).unwrap() {
        let text = fs::read_to_string(entry.unwrap().path()).unwrap();
        for line in text.lines() {
            let line = line.trim().trim_start_matches("TOOLS: ");
            let words: Vec<&str> = line.split_whitespace().collect();
            let (asset, digest, member) = match words.as_slice() {
                [
                    _,
                    platform @ ("linux" | "windows" | "macos"),
                    asset,
                    digest,
                    _,
                    members @ ..,
                ] => {
                    let locked_asset = locked_platform(dir, words[0], platform, "url");
                    let locked_digest = locked_platform(dir, words[0], platform, "checksum");
                    assert_eq!(
                        format!("https://github.com/{}", asset.replace("%2F", "/")),
                        locked_asset,
                        "{} {platform}",
                        words[0]
                    );
                    assert_eq!(
                        format!("sha256:{digest}"),
                        locked_digest,
                        "{} {platform}",
                        words[0]
                    );
                    (*asset, *digest, members.first().copied())
                }
                [name, asset, digest, members @ ..] if asset.contains("/releases/download/") => {
                    let _ = name;
                    (*asset, *digest, members.first().copied())
                }
                _ => continue,
            };
            rows.push((
                words[0].to_owned(),
                asset.to_owned(),
                digest.to_owned(),
                member.map(str::to_owned),
            ));
        }
    }
    rows
}

fn locked(dir: &Path, name: &str, field: &str) -> String {
    locked_platform(dir, name, "linux", field)
}

fn locked_platform(dir: &Path, name: &str, platform: &str, field: &str) -> String {
    let output = tool("jaq")
        .args([
            "-r", "--from", "toml", "--arg", "t", name, "--arg", "p", platform, "--arg", "f", field,
        ])
        .arg(r#".tools[$t][0][("platforms." + $p + "-x64")][$f] // "missing""#)
        .arg(dir.join("mise.lock"))
        .output()
        .unwrap();
    succeeds(&output);
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
fn every_install_row_is_the_asset_mise_locked() {
    // Two records of one download drift apart; the lock is the one a tool
    // update rewrites, so every workflow row must say what it says. A lock
    // that keeps another hash function than SHA-256 still pins the URL.
    // mise installs the rest, so scripts/bootstrap.sh pins mise itself.
    let rows = install_rows(&root());
    assert!(rows.len() > 20, "only {} install rows", rows.len());
    let bootstrap = fs::read_to_string(root().join("scripts/bootstrap.sh")).unwrap();
    let pinned = |key: &str| {
        bootstrap
            .lines()
            .find_map(|line| line.strip_prefix(&format!("readonly {key}=")))
            .unwrap()
            .to_owned()
    };
    for (name, asset, digest, _) in rows {
        let platform = if asset.contains("windows-msvc") {
            "windows"
        } else if asset.contains("apple-darwin") {
            "macos"
        } else {
            "linux"
        };
        let url = format!("https://github.com/{}", asset.replace("%2F", "/"));
        // The Codecov CLI runs only in ci.yml's upload job, never locally, so
        // mise does not lock it; update-tools checks its row against the
        // digest GitHub records for the release asset.
        if name == "codecov" {
            assert!(
                asset.starts_with("codecov/codecov-cli/releases/download/v")
                    && asset.ends_with("/codecovcli_linux"),
                "{asset}"
            );
            assert!(digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
            continue;
        }
        if name == "mise" {
            let version = pinned("MISE_VERSION");
            assert_eq!(
                url,
                format!(
                    "https://github.com/jdx/mise/releases/download/{version}/\
                     mise-{version}-linux-x64.tar.gz"
                )
            );
            assert_eq!(digest, pinned("MISE_SHA256"));
            continue;
        }
        assert_eq!(
            locked_platform(&root(), &name, platform, "url"),
            url,
            "{name} {platform}"
        );
        let checksum = locked_platform(&root(), &name, platform, "checksum");
        if let Some(sha256) = checksum.strip_prefix("sha256:") {
            assert_eq!(sha256, digest, "{name}");
        }
    }
}

/// cargo-mutants' version in mise.toml and its locked SHA-256 today, so the
/// tests follow the pins instead of freezing them.
fn mutants_pin() -> (String, String) {
    let output = tool("jaq")
        .args(["-r", "--from", "toml", r#".tools["cargo-mutants"].version"#])
        .arg(root().join("mise.toml"))
        .output()
        .unwrap();
    succeeds(&output);
    let version = String::from_utf8(output.stdout).unwrap().trim().to_owned();
    let checksum = locked(&root(), "cargo-mutants", "checksum");
    let sha256 = checksum.strip_prefix("sha256:").unwrap().to_owned();
    (version, sha256)
}

/// The Codecov CLI release ci.yml's upload job installs today.
fn codecov_pin() -> String {
    let rows = install_rows(&root());
    let (_, asset, _, _) = rows.iter().find(|row| row.0 == "codecov").unwrap();
    asset.split('/').nth(4).unwrap().to_owned()
}

/// The next major release after a version.
fn next_major(version: &str) -> String {
    let major: u64 = version.split('.').next().unwrap().parse().unwrap();
    format!("{}.0.0", major + 1)
}

fn mutants_member(asset: &str) -> &'static str {
    if asset.contains("windows-msvc") {
        "cargo-mutants.exe"
    } else {
        "cargo-mutants"
    }
}

/// A copy of what `update-tools` reads and writes, with stand-ins for the
/// network: mise names the latest releases, curl returns fixed bytes and gh
/// names the latest Codecov CLI.
fn sandbox(latest_mutants: &str, codecov: &str) -> PathBuf {
    let dir = temp_dir("tool-updates");
    for file in ["justfile", "mise.toml", "mise.lock"] {
        fs::copy(root().join(file), dir.join(file)).unwrap();
    }
    fs::create_dir_all(dir.join(".github/workflows")).unwrap();
    for entry in fs::read_dir(root().join(".github/workflows")).unwrap() {
        let path = entry.unwrap().path();
        fs::copy(
            &path,
            dir.join(".github/workflows")
                .join(path.file_name().unwrap()),
        )
        .unwrap();
    }
    let (pinned, sha256) = mutants_pin();
    let windows_sha256 = locked_platform(&root(), "cargo-mutants", "windows", "checksum")
        .strip_prefix("sha256:")
        .unwrap()
        .to_owned();
    let bin = dir.join(".tools/bin");
    fs::create_dir_all(&bin).unwrap();
    let current = concat!(
        r#"jaq -r --from toml --arg t "$2" "#,
        r#"'.tools[$t] | if type == "object" then .version else . end' mise.toml"#
    );
    let stubs = [
        (
            "mise",
            format!(
                r#"case "$1" in
  latest) if [[ "$2" == cargo-mutants ]]; then echo {latest_mutants}; else {current}; fi ;;
  lock) echo '→ Targeting 2 platform(s), as mise prints without a terminal'
        sum=$(printf 'new cargo-mutants' | sha256sum | cut -d' ' -f1)
        from=cargo-mutants/releases/download/v{pinned}/
        to=cargo-mutants/releases/download/v{latest_mutants}/
        sed -i -e "s#$from#$to#" -e "s#{sha256}#$sum#" \
          -e "s#{windows_sha256}#$sum#" mise.lock ;;
  *) exit 1 ;;
esac"#
            ),
        ),
        (
            "curl",
            r#"out=""; while [[ $# -gt 0 ]]; do [[ "$1" == -o ]] && out=$2; shift; done
printf 'new cargo-mutants' > "$out""#
                .to_owned(),
        ),
        // The latest Codecov CLI and the digest GitHub records for its asset.
        (
            "gh",
            format!(
                r#"sum=$(printf 'new cargo-mutants' | sha256sum | cut -d' ' -f1)
echo "{codecov} sha256:$sum""#
            ),
        ),
    ];
    for (name, body) in stubs {
        write_executable(
            &bin.join(name),
            &format!("#!/bin/bash\nset -euo pipefail\n{body}\n"),
        );
    }
    dir
}

fn update(dir: &Path) -> String {
    let output = tool("just")
        .arg("--justfile")
        .arg(dir.join("justfile"))
        .arg("--working-directory")
        .arg(dir)
        .arg("update-tools")
        .output()
        .unwrap();
    succeeds(&output);
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn update_tools_moves_a_pin_everywhere_it_is_installed() {
    let (pinned, _) = mutants_pin();
    let next = next_major(&pinned);
    let used = codecov_pin();
    let dir = sandbox(&next, "v99.0.0");
    let before = install_rows(&dir);
    let said = update(&dir);
    assert!(
        said.contains(&format!("cargo-mutants {pinned} -> {next} (major)")),
        "{said}"
    );
    assert!(
        said.contains(&format!("codecov-cli {used} -> v99.0.0")),
        "{said}"
    );
    // The moves become a commit message, so mise's progress stays off them.
    assert!(!said.contains("Targeting"), "{said}");
    assert!(
        fs::read_to_string(dir.join("mise.toml"))
            .unwrap()
            .contains(&format!(r#"cargo-mutants = {{ version = "{next}""#))
    );
    // The digest is computed from the bytes the new asset serves.
    let digest = tool("sha256sum")
        .current_dir(&dir)
        .arg("/dev/stdin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child
                .stdin
                .take()
                .unwrap()
                .write_all(b"new cargo-mutants")?;
            child.wait_with_output()
        })
        .unwrap();
    let digest = String::from_utf8(digest.stdout).unwrap();
    let digest = digest.split_whitespace().next().unwrap();
    let after = install_rows(&dir);
    let mutants: Vec<_> = after
        .iter()
        .filter(|row| row.0 == "cargo-mutants")
        .collect();
    assert!(!mutants.is_empty());
    for (_, asset, sha, member) in mutants {
        assert!(asset.contains(&format!("/v{next}/")), "{asset}");
        assert_eq!(sha, digest);
        assert_eq!(member.as_deref(), Some(mutants_member(asset)));
    }
    // Every other row is exactly as it was.
    let others = |rows: &[(String, String, String, Option<String>)]| -> Vec<_> {
        rows.iter()
            .filter(|row| row.0 != "cargo-mutants" && row.0 != "codecov")
            .cloned()
            .collect()
    };
    assert_eq!(others(&before), others(&after));
    // The Codecov CLI's row moves too, at the digest of the bytes it serves.
    let codecov: Vec<_> = after.iter().filter(|row| row.0 == "codecov").collect();
    assert_eq!(codecov.len(), 1);
    assert_eq!(
        codecov[0].1,
        "codecov/codecov-cli/releases/download/v99.0.0/codecovcli_linux"
    );
    assert_eq!(codecov[0].2, digest);
    assert!(!codecov[0].1.contains(&format!("/{used}/")));
}

#[test]
fn update_tools_changes_nothing_when_every_pin_is_current() {
    let dir = sandbox(&mutants_pin().0, &codecov_pin());
    let snapshot = |dir: &Path| -> Vec<String> {
        ["mise.toml", "mise.lock", ".github/workflows/ci.yml"]
            .iter()
            .map(|file| fs::read_to_string(dir.join(file)).unwrap())
            .collect()
    };
    let before = snapshot(&dir);
    assert_eq!(update(&dir).trim(), "");
    assert_eq!(snapshot(&dir), before);
}

#[test]
fn a_weekly_run_opens_one_pull_request_on_the_bot_token() {
    let data = workflow("tool-updates");
    assert!(data["on"]["schedule"][0]["cron"].is_string());
    assert_eq!(data["permissions"], serde_json::json!({"contents": "read"}));
    let steps = data["jobs"]["update"]["steps"].as_array().unwrap();
    let bodies: String = steps
        .iter()
        .filter_map(|step| step["run"].as_str())
        .collect::<Vec<_>>()
        .concat();
    assert!(bodies.contains("just update-tools"));
    // The organization merges only signed commits, and a commit the runner
    // pushes is unsigned: GitHub creates, and signs, the one it records.
    assert!(!bodies.contains("git push") && !bodies.contains("git commit"));
    assert!(bodies.contains("just _commit-as-bot"));

    let token = steps
        .iter()
        .find(|step| {
            step["uses"]
                .as_str()
                .is_some_and(|uses| uses.starts_with("actions/create-github-app-token@"))
        })
        .unwrap();
    for scope in ["contents", "pull-requests", "workflows"] {
        assert_eq!(
            token["with"][format!("permission-{scope}")],
            "write",
            "{scope}"
        );
    }
}

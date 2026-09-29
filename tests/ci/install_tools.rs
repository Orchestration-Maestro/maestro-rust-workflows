//! `rust-gate install-tools`: what it refuses, what it honours, and what
//! `ci.yml` asks it to install.

use crate::harness::{Fixture, refused, succeeds, tool_rows, workflow};
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fs;

/// The stand-ins a download needs: curl leaves verified bytes, tar creates the
/// selected member, install creates the target.
fn prepare(fixture: &Fixture, checksum_holds: bool) {
    // curl must leave the file behind, or verification has nothing to read.
    let downloaded = if checksum_holds { "abc" } else { "tampered" };
    let curl = format!(
        concat!(
            "out=\"\"\n",
            "while [[ $# -gt 0 ]]; do\n",
            "  [[ \"$1\" == --output ]] && {{ out=$2; shift 2; continue; }}\n",
            "  shift\n",
            "done\n",
            "printf '{downloaded}' > \"$out\""
        ),
        downloaded = downloaded
    );
    fixture.stub("curl", &curl);
    fixture.stub(
        "tar",
        r#"directory=${@: -2:1}
member=${@: -1}
mkdir -p "$directory/$(dirname "$member")"
printf abc > "$directory/$member""#,
    );
    fixture.stub(
        "install",
        r#"dst=${@: -1}
mkdir -p "$(dirname "$dst")"
printf '#!/bin/bash\nexit 0\n' > "$dst"
chmod +x "$dst""#,
    );
}

/// How many release assets the fixture fetched.
fn downloads(fixture: &Fixture) -> usize {
    fixture
        .calls()
        .lines()
        .filter(|line| line.contains("releases/download/"))
        .count()
}

/// Two tools: a bare binary and an archive member, with a comment and a
/// blank line between them.
const TABLE: &str = "jaq 01mf02/jaq/releases/download/v3.1.1/jaq-x86_64-unknown-linux-gnu \
    ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\n\
    # a comment, and a blank line, are allowed between tools\n\n\
    cargo-vet mozilla/cargo-vet/releases/download/v0.10.0/\
    cargo-vet-x86_64-unknown-linux-gnu.tar.xz \
    ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad \
    cargo-vet-x86_64-unknown-linux-gnu/cargo-vet\n";

#[test]
fn tool_installation_verifies_every_download_and_fetches_nothing_unasked() {
    // `rust-gate install-tools` is how untrusted bytes become executables on
    // a self-hosted runner. A download whose checksum is not enforced is a
    // supply-chain entry point, and an optional tool fetched without being
    // asked for is an unreviewed dependency every consumer inherits.
    // One download per listed tool, each installed under its own name, the
    // archive member extracted and the bare binary copied, and the directory
    // handed to every later step.
    let mut listed = Fixture::new();
    prepare(&listed, true);
    listed.set("TOOLS", TABLE);
    succeeds(&listed.run("ci", "install"));
    assert_eq!(downloads(&listed), 2);
    let bin = listed.root.join("rust-tools/bin");
    assert!(bin.join("jaq").is_file() && bin.join("cargo-vet").is_file());
    assert!(
        listed
            .calls()
            .contains(" cargo-vet-x86_64-unknown-linux-gnu/cargo-vet\n")
    );
    assert!(
        fs::read_to_string(listed.root.join("path"))
            .unwrap()
            .contains("rust-tools/bin")
    );

    // A digest that does not match must stop the job before the bytes reach a
    // parser or the PATH. This is the check the whole command exists for.
    let mut tampered = Fixture::new();
    prepare(&tampered, false);
    tampered.set("TOOLS", TABLE);
    let output = tampered.run("ci", "install");
    refused(&output, "sha256 mismatch for jaq; refusing extraction");
    let calls = tampered.calls();
    assert!(
        !calls.contains("tar\n") && !calls.contains("install\n"),
        "{calls}"
    );
    assert!(!tampered.root.join("path").exists());
}

#[test]
fn unsupported_runner_os_fails_before_downloading_tools() {
    let mut fixture = Fixture::new();
    fixture.set("RUNNER_OS", "FreeBSD");
    fixture.set("TOOLS", TABLE);
    refused(
        &fixture.run("ci", "install"),
        "Unsupported runner OS: FreeBSD",
    );
    assert_eq!(downloads(&fixture), 0);
}

#[test]
fn windows_assets_refuse_a_non_windows_host_before_downloading() {
    let mut fixture = Fixture::new();
    prepare(&fixture, true);
    fixture.set("RUNNER_OS", "Windows");
    fixture.set(
        "TOOLS",
        concat!(
            "jaq windows 01mf02/jaq/releases/download/v3.1.1/jaq-x86_64-pc-windows-msvc.exe ",
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad file jaq.exe\n",
            "cargo-mutants windows sourcefrog/cargo-mutants/releases/download/v27.1.0/",
            "cargo-mutants-x86_64-pc-windows-msvc.zip ",
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad zip cargo-mutants.exe"
        ),
    );
    refused(
        &fixture.run("ci", "install"),
        "RUNNER_OS windows requires an x86_64 windows host",
    );
    let bin = fixture.root.join("rust-tools/bin");
    assert!(!bin.join("jaq.exe").exists() && !bin.join("cargo-mutants.exe").exists());
    assert!(!fixture.calls().contains("curl\n"));
}

#[test]
fn a_dropped_connection_is_retried_before_a_download_fails() {
    // A connection GitHub's release storage reset once failed a whole run
    // (curl exit 35), and five HTTP 500 in a row another. curl retries only
    // timeouts and server errors unless it is told to retry every error, and
    // seven retries, one second apart and doubling, wait two minutes.
    let mut fixture = Fixture::new();
    prepare(&fixture, true);
    fixture.set("TOOLS", TABLE);
    succeeds(&fixture.run("ci", "install"));
    assert_eq!(downloads(&fixture), 2);
    for call in fixture
        .calls()
        .lines()
        .filter(|line| line.contains("releases/download/"))
    {
        assert!(call.contains("--retry 7 --retry-all-errors "), "{call}");
    }
}

#[test]
fn every_download_comes_directly_from_github_releases() {
    let mut fixture = Fixture::new();
    prepare(&fixture, true);
    fixture.set("TOOLS", TABLE);
    succeeds(&fixture.run("ci", "install"));
    assert_eq!(downloads(&fixture), 2);
    for url in fixture
        .calls()
        .lines()
        .filter(|line| line.contains("releases/download/"))
    {
        assert!(
            url.contains(" https://github.com/") && !url.contains("github.com//"),
            "download not from GitHub releases: {url}"
        );
    }
}

#[test]
fn a_tool_line_names_an_immutable_release_asset_with_its_own_digest() {
    // Every line must name an immutable release asset with its own digest. An
    // absolute URL, a tag or branch archive, a malformed digest, or a name or
    // member that escapes its directory is refused before anything is fetched.
    let digest = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    let asset = "01mf02/jaq/releases/download/v3.1.1/jaq-x86_64-unknown-linux-gnu";
    for (line, message) in [
        (
            format!("jaq https://evil.example/jaq {digest}"),
            "Not a release asset path:",
        ),
        (
            format!("jaq 01mf02/jaq/archive/refs/tags/v3.1.1.tar.gz {digest}"),
            "Not a release asset path:",
        ),
        (format!("jaq {asset} notadigest"), "Invalid sha256 for"),
        (
            format!("jaq {asset} {digest} ../../bin/jaq"),
            "Invalid archive member for",
        ),
        (format!("../jaq {asset} {digest}"), "Invalid tool name:"),
        (
            format!("jaq ../{asset} {digest}"),
            "Not a release asset path:",
        ),
        (String::new(), "tools lists nothing to install"),
    ] {
        let mut fixture = Fixture::new();
        prepare(&fixture, true);
        fixture.set("TOOLS", &line);
        refused(&fixture.run("ci", "install"), message);
        assert_eq!(downloads(&fixture), 0, "fetched before refusing {line:?}");
    }
}

#[test]
fn ci_installs_its_toolbelt_once_and_each_optional_tool_behind_its_gate() {
    // In ci.yml the mandatory toolbelt is one unconditional call and each
    // optional gate's tool is its own call, conditional on exactly that gate:
    // nothing is fetched unasked. Every asset is an immutable release, and no
    // digest is shared between two tools.
    let ci = workflow("ci");
    let mut gated = BTreeMap::new();
    let mut digests = Vec::new();
    let mut mandatory = 0;
    for step in ci["jobs"]["checks"]["steps"].as_array().unwrap() {
        let rows = tool_rows(step);
        if rows.is_empty() {
            continue;
        }
        for row in &rows {
            assert!(
                !row.asset.starts_with("http") && row.asset.contains("/releases/download/"),
                "not a release asset: {}",
                row.asset
            );
            let archive = [".tar.gz", ".tgz", ".tar.xz", ".zip"]
                .iter()
                .any(|suffix| row.asset.ends_with(suffix));
            assert_eq!(
                archive,
                row.member.is_some(),
                "an archive names its member and a bare binary none: {}",
                row.asset
            );
            digests.push(row.digest.clone());
        }
        match step["if"].as_str() {
            None => mandatory += rows.len(),
            Some(condition) => {
                assert_eq!(
                    rows.len(),
                    1,
                    "a gate must add exactly one tool: {condition}"
                );
                gated.insert(rows[0].name.clone(), condition.to_owned());
            }
        }
    }
    assert!(mandatory > 5, "a default run must install its own tools");
    let expected: BTreeMap<String, String> = [
        // What validate exported, from the caller's inputs or from `[ci]`.
        ("cargo-mutants", "${{ env.MUTATION_TEST == 'true' }}"),
        (
            "cargo-semver-checks",
            "${{ env.API_COMPATIBILITY == 'true' }}",
        ),
        ("clippy-sarif", "${{ env.SARIF_REPORTS == 'true' }}"),
        ("cargo-machete", "${{ env.UNUSED_DEPENDENCIES == 'true' }}"),
        ("cargo-vet", "${{ env.DEPENDENCY_AUDIT == 'true' }}"),
    ]
    .into_iter()
    .map(|(tool, condition)| (tool.to_owned(), condition.to_owned()))
    .collect();
    assert_eq!(gated, expected);
    let unique: BTreeSet<_> = digests.iter().collect();
    assert_eq!(
        digests.len(),
        unique.len(),
        "a checksum is reused between tools"
    );
}

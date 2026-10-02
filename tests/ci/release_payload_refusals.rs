//! The release payload's refusals: a lockfile that drifts while the bill of
//! materials is generated, a binary that is not hardened or not reproducible,
//! and a staging input that is malformed or reaches outside the checkout.

use crate::harness::{Fixture, refused};
use serde_json::json;
use std::fs;
use std::os::unix::fs::symlink;

/// A first build: one executable under `target/release` and the build's
/// JSON record naming it.
fn built(fixture: &Fixture) -> String {
    let target = fixture.root.join("target");
    fs::create_dir_all(target.join("release")).unwrap();
    fs::write(target.join("release/app"), "first").unwrap();
    fs::write(
        fixture.root.join("build.jsonl"),
        json!({"reason": "compiler-artifact", "executable": target.join("release/app"),
               "target": {"name": "app"}})
        .to_string(),
    )
    .unwrap();
    target.display().to_string()
}

/// A `readelf` whose answers name a hardened binary, except for `flaw`.
fn readelf(flaw: &str) -> String {
    let header = if flaw == "pie" {
        "EXEC (Executable file)"
    } else {
        "DYN (Position-Independent Executable file)"
    };
    let stack = if flaw == "stack" { "RWE" } else { "RW " };
    let dynamic = if flaw == "bind-now" {
        "(FLAGS) X"
    } else {
        "(BIND_NOW)"
    };
    format!(
        r#"case "$1" in
  -h) echo '  Type:                              {header}' ;;
  -l) echo '  GNU_RELRO      0x000000'; echo '  GNU_STACK      0x000000'; echo '      {stack}' ;;
  -d) echo ' 0x000000018 {dynamic}' ;;
  -S) echo '  [ 1] .dep-v0           PROGBITS' ;;
esac"#
    )
}

#[test]
fn a_lockfile_that_drifts_during_sbom_generation_fails_the_build() {
    let mut fixture = Fixture::new();
    let target = fixture.root.join("target");
    fixture.set("CARGO_TARGET_DIR", &target.display().to_string());
    fixture.stub(
        "cargo",
        r#"[[ "$1" == cyclonedx ]] && echo drift >> Cargo.lock; exit 0"#,
    );
    refused(
        &fixture.run("ci", "build"),
        "Cargo.lock changed while the SBOM was generated; commit a resolved lockfile",
    );
}

#[test]
fn every_hardening_flaw_and_a_missing_rebuild_are_refused_by_name() {
    let rebuilt = r#"mkdir -p "$CARGO_TARGET_DIR/release"
printf 'first' > "$CARGO_TARGET_DIR/release/app""#;
    for (flaw, message) in [
        ("pie", "is not position independent"),
        ("bind-now", "lacks full RELRO"),
        ("stack", "has an executable stack"),
    ] {
        let mut fixture = Fixture::new();
        let target = built(&fixture);
        fixture.set("CARGO_TARGET_DIR", &target);
        fixture.stub("cargo", rebuilt);
        fixture.stub("readelf", &readelf(flaw));
        refused(&fixture.run("ci", "hardening"), message);
    }
    let mut fixture = Fixture::new();
    let target = built(&fixture);
    fixture.set("CARGO_TARGET_DIR", &target);
    fixture.stub("cargo", "");
    refused(
        &fixture.run("ci", "hardening"),
        "Rebuilt binary missing for",
    );
}

/// A staging fixture: one member whose `CycloneDX` document is valid, one
/// executable, and the revision the payload records.
fn staged(fixture: &mut Fixture) {
    let target = fixture.root.join("target");
    fs::create_dir_all(&target).unwrap();
    fixture.set("CARGO_TARGET_DIR", &target.display().to_string());
    fixture.set("REVISION", &"a".repeat(40));
    let manifest = fixture.root.join("project/Cargo.toml");
    fs::write(
        fixture.root.join("metadata.json"),
        members(&[("fixture", &manifest.display().to_string())]),
    )
    .unwrap();
    fs::write(fixture.root.join("build.jsonl"), "").unwrap();
    let bom = json!({"bomFormat": "CycloneDX", "specVersion": "1.5", "version": 1,
        "metadata": {"component": {"name": "fixture", "type": "library"}}, "components": []});
    fs::write(
        fixture.root.join("project/fixture.cdx.json"),
        bom.to_string(),
    )
    .unwrap();
}

/// The `cargo metadata` record of the given members.
fn members(members: &[(&str, &str)]) -> String {
    let packages: Vec<_> = members
        .iter()
        .map(|(name, manifest)| json!({"id": name, "name": name, "manifest_path": manifest}))
        .collect();
    let ids: Vec<&str> = members.iter().map(|(name, _)| *name).collect();
    json!({"workspace_members": ids, "packages": packages}).to_string()
}

#[test]
fn staging_refuses_names_paths_and_documents_it_cannot_trust() {
    let fresh = || {
        let mut fixture = Fixture::new();
        staged(&mut fixture);
        fixture
    };
    let first = fresh();
    let manifest = first.root.join("project/Cargo.toml").display().to_string();
    for (table, message) in [
        (
            members(&[("bad name", &manifest)]),
            "Invalid workspace package name",
        ),
        (
            members(&[("fixture", "/etc/hostname")]),
            "Workspace member escapes checkout",
        ),
    ] {
        let fixture = fresh();
        fs::write(fixture.root.join("metadata.json"), table).unwrap();
        refused(&fixture.run("ci", "stage"), message);
    }
    let linked = fresh();
    fs::remove_file(linked.root.join("project/fixture.cdx.json")).unwrap();
    symlink(
        "/etc/hostname",
        linked.root.join("project/fixture.cdx.json"),
    )
    .unwrap();
    refused(&linked.run("ci", "stage"), "SBOM path escapes checkout");
    for (name, outside, message) in [
        ("bad name", false, "Invalid binary name"),
        ("app", true, "Invalid or duplicate binary output"),
    ] {
        let fixture = fresh();
        let inside = fixture.root.join("target/release/app");
        fs::create_dir_all(inside.parent().unwrap()).unwrap();
        fs::write(&inside, "binary").unwrap();
        let path = if outside {
            "/etc/hostname".to_owned()
        } else {
            inside.display().to_string()
        };
        let record = json!({"reason": "compiler-artifact", "executable": path,
                            "target": {"name": name}});
        fs::write(fixture.root.join("build.jsonl"), record.to_string()).unwrap();
        refused(&fixture.run("ci", "stage"), message);
    }
    let merged = fresh();
    merged.stub(
        "cyclonedx",
        r#"[[ "$1" == merge ]] || exit 0
while [[ $# -gt 0 ]]; do [[ "$1" == --output-file ]] && printf '{}' > "$2"; shift; done"#,
    );
    refused(
        &merged.run("ci", "stage"),
        "Merged payload SBOM is not a usable CycloneDX 1.5 document",
    );
    let spdx = fresh();
    spdx.stub("cargo", r#"[[ "$1" == sbom ]] && printf '{}'; exit 0"#);
    refused(
        &spdx.run("ci", "stage"),
        "is not a usable SPDX 2.3 document",
    );
    let archive = fresh();
    fs::create_dir_all(archive.root.join("target/package")).unwrap();
    symlink(
        "/etc/hostname",
        archive.root.join("target/package/fixture-0.1.0.crate"),
    )
    .unwrap();
    refused(
        &archive.run("ci", "stage"),
        "Package archive must not be a symlink",
    );
}

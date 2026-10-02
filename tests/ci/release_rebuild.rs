//! Clean same-path release builds retain flags and generated-code locations.

use crate::harness::{Fixture, capture, refused, succeeds, tool, write_executable};
use std::fs;

/// A private binary with a dependency whose panic code is generated under `OUT_DIR`.
fn generated_dependency_fixture(nonreproducible: bool) -> Fixture {
    let mut fixture = Fixture::new();
    let project = fixture.root.join("project");
    fs::write(
        project.join("Cargo.toml"),
        concat!(
            "[package]\nname='fixture'\nversion='0.1.0'\nedition='2024'\npublish=false\n",
            "[dependencies]\ngenerated={path='generated'}\n",
        ),
    )
    .unwrap();
    fs::write(
        project.join("src/main.rs"),
        "fn main() { generated::generated(); }\n",
    )
    .unwrap();
    fs::create_dir_all(project.join("generated/src")).unwrap();
    fs::write(
        project.join("generated/Cargo.toml"),
        "[package]\nname='generated'\nversion='0.1.0'\nedition='2024'\npublish=false\n",
    )
    .unwrap();
    fs::write(
        project.join("generated/src/lib.rs"),
        "include!(concat!(env!(\"OUT_DIR\"), \"/generated.rs\"));\n",
    )
    .unwrap();
    let code = if nonreproducible {
        // Mutable build-script state must differ on a truly fresh rebuild.
        "{ let counter = std::path::Path::new(\"generation-count\"); \
         let count = std::fs::read_to_string(counter).unwrap_or_default()\
         .parse::<u64>().unwrap_or(0) + 1; \
         std::fs::write(counter, count.to_string()).unwrap(); \
         format!(\"pub fn generated() {{ panic!(\\\"generation {count}\\\"); }}\") }"
    } else {
        "String::from(\"pub fn generated() { panic!(\\\"generated panic\\\"); }\")"
    };
    fs::write(
        project.join("generated/build.rs"),
        format!(
            "fn main() {{\nprintln!(\"cargo:rerun-if-changed=build.rs\");\n\
             let out = std::env::var(\"OUT_DIR\").unwrap();\n\
             std::fs::write(std::path::Path::new(&out).join(\"generated.rs\"),\n\
             {code}).unwrap();\n}}\n"
        ),
    )
    .unwrap();
    fixture.set("CARGO_NET_OFFLINE", "true");
    let target = fixture.root.join("target with spaces");
    fixture.set("CARGO_TARGET_DIR", &target.display().to_string());
    // Only SBOM generation is irrelevant to this real build/hardening contract.
    fixture.set(
        "REAL_CARGO",
        capture(tool("bash").args(["-c", "command -v cargo"])).trim(),
    );
    fixture.stub(
        "cargo",
        "if [[ $1 == cyclonedx ]]; then exit 0; fi\nexec \"$REAL_CARGO\" \"$@\"",
    );
    succeeds(&fixture.run_body("cd project && cargo generate-lockfile --offline"));
    fixture
}

#[test]
fn generated_dependency_panic_locations_pass_a_clean_same_path_rebuild() {
    let fixture = generated_dependency_fixture(false);
    succeeds(&fixture.run("ci", "build"));
    let binary = fixture.root.join("target with spaces/release/fixture");
    let strings = capture(tool("strings").arg(&binary));
    assert!(strings.contains(&format!(
        "{}/release/build/generated-",
        fixture.root.join("target with spaces").display()
    )));
    let original = fs::read(&binary).unwrap();
    fs::write(
        fixture.root.join("target with spaces/cache-sentinel"),
        "cached",
    )
    .unwrap();
    succeeds(&fixture.run("ci", "hardening"));
    assert!(
        fs::read(&binary).unwrap() == original,
        "shipped bytes must be restored"
    );
    assert!(
        fixture
            .root
            .join("target with spaces/cache-sentinel")
            .is_file()
    );
    assert!(!fs::read_dir(&fixture.root).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("release-original.")
    }));
    assert_eq!(
        fs::read_to_string(fixture.root.join("reports/hardening.txt")).unwrap(),
        "fixture reproducible pie relro bind-now noexec-stack auditable\n"
    );
}

#[test]
fn fresh_build_script_state_still_refuses_nonreproducible_release_bytes() {
    let fixture = generated_dependency_fixture(true);
    succeeds(&fixture.run("ci", "build"));
    let binary = fixture.root.join("target with spaces/release/fixture");
    let original = fs::read(&binary).unwrap();
    refused(
        &fixture.run("ci", "hardening"),
        "Release binary fixture is not reproducible across build directories",
    );
    assert!(
        fs::read(binary).unwrap() == original,
        "shipped bytes must be restored"
    );
}

#[test]
fn consumer_flags_and_existing_wrapper_reach_both_release_builds() {
    for source in ["environment", "build", "target"] {
        let mut fixture = generated_dependency_fixture(false);
        let project = fixture.root.join("project");
        let guard = "#[cfg(not(consumer_flag))]\ncompile_error!(\"consumer flag lost\");\n";
        for file in ["src/main.rs", "generated/src/lib.rs"] {
            let path = project.join(file);
            let original = fs::read_to_string(&path).unwrap();
            fs::write(path, format!("{guard}{original}")).unwrap();
        }
        let flags = [
            "--cfg",
            "consumer_flag",
            "--check-cfg",
            "cfg(consumer_flag)",
        ];
        if source == "environment" {
            fixture.set("RUSTFLAGS", &flags.join(" "));
        } else {
            fs::create_dir(project.join(".cargo")).unwrap();
            let table = if source == "build" {
                "build"
            } else {
                "target.'cfg(all())'"
            };
            fs::write(
                project.join(".cargo/config.toml"),
                format!("[{table}]\nrustflags={flags:?}\n"),
            )
            .unwrap();
        }
        let wrapper = fixture.root.join("original wrapper");
        write_executable(
            &wrapper,
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$WRAPPER_CALLS\"\nexec \"$@\"\n",
        );
        fixture.set("RUSTC_WRAPPER", &wrapper.display().to_string());
        fixture.set(
            "WRAPPER_CALLS",
            &fixture.root.join("wrapper-calls").display().to_string(),
        );
        succeeds(&fixture.run("ci", "build"));
        succeeds(&fixture.run("ci", "hardening"));
        let calls = fs::read_to_string(fixture.root.join("wrapper-calls")).unwrap();
        for name in ["fixture", "generated"] {
            assert!(
                calls
                    .lines()
                    .filter(|line| line.contains(&format!("--crate-name {name} "))
                        && line.contains("--cfg consumer_flag"))
                    .count()
                    >= 2,
                "{source}: wrapper must receive {name} and flags in both builds\n{calls}"
            );
        }
    }
}

#[test]
fn failed_fresh_build_restores_shipped_bytes_and_cached_objects() {
    let fixture = generated_dependency_fixture(false);
    succeeds(&fixture.run("ci", "build"));
    let binary = fixture.root.join("target with spaces/release/fixture");
    let original = fs::read(&binary).unwrap();
    let sentinel = fixture.root.join("target with spaces/cache-sentinel");
    fs::write(&sentinel, "cached").unwrap();
    fixture.stub(
        "cargo",
        "mkdir -p \"$CARGO_TARGET_DIR/release\"\n\
         printf partial > \"$CARGO_TARGET_DIR/release/fixture\"\nexit 17",
    );
    let output = fixture.run("ci", "hardening");
    assert_eq!(output.status.code(), Some(17));
    assert!(
        fs::read(&binary).unwrap() == original,
        "shipped bytes must be restored"
    );
    assert_eq!(fs::read_to_string(sentinel).unwrap(), "cached");
}

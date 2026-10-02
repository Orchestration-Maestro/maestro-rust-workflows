//! The repository under test: its root, the toolbelt provisioned beside it,
//! commands run to completion, private temporary directories, the stand-in
//! executables tests run, and every Rust file of these tests.

use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process;
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

pub(crate) fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned()
}

/// The PATH every tool this crate launches sees: the pinned toolbelt first,
/// when `just setup` has linked it into `.tools/bin`, then whatever the caller
/// inherited. A launcher that never exported that directory, such as an
/// editor's test runner, still finds the pinned binaries instead of failing on
/// a missing `jaq`.
pub(crate) fn toolbelt_path() -> String {
    let inherited = env::var("PATH").unwrap_or_default();
    let bin = root().join(".tools/bin");
    if bin.is_dir() {
        format!("{}:{inherited}", bin.display())
    } else {
        inherited
    }
}

/// A command that resolves through the pinned toolbelt.
pub(crate) fn tool(name: &str) -> Command {
    let mut command = Command::new(name);
    command.env("PATH", toolbelt_path());
    command
}

/// A command from one line, the program resolved through the toolbelt, so a
/// reader and the North Star test see `cargo audit` as written.
pub(crate) fn command_line(line: &str) -> Command {
    let mut words = line.split_whitespace();
    let mut command = tool(words.next().expect("a command line names a program"));
    command.args(words);
    command
}

/// Run to completion and hand back stdout; anything but success fails here.
pub(crate) fn capture(command: &mut Command) -> String {
    let output = command
        .output()
        .expect("the toolbelt must be installed: just setup");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A private temporary directory for one test.
pub(crate) fn temp_dir(purpose: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = env::temp_dir().join(format!(
        "rust-workflows-{purpose}-{}-{}",
        process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&path).unwrap();
    path
}

/// Write an executable stand-in a test then runs, through `install` in a
/// process of its own. Tests run on many threads, and a child one of them
/// starts inherits every descriptor this process holds until it runs its own
/// program: had this process held the new file open for writing, running it in
/// that moment would fail with "Text file busy".
pub(crate) fn write_executable(path: &Path, contents: &str) {
    let mut install = Command::new("install")
        .args(["-m", "0755", "/dev/stdin"])
        .arg(path)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    // The pipe closes as the handle drops, and install sees the end.
    install
        .stdin
        .take()
        .unwrap()
        .write_all(contents.as_bytes())
        .unwrap();
    assert!(
        install.wait().unwrap().success(),
        "cannot write {}",
        path.display()
    );
}

/// Every Rust file of the contract tests, the harness included, so a scan of
/// what the tests define sees all of it.
pub(crate) fn test_sources() -> Vec<PathBuf> {
    let consumer = root().join("tests/fixtures/native-consumer-project");
    // The committed consumer runs under Cargo, not in the contract-test process.
    rust_files(&root().join("tests"))
        .into_iter()
        .filter(|path| !path.starts_with(&consumer))
        .collect()
}

/// Every Rust file under `directory`, build output left out, sorted.
pub(crate) fn rust_files(directory: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut queue = vec![directory.to_path_buf()];
    while let Some(directory) = queue.pop() {
        for entry in fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() && path.file_name().is_some_and(|name| name != "target") {
                queue.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

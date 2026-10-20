//! Standalone caller for a disposable workflow timeout qualification.

#[test]
fn caller_child_transcript_blocks_until_step_timeout() -> std::io::Result<()> {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    let executable = std::fs::canonicalize("/usr/bin/python3")?;
    let mut child = Command::new(&executable)
        .args([
            "-u",
            "-c",
            "import threading; print('CHILD-READY-MARKER', flush=True); threading.Event().wait()",
        ])
        .stdout(Stdio::piped())
        .spawn()?;
    let mut line = String::new();
    BufReader::new(
        child
            .stdout
            .take()
            .ok_or_else(|| std::io::Error::other("child stdout was not piped"))?,
    )
    .read_line(&mut line)?;
    assert_eq!(line, "CHILD-READY-MARKER\n");
    let directory = std::env::var_os("MAESTRO_TEST_DIAGNOSTICS_DIR")
        .ok_or_else(|| std::io::Error::other("workflow diagnostic path missing"))?;
    let path = std::path::PathBuf::from(directory).join(format!("child-{}.txt", child.id()));
    let mut transcript = std::fs::File::create_new(path)?;
    writeln!(transcript, "executable: {}", executable.display())?;
    writeln!(transcript, "pid: {}", child.id())?;
    transcript.write_all(line.as_bytes())?;
    writeln!(transcript, "phase: waiting-for-child-exit")?;
    transcript.flush()?;
    eprintln!("{line}phase: waiting-for-child-exit");
    let status = child.wait()?;
    assert!(status.success(), "qualification child failed");
    Ok(())
}

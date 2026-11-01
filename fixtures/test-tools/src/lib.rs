//! Caller-pinned developer tool fixture.

#[cfg(test)]
mod tests {
    use std::process::Command;

    /// Checks exact tool versions without depending on optional release metadata.
    #[test]
    fn caller_pinned_tools_are_available() {
        for (tool, expected) in [("just", "1.58.0"), ("prek", "0.5.3")] {
            let output = Command::new(tool).arg("--version").output().unwrap();
            assert!(output.status.success(), "{tool} failed");
            let stdout = String::from_utf8(output.stdout).unwrap();
            let mut words = stdout.split_whitespace();
            assert_eq!(words.next(), Some(tool));
            assert_eq!(words.next(), Some(expected));
        }
    }
}

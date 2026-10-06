//! Caller-pinned developer tool fixture.

#[cfg(test)]
mod tests {
    use std::process::Command;

    #[test]
    fn caller_pinned_tools_are_available() {
        for (tool, expected) in [("just", "just 1.58.0"), ("prek", "prek 0.5.3")] {
            let output = Command::new(tool).arg("--version").output().unwrap();
            assert!(output.status.success(), "{tool} failed");
            assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), expected);
        }
    }
}

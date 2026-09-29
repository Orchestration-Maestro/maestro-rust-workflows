//! Small Windows-only behavior owned by the native mutation job.

/// Return the line ending used by Windows text files.
#[must_use]
pub fn line_ending() -> &'static str {
    "\r\n"
}

#[cfg(test)]
mod tests {
    use super::line_ending;

    #[test]
    fn windows_text_uses_crlf_line_endings() {
        assert_eq!(line_ending(), "\r\n");
    }
}

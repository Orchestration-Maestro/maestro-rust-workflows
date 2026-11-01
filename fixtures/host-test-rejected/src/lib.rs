//! A fixture whose test rejects non-Linux hosts.

#[cfg(test)]
mod tests {
    /// Fails at runtime on a native macOS or Windows test runner.
    #[test]
    fn native_test_host_is_rejected() {
        assert!(cfg!(target_os = "linux"), "fixture rejects native test host");
    }
}

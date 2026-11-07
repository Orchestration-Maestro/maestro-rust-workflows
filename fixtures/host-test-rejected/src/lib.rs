//! A fixture whose test rejects non-Linux hosts.

/// Exercises runtime host rejection without preventing compilation.
#[cfg(test)]
mod tests {
    /// Fails at runtime on a native macOS or Windows test runner.
    #[test]
    fn native_test_host_is_rejected() {
        assert_eq!(
            std::env::consts::OS,
            "linux",
            "fixture rejects native test host"
        );
    }
}

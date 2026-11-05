//! Test-only sources stay outside the coverage denominator.

/// Proves the shared public function from an integration test.
#[test]
fn doubles_value() {
    assert_eq!(sample::double(3), 6);
}

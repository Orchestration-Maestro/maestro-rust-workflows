//! Test-only sources stay outside the coverage denominator.

#[test]
fn doubles_value() {
    assert_eq!(sample::double(3), 6);
}

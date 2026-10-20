//! Planted defects for the nightly fuzz job's self-test.

/// Panics when the input contains `!`.
pub fn crash(data: &[u8]) {
    assert!(!data.contains(&b'!'), "planted crash");
}

/// Never returns when the input starts with `~`.
pub fn stall(data: &[u8]) {
    while data.first() == Some(&b'~') {
        std::hint::spin_loop();
    }
}

/// Allocates a gibibyte when the input starts with `+`.
pub fn hoard(data: &[u8]) -> usize {
    if data.first() == Some(&b'+') {
        return std::hint::black_box(vec![1_u8; 1 << 30]).len();
    }
    0
}

/// Panics only for inputs longer than eight bytes.
pub fn bounded(data: &[u8]) {
    assert!(data.len() <= 8, "input longer than the declared limit");
}

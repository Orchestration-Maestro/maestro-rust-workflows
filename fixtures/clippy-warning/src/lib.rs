//! A small arithmetic fixture for the shared Rust CI.

/// Returns twice the input.
pub fn double(value: u32) -> u32 {
    value * 2 + 0
}

#[cfg(test)]
mod tests {
    use super::double;

    #[test]
    fn doubles_value() {
        assert_eq!(double(2), 4);
    }
}

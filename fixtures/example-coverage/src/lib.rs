//! A small arithmetic fixture for the shared Rust CI.

#[cfg(test)]
extern crate self as sample;

#[cfg(test)]
#[path = "tests/coverage_example.rs"]
mod coverage_example;

/// Returns twice the input.
pub fn double(value: u32) -> u32 {
    value * 2
}

#[cfg(test)]
mod tests {
    use super::double;

    #[test]
    fn doubles_value() {
        assert_eq!(double(2), 4);
    }
}

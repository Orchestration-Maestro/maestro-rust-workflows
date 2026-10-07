//! A small arithmetic fixture for the shared Rust CI.

/// Returns twice the input.
pub fn double(value: u32) -> u32 {
    value * 2
}

/// Returns the sum of three inputs.
pub fn sum_three(first: u32, second: u32, third: u32) -> u32 {
    let partial = first + second;
    partial + third
}

#[cfg(test)]
mod tests {
    use super::double;

    #[test]
    fn doubles_value() {
        assert_eq!(double(2), 4);
    }
}

//! Arithmetic shared by the mutation and instruction-count proofs.

/// Returns twice the input.
pub fn double(value: u64) -> u64 {
    value * 2
}

#[cfg(test)]
mod tests {
    use super::double;

    #[test]
    fn doubles_values() {
        assert_eq!(double(0), 0);
        assert_eq!(double(21), 42);
    }
}

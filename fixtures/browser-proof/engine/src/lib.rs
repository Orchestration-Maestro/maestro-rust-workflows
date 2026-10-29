//! The dependency of the browser crate in the merge-group selection proof.

/// Returns the value the browser crate reports.
pub fn value() -> u32 {
    4
}

#[cfg(test)]
mod tests {
    #[test]
    fn value_is_stable() {
        assert_eq!(super::value(), 4);
    }
}

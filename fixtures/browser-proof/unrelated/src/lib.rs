//! A crate the browser crate does not depend on in the merge-group selection proof.

/// Returns a value unrelated to the browser crate.
pub fn unrelated() -> u32 {
    2
}

#[cfg(test)]
mod tests {
    #[test]
    fn unrelated_is_stable() {
        assert_eq!(super::unrelated(), 2);
    }
}

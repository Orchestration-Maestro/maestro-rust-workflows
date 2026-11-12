//! A library whose test needs the provider's `clock` feature without asking for it.

#[cfg(test)]
mod tests {
    /// Reads the clock; compiles only when another package enables `clock`.
    #[test]
    fn reads_the_clock() {
        assert_eq!(provider::tick(), 1);
    }
}

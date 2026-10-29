//! The browser crate in the merge-group selection proof.

/// Returns the engine value the browser reports.
pub fn reported() -> u32 {
    engine::value()
}

#[cfg(test)]
mod tests {
    #[test]
    fn reports_engine_value() {
        assert_eq!(super::reported(), engine::value());
    }
}

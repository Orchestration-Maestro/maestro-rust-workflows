//! A negative control requiring original home state.

#[cfg(test)]
mod tests {
    #[test]
    fn requires_original_home_state() {
        let path =
            std::path::Path::new(&std::env::var("HOME").unwrap()).join("original-home-marker");
        assert_eq!(std::fs::read_to_string(path).unwrap(), "controlled-home");
    }
}

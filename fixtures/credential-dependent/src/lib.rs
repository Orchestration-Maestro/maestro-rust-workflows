//! A negative control requiring an ambient provider credential.

#[cfg(test)]
mod tests {
    #[test]
    fn requires_provider_variable() {
        assert_eq!(
            std::env::var("OPENAI_API_KEY").unwrap(),
            "controlled-provider"
        );
    }
}

//! Feature-partition regression.
#[cfg(feature = "engine")]
pub mod engine;
pub fn answer() -> u8 {
    7
}
#[cfg(test)]
mod tests {
    #[test]
    fn answer_is_seven() {
        assert_eq!(super::answer(), 7);
    }
}

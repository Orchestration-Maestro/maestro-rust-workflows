//! Engine-only regression.
pub fn engine_answer() -> u8 {
    9
}
#[cfg(test)]
mod tests {
    #[test]
    fn engine_answer_is_nine() {
        assert_eq!(super::engine_answer(), 9);
    }
}

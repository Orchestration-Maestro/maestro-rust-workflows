//! Names as the inputs spell them: one validator for every "simple name"
//! rule, and hexadecimal strings of a fixed width.

/// A simple name: the first character is alphanumeric or one of `first`, and
/// every character is alphanumeric or one of `rest`. Empty is never simple.
pub(crate) fn simple(value: &str, first: &str, rest: &str) -> bool {
    value
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_alphanumeric() || first.contains(character))
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || rest.contains(character))
}

/// A lowercase hexadecimal string of exactly `length` digits: a commit SHA at
/// 40, a sha256 at 64.
pub(crate) fn is_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::{is_hex, simple};

    #[test]
    fn hexadecimal_values_refuse_uppercase_and_nonhexadecimal_bytes() {
        assert!(is_hex("0123456789abcdef", 16));
        assert!(!is_hex("ABCDEF", 6));
        assert!(!is_hex("ghijkl", 6));
        assert!(!is_hex("abcdef", 5));
    }

    #[test]
    fn a_simple_name_is_bounded_by_its_two_character_sets() {
        assert!(simple("cargo-vet", "", "._-") && simple("_x", "_", "_-"));
        for bad in ["", "-x", ".x", "a b", "a/b"] {
            assert!(!simple(bad, "_", "_-"), "{bad}");
        }
    }
}

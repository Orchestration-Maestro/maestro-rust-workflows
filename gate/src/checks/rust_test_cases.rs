//! Edge cases for Rust source scanning and function boundaries.

#[cfg(test)]
mod scanner_cases {
    use super::super::rust_tests::{
        attribute_end, body, enclosing_function, function_after, test_functions, waits,
    };

    #[test]
    fn enclosing_functions_exclude_both_braces_and_later_declarations() {
        let source = "fn first() { body; } fn later() { next; }";
        let open = source.find('{').unwrap();
        let close = source.find('}').unwrap();
        assert_eq!(
            enclosing_function(source, open + 1),
            Some("first".to_owned())
        );
        assert_eq!(enclosing_function(source, open), None);
        assert_eq!(enclosing_function(source, close), None);
        assert_eq!(enclosing_function(source, 0), None);
        assert_eq!(enclosing_function(source, close + 1), None);
        assert_eq!(enclosing_function("notfn wrong() { body; }", 16), None);
    }

    #[test]
    fn nested_test_attributes_and_restricted_visibility_find_the_function() {
        let source = "#[test]\n#[custom([a], [b, [c]])]\npub(in crate::tests) fn named_case() {}";
        assert_eq!(attribute_end(source, 8), source.find("\npub").unwrap());
        assert_eq!(test_functions(source), [(3, "named_case".to_owned())]);
        for visibility in ["pub(crate)", "pub(super)", "pub(in crate::tests)"] {
            let code = format!("{visibility} fn named_case() {{}}");
            assert_eq!(
                function_after(&code, 0),
                Some((code.find("named_case").unwrap(), "named_case".to_owned()))
            );
        }
    }

    #[test]
    fn function_names_skip_every_byte_of_leading_whitespace() {
        for whitespace in [" ", "\n  ", "\t\n    "] {
            let code = format!("fn{whitespace}named_case() {{}}");
            assert_eq!(
                function_after(&code, 0),
                Some((2 + whitespace.len(), "named_case".to_owned()))
            );
        }
    }

    #[test]
    fn body_delimiters_ignore_signature_arrays_and_nested_expressions() {
        for source in [
            "fn array(value: [[u8; 2]; 3]) { inner(); }",
            "fn constant(value: [u8; { 1; 2 }]) { inner(); }",
            "fn nested() { call((value)); { other(); } last(); }",
        ] {
            let open = source
                .find("{ inner")
                .or_else(|| source.find("{ call"))
                .unwrap();
            let close = source.rfind('}').unwrap();
            assert_eq!(body(source.as_bytes(), 0), Some((open, close)), "{source}");
            let offset = source
                .find("inner")
                .or_else(|| source.find("last"))
                .unwrap();
            assert!(enclosing_function(source, offset).is_some(), "{source}");
        }
    }

    #[test]
    fn bodyless_declarations_never_capture_the_following_function() {
        let source = "fn declared(); fn real() { std::thread::sleep(d); }";
        assert_eq!(body(source.as_bytes(), 0), None);
        assert_eq!(waits(source)[0].function.as_deref(), Some("real"));
        assert_eq!(body(b"fn declared();", 0), None);
    }
}

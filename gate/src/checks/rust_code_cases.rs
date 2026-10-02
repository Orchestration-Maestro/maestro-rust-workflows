//! Edge cases for Rust source scanning and function boundaries.

#[cfg(test)]
mod scanner_cases {
    use super::super::rust_code::{
        attribute_end, attributes_end, blanked, comments, head, items, literal_end, raw_string_end,
        visibility_of,
    };

    #[test]
    fn literal_prefixes_and_multibyte_characters_blank_completely() {
        for literal in [
            "br##\"x\"##",
            "cr#\"x\"#",
            "b\"x\"",
            "c\"x\"",
            "b'x'",
            "'中'",
        ] {
            let source = format!("{literal} tail");
            assert_eq!(literal_end(source.as_bytes(), 0), Some(literal.len()));
            assert_eq!(
                blanked(&source),
                format!("{} tail", " ".repeat(literal.len()))
            );
            let later = format!("let value = {literal};");
            assert_eq!(
                blanked(&later),
                format!("let value = {};", " ".repeat(literal.len()))
            );
        }
        assert_eq!(literal_end(b"identifier", 3), None);
        assert_eq!(literal_end(b"\"abc\"", 4), None);
    }

    #[test]
    fn raw_string_bodies_start_after_every_opening_hash() {
        for (source, start, expected) in [
            ("r\"\" tail", 1, 3),
            ("r##\"\"## tail", 1, 7),
            ("let s = r##\"\"## tail", 9, 15),
            ("r##\"one\"#two\"## tail", 1, 15),
            ("r#\"open", 1, 7),
        ] {
            assert_eq!(
                raw_string_end(source.as_bytes(), start),
                Some(expected),
                "{source}"
            );
        }
        for source in ["\"last\"", "r\"last\"", "br#\"last\"#", "cr##\"last\"##"] {
            assert_eq!(literal_end(source.as_bytes(), 0), Some(source.len()));
            assert_eq!(blanked(source), " ".repeat(source.len()));
            assert!(comments(source).is_empty());
        }
        assert_eq!(comments("// last"), [(1, "// last")]);
        assert_eq!(blanked("// last"), "       ");
    }

    #[test]
    fn nested_attributes_end_only_after_the_outer_bracket() {
        let source = "#[custom([a], [b, [c]])] #[test] fn named() {}";
        let close = source.find(" #[test]").unwrap();
        assert_eq!(attribute_end(source.as_bytes(), 1), close);
        assert_eq!(attributes_end(source, 0), source.find("fn ").unwrap());
        let found = items(source);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "named");
        assert_eq!(found[0].attributes, "#[custom([a], [b, [c]])] #[test]");
        let inner = "     #![custom([a], [b])] fn named() {}";
        assert_eq!(attributes_end(inner, 5), inner.find("fn ").unwrap());
    }

    #[test]
    fn extern_and_visibility_heads_require_whole_keywords() {
        assert_eq!(
            visibility_of("pub(crate) fn offered() {}"),
            ("pub(crate)".to_owned(), " fn offered() {}")
        );
        for (source, visibility, kind, name) in [
            ("extern crate alloc;", "", "extern crate", "alloc"),
            ("extern fn declared() {}", "", "fn", "declared"),
            ("crate unrelated;", "", "crate", "unrelated"),
            ("public fn private() {}", "", "public", "fn"),
            ("pub_item!();", "", "macro", ""),
            ("pub(crate) fn offered() {}", "pub(crate)", "fn", "offered"),
        ] {
            assert_eq!(
                head(source),
                (visibility.to_owned(), kind.to_owned(), name.to_owned())
            );
        }
    }

    #[test]
    fn empty_source_has_no_literals_to_blank() {
        assert_eq!(blanked(""), "");
    }
}

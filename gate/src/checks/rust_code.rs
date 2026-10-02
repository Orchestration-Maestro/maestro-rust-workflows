//! Rust source as the architecture rules read it: every comment and every
//! literal blanked to spaces, so a path inside a string or a comment is never
//! read as an import, each line where it was; and the top-level items of a
//! file, each with its attributes, visibility, kind and name.

use std::iter;

/// One top-level item of a file.
#[derive(Debug)]
pub(crate) struct Item {
    /// The line its first word is on, counting from 1.
    pub(crate) line: usize,
    /// The attributes written on it, `#[cfg(test)]` among them.
    pub(crate) attributes: String,
    /// Its visibility, whitespace collapsed: empty, `pub`, `pub(crate)`,
    /// `pub(super)` or `pub(in crate::path)`.
    pub(crate) visibility: String,
    /// What it is: `mod`, `use`, `fn`, `struct`, `extern crate`,
    /// `macro_rules!`, or `macro` for any other macro called at the top level.
    pub(crate) kind: String,
    /// The name it declares; empty for `use`, `impl` and macro calls.
    pub(crate) name: String,
    /// Its text, from its first word to its end.
    pub(crate) text: String,
    /// Where it starts, attributes included, and where it ends, in bytes.
    pub(crate) span: (usize, usize),
}

impl Item {
    /// Whether the item exists only under test: `#[cfg(test)]` on it.
    pub(crate) fn is_test(&self) -> bool {
        self.attributes
            .replace(char::is_whitespace, "")
            .contains("cfg(test)")
    }

    /// Whether its visibility reaches past the parent module: `pub`,
    /// `pub(crate)` or `pub(in path)`, but neither `pub(super)` nor
    /// `pub(self)`.
    pub(crate) fn is_offered(&self) -> bool {
        self.visibility.starts_with("pub")
            && !matches!(
                self.visibility.as_str(),
                "pub(super)" | "pub(self)" | "pub(in super)" | "pub(in self)"
            )
    }

    /// Whether it declares a module kept in a file of its own: `mod name;`.
    pub(crate) fn is_module_file(&self) -> bool {
        self.kind == "mod" && self.text.trim_end().ends_with(';')
    }
}

/// The source with every comment and every string, byte-string, C-string,
/// raw-string and character literal replaced by spaces. Line breaks stay, so
/// every byte offset and every line number is the original's.
pub(crate) fn blanked(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = bytes.to_vec();
    let mut consumed = 0;
    for index in 0..bytes.len() {
        if index < consumed {
            continue;
        }
        if let Some(end) = literal_end(bytes, index) {
            blank(out.get_mut(index..end).unwrap_or_default());
            consumed = end;
        }
    }
    String::from_utf8(out).unwrap_or_default()
}

/// Every comment of a Rust source, line and block, with the line it starts
/// on.
pub(crate) fn comments(source: &str) -> Vec<(usize, &str)> {
    let bytes = source.as_bytes();
    let mut found = Vec::new();
    let mut consumed = 0;
    for index in 0..bytes.len() {
        if index < consumed {
            continue;
        }
        if let Some(end) = literal_end(bytes, index) {
            let text = source.get(index..end).unwrap_or_default();
            if text.starts_with("//") || text.starts_with("/*") {
                found.push((line_at(source, index), text));
            }
            consumed = end;
        }
    }
    found
}

/// Turn every byte but a line break into a space.
pub(crate) fn blank(bytes: &mut [u8]) {
    for byte in bytes.iter_mut().filter(|byte| **byte != b'\n') {
        *byte = b' ';
    }
}

/// Where the comment or literal starting at `start` ends, or `None` when
/// none starts there.
pub(super) fn literal_end(bytes: &[u8], start: usize) -> Option<usize> {
    let rest = bytes.get(start..)?;
    if rest.starts_with(b"//") {
        let length = rest.iter().position(|&byte| byte == b'\n');
        return Some(start + length.unwrap_or(rest.len()));
    }
    if rest.starts_with(b"/*") {
        return Some(block_comment_end(bytes, start));
    }
    if start > 0
        && bytes
            .get(start - 1)
            .is_some_and(|&byte| is_identifier_byte(byte))
    {
        return None;
    }
    let (prefix, raw) = match rest {
        [b'b' | b'c', b'r', ..] => (2, true),
        [b'r', ..] => (1, true),
        [b'b' | b'c', ..] => (1, false),
        _ => (0, false),
    };
    if raw {
        return raw_string_end(bytes, start + prefix);
    }
    match bytes.get(start + prefix) {
        Some(b'"') => Some(string_end(bytes, start + prefix)),
        Some(b'\'') => char_end(bytes, start + prefix),
        _ => None,
    }
}

/// The end of the block comment opening at `start`, nested ones included.
fn block_comment_end(bytes: &[u8], start: usize) -> usize {
    let mut depth = 0usize;
    let mut index = start;
    while let (Some(&first), Some(&second)) = (bytes.get(index), bytes.get(index + 1)) {
        match (first, second) {
            (b'/', b'*') => {
                depth += 1;
                index += 2;
            }
            (b'*', b'/') => {
                depth = depth.saturating_sub(1);
                index += 2;
                if depth == 0 {
                    return index;
                }
            }
            _ => index += 1,
        }
    }
    bytes.len()
}

/// The end of the raw string whose hashes or quote start at `at`, or `None`
/// when `at` starts a raw identifier such as `r#type`.
pub(super) fn raw_string_end(bytes: &[u8], at: usize) -> Option<usize> {
    let tail = bytes.get(at..)?;
    let hashes = tail.iter().take_while(|&&byte| byte == b'#').count();
    if tail.get(hashes) != Some(&b'"') {
        return None;
    }
    let mut closing = vec![b'"'];
    closing.extend(iter::repeat_n(b'#', hashes));
    let body = at + hashes + 1;
    let found = bytes
        .get(body..)?
        .windows(closing.len())
        .position(|window| window == closing.as_slice());
    Some(found.map_or(bytes.len(), |offset| body + offset + closing.len()))
}

/// The end of the string whose opening quote is at `at`: past the first
/// quote no backslash escapes.
fn string_end(bytes: &[u8], at: usize) -> usize {
    let mut index = at + 1;
    while let Some(&byte) = bytes.get(index) {
        match byte {
            b'\\' => index += 2,
            b'"' => return index + 1,
            _ => index += 1,
        }
    }
    bytes.len()
}

/// The end of the character literal opening at `at`, or `None` when the
/// quote starts a lifetime or a label such as `'a` or `'static`.
fn char_end(bytes: &[u8], at: usize) -> Option<usize> {
    let first = *bytes.get(at + 1)?;
    if first == b'\\' {
        let close = bytes
            .get(at + 3..)?
            .iter()
            .position(|&byte| byte == b'\'')?;
        return Some(at + 3 + close + 1);
    }
    let width = match first {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    };
    (bytes.get(at + 1 + width) == Some(&b'\'')).then_some(at + 2 + width)
}

/// Whether a byte can continue an identifier: ASCII letters, digits, `_`,
/// and any byte of a non-ASCII character.
pub(crate) fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

/// The line, counting from 1, that byte `offset` of `text` is on.
pub(crate) fn line_at(text: &str, offset: usize) -> usize {
    text.as_bytes()
        .get(..offset)
        .unwrap_or_default()
        .split(|&byte| byte == b'\n')
        .count()
}

/// The top-level items of blanked code, in order.
pub(crate) fn items(code: &str) -> Vec<Item> {
    let mut found = Vec::new();
    let mut index = 0;
    while let Some(start) = next_word(code, index) {
        // An inner attribute is the enclosing module's, never the next item's:
        // a crate root's `#![cfg(test)]` makes no single item test-only.
        if code
            .get(start..)
            .is_some_and(|rest| rest.starts_with("#!["))
        {
            index = attribute_end(code.as_bytes(), start + 2);
            continue;
        }
        let Some(first) = next_word(code, attributes_end(code, start)) else {
            break;
        };
        let (visibility, kind, name) = head(code.get(first..).unwrap_or_default());
        let end = item_end(code.as_bytes(), first, ends_at_semicolon(&kind));
        found.push(Item {
            line: line_at(code, first),
            attributes: code.get(start..first).unwrap_or_default().trim().to_owned(),
            visibility,
            kind,
            name,
            text: code.get(first..end).unwrap_or_default().to_owned(),
            span: (start, end),
        });
        index = end;
    }
    found
}

/// Blanked code with its `#[cfg(test)]` items blanked too: what the import
/// graph reads, since a test names its own module's items without that being
/// a dependency.
pub(crate) fn without_tests(code: &str) -> String {
    let mut out = code.as_bytes().to_vec();
    for item in items(code).iter().filter(|item| item.is_test()) {
        blank(out.get_mut(item.span.0..item.span.1).unwrap_or_default());
    }
    String::from_utf8(out).unwrap_or_default()
}

/// The offset of the first non-whitespace byte at or after `from`.
fn next_word(code: &str, from: usize) -> Option<usize> {
    code.get(from..)?
        .find(|character: char| !character.is_whitespace())
        .map(|offset| from + offset)
}

/// Where the attributes starting at `start` end: past every `#[...]` and
/// `#![...]`, and the whitespace after each.
pub(super) fn attributes_end(code: &str, start: usize) -> usize {
    let bytes = code.as_bytes();
    let mut index = start;
    loop {
        let rest = bytes.get(index..).unwrap_or_default();
        let open = if rest.starts_with(b"#[") {
            1
        } else if rest.starts_with(b"#![") {
            2
        } else {
            return index;
        };
        let cursor = attribute_end(bytes, index + open);
        index = next_word(code, cursor).unwrap_or(bytes.len());
    }
}

/// The byte after the `]` that closes the attribute whose `[` is at `cursor`.
pub(super) fn attribute_end(bytes: &[u8], mut cursor: usize) -> usize {
    let mut depth = 0usize;
    while let Some(&byte) = bytes.get(cursor) {
        cursor += 1;
        match byte {
            b'[' => depth += 1,
            b']' if depth <= 1 => break,
            b']' => depth -= 1,
            _ => {}
        }
    }
    cursor
}

/// The visibility, kind and name an item's text opens with.
pub(super) fn head(text: &str) -> (String, String, String) {
    let (visibility, rest) = visibility_of(text);
    let mut words = rest
        .split(|character: char| {
            !(character.is_alphanumeric() || character == '_' || character == '!')
        })
        .filter(|word| !word.is_empty())
        .take(8)
        .peekable();
    while let Some(word) = words.next() {
        let next = words.peek().copied().unwrap_or_default();
        if word == "extern" && next == "crate" {
            words.next();
            let name = words.next().unwrap_or_default();
            return (visibility, "extern crate".to_owned(), name.to_owned());
        }
        let qualifier = matches!(word, "unsafe" | "async" | "default" | "safe" | "extern")
            || (word == "const" && matches!(next, "fn" | "unsafe" | "async" | "extern"));
        if qualifier {
            continue;
        }
        let call = word.ends_with('!') && word != "macro_rules!";
        let kind = if call { "macro" } else { word };
        let name = if call || matches!(word, "use" | "impl") {
            ""
        } else {
            next
        };
        return (visibility, kind.to_owned(), name.to_owned());
    }
    (visibility, String::new(), String::new())
}

/// The visibility an item's text opens with, whitespace collapsed, and
/// the text after it.
pub(super) fn visibility_of(text: &str) -> (String, &str) {
    let Some(after) = text.strip_prefix("pub") else {
        return (String::new(), text);
    };
    if after.starts_with(|character: char| character.is_alphanumeric() || character == '_') {
        return (String::new(), text);
    }
    let after = after.trim_start();
    let Some(group) = after.strip_prefix('(') else {
        return ("pub".to_owned(), after);
    };
    let close = group.find(')').unwrap_or(group.len());
    let inner = group
        .get(..close)
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    (
        format!("pub({inner})"),
        group.get(close + 1..).unwrap_or_default(),
    )
}

/// Whether an item of `kind` ends at its `;` even after a braced value, as
/// `const X: S = S { a: 1 };` does.
fn ends_at_semicolon(kind: &str) -> bool {
    matches!(kind, "const" | "static" | "type" | "use" | "extern crate")
}

/// Where the item whose first word is at `from` ends: past its `;` at depth
/// zero, or past the `}` that brings a braced item back to depth zero.
fn item_end(bytes: &[u8], from: usize, at_semicolon: bool) -> usize {
    let mut depth = 0usize;
    for (offset, &byte) in bytes.get(from..).unwrap_or_default().iter().enumerate() {
        match byte {
            b'{' | b'(' | b'[' => depth += 1,
            b'}' | b')' | b']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 && byte == b'}' && !at_semicolon {
                    return from + offset + 1;
                }
            }
            b';' if depth == 0 => return from + offset + 1,
            _ => {}
        }
    }
    bytes.len()
}

#[cfg(test)]
mod tests {
    use super::{blanked, items, without_tests};

    #[test]
    fn comments_and_literals_blank_to_spaces_on_their_own_lines() {
        let source = concat!(
            "a // crate::x\n",
            "b /* crate::y /* nested */ crate::z */ c\n",
            "d \"crate::s \\\" crate::t\" r#\"crate::u\"# b\"crate::v\"\n",
            "e 'x' '\\'' '\\u{e9}' 'é' '\"' f\n",
        );
        let code = blanked(source);
        assert_eq!(code.len(), source.len());
        assert_eq!(code.lines().count(), 4);
        assert!(!code.contains("crate"), "{code}");
        assert_eq!(
            code.split_whitespace().collect::<Vec<_>>(),
            ["a", "b", "c", "d", "e", "f"]
        );
    }

    #[test]
    fn comments_are_listed_with_their_line_and_strings_are_not() {
        let source = "let a = \"// not one\";\n// one\nlet b = 1; /* two\n */\n";
        assert_eq!(super::comments(source), [(2, "// one"), (3, "/* two\n */")]);
    }

    #[test]
    fn lifetimes_labels_and_raw_identifiers_stay_code() {
        let source =
            "fn f<'a>(x: &'a str) -> &'static str { 'outer: loop { break 'outer r#type(x); } }";
        assert_eq!(blanked(source), source);
    }

    #[test]
    fn top_level_items_carry_attributes_visibility_kind_and_name() {
        let code = blanked(concat!(
            "//! Door.\n",
            "#![forbid(unsafe_code)]\n",
            "mod a;\n",
            "pub(crate) mod b;\n",
            "pub(in crate::x) use a::{C, D};\n",
            "#[cfg(test)]\n",
            "mod tests {\n",
            "    fn t() {}\n",
            "}\n",
            "const X: [u8; 2] = [1, 2];\n",
            "pub(crate) const fn f() -> S { S {} }\n",
            "extern crate alloc;\n",
            "thread_local! { static Y: u8 = 0; }\n",
            "impl S { fn g(&self) {} }\n",
        ));
        let found = items(&code);
        let heads: Vec<(usize, &str, &str, &str)> = found
            .iter()
            .map(|item| {
                (
                    item.line,
                    item.visibility.as_str(),
                    item.kind.as_str(),
                    item.name.as_str(),
                )
            })
            .collect();
        assert_eq!(
            heads,
            [
                (3, "", "mod", "a"),
                (4, "pub(crate)", "mod", "b"),
                (5, "pub(in crate::x)", "use", ""),
                (7, "", "mod", "tests"),
                (10, "", "const", "X"),
                (11, "pub(crate)", "fn", "f"),
                (12, "", "extern crate", "alloc"),
                (13, "", "macro", ""),
                (14, "", "impl", ""),
            ]
        );
        assert!(found[3].is_test() && !found[0].is_test());
        assert!(found[0].is_module_file() && !found[3].is_module_file());
        assert!(found[1].is_offered() && found[2].is_offered() && !found[0].is_offered());
        assert!(found[0].attributes.is_empty(), "{}", found[0].attributes);
    }

    #[test]
    fn an_inner_attribute_belongs_to_the_module_not_the_next_item() {
        let code = blanked("//! Tests.\n#![cfg(test)]\n#![forbid(unsafe_code)]\n\nmod ci;\n");
        let found = items(&code);
        assert_eq!(found.len(), 1);
        assert!(!found[0].is_test());
        assert_eq!(without_tests(&code), code);
    }

    #[test]
    fn items_under_test_leave_the_code_the_import_graph_reads() {
        let code = blanked("use crate::a;\n#[cfg(test)]\nmod tests {\n    use super::*;\n}\n");
        let kept = without_tests(&code);
        assert!(
            kept.contains("crate::a") && !kept.contains("super"),
            "{kept}"
        );
        assert_eq!(kept.lines().count(), code.lines().count());
    }
}

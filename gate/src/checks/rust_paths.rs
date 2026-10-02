//! The paths a Rust file names: every `use` tree expanded to its leaves, and
//! every other `a::b` chain, each with its line. Read from blanked code, so a
//! path never comes out of a string or a comment.

use super::rust_code::{blank, is_identifier_byte, line_at};

/// One path a file names, and its line.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct NamedPath {
    /// The line, counting from 1.
    pub(crate) line: usize,
    /// Its segments: `["crate", "runner", "Cmd"]`.
    pub(crate) segments: Vec<String>,
}

/// Every path of two segments or more that blanked `code` names, the `use`
/// declarations first.
pub(crate) fn paths(code: &str) -> Vec<NamedPath> {
    let mut rest = code.as_bytes().to_vec();
    let mut found = Vec::new();
    for (start, end) in use_declarations(code) {
        let line = line_at(code, start);
        let leaves = use_leaves(code.get(start..end).unwrap_or_default());
        found.extend(
            leaves
                .into_iter()
                .filter(|segments| segments.len() >= 2)
                .map(|segments| NamedPath { line, segments }),
        );
        blank(rest.get_mut(start..end).unwrap_or_default());
    }
    blank_visibilities(&mut rest);
    found.extend(chains(&String::from_utf8(rest).unwrap_or_default()));
    found
}

/// Blank every `pub(in path)` group: a visibility names a module without
/// depending on it.
fn blank_visibilities(code: &mut [u8]) {
    let mut index = 0;
    while let Some(offset) = code
        .get(index..)
        .and_then(|rest| rest.windows(4).position(|window| window == b"pub("))
    {
        let open = index + offset;
        let close = code
            .get(open..)
            .and_then(|rest| rest.iter().position(|&byte| byte == b')'))
            .map_or(code.len(), |length| open + length);
        blank(code.get_mut(open..close).unwrap_or_default());
        index = close;
    }
}

/// The leaves of one `use` declaration: `use a::{b, c::d};` gives `a::b` and
/// `a::c::d`. `self` names the module its braces hang from, and a renamed
/// import keeps its original name.
pub(crate) fn use_leaves(declaration: &str) -> Vec<Vec<String>> {
    let Some(start) = word_offsets(declaration, "use").first().copied() else {
        return Vec::new();
    };
    let tree = declaration.get(start + 3..).unwrap_or_default();
    expand(&[], &normalize(tree.trim_end().trim_end_matches(';')))
}

/// Where each `use` declaration starts and ends, before its trailing semicolon.
fn use_declarations(code: &str) -> Vec<(usize, usize)> {
    word_offsets(code, "use")
        .into_iter()
        .map(|start| {
            let tail = code.get(start..).unwrap_or_default();
            (start, start + tail.find(';').unwrap_or(tail.len()))
        })
        .collect()
}

/// Every offset where `word` stands as a whole word in `text`.
fn word_offsets(text: &str, word: &str) -> Vec<usize> {
    let bytes = text.as_bytes();
    text.match_indices(word)
        .map(|(offset, _)| offset)
        .filter(|&offset| {
            let before = offset.checked_sub(1).and_then(|index| bytes.get(index));
            let after = bytes.get(offset + word.len());
            !before.is_some_and(|&byte| is_identifier_byte(byte))
                && !after.is_some_and(|&byte| is_identifier_byte(byte))
        })
        .collect()
}

/// A `use` tree with its whitespace gone, but for the spaces around `as`.
fn normalize(tree: &str) -> String {
    let words: Vec<&str> = tree.split_whitespace().collect();
    let mut out = String::new();
    for (index, word) in words.iter().enumerate() {
        let previous = index
            .checked_sub(1)
            .and_then(|previous| words.get(previous));
        if *word == "as" || previous.is_some_and(|previous| *previous == "as") {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

/// The leaves of a normalized `use` tree, each after `prefix`.
fn expand(prefix: &[String], tree: &str) -> Vec<Vec<String>> {
    if let Some(open) = tree.find('{') {
        let close = tree.rfind('}').unwrap_or(tree.len());
        let mut base = prefix.to_vec();
        base.extend(segments(tree.get(..open).unwrap_or_default()));
        return split_top_level(tree.get(open + 1..close).unwrap_or_default())
            .into_iter()
            .flat_map(|part| expand(&base, part))
            .collect();
    }
    let original = tree.split(" as ").next().unwrap_or_default();
    let mut path = prefix.to_vec();
    path.extend(segments(original));
    if path.last().is_some_and(|last| last == "self") {
        path.pop();
    }
    vec![path]
}

/// The segments of a path written with `::`.
fn segments(path: &str) -> Vec<String> {
    path.split("::")
        .filter(|segment| !segment.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The parts of a brace group, split at its top-level commas.
fn split_top_level(group: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (index, character) in group.char_indices() {
        match character {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(group.get(start..index).unwrap_or_default());
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(group.get(start..).unwrap_or_default());
    parts.into_iter().filter(|part| !part.is_empty()).collect()
}

/// Whether a byte can start an identifier.
fn starts_identifier(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80
}

/// Every `a::b` chain of `code`: one starts where an identifier follows
/// neither another identifier nor `:`.
fn chains(code: &str) -> Vec<NamedPath> {
    let bytes = code.as_bytes();
    let mut found = Vec::new();
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        let boundary = index
            .checked_sub(1)
            .and_then(|previous| bytes.get(previous))
            .is_none_or(|&previous| !is_identifier_byte(previous) && previous != b':');
        if !(boundary && starts_identifier(byte)) {
            index += 1;
            continue;
        }
        let (segments, end) = chain_at(code, index);
        if segments.len() >= 2 {
            found.push(NamedPath {
                line: line_at(code, index),
                segments,
            });
        }
        index = end;
    }
    found
}

/// The segments of the chain starting at `start`, and where it ends: words
/// joined by `::`, whitespace allowed around it; `::<` ends it.
fn chain_at(code: &str, start: usize) -> (Vec<String>, usize) {
    let bytes = code.as_bytes();
    let mut segments = Vec::new();
    let mut index = start;
    loop {
        let length = bytes
            .get(index..)
            .unwrap_or_default()
            .iter()
            .take_while(|&&byte| is_identifier_byte(byte))
            .count();
        segments.push(
            code.get(index..index + length)
                .unwrap_or_default()
                .to_owned(),
        );
        let word_end = index + length;
        let colons = skip_spaces(bytes, word_end);
        if !bytes.get(colons..).unwrap_or_default().starts_with(b"::") {
            return (segments, word_end);
        }
        let next = skip_spaces(bytes, colons + 2);
        if !bytes.get(next).is_some_and(|&byte| starts_identifier(byte)) {
            return (segments, word_end);
        }
        index = next;
    }
}

/// The first offset at or after `from` that is not whitespace.
fn skip_spaces(bytes: &[u8], from: usize) -> usize {
    from + bytes
        .get(from..)
        .unwrap_or_default()
        .iter()
        .take_while(|byte| byte.is_ascii_whitespace())
        .count()
}

#[cfg(test)]
mod tests {
    use super::{paths, use_leaves};

    /// Owned segments, for comparing with what the reader returns.
    fn owned(paths: &[&[&str]]) -> Vec<Vec<String>> {
        paths
            .iter()
            .map(|path| path.iter().map(|segment| (*segment).to_owned()).collect())
            .collect()
    }

    #[test]
    fn nested_imports_after_visibility_keep_all_following_dependencies() {
        assert_eq!(
            paths("pub(crate) use crate::{a::{B, C}, D}; crate::after::Name;")
                .iter()
                .map(|path| path.segments.join("::"))
                .collect::<Vec<_>>(),
            [
                "crate::a::B",
                "crate::a::C",
                "crate::D",
                "crate::after::Name"
            ]
        );
    }

    #[test]
    fn adjacent_nested_groups_keep_every_following_leaf() {
        assert_eq!(
            use_leaves("use crate::{a::{B, C}, d::{E, F}, G};"),
            owned(&[
                &["crate", "a", "B"],
                &["crate", "a", "C"],
                &["crate", "d", "E"],
                &["crate", "d", "F"],
                &["crate", "G"],
            ])
        );
    }

    #[test]
    fn identifier_boundaries_refuse_partial_words_and_colon_suffixes() {
        assert!(use_leaves("abuse crate::Wrong;").is_empty());
        assert!(use_leaves("useful crate::Wrong;").is_empty());
        assert_eq!(
            paths("9bad::Name; ::root::Name; _local::Name; aé::Name;")
                .iter()
                .map(|path| path.segments.join("::"))
                .collect::<Vec<_>>(),
            ["_local::Name", "aé::Name"]
        );
    }

    #[test]
    fn repeated_visibility_groups_do_not_expose_module_dependencies() {
        assert!(paths("pub(in crate::a) fn f() {} pub(in crate::b) fn g() {}").is_empty());
        assert!(paths("pub(in crate::a").is_empty());
        assert_eq!(
            paths("pub(in crate::aé) fn f() { crate::real::Name; }")
                .iter()
                .map(|path| path.segments.join("::"))
                .collect::<Vec<_>>(),
            ["crate::real::Name"]
        );
    }

    #[test]
    fn use_trees_expand_to_every_leaf_with_self_and_renames_resolved() {
        let leaves = use_leaves(
            "pub(crate) use crate::runner::{self, Cmd as Command, jobs::{Job, summary}};",
        );
        assert_eq!(
            leaves,
            owned(&[
                &["crate", "runner"],
                &["crate", "runner", "Cmd"],
                &["crate", "runner", "jobs", "Job"],
                &["crate", "runner", "jobs", "summary"],
            ])
        );
        assert_eq!(use_leaves("use std :: fs ;"), owned(&[&["std", "fs"]]));
    }

    #[test]
    fn a_visibility_path_is_not_an_import() {
        assert!(
            paths("pub(in crate::steps) const STEPS: u8 = crate::a::B;\n")
                .iter()
                .all(
                    |path| path.segments.first().is_some_and(|first| first == "crate")
                        && path.segments.get(1).is_some_and(|second| second == "a")
                )
        );
        assert!(paths("pub(in crate::steps) fn f() {}\n").is_empty());
    }

    #[test]
    fn chains_outside_use_declarations_come_with_their_line() {
        let code =
            "use std::fs;\nfn f() {\n    crate::a::b(super::c::D::new());\n    x.y::<u8>();\n}\n";
        let found: Vec<(usize, String)> = paths(code)
            .iter()
            .map(|path| (path.line, path.segments.join("::")))
            .collect();
        assert_eq!(
            found,
            [
                (1, "std::fs".to_owned()),
                (3, "crate::a::b".to_owned()),
                (3, "super::c::D::new".to_owned()),
            ]
        );
    }
}

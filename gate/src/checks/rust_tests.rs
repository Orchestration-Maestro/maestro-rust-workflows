//! The tests inside Rust source, as the naming and timing rules read them:
//! every test function with its line, the code only tests compile, the waits
//! on time it holds, and the function enclosing a line.

use super::rust_code::{blank, is_identifier_byte, items, line_at};
use super::rust_paths::paths;

/// The modules that make a `sleep` a wait on time.
const CLOCKS: &[&str] = &["thread", "time", "task"];

/// Every function a test attribute marks, at any depth: `#[test]`, or any
/// attribute whose path ends in `::test`, such as `#[tokio::test]`.
pub(crate) fn test_functions(code: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (offset, _) in code.match_indices("#[") {
        let end = attribute_end(code, offset);
        let attribute: String = code
            .get(offset + 2..end.saturating_sub(1))
            .unwrap_or_default()
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect();
        let path = attribute.split('(').next().unwrap_or_default();
        if path != "test" && !path.ends_with("::test") {
            continue;
        }
        if let Some((name_offset, name)) = function_after(code, end) {
            found.push((line_at(code, name_offset), name));
        }
    }
    found
}

/// Blanked code with everything but its `#[cfg(test)]` items blanked: the
/// code only a test build compiles.
pub(crate) fn only_tests(code: &str) -> String {
    let mut out = code.as_bytes().to_vec();
    blank(&mut out);
    for item in items(code).iter().filter(|item| item.is_test()) {
        let (start, end) = item.span;
        if let (Some(target), Some(source)) =
            (out.get_mut(start..end), code.as_bytes().get(start..end))
        {
            target.copy_from_slice(source);
        }
    }
    String::from_utf8(out).unwrap_or_default()
}

/// One wait on time: where it is, the path it names and the function whose
/// body holds it.
pub(crate) struct Wait {
    /// The line, counting from 1.
    pub(crate) line: usize,
    /// The path: `std::thread::sleep`.
    pub(crate) path: String,
    /// The innermost function holding it, when one does.
    pub(crate) function: Option<String>,
}

/// Every path in `code` that waits on time: one ending in `thread::sleep`,
/// `time::sleep` or `task::sleep`.
pub(crate) fn waits(code: &str) -> Vec<Wait> {
    paths(code)
        .into_iter()
        .filter(|path| {
            let mut last = path.segments.iter().rev();
            last.next().is_some_and(|name| name == "sleep")
                && last
                    .next()
                    .is_some_and(|clock| CLOCKS.contains(&clock.as_str()))
        })
        .map(|path| {
            let offset = line_offset(code, path.line).and_then(|start| {
                let text = code.get(start..)?;
                let end = text.find('\n').unwrap_or(text.len());
                Some(start + text.get(..end)?.find("sleep")?)
            });
            Wait {
                line: path.line,
                path: path.segments.join("::"),
                function: offset.and_then(|offset| enclosing_function(code, offset)),
            }
        })
        .collect()
}

/// The innermost function whose body holds byte `offset`, by name.
pub(super) fn enclosing_function(code: &str, offset: usize) -> Option<String> {
    let bytes = code.as_bytes();
    let mut found = None;
    for (start, _) in code.match_indices("fn ") {
        let whole = start
            .checked_sub(1)
            .and_then(|previous| bytes.get(previous))
            .is_none_or(|&byte| !is_identifier_byte(byte));
        if !whole {
            continue;
        }
        let Some((_, name)) = function_after(code, start) else {
            continue;
        };
        if body(bytes, start).is_some_and(|(open, close)| open < offset && offset < close) {
            found = Some(name);
        }
    }
    found
}

/// Where the attribute opening at `start` ends: past its matching `]`.
pub(super) fn attribute_end(code: &str, start: usize) -> usize {
    let mut depth = 0usize;
    for (offset, byte) in code.bytes().enumerate().skip(start) {
        match byte {
            b'[' => depth += 1,
            b']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return offset + 1;
                }
            }
            _ => {}
        }
    }
    code.len()
}

/// The function a declaration starting at `from` names, past attributes and
/// qualifiers: the offset of its name and the name.
pub(super) fn function_after(code: &str, from: usize) -> Option<(usize, String)> {
    let mut index = from;
    loop {
        let rest = code.get(index..)?;
        let skipped = rest.len() - rest.trim_start().len();
        index += skipped;
        let rest = code.get(index..)?;
        if rest.starts_with("#[") {
            index = attribute_end(code, index);
            continue;
        }
        if let Some(group) = rest.strip_prefix("pub(") {
            index += 4 + group.find(')')? + 1;
            continue;
        }
        let word: String = rest
            .chars()
            .take_while(|&character| character.is_alphanumeric() || character == '_')
            .collect();
        index += word.len();
        match word.as_str() {
            "fn" => {
                let rest = code.get(index..)?;
                let start = index + rest.len() - rest.trim_start().len();
                let name: String = code
                    .get(start..)?
                    .chars()
                    .take_while(|&character| character.is_alphanumeric() || character == '_')
                    .collect();
                return (!name.is_empty()).then_some((start, name));
            }
            "pub" | "async" | "unsafe" | "const" | "extern" => {}
            _ => return None,
        }
    }
}

/// The braces of the body of the function declared at `start`, or `None`
/// for a declaration that ends in `;`.
pub(super) fn body(bytes: &[u8], start: usize) -> Option<(usize, usize)> {
    let mut depth = 0usize;
    let mut open = None;
    for (offset, &byte) in bytes.iter().enumerate().skip(start) {
        match byte {
            b'(' | b'[' if open.is_none() => depth += 1,
            b')' | b']' if open.is_none() => depth = depth.saturating_sub(1),
            b';' if open.is_none() && depth == 0 => return None,
            b'{' => {
                if open.is_none() && depth == 0 {
                    open = Some(offset);
                }
                depth += 1;
            }
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return open.map(|open| (open, offset));
                }
            }
            _ => {}
        }
    }
    None
}

/// The byte offset where `line`, counting from 1, starts.
fn line_offset(code: &str, line: usize) -> Option<usize> {
    if line == 1 {
        return Some(0);
    }
    code.match_indices('\n')
        .nth(line.checked_sub(2)?)
        .map(|(offset, _)| offset + 1)
}

#[cfg(test)]
mod tests {
    use super::{only_tests, test_functions, waits};
    use crate::checks::rust_code::blanked;

    #[test]
    fn attributes_ending_in_test_mark_functions_at_any_depth() {
        let code = blanked(concat!(
            "fn helper() {}\n",
            "#[cfg(test)]\n",
            "mod tests {\n",
            "    #[test]\n",
            "    #[should_panic(expected = \"x\")]\n",
            "    fn a_refusal_names_its_rule() {}\n",
            "    #[tokio::test(flavor = \"multi_thread\")]\n",
            "    async fn an_async_wait_ends() {}\n",
            "}\n",
        ));
        assert_eq!(
            test_functions(&code),
            [
                (6, "a_refusal_names_its_rule".to_owned()),
                (8, "an_async_wait_ends".to_owned()),
            ]
        );
    }

    #[test]
    fn only_test_items_survive_and_their_waits_are_found() {
        let code = blanked(concat!(
            "fn live() { std::thread::sleep(d); }\n",
            "#[cfg(test)]\n",
            "mod tests {\n",
            "    fn helper() {\n",
            "        tokio::time::sleep(d);\n",
            "        std::thread::yield_now();\n",
            "    }\n",
            "}\n",
        ));
        let tests = only_tests(&code);
        assert_eq!(tests.lines().count(), code.lines().count());
        let found = waits(&tests);
        assert_eq!(found.len(), 1);
        assert_eq!(
            (
                found[0].line,
                found[0].path.as_str(),
                found[0].function.as_deref()
            ),
            (5, "tokio::time::sleep", Some("helper"))
        );
        let live = waits(&code);
        assert_eq!(live[0].function.as_deref(), Some("live"));
        assert!(
            waits("static WAIT: u8 = std::thread::sleep;\n")[0]
                .function
                .is_none()
        );
    }
}

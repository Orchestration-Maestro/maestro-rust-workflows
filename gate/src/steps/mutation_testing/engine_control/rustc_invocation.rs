//! Read Cargo's shell-escaped rustc commands without executing them.

use crate::runner::Failure;
use std::mem;
use std::path::Path;

/// Split Cargo's Unix shell escaping, including embedded quotes and escaped whitespace.
pub(super) fn words(command: &str) -> Result<Vec<String>, Failure> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut escaped = false;
    let mut started = false;
    for character in command.chars() {
        match character {
            _ if escaped => {
                word.push(character);
                escaped = false;
            }
            '\\' if !quoted => {
                escaped = true;
                started = true;
            }
            '\'' => {
                quoted = !quoted;
                started = true;
            }
            _ if character.is_whitespace() && !quoted => {
                if started {
                    words.push(mem::take(&mut word));
                    started = false;
                }
            }
            _ => {
                word.push(character);
                started = true;
            }
        }
    }
    if quoted || escaped {
        return Err(
            format!("featureless rustc invocation has malformed quoting: {command}").into(),
        );
    }
    if started {
        words.push(word);
    }
    Ok(words)
}

/// Read one rustc option in its separate or equals spelling.
pub(super) fn option<'a>(words: &'a [String], name: &str) -> Option<&'a str> {
    words.iter().enumerate().find_map(|(index, word)| {
        if word == name {
            words.get(index + 1).map(String::as_str)
        } else {
            word.strip_prefix(name)?.strip_prefix('=')
        }
    })
}

/// Derive the emitted filename from rustc's own output options, never Cargo's uplifted file.
pub(super) fn dep_path(words: &[String]) -> Result<String, Failure> {
    let name = option(words, "--crate-name").unwrap_or("unknown");
    let emit = option(words, "--emit").unwrap_or_default();
    if !emit.split(',').any(|kind| kind == "dep-info") {
        return Err(format!("featureless rustc unit has no dep-info emission: {name}").into());
    }
    let directory = option(words, "--out-dir")
        .ok_or_else(|| format!("featureless rustc unit has no output directory: {name}"))?;
    let suffix = words
        .iter()
        .enumerate()
        .find_map(|(index, word)| {
            let setting = if word == "-C" {
                words.get(index + 1)?.as_str()
            } else {
                word.strip_prefix("-C")?
            };
            setting.strip_prefix("extra-filename=")
        })
        .unwrap_or_default();
    Ok(Path::new(directory)
        .join(format!("{name}{suffix}.d"))
        .to_string_lossy()
        .into_owned())
}

#[cfg(test)]
mod tests {
    use super::{dep_path, option, words};

    #[test]
    fn cargo_quoting_keeps_paths_spaces_and_embedded_quotes() {
        assert_eq!(
            words("rustc 'a b' 'it'\\''s.rs' a\\ b").unwrap(),
            ["rustc", "a b", "it's.rs", "a b"]
        );
        assert_eq!(
            words("rustc '' probe ''").unwrap(),
            ["rustc", "", "probe", ""]
        );
        for command in ["rustc 'unfinished", "rustc unfinished\\"] {
            let error = words(command).unwrap_err();
            assert!(
                error
                    .message
                    .unwrap()
                    .contains("featureless rustc invocation has malformed quoting:")
            );
        }
        assert_eq!(
            option(&words("--crate-name=probe").unwrap(), "--crate-name"),
            Some("probe")
        );
        assert_eq!(
            option(&words("--crate-name").unwrap(), "--crate-name"),
            None
        );
    }

    #[test]
    fn compiler_options_select_exact_hashed_and_unhashed_dep_info() {
        for (flags, expected) in [
            ("-C extra-filename=-1234", "/target space/deps/probe-1234.d"),
            ("-Cextra-filename=-5678", "/target space/deps/probe-5678.d"),
            ("", "/target space/deps/probe.d"),
        ] {
            let command = format!(
                "rustc --crate-name probe --emit=dep-info,link \
                 --out-dir '/target space/deps' {flags}"
            );
            assert_eq!(dep_path(&words(&command).unwrap()).unwrap(), expected);
        }
        for (command, message) in [
            (
                "rustc --crate-name probe --emit=link",
                "featureless rustc unit has no dep-info emission: probe",
            ),
            (
                "rustc --crate-name probe --emit=dep-info",
                "featureless rustc unit has no output directory: probe",
            ),
        ] {
            assert_eq!(
                dep_path(&words(command).unwrap())
                    .unwrap_err()
                    .message
                    .as_deref(),
                Some(message)
            );
        }
    }
}

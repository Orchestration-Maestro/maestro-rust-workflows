//! Neutralize Gitleaks' automatic root ignore-file loading without dropping its content.

use crate::runner::Failure;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

/// Gitleaks loads this file even when its explicit ignore path points elsewhere.
pub(super) fn relocate_ignore(source: &Path) -> Result<Option<String>, Failure> {
    let original = source.join(".gitleaksignore");
    match fs::symlink_metadata(&original) {
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot inspect {}: {error}", original.display()).into()),
        Ok(_) => {}
    }
    let mut index = 0u64;
    loop {
        let name = format!(".rust-gate-gitleaksignore-{index}");
        let renamed = source.join(&name);
        match fs::symlink_metadata(&renamed) {
            Err(error) if error.kind() == ErrorKind::NotFound => {
                fs::rename(&original, &renamed)
                    .map_err(|error| format!("cannot relocate {}: {error}", original.display()))?;
                return Ok(Some(name));
            }
            Err(error) => {
                return Err(format!("cannot inspect {}: {error}", renamed.display()).into());
            }
            Ok(_) => index += 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::relocate_ignore;
    use crate::checks::private_directories::private_directory;
    use std::{env, fs};

    #[test]
    fn ignore_relocation_preserves_content_and_never_overwrites_source() {
        let source = private_directory(env::temp_dir().to_str().unwrap(), "secret-ignore").unwrap();
        assert!(relocate_ignore(&source).unwrap().is_none());
        fs::write(source.join(".gitleaksignore"), "consumer content").unwrap();
        fs::write(
            source.join(".rust-gate-gitleaksignore-0"),
            "existing content",
        )
        .unwrap();
        let name = relocate_ignore(&source).unwrap().unwrap();
        assert!(!source.join(".gitleaksignore").exists());
        assert_eq!(
            fs::read_to_string(source.join(name)).unwrap(),
            "consumer content"
        );
        assert_eq!(
            fs::read_to_string(source.join(".rust-gate-gitleaksignore-0")).unwrap(),
            "existing content"
        );
        fs::remove_dir_all(source).unwrap();
    }
}

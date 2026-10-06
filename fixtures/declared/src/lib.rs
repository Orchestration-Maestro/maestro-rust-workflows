//! A declaration-driven shared workflow fixture.

#[cfg(test)]
mod tests {
    use std::{env, fs, path::PathBuf};

    #[test]
    fn test_environment_is_isolated() {
        for (name, _) in env::vars() {
            assert!(!name.ends_with("_API_KEY"));
        }
        assert!(env::var_os("UNRELATED_AMBIENT_CANARY").is_none());
        assert_eq!(env::var("MAESTRO_NO_LOCAL_LLM").unwrap(), "1");
        let names = [
            "HOME",
            "TMPDIR",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_CACHE_HOME",
            "XDG_STATE_HOME",
            "XDG_RUNTIME_DIR",
            "XDG_CONFIG_DIRS",
            "XDG_DATA_DIRS",
        ];
        let paths: Vec<_> = names
            .iter()
            .map(|name| PathBuf::from(env::var(name).unwrap()))
            .collect();
        for (index, path) in paths.iter().enumerate() {
            assert!(path.is_dir());
            assert_eq!(path.parent(), paths[0].parent());
            assert!(!paths[..index].contains(path));
            assert_eq!(fs::read_dir(path).unwrap().count(), 0);
        }
        assert_eq!(env::var("TMP").unwrap(), env::var("TMPDIR").unwrap());
        assert_eq!(env::var("TEMP").unwrap(), env::var("TMPDIR").unwrap());
        assert!(!paths[0].join("original-home-marker").exists());
        assert!(!paths[0].join(".maestro/auth.json").exists());
    }
    #[test]
    fn search_commands_read_fixture_files() {
        use std::process::Command;
        let directory = PathBuf::from(env::var("TMPDIR").unwrap())
            .parent()
            .unwrap()
            .join("search-fixture");
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("needle.txt"), "shared-search-canary\n").unwrap();
        for (tool, binary) in [("rg", "/usr/bin/rg"), ("fd", "/usr/bin/fdfind")] {
            let resolved = Command::new("sh")
                .args(["-c", &format!("command -v {tool}")])
                .output()
                .unwrap();
            assert!(resolved.status.success());
            let path = String::from_utf8(resolved.stdout).unwrap();
            assert_eq!(
                fs::canonicalize(path.trim()).unwrap(),
                fs::canonicalize(binary).unwrap()
            );
        }
        let rg = Command::new("rg")
            .args(["--no-config", "shared-search-canary", "needle.txt"])
            .current_dir(&directory)
            .output()
            .unwrap();
        assert!(rg.status.success());
        assert_eq!(
            String::from_utf8(rg.stdout).unwrap(),
            "shared-search-canary\n"
        );
        let fd = Command::new("fd")
            .args(["--color", "never", "--glob", "needle.txt", "."])
            .current_dir(&directory)
            .output()
            .unwrap();
        assert!(fd.status.success());
        assert_eq!(String::from_utf8(fd.stdout).unwrap(), "./needle.txt\n");
        fs::remove_dir_all(directory).unwrap();
    }
}

use std::{error::Error, process::Command};

fn main() -> Result<(), Box<dyn Error>> {
    let result = Command::new("just")
        .arg("--version")
        .output()
        .map_err(|error| format!("caller build tool just is unavailable: {error}"))?;
    if !result.status.success() {
        return Err("caller build tool just --version failed".into());
    }
    println!("cargo::rerun-if-changed=mise.toml");
    Ok(())
}

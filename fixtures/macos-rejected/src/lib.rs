//! A deliberately macos-incompatible fixture.

#[cfg(target_os = "macos")]
compile_error!("fixture rejects macos");

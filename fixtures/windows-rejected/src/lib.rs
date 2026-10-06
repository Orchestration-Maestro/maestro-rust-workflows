//! A deliberately windows-incompatible fixture.

#[cfg(windows)]
compile_error!("fixture rejects windows");

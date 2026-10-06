//! A deliberately wasm-incompatible fixture.

#[cfg(target_arch = "wasm32")]
compile_error!("fixture rejects wasm32");

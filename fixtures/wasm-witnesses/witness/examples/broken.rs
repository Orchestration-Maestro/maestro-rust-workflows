//! A witness that only the wasm32 target rejects.

#[cfg(target_arch = "wasm32")]
compile_error!("witness rejects wasm32");

fn main() {}

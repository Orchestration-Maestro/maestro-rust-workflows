//! A fixture whose WebAssembly-only item is undocumented.

/// Returns twice the input.
pub fn double(value: u32) -> u32 {
    value * 2
}

#[cfg(target_arch = "wasm32")]
pub fn undocumented_wasm_only() {}

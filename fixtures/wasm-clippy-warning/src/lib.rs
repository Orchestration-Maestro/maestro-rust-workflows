//! A fixture whose WebAssembly-only body fails Clippy.

/// Returns twice the input.
pub fn double(value: u32) -> u32 {
    value * 2
}

/// Returns the input unchanged.
#[cfg(target_arch = "wasm32")]
pub fn unchanged(value: u32) -> u32 {
    return value;
}

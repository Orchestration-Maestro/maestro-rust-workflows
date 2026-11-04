//! A fixture whose non-default feature hides an undocumented item.

/// Returns twice the input.
pub fn double(value: u32) -> u32 {
    value * 2
}

#[cfg(feature = "extra")]
pub fn undocumented_extra() {}

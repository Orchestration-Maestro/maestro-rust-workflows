//! A library whose `tick` function exists only with the `clock` feature.

/// Returns the clock reading; present only with the `clock` feature.
#[cfg(feature = "clock")]
pub fn tick() -> u32 {
    1
}

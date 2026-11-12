//! A library that enables the provider's `clock` feature for the whole workspace build.

/// Reads the provider's clock.
pub fn now() -> u32 {
    provider::tick()
}

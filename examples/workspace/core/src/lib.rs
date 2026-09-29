//! Shared checked arithmetic for the workspace consumer fixture.

#![forbid(unsafe_code)]

mod arithmetic;
#[cfg(windows)]
mod windows;

pub use arithmetic::checked_sum;
#[cfg(windows)]
pub use windows::line_ending;

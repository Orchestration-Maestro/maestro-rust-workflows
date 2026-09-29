//! Shared checked arithmetic for the workspace consumer fixture.

#![forbid(unsafe_code)]

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::line_ending;

/// Adds two bounded integers, returning `None` on overflow.
///
/// # Examples
///
/// ```
/// assert_eq!(maestro_workspace_arithmetic::checked_sum(20, 22), Some(42));
/// assert_eq!(maestro_workspace_arithmetic::checked_sum(u32::MAX, 1), None);
/// ```
#[must_use]
pub const fn checked_sum(left: u32, right: u32) -> Option<u32> {
    right.checked_add(left)
}

#[cfg(test)]
mod tests {
    use super::checked_sum;

    #[test]
    fn checked_sum_adds_and_refuses_overflow() {
        assert_eq!(checked_sum(20, 22), Some(42));
        assert_eq!(checked_sum(u32::MAX, 1), None);
        assert_eq!(checked_sum(0, 0), Some(0));
    }
}

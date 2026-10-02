//! Rust version strings, compared the way `sort -V` compared them.

/// `major.minor.patch`, or `major.minor` when `patch_optional`, digits only.
/// Missing patch counts as zero, so `1.85` sits at `1.85.0`.
pub(crate) fn parse(value: &str, patch_optional: bool) -> Option<(u64, u64, u64)> {
    let parts: Vec<&str> = value.split('.').collect();
    let number = |part: &str| -> Option<u64> {
        (!part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| part.parse().ok())
            .flatten()
    };
    match parts.as_slice() {
        [major, minor, patch] => Some((number(major)?, number(minor)?, number(patch)?)),
        [major, minor] if patch_optional => Some((number(major)?, number(minor)?, 0)),
        _ => None,
    }
}

/// `1.x.y`, digits only: a channel or a minor-only value is not a pin.
pub(crate) fn is_exact_stable(value: &str) -> bool {
    parse(value, false).is_some_and(|(major, _, _)| major == 1)
}

/// `nightly` or `nightly-YYYY-MM-DD`.
pub(crate) fn is_nightly(value: &str) -> bool {
    match value.strip_prefix("nightly") {
        Some("") => true,
        Some(date) => {
            let digits: Vec<&str> = date
                .strip_prefix('-')
                .map(|rest| rest.split('-').collect())
                .unwrap_or_default();
            matches!(digits.as_slice(), [year, month, day]
                if year.len() == 4 && month.len() == 2 && day.len() == 2)
                && digits
                    .iter()
                    .all(|part| part.bytes().all(|byte| byte.is_ascii_digit()))
        }
        None => false,
    }
}

/// The value of a `channel = "..."` line, if this is one.
pub(crate) fn channel_value(line: &str) -> Option<String> {
    let rest = line.trim_start().strip_prefix("channel")?.trim_start();
    let rest = rest.strip_prefix('=')?.trim_start();
    let quote = rest
        .chars()
        .next()
        .filter(|character| *character == '"' || *character == '\'')?;
    let value: String = rest[1..]
        .chars()
        .take_while(|character| *character != quote)
        .collect();
    rest[1..].contains(quote).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::{channel_value, is_exact_stable, parse};

    #[test]
    fn pins_are_exact_stable_versions() {
        assert!(is_exact_stable("1.98.1") && is_exact_stable("1.85.0"));
        for not_a_pin in [
            "stable",
            "1.98",
            "nightly-2026-09-14",
            "2.0.0",
            "1.98.1.2",
            "1.a.0",
            "1.+98.1",
            "+1.98.1",
        ] {
            assert!(!is_exact_stable(not_a_pin), "{not_a_pin}");
        }
        assert!(parse("1.100.0", false) > parse("1.99.9", false));
    }

    #[test]
    fn a_declared_msrv_may_omit_the_patch() {
        assert_eq!(parse("1.85", true), Some((1, 85, 0)));
        assert_eq!(parse("1.85", false), None);
        assert!(parse("1.85", true) <= parse("1.85.0", false));
    }

    #[test]
    fn nightly_is_the_channel_or_a_dated_one() {
        assert!(super::is_nightly("nightly") && super::is_nightly("nightly-2026-09-14"));
        for bad in [
            "stable",
            "1.98.1",
            "nightly-2026-9-14",
            "nightly-",
            "nightlyx",
        ] {
            assert!(!super::is_nightly(bad), "{bad}");
        }
    }

    #[test]
    fn the_channel_line_is_read_like_the_sed_it_replaces() {
        assert_eq!(channel_value("channel = \"1.98.1\""), Some("1.98.1".into()));
        assert_eq!(
            channel_value("  channel='1.85.0' # pin"),
            Some("1.85.0".into())
        );
        assert_eq!(channel_value("components = [\"rustfmt\"]"), None);
        assert_eq!(channel_value("channel = 1.98.1"), None);
    }
}

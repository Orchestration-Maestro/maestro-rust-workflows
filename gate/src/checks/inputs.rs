//! The `ci.yml` inputs with a shape of their own, each read and refused in
//! one place: the three closed-set policies, the coverage threshold and the
//! artifact key. `validate` refuses a malformed value before any work, and a
//! later step reads the typed value, never the text again.
//!
//! One accessor per input rather than one value holding them all: a step reads
//! only what it declared, which is what makes the refusal of an undeclared
//! input verifiable step by step. A bundle would make a step that needs the
//! coverage threshold declare the artifact key and the three policies as well,
//! and `docs/steps.md` would then name inputs the step never uses.

use super::simple_names::simple;
use crate::runner::{input, optional};

/// `license-policy`, kept for the repositories that set it: the
/// organization's licence policy applies whatever it says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LicensePolicy {
    /// The organization's policy.
    Auto,
    /// The organization's policy, the same as `auto`.
    Enforce,
    /// Refused by `validate`: no repository opts out.
    Off,
}

impl LicensePolicy {
    /// The value as `ci.yml` spells it.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Enforce => "enforce",
            Self::Off => "off",
        }
    }
}

/// `unsafe-policy`: whether an `unsafe` block in a workspace member fails
/// the run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UnsafePolicy {
    /// The project decides; an FFI crate needs it.
    Allow,
    /// Refused in every workspace member, never in a dependency.
    Deny,
}

impl UnsafePolicy {
    /// The value as `ci.yml` spells it.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }
}

/// `clippy-level`: the Clippy groups denied on top of the default ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClippyLevel {
    /// Clippy's default groups, every warning denied.
    Default,
    /// `clippy::pedantic` denied as well.
    Pedantic,
    /// `clippy::nursery` denied on top of pedantic: the unstable group is
    /// only meaningful over it.
    Nursery,
}

impl ClippyLevel {
    /// The value as `ci.yml` spells it.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Pedantic => "pedantic",
            Self::Nursery => "nursery",
        }
    }

    /// The groups this level denies beyond the defaults, in the order they
    /// are passed to Clippy.
    pub(crate) fn denied(self) -> impl Iterator<Item = &'static str> {
        let groups = match self {
            Self::Default => 0,
            Self::Pedantic => 1,
            Self::Nursery => 2,
        };
        ["clippy::pedantic", "clippy::nursery"]
            .into_iter()
            .take(groups)
    }
}

/// `license-policy`, read from `LICENSE_POLICY`: one of its three values.
pub(crate) fn license_policy() -> Result<LicensePolicy, String> {
    one_of(
        "license-policy",
        &input("LICENSE_POLICY")?,
        &[
            LicensePolicy::Auto,
            LicensePolicy::Enforce,
            LicensePolicy::Off,
        ],
        LicensePolicy::as_str,
    )
}

/// `unsafe-policy`, read from `UNSAFE_POLICY`: one of its two values.
pub(crate) fn unsafe_policy() -> Result<UnsafePolicy, String> {
    one_of(
        "unsafe-policy",
        &input("UNSAFE_POLICY")?,
        &[UnsafePolicy::Allow, UnsafePolicy::Deny],
        UnsafePolicy::as_str,
    )
}

/// `clippy-level`, read from `CLIPPY_LEVEL`: one of its three values.
pub(crate) fn clippy_level() -> Result<ClippyLevel, String> {
    one_of(
        "clippy-level",
        &input("CLIPPY_LEVEL")?,
        &[
            ClippyLevel::Default,
            ClippyLevel::Pedantic,
            ClippyLevel::Nursery,
        ],
        ClippyLevel::as_str,
    )
}

/// `COVERAGE`, `coverage-threshold` in `ci.yml`: a plain decimal from the
/// organization's floor, COV-001's 90, to 100, kept as the consumer wrote it,
/// the way `cargo llvm-cov` takes it.
pub(crate) fn coverage_threshold() -> Result<String, String> {
    let coverage = input("COVERAGE")?;
    if !is_decimal(&coverage) {
        return Err("coverage-threshold must be numeric".into());
    }
    if !coverage
        .parse::<f64>()
        .is_ok_and(|value| (90.0..=100.0).contains(&value))
    {
        return Err(
            "coverage-threshold must be between 90, the organization's floor, and 100".into(),
        );
    }
    Ok(coverage)
}

/// `mutation-shards`: one keeps today's serial run, zero selects automatic
/// count-based sharding, and two through thirty-two request a fixed count.
pub(crate) fn mutation_shards() -> Result<usize, String> {
    whole_number("mutation-shards", &input("MUTATION_SHARDS")?, 0, 32)
}

/// An internal-only input for this repository's hosted sharding contract test.
pub(crate) fn internal_shard_selftest() -> Result<bool, String> {
    match optional("INTERNAL_SHARD_SELFTEST")?.as_str() {
        "" | "false" => Ok(false),
        "true" if input("GITHUB_REPOSITORY")? == "Orchestration-Maestro/maestro-rust-workflows" => {
            Ok(true)
        }
        "true" => Err(concat!(
            "internal-shard-selftest is only allowed in ",
            "Orchestration-Maestro/maestro-rust-workflows"
        )
        .into()),
        _ => Err("internal-shard-selftest must be true or false".into()),
    }
}

/// `mutation-mutants-per-shard`: automatic mode's target, checked even when
/// fixed-count or disabled mode means it is not used.
pub(crate) fn mutation_mutants_per_shard() -> Result<usize, String> {
    whole_number(
        "mutation-mutants-per-shard",
        &input("MUTATION_MUTANTS_PER_SHARD")?,
        1,
        1000,
    )
}

/// A finite integral number inside the input's inclusive range.
fn whole_number(name: &str, value: &str, minimum: u32, maximum: u32) -> Result<usize, String> {
    let refusal = || format!("{name} must be a whole number between {minimum} and {maximum}");
    let Ok(number) = value.parse::<f64>() else {
        return Err(refusal());
    };
    if !number.is_finite()
        || number.fract() != 0.0
        || !(f64::from(minimum)..=f64::from(maximum)).contains(&number)
    {
        return Err(refusal());
    }
    format!("{number:.0}")
        .parse::<usize>()
        .map_err(|_| refusal())
}

/// `ARTIFACT_KEY`, `artifact-key` in `ci.yml`: one to forty safe characters,
/// the caller's part of the artifact name.
pub(crate) fn artifact_key() -> Result<String, String> {
    let artifact_key = input("ARTIFACT_KEY")?;
    if artifact_key.len() > 40 || !simple(&artifact_key, "", "_-") {
        return Err("artifact-key must be 1-40 safe characters".into());
    }
    Ok(artifact_key)
}

/// The one value of a closed set spelled like `value`, or the refusal
/// `<name> must be <every spelling>` in the order given, the input named as
/// `ci.yml` names it.
/// Boolean inputs are typed by GitHub itself and rejected before the job
/// starts, so they have no set here.
fn one_of<T: Copy>(
    name: &str,
    value: &str,
    values: &[T],
    spelling: fn(T) -> &'static str,
) -> Result<T, String> {
    values
        .iter()
        .copied()
        .find(|candidate| spelling(*candidate) == value)
        .ok_or_else(|| {
            let spellings: Vec<&str> = values.iter().map(|value| spelling(*value)).collect();
            format!("{name} must be {}", spellings.join(", "))
        })
}

/// Digits with at most one fractional part, the shape `coverage-threshold`
/// takes.
fn is_decimal(value: &str) -> bool {
    let mut parts = value.split('.');
    let whole = parts.next().unwrap_or_default();
    let fraction = parts.next();
    parts.next().is_none()
        && !whole.is_empty()
        && whole.bytes().all(|byte| byte.is_ascii_digit())
        && fraction.is_none_or(|digits| {
            !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
        })
}

#[cfg(test)]
mod tests {
    use super::{ClippyLevel, LicensePolicy, UnsafePolicy, is_decimal, one_of};

    #[test]
    fn coverage_is_a_plain_decimal() {
        assert!(is_decimal("80") && is_decimal("80.5") && is_decimal("0"));
        for bad in ["", ".5", "80.", "8e1", "-1", "1.2.3"] {
            assert!(!is_decimal(bad), "{bad}");
        }
    }

    #[test]
    fn a_closed_set_takes_its_spellings_and_refuses_the_rest_by_name() {
        assert_eq!(
            one_of(
                "license-policy",
                "enforce",
                &[
                    LicensePolicy::Auto,
                    LicensePolicy::Enforce,
                    LicensePolicy::Off
                ],
                LicensePolicy::as_str
            ),
            Ok(LicensePolicy::Enforce)
        );
        assert_eq!(
            one_of(
                "unsafe-policy",
                "deny",
                &[UnsafePolicy::Allow, UnsafePolicy::Deny],
                UnsafePolicy::as_str
            ),
            Ok(UnsafePolicy::Deny)
        );
        assert_eq!(
            one_of(
                "clippy-level",
                "nursery",
                &[
                    ClippyLevel::Default,
                    ClippyLevel::Pedantic,
                    ClippyLevel::Nursery
                ],
                ClippyLevel::as_str
            ),
            Ok(ClippyLevel::Nursery)
        );
        assert_eq!(
            one_of(
                "license-policy",
                "Auto",
                &[
                    LicensePolicy::Auto,
                    LicensePolicy::Enforce,
                    LicensePolicy::Off
                ],
                LicensePolicy::as_str
            ),
            Err("license-policy must be auto, enforce, off".to_owned())
        );
        assert_eq!(
            one_of(
                "unsafe-policy",
                "",
                &[UnsafePolicy::Allow, UnsafePolicy::Deny],
                UnsafePolicy::as_str
            ),
            Err("unsafe-policy must be allow, deny".to_owned())
        );
        assert_eq!(
            one_of(
                "clippy-level",
                "strict",
                &[
                    ClippyLevel::Default,
                    ClippyLevel::Pedantic,
                    ClippyLevel::Nursery
                ],
                ClippyLevel::as_str
            ),
            Err("clippy-level must be default, pedantic, nursery".to_owned())
        );
    }

    #[test]
    fn nursery_implies_pedantic_and_the_default_denies_no_group() {
        let denied = |level: ClippyLevel| level.denied().collect::<Vec<_>>();
        assert!(denied(ClippyLevel::Default).is_empty());
        assert_eq!(denied(ClippyLevel::Pedantic), ["clippy::pedantic"]);
        assert_eq!(
            denied(ClippyLevel::Nursery),
            ["clippy::pedantic", "clippy::nursery"]
        );
    }
}

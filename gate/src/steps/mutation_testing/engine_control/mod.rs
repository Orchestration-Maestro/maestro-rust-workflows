//! Verify featureless compile membership before classifying inactive control mutants.

mod dep_info;
mod membership;
mod outcomes;
mod package_build;
mod run;
mod source;

pub(super) use outcomes::validate;
pub(super) use run::execute;

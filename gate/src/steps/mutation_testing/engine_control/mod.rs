//! Verify featureless compile membership before classifying inactive control mutants.

mod membership;
mod outcomes;
mod run;

pub(super) use outcomes::validate;
pub(super) use run::execute;

//! Native, cross-platform fixture preparation and observed coverage evidence.

mod mutations;
mod runtime;

pub(crate) use mutations::{assert_engine_plan, verify_mutation_fixture};
pub(crate) use runtime::{
    assert_clean_checkout, prepare_hosted_fixture, prepare_hosted_mutation_fixture,
    verify_hosted_fixture,
};

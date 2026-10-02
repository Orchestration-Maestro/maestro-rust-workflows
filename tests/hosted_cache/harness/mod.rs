//! Native, cross-platform fixture preparation and observed coverage evidence.

mod runtime;

pub(crate) use runtime::{assert_clean_checkout, prepare_hosted_fixture, verify_hosted_fixture};

//! Source secret scanning with gate-owned, reviewed exact-content exceptions.

mod archive;
mod policy;
mod reports;
mod step;

pub(super) use step::STEPS;

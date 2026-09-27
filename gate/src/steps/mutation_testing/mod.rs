//! `rust-gate`'s mutation planning, scoped execution and shard aggregation.

mod aggregate;
mod plan;
mod scope;
mod step;

pub(super) use step::STEPS;

//! `rust-gate`'s mutation planning, scoped execution and shard aggregation.

mod aggregate;
mod plan;
mod reports;
mod scope;
mod selftest;
mod step;
mod windows;

pub(super) use step::STEPS;

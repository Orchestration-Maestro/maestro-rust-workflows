//! `rust-gate`'s mutation planning, scoped execution and shard aggregation.

mod aggregate;
mod engine_control;
mod engine_plan;
mod engine_run;
mod host_plan;
mod plan;
mod plan_identity;
mod reports;
mod scope;
mod selftest;
mod step;
mod windows;

pub(super) use step::STEPS;

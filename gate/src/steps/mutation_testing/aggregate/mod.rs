//! Validate every worker receipt and raw result before merging its counters.

mod artifacts;
mod engine_evidence;
pub(super) mod evidence;
mod merge;
mod outcomes;
mod run;
mod viability;
mod windows_evidence;

pub(super) use run::run;

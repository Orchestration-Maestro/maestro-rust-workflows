//! Validate every worker receipt and raw result before merging its counters.

mod artifacts;
mod evidence;
mod merge;
mod outcomes;
mod run;

pub(super) use run::run;

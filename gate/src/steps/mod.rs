//! One module per step, private to this directory: a step reaches the runner
//! and the checks, never another step. Each module declares its steps as
//! data, `STEPS`; the registry lists them and holds `run` and `describe`, the
//! two doors in.

mod api_compatibility;
mod architecture;
mod attest_binaries;
mod binary_hardening;
mod changed_coverage;
mod commit_hooks;
mod configure_cargo_registry;
mod copilot_guide;
mod declared_msrv;
mod dependency_policy;
mod feature_combinations;
mod format_lint_test;
mod fuzz_regression;
mod gate_rules;
mod hygiene;
mod hygiene_workflow;
mod install_toolchain;
mod install_tools;
mod line_coverage;
mod local_ci;
mod local_runs;
mod managed_files;
mod mutation_testing;
mod native_cache_transport;
mod performance;
mod publish_binaries;
mod publish_crate;
mod publish_evidence;
mod pull_request_rules;
mod quality_scorecard;
mod recorded_audits;
mod registry;
mod release_build;
mod report_duplicates;
mod report_sizes;
mod require_every_check;
mod rule_map;
mod secret_scan;
mod stage_payload;
mod toolbelt_setup;
mod unsafe_audit;
mod unused_dependencies;
mod validate_inputs;
mod verify_payload;
mod vulnerability_audit;
mod write_lints;

pub(crate) use registry::{describe, run};

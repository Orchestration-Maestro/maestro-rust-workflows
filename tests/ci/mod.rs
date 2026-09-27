//! `ci.yml`: what each of its steps accepts, refuses, builds and reports,
//! one module per gate.

mod api_compatibility;
mod architecture_rules;
mod central_uploads;
mod commit_hooks;
mod complexity_report;
mod copilot_guide;
mod duplication_report;
mod feature_combinations;
mod input_validation;
mod install_tools;
mod local_ci_run;
mod managed_files;
mod mutation_shards;
mod organization_lints;
mod performance_budget;
mod platform_portability;
mod pull_request_rules;
mod quality_gates;
mod quality_reports;
mod release_payload;
mod release_payload_refusals;
mod repository_hygiene;
mod rule_map;
mod ruleset_settings;
mod scorecard_and_required_status;
mod scorecard_states;
mod source_rules;
mod supply_chain;
mod toolbelt_setup;
mod workspace_boundary;
mod workspace_packaging;

//! What the steps check and share, one module per concern so a call site
//! names the kind of rule it reaches for: paths in the checkout, simple
//! names, Rust versions, the release vocabulary, private directories,
//! Cargo's records, the typed `ci.yml` inputs, Rust source read into module
//! trees, `maestro-quality.toml`, the tools' configuration passed at run time,
//! the findings rules report and the toolbelt every repository installs.
//! Built on the runner, never on a step.

pub(crate) mod cargo_metadata;
pub(crate) mod checkout_paths;
pub(crate) mod coverage_features;
pub(crate) mod digests;
pub(crate) mod findings;
pub(crate) mod gate_rules;
pub(crate) mod inputs;
pub(crate) mod lint_policy;
pub(crate) mod manifests;
pub(crate) mod module_tree;
pub(crate) mod mutation_engine;
pub(crate) mod mutation_host;
pub(crate) mod mutation_windows;
pub(crate) mod native_cache;
pub(crate) mod native_cache_inventory;
pub(crate) mod native_cache_roots;
pub(crate) mod organization_config;
pub(crate) mod private_directories;
pub(crate) mod pull_request;
pub(crate) mod quality_config;
#[cfg(test)]
mod quality_config_cases;
pub(crate) mod release_boundary;
pub(crate) mod rust_code;
#[cfg(test)]
mod rust_code_cases;
mod rust_paths;
#[cfg(test)]
mod rust_test_cases;
pub(crate) mod rust_tests;
pub(crate) mod rust_versions;
pub(crate) mod simple_names;
pub(crate) mod toolbelt;
pub(crate) mod workflow_home;

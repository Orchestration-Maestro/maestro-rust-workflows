//! The one door of the contract tests: the repository and its toolbelt, the
//! readers of workflow YAML, what the gate declares about itself, and the
//! fixture that runs a step against stand-ins. Each part names what it takes
//! from its siblings; nothing here names a test module.

mod engine_mutations;
mod engine_workspace;
mod fixture;
mod gate_declarations;
mod mutation_shards;
mod native_cache;
mod replay_processes;
mod repository;
mod workflow_yaml;

pub(crate) use fixture::{Fixture, SCORECARD_OUTCOMES, checksums, refused, succeeds};
pub(crate) use gate_declarations::{Described, describe_text, described, described_step, gate_bin};
pub(crate) use mutation_shards::{
    aggregation_fixture, copy_tree, incomplete_reason, output, planning_fixture, shard_outcomes,
};
pub(crate) use native_cache::{cache_fixture, coverage_child_fixture};
pub(crate) use repository::{
    capture, command_line, root, rust_files, temp_dir, test_sources, tool, toolbelt_path,
    write_executable,
};
pub(crate) use workflow_yaml::{
    GATE_STEPS, action, query, step, tool_rows, workflow, workflow_steps,
};

pub(crate) use engine_mutations::{
    engine_aggregation_fixture, engine_fixture, engine_inactive_fixture, evidence_hash,
    summarize_engine,
};

pub(crate) use engine_workspace::{engine_workspace, fixture_git};

pub(crate) use replay_processes::{MEMORY_CAP_WRAPPER, verified_memory_cap};

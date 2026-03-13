#![deny(unreachable_pub)]

pub mod agent;
pub(crate) mod agent_budget;
pub(crate) mod agent_events;
#[allow(dead_code)]
pub(crate) mod agent_impl_guard;
pub(crate) mod agent_output_sanitize;
#[allow(dead_code)]
pub(crate) mod agent_runtime;
pub(crate) mod agent_taint_helpers;
pub(crate) mod agent_tool_exec;
pub(crate) mod agent_utils;
pub(crate) mod agent_worker_protocol;
pub mod checks;
#[allow(dead_code)]
pub(crate) mod cli_args;
pub(crate) use cli_args::{AgentMode, Cli, DockerNetwork, RunArgs, RunOutputMode};
pub mod compaction;
pub mod diagnostics;
pub mod eval;
pub mod events;
pub mod gate;
pub mod hooks;
#[allow(dead_code)]
pub(crate) mod instruction_runtime;
pub mod instructions;
pub mod learning;
pub mod lsp_context;
#[allow(dead_code)]
pub(crate) mod lsp_context_provider;
#[doc(hidden)]
pub mod lsp_context_typescript;
pub mod mcp;
pub mod operator_queue;
#[allow(dead_code)]
pub(crate) mod ops_helpers;
pub mod packs;
pub mod planner;
#[allow(dead_code)]
pub(crate) mod planner_runtime;
pub mod project_guidance;
pub mod providers;
#[allow(dead_code)]
pub(crate) mod qualification;
pub mod repo_map;
pub mod repro;
#[allow(dead_code)]
pub(crate) mod run_prep;
#[allow(dead_code)]
pub(crate) mod runtime_events;
#[allow(dead_code)]
pub(crate) mod runtime_flags;
#[allow(dead_code)]
pub(crate) mod runtime_paths;
#[allow(dead_code)]
pub(crate) mod runtime_wiring;
pub mod scaffold;
pub mod session;
pub mod store;
pub mod taint;
pub mod target;
pub mod taskgraph;
pub use agent::AgentExitReason;
pub mod tools;
pub mod trust;
pub mod tui;
pub mod types;

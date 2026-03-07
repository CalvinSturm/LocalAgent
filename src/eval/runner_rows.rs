use std::path::Path;

use uuid::Uuid;

use crate::eval::tasks::EvalTask;
use crate::eval::types::{
    EvalConfig, EvalRunMetrics, EvalRunRow, EvalRunStats, EvalVerifierResult,
};

struct EvalFailureRowInput<'a> {
    config: &'a EvalConfig,
    model: &'a str,
    task: &'a EvalTask,
    run_index: usize,
    run_dir: &'a Path,
    run_id: String,
}

pub(crate) fn build_eval_run_error_row(
    config: &EvalConfig,
    model: &str,
    task: &EvalTask,
    run_index: usize,
    run_dir: &Path,
    run_id: String,
    error: String,
) -> EvalRunRow {
    let input = EvalFailureRowInput {
        config,
        model,
        task,
        run_index,
        run_dir,
        run_id,
    };
    build_failed_eval_row(input, "provider_error", vec![error])
}

pub(crate) fn build_eval_timeout_row(
    config: &EvalConfig,
    model: &str,
    task: &EvalTask,
    run_index: usize,
    run_dir: &Path,
    run_id: String,
) -> EvalRunRow {
    let input = EvalFailureRowInput {
        config,
        model,
        task,
        run_index,
        run_dir,
        run_id,
    };
    build_failed_eval_row(input, "timeout", vec!["timeout".to_string()])
}

fn build_failed_eval_row(
    input: EvalFailureRowInput<'_>,
    exit_reason: &str,
    failures: Vec<String>,
) -> EvalRunRow {
    EvalRunRow {
        model: input.model.to_string(),
        task_id: input.task.id.clone(),
        run_index: input.run_index,
        workdir: if input.config.keep_workdir || input.config.workdir_override.is_some() {
            Some(input.run_dir.display().to_string())
        } else {
            None
        },
        run_id: input.run_id,
        exit_reason: exit_reason.to_string(),
        status: "failed".to_string(),
        skip_reason: None,
        required_flags: input.task.required_flags(),
        passed: false,
        failures,
        stats: EvalRunStats {
            steps: 0,
            tool_calls: 0,
        },
        metrics: None,
        tokens: None,
        estimated_cost_usd: None,
        verifier: None,
    }
}

pub(crate) fn missing_required_tool_reason(
    task: &EvalTask,
    enable_write_tools: bool,
    enabled_mcp: &[String],
) -> Option<String> {
    for req in &task.required_tools {
        if (req == "write_file" || req == "apply_patch" || req == "str_replace") && !enable_write_tools {
            return Some(format!("skipped: required tool '{}' not enabled", req));
        }
        if req.starts_with("mcp.playwright") && !enabled_mcp.iter().any(|m| m == "playwright") {
            return Some("skipped: required MCP server 'playwright' not enabled".to_string());
        }
    }
    None
}

pub(crate) fn missing_capability_reason(
    task: &EvalTask,
    config: &EvalConfig,
    mcp_playwright_enabled: bool,
) -> Option<String> {
    if (task.required_capabilities.needs_write_tools || task.needs_write)
        && !(config.enable_write_tools && (config.allow_write || config.unsafe_bypass_allow_flags))
    {
        return Some(
            "requires --enable-write-tools and --allow-write (or --unsafe-bypass-allow-flags)"
                .to_string(),
        );
    }
    if task.required_capabilities.needs_shell
        && !(config.allow_shell || config.unsafe_bypass_allow_flags)
    {
        return Some("requires --allow-shell (or --unsafe-bypass-allow-flags)".to_string());
    }
    if task.required_capabilities.needs_mcp && !mcp_playwright_enabled {
        return Some("requires --mcp playwright".to_string());
    }
    None
}

pub(crate) fn skipped_row(
    model: &str,
    task: &EvalTask,
    run_index: usize,
    reason: &str,
) -> EvalRunRow {
    EvalRunRow {
        model: model.to_string(),
        task_id: task.id.clone(),
        run_index,
        workdir: None,
        run_id: format!("skipped-{}", Uuid::new_v4()),
        exit_reason: "skipped".to_string(),
        status: "skipped".to_string(),
        skip_reason: Some(reason.to_string()),
        required_flags: task.required_flags(),
        passed: false,
        failures: vec![reason.to_string()],
        stats: EvalRunStats {
            steps: 0,
            tool_calls: 0,
        },
        metrics: Some(EvalRunMetrics::default()),
        tokens: None,
        estimated_cost_usd: None,
        verifier: Some(EvalVerifierResult {
            ran: false,
            ok: false,
            summary: "not run".to_string(),
            stdout_truncated: false,
            stderr_truncated: false,
        }),
    }
}

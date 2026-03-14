use anyhow::anyhow;

use crate::agent;
use crate::planner;
use crate::types::{Message, Role};
use crate::RunArgs;

fn write_capability_available(args: &RunArgs) -> bool {
    (args.enable_write_tools && args.allow_write) || args.unsafe_bypass_allow_flags
}

pub(super) fn task_kind_enforces_implementation_guard(
    task_kind: Option<&str>,
    selected_task_kind: Option<&str>,
) -> bool {
    task_kind.is_some_and(crate::agent::task_contract::task_kind_enables_implementation_guard)
        || selected_task_kind
            .is_some_and(crate::agent::task_contract::task_kind_enables_implementation_guard)
}

pub(super) fn should_enable_implementation_guard(
    args: &RunArgs,
    selected_task_kind: Option<&str>,
) -> bool {
    if args.disable_implementation_guard {
        return false;
    }
    if !write_capability_available(args) {
        return false;
    }
    if args.task_kind.is_some() || selected_task_kind.is_some() {
        return task_kind_enforces_implementation_guard(
            args.task_kind.as_deref(),
            selected_task_kind,
        );
    }
    task_kind_enforces_implementation_guard(args.task_kind.as_deref(), selected_task_kind)
}

pub(super) fn maybe_append_implementation_guard_message(
    base_instruction_messages: &mut Vec<Message>,
    args: &RunArgs,
    selected_task_kind: Option<&str>,
) {
    if should_enable_implementation_guard(args, selected_task_kind) {
        base_instruction_messages.push(Message {
            role: Role::System,
            content: Some(agent::INTERNAL_ENFORCE_IMPLEMENTATION_GUARD_FLAG.to_string()),
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
        });
    }
}

pub(super) fn validate_runtime_owned_http_timeouts(
    args: &RunArgs,
    planner_strict_effective: bool,
    selected_task_kind: Option<&str>,
) -> anyhow::Result<()> {
    let planner_strict_runtime_owned =
        planner_strict_effective && matches!(args.mode, planner::RunMode::PlannerWorker);
    let runtime_owned_mode = planner_strict_runtime_owned
        || should_enable_implementation_guard(args, selected_task_kind);
    if !runtime_owned_mode {
        return Ok(());
    }
    if args.http_timeout_ms == 0 {
        return Err(anyhow!(
            "invalid timeout config for strict/runtime-owned mode: --http-timeout-ms must be > 0"
        ));
    }
    if args.http_stream_idle_timeout_ms == 0 {
        return Err(anyhow!(
            "invalid timeout config for strict/runtime-owned mode: --http-stream-idle-timeout-ms must be > 0"
        ));
    }
    Ok(())
}

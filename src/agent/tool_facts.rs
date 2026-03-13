use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::agent_impl_guard::ToolExecutionRecord;
use crate::types::{Message, Role, ToolCall};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolFactV1 {
    Read {
        sequence: u32,
        tool_call_id: String,
        tool: String,
        path: String,
        ok: bool,
    },
    Write {
        sequence: u32,
        tool_call_id: String,
        tool: String,
        path: String,
        ok: bool,
        changed: Option<bool>,
    },
    Shell {
        sequence: u32,
        tool_call_id: String,
        command: String,
        ok: bool,
    },
    Validation {
        sequence: u32,
        tool_call_id: String,
        command: String,
        ok: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolFactSourceV1 {
    ExecutionRecords,
    Transcript,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolFactProvenanceV1 {
    pub source: ToolFactSourceV1,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint_phase: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolFactEnvelopeV1 {
    pub fact: ToolFactV1,
    pub provenance: ToolFactProvenanceV1,
}

pub(crate) fn tool_facts_from_calls_and_executions(
    user_prompt: &str,
    observed_tool_calls: &[ToolCall],
    tool_executions: &[ToolExecutionRecord],
) -> Vec<ToolFactV1> {
    let required_validation_command =
        crate::agent_impl_guard::prompt_required_validation_command(user_prompt);
    let mut sequence = 0u32;
    let mut facts = Vec::new();
    let mut execution_idx = 0usize;
    for call in observed_tool_calls {
        while let Some(execution) = tool_executions.get(execution_idx) {
            if tool_call_alignment_is_consistent(call, execution.name.as_str()) {
                facts.extend(facts_for_observed_tool(
                    &mut sequence,
                    call,
                    execution.name.as_str(),
                    execution.ok,
                    normalized_path_from_execution_or_call(execution.path.as_deref(), call),
                    execution.changed,
                    required_validation_command,
                ));
                execution_idx += 1;
                break;
            }
            facts.extend(facts_for_runtime_execution(
                &mut sequence,
                execution,
                required_validation_command,
            ));
            execution_idx += 1;
        }
    }
    for execution in tool_executions.iter().skip(execution_idx) {
        facts.extend(facts_for_runtime_execution(
            &mut sequence,
            execution,
            required_validation_command,
        ));
    }
    facts
}

pub(crate) fn tool_facts_from_transcript(
    user_prompt: &str,
    observed_tool_calls: &[ToolCall],
    messages: &[Message],
) -> Vec<ToolFactV1> {
    let required_validation_command =
        crate::agent_impl_guard::prompt_required_validation_command(user_prompt);
    let mut sequence = 0u32;
    let mut facts = Vec::new();
    for (call, message) in observed_tool_calls
        .iter()
        .zip(messages.iter().filter(|message| matches!(message.role, Role::Tool)))
    {
        if !tool_call_alignment_is_consistent(call, call.name.as_str()) {
            continue;
        }
        let ok = message
            .content
            .as_deref()
            .map(|content| !crate::agent_tool_exec::tool_result_has_error(content))
            .unwrap_or(false);
        let changed = message
            .content
            .as_deref()
            .and_then(crate::agent_tool_exec::tool_result_changed_flag);
        facts.extend(facts_for_observed_tool(
            &mut sequence,
            call,
            call.name.as_str(),
            ok,
            normalized_path_from_call(call),
            changed,
            required_validation_command,
        ));
    }
    facts
}

pub(crate) fn tool_fact_envelopes_from_facts(
    facts: &[ToolFactV1],
    source: ToolFactSourceV1,
    phase: Option<&str>,
    checkpoint_phase: Option<&str>,
) -> Vec<ToolFactEnvelopeV1> {
    let provenance = ToolFactProvenanceV1 {
        source,
        phase: phase.map(ToOwned::to_owned),
        checkpoint_phase: checkpoint_phase.map(ToOwned::to_owned),
    };
    facts.iter()
        .cloned()
        .map(|fact| ToolFactEnvelopeV1 {
            fact,
            provenance: provenance.clone(),
        })
        .collect()
}

fn facts_for_observed_tool(
    sequence: &mut u32,
    call: &ToolCall,
    tool_name: &str,
    ok: bool,
    normalized_path: Option<String>,
    changed: Option<bool>,
    required_validation_command: Option<&'static str>,
) -> Vec<ToolFactV1> {
    let mut facts = Vec::new();
    match tool_name {
        "read_file" => {
            if let Some(path) = normalized_path {
                facts.push(ToolFactV1::Read {
                    sequence: next_sequence(sequence),
                    tool_call_id: call.id.clone(),
                    tool: tool_name.to_string(),
                    path,
                    ok,
                });
            }
        }
        "apply_patch" | "edit" | "write_file" | "str_replace" => {
            if let Some(path) = normalized_path {
                facts.push(ToolFactV1::Write {
                    sequence: next_sequence(sequence),
                    tool_call_id: call.id.clone(),
                    tool: tool_name.to_string(),
                    path,
                    ok,
                    changed,
                });
            }
        }
        "shell" => {
            if let Some(command) = shell_command_text(call) {
                facts.push(ToolFactV1::Shell {
                    sequence: next_sequence(sequence),
                    tool_call_id: call.id.clone(),
                    command: command.clone(),
                    ok,
                });
                if required_validation_command
                    .is_some_and(|required| command.to_ascii_lowercase().contains(required))
                {
                    facts.push(ToolFactV1::Validation {
                        sequence: next_sequence(sequence),
                        tool_call_id: call.id.clone(),
                        command,
                        ok,
                    });
                }
            }
        }
        _ => {}
    }
    facts
}

fn next_sequence(sequence: &mut u32) -> u32 {
    let current = *sequence;
    *sequence = sequence.saturating_add(1);
    current
}

fn tool_call_alignment_is_consistent(call: &ToolCall, observed_tool_name: &str) -> bool {
    !call.id.trim().is_empty() && call.name == observed_tool_name
}

fn normalized_path_from_execution_or_call(
    execution_path: Option<&str>,
    call: &ToolCall,
) -> Option<String> {
    execution_path
        .map(crate::agent_impl_guard::normalize_tool_path)
        .or_else(|| normalized_path_from_call(call))
}

fn facts_for_runtime_execution(
    sequence: &mut u32,
    execution: &ToolExecutionRecord,
    required_validation_command: Option<&'static str>,
) -> Vec<ToolFactV1> {
    let synthetic_tool_call_id = format!("runtime_execution_{}", *sequence);
    let normalized_path = execution
        .path
        .as_deref()
        .map(crate::agent_impl_guard::normalize_tool_path);
    match execution.name.as_str() {
        "read_file" | "apply_patch" | "edit" | "write_file" | "str_replace" => {
            let Some(path) = normalized_path else {
                return Vec::new();
            };
            let tool_name = execution.name.as_str();
            facts_for_observed_tool(
                sequence,
                &ToolCall {
                    id: synthetic_tool_call_id,
                    name: tool_name.to_string(),
                    arguments: serde_json::json!({ "path": path }),
                },
                tool_name,
                execution.ok,
                Some(path),
                execution.changed,
                required_validation_command,
            )
        }
        _ => Vec::new(),
    }
}

fn normalized_path_from_call(call: &ToolCall) -> Option<String> {
    call.arguments
        .get("path")
        .and_then(|value| value.as_str())
        .map(crate::agent_impl_guard::normalize_tool_path)
}

fn shell_command_text(call: &ToolCall) -> Option<String> {
    if call.name != "shell" {
        return None;
    }
    let obj = call.arguments.as_object()?;
    if let Some(command) = obj.get("command").and_then(|v| v.as_str()) {
        return Some(command.to_string());
    }
    let cmd = obj.get("cmd").and_then(|v| v.as_str())?;
    let mut parts = vec![cmd.to_string()];
    if let Some(args) = obj.get("args").and_then(|v| v.as_array()) {
        for arg in args {
            if let Some(arg) = arg.as_str() {
                parts.push(arg.to_string());
            }
        }
    }
    Some(parts.join(" "))
}

pub(crate) fn pending_post_write_verification_paths_from_facts(
    facts: &[ToolFactV1],
) -> BTreeSet<String> {
    let mut pending = BTreeSet::<String>::new();
    for fact in facts {
        match fact {
            ToolFactV1::Read { path, ok, .. } if *ok => {
                pending.remove(path);
            }
            ToolFactV1::Write { path, ok, .. } if *ok => {
                pending.insert(path.clone());
            }
            _ => {}
        }
    }
    pending
}

pub(crate) fn required_validation_command_satisfied_from_facts(
    user_prompt: &str,
    tool_facts: &[ToolFactV1],
) -> bool {
    let Some(required) = crate::agent_impl_guard::prompt_required_validation_command(user_prompt) else {
        return true;
    };
    tool_facts.iter().any(|fact| {
        matches!(
            fact,
            ToolFactV1::Validation {
                command,
                ok: true,
                ..
            } if command.to_ascii_lowercase().contains(required)
        )
    })
}

pub(crate) fn required_validation_failure_needs_repair_from_facts(
    user_prompt: &str,
    tool_facts: &[ToolFactV1],
) -> bool {
    let Some(required) = crate::agent_impl_guard::prompt_required_validation_command(user_prompt) else {
        return false;
    };
    let mut ordered = tool_facts.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|fact| fact.sequence());
    let mut needs_repair = false;
    for fact in ordered {
        match fact {
            ToolFactV1::Write {
                ok,
                changed,
                ..
            } if *ok && changed.unwrap_or(true) => {
                needs_repair = false;
            }
            ToolFactV1::Validation { command, ok, .. }
                if command.to_ascii_lowercase().contains(required) =>
            {
                needs_repair = !ok;
            }
            _ => {}
        }
    }
    needs_repair
}

pub(crate) fn implementation_integrity_violation_from_facts(
    user_prompt: &str,
    final_output: &str,
    tool_facts: &[ToolFactV1],
    enforce_implementation_integrity_guard: bool,
) -> Option<String> {
    if !enforce_implementation_integrity_guard {
        return None;
    }
    if tool_facts.is_empty() {
        return Some(
            "implementation guard: file-edit task finalized without any tool calls".to_string(),
        );
    }
    if output_has_placeholder_artifacts(final_output) {
        return Some(
            "implementation guard: final answer contains placeholder artifacts instead of concrete implementation".to_string(),
        );
    }
    if let Some(reason) = read_before_edit_violation_from_facts(user_prompt, tool_facts) {
        return Some(reason);
    }
    if let Some(path) = pending_post_write_verification_paths_from_facts(tool_facts)
        .iter()
        .next()
    {
        return Some(format!(
            "implementation guard: post-write verification missing read_file on '{path}'"
        ));
    }
    if prompt_requires_effective_write(user_prompt)
        && !tool_facts.iter().any(ToolFactV1::is_effective_write)
    {
        return Some(
            "implementation guard: file-edit task finalized without an effective write (writes failed or write tool changed:false)".to_string(),
        );
    }
    None
}

pub(crate) fn read_before_edit_violation_from_facts(
    user_prompt: &str,
    tool_facts: &[ToolFactV1],
) -> Option<String> {
    let mut successful_read_paths = BTreeSet::<String>::new();
    let allow_new_file_without_read = prompt_allows_new_file_without_read(user_prompt);
    let mut ordered = tool_facts.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|fact| fact.sequence());
    for fact in ordered {
        match fact {
            ToolFactV1::Read { path, ok, .. } => {
                if *ok {
                    successful_read_paths.insert(path.clone());
                }
            }
            ToolFactV1::Write {
                tool,
                path,
                ok,
                ..
            } => {
                if !ok {
                    continue;
                }
                if (tool != "write_file" || !allow_new_file_without_read)
                    && !successful_read_paths.contains(path)
                {
                    return Some(format!(
                        "implementation guard: {} on '{path}' requires prior read_file on the same path",
                        tool
                    ));
                }
            }
            _ => {}
        }
    }
    None
}

impl ToolFactV1 {
    pub(crate) fn sequence(&self) -> u32 {
        match self {
            ToolFactV1::Read { sequence, .. }
            | ToolFactV1::Write { sequence, .. }
            | ToolFactV1::Shell { sequence, .. }
            | ToolFactV1::Validation { sequence, .. } => *sequence,
        }
    }

    pub(crate) fn is_effective_write(&self) -> bool {
        matches!(
            self,
            ToolFactV1::Write {
                ok: true,
                changed,
                ..
            } if changed.unwrap_or(true)
        )
    }
}

fn prompt_allows_new_file_without_read(prompt: &str) -> bool {
    let p = prompt.to_ascii_lowercase();
    p.contains("create a new file")
        || p.contains("create new file")
        || p.contains("new file at")
        || p.contains("add new file")
        || p.contains("create `")
        || p.contains("create the file")
}

fn prompt_requires_effective_write(prompt: &str) -> bool {
    let p = prompt.to_ascii_lowercase();
    p.contains("apply_patch")
        || p.contains("write_file")
        || p.contains("edit ")
        || p.contains("fix ")
        || p.contains("modify ")
        || p.contains("update ")
        || p.contains("change ")
}

fn output_has_placeholder_artifacts(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    let patterns = [
        "... (full implementation) ...",
        "...full implementation...",
        "same css as before",
        "same html structure",
        "additional improvements coming",
        "todo:",
        "coming soon",
    ];
    patterns.iter().any(|p| t.contains(p))
}

#[cfg(test)]
mod tests {
    use super::{
        implementation_integrity_violation_from_facts, pending_post_write_verification_paths_from_facts,
        read_before_edit_violation_from_facts, required_validation_command_satisfied_from_facts,
        required_validation_failure_needs_repair_from_facts, tool_fact_envelopes_from_facts,
        tool_facts_from_calls_and_executions, ToolFactEnvelopeV1, ToolFactSourceV1, ToolFactV1,
    };
    use crate::agent_impl_guard::ToolExecutionRecord;
    use crate::types::ToolCall;
    use serde_json::json;

    #[test]
    fn facts_include_read_write_shell_and_validation() {
        let calls = vec![
            ToolCall {
                id: "tc_read".to_string(),
                name: "read_file".to_string(),
                arguments: json!({"path":"src/main.rs"}),
            },
            ToolCall {
                id: "tc_write".to_string(),
                name: "apply_patch".to_string(),
                arguments: json!({"path":"src/main.rs"}),
            },
            ToolCall {
                id: "tc_shell".to_string(),
                name: "shell".to_string(),
                arguments: json!({"command":"cargo test"}),
            },
        ];
        let executions = vec![
            ToolExecutionRecord {
                name: "read_file".to_string(),
                path: Some("src/main.rs".to_string()),
                ok: true,
                changed: None,
            },
            ToolExecutionRecord {
                name: "apply_patch".to_string(),
                path: Some("src/main.rs".to_string()),
                ok: true,
                changed: Some(true),
            },
            ToolExecutionRecord {
                name: "shell".to_string(),
                path: None,
                ok: true,
                changed: None,
            },
        ];
        let facts = tool_facts_from_calls_and_executions(
            "Before finishing, run cargo test successfully.",
            &calls,
            &executions,
        );
        assert!(facts.iter().any(|fact| matches!(fact, ToolFactV1::Read { .. })));
        assert!(facts.iter().any(|fact| matches!(fact, ToolFactV1::Write { .. })));
        assert!(facts.iter().any(|fact| matches!(fact, ToolFactV1::Shell { .. })));
        assert!(facts.iter().any(|fact| matches!(fact, ToolFactV1::Validation { .. })));
    }

    #[test]
    fn facts_drive_pending_post_write_readback() {
        let facts = vec![
            ToolFactV1::Read {
                sequence: 0,
                tool_call_id: "tc1".to_string(),
                tool: "read_file".to_string(),
                path: "src/main.rs".to_string(),
                ok: true,
            },
            ToolFactV1::Write {
                sequence: 1,
                tool_call_id: "tc2".to_string(),
                tool: "apply_patch".to_string(),
                path: "src/main.rs".to_string(),
                ok: true,
                changed: Some(true),
            },
        ];
        let pending = pending_post_write_verification_paths_from_facts(&facts);
        assert!(pending.contains("src/main.rs"));
    }

    #[test]
    fn facts_drive_effective_write_and_readback_guard() {
        let facts = vec![
            ToolFactV1::Read {
                sequence: 0,
                tool_call_id: "tc1".to_string(),
                tool: "read_file".to_string(),
                path: "src/main.rs".to_string(),
                ok: true,
            },
            ToolFactV1::Write {
                sequence: 1,
                tool_call_id: "tc2".to_string(),
                tool: "apply_patch".to_string(),
                path: "src/main.rs".to_string(),
                ok: true,
                changed: Some(true),
            },
            ToolFactV1::Read {
                sequence: 2,
                tool_call_id: "tc3".to_string(),
                tool: "read_file".to_string(),
                path: "src/main.rs".to_string(),
                ok: true,
            },
        ];
        let err = implementation_integrity_violation_from_facts(
            "fix src/main.rs",
            "done",
            &facts,
            true,
        );
        assert!(err.is_none());
    }

    #[test]
    fn validation_facts_are_derived_from_validation_tool_facts() {
        let facts = vec![ToolFactV1::Validation {
            sequence: 0,
            tool_call_id: "tc1".to_string(),
            command: "cargo test".to_string(),
            ok: true,
        }];
        assert!(required_validation_command_satisfied_from_facts(
            "Before finishing, run cargo test successfully.",
            &facts,
        ));
    }

    #[test]
    fn failed_validation_is_cleared_by_later_effective_write() {
        let facts = vec![
            ToolFactV1::Validation {
                sequence: 0,
                tool_call_id: "tc1".to_string(),
                command: "cargo test".to_string(),
                ok: false,
            },
            ToolFactV1::Write {
                sequence: 1,
                tool_call_id: "tc2".to_string(),
                tool: "edit".to_string(),
                path: "src/main.rs".to_string(),
                ok: true,
                changed: Some(true),
            },
        ];
        assert!(!required_validation_failure_needs_repair_from_facts(
            "Before finishing, run cargo test successfully.",
            &facts,
        ));
    }

    #[test]
    fn read_before_edit_rule_is_fact_backed() {
        let facts = vec![ToolFactV1::Write {
            sequence: 0,
            tool_call_id: "tc1".to_string(),
            tool: "apply_patch".to_string(),
            path: "src/main.rs".to_string(),
            ok: true,
            changed: Some(true),
        }];
        let err = read_before_edit_violation_from_facts("fix src/main.rs", &facts)
            .expect("expected violation");
        assert!(err.contains("requires prior read_file"));
    }

    #[test]
    fn envelopes_attach_provenance_without_changing_fact_payload() {
        let envelopes = tool_fact_envelopes_from_facts(
            &[ToolFactV1::Shell {
                sequence: 0,
                tool_call_id: "tc1".to_string(),
                command: "cargo test".to_string(),
                ok: true,
            }],
            ToolFactSourceV1::Transcript,
            Some("finalize"),
            Some("waiting_for_approval"),
        );
        assert_eq!(envelopes.len(), 1);
        assert!(matches!(
            envelopes.first(),
            Some(ToolFactEnvelopeV1 {
                provenance,
                ..
            }) if provenance.phase.as_deref() == Some("finalize")
                && provenance.checkpoint_phase.as_deref() == Some("waiting_for_approval")
        ));
    }
}

use std::time::Duration;

use crate::chat_commands;
use crate::chat_runtime;
use crate::chat_tui::overlay::LearnOverlayState;
use crate::mcp::registry::McpRegistry;
use crate::project_guidance;
use crate::runtime_config;
use crate::runtime_paths;
use crate::session::SessionStore;
use crate::store;
use crate::tui::state::UiState;
use crate::RunArgs;

pub(crate) enum SlashCommandDispatchOutcome {
    Handled,
    ExitRequested,
}

pub(crate) struct TuiSlashCommandDispatchInput<'a> {
    pub(crate) line: &'a str,
    pub(crate) slash_menu_index: usize,
    pub(crate) run_busy: bool,
    pub(crate) active_run: &'a mut RunArgs,
    pub(crate) paths: &'a store::StatePaths,
    pub(crate) logs: &'a mut Vec<String>,
    pub(crate) show_logs: &'a mut bool,
    pub(crate) show_tools: &'a mut bool,
    pub(crate) show_approvals: &'a mut bool,
    pub(crate) timeout_notice_active: &'a mut bool,
    pub(crate) pending_timeout_input: &'a mut bool,
    pub(crate) pending_params_input: &'a mut bool,
    pub(crate) transcript: &'a mut Vec<(String, String)>,
    pub(crate) transcript_thinking: &'a mut std::collections::BTreeMap<usize, String>,
    pub(crate) ui_state: &'a mut UiState,
    pub(crate) streaming_assistant: &'a mut String,
    pub(crate) transcript_scroll: &'a mut usize,
    pub(crate) follow_output: &'a mut bool,
    pub(crate) shared_chat_mcp_registry: &'a mut Option<std::sync::Arc<McpRegistry>>,
    pub(crate) learn_overlay: &'a mut Option<LearnOverlayState>,
    pub(crate) learn_overlay_cursor: &'a mut usize,
}

pub(crate) async fn handle_tui_slash_command(
    input: TuiSlashCommandDispatchInput<'_>,
) -> anyhow::Result<SlashCommandDispatchOutcome> {
    let line = input.line;
    if line.trim() == "/learn" {
        *input.learn_overlay = Some(LearnOverlayState::default());
        *input.learn_overlay_cursor = 0;
        *input.show_logs = false;
        return Ok(SlashCommandDispatchOutcome::Handled);
    }
    if line.starts_with("/learn") {
        if input.run_busy {
            input.logs.push("ERR_TUI_BUSY_TRY_AGAIN".to_string());
            *input.show_logs = true;
            return Ok(SlashCommandDispatchOutcome::Handled);
        }
        match crate::chat_tui_learn_adapter::parse_and_dispatch_learn_slash(
            line,
            input.active_run,
            input.paths,
        )
        .await
        {
            Ok(output) => {
                if !output.is_empty() {
                    input.logs.push(output);
                }
            }
            Err(e) => input.logs.push(format!("learn command failed: {e}")),
        }
        *input.show_logs = true;
        return Ok(SlashCommandDispatchOutcome::Handled);
    }
    let resolved = chat_commands::selected_slash_command(line, input.slash_menu_index)
        .or_else(|| chat_commands::resolve_slash_command(line))
        .unwrap_or(line);
    match resolved {
        "/exit" => return Ok(SlashCommandDispatchOutcome::ExitRequested),
        "/help" => {
            input.logs.push(
                "commands: /help /mode <safe|coding|web|custom> /timeout [seconds|+N|-N|off] /params [key value] /project guidance /tool docs <name> /learn help|list|show|archive|capture|promote /dismiss /clear /exit /hide tools|approvals|logs /show tools|approvals|logs|all ; slash dropdown: type / then Up/Down + Enter ; panes: Ctrl+T/Ctrl+Y/Ctrl+G (Ctrl+1/2/3 aliases, terminal-dependent) ; scroll: PgUp/PgDn, Ctrl+U/Ctrl+D, mouse wheel ; approvals: Ctrl+J/K select, Ctrl+A approve, Ctrl+X deny, Ctrl+R refresh ; history: Up/Down ; Esc quits"
                    .to_string(),
            );
            *input.show_logs = true;
        }
        "/mode" => {
            input.logs.push(format!(
                "current mode: {} (use /mode <safe|coding|web|custom>)",
                chat_runtime::chat_mode_display_label(input.active_run)
            ));
            *input.show_logs = true;
        }
        "/timeout" => {
            *input.pending_timeout_input = true;
            input
                .logs
                .push(runtime_config::timeout_settings_summary(input.active_run));
            input
                .logs
                .push("enter seconds, +N, -N, or 'cancel' on the next line".to_string());
            *input.show_logs = true;
        }
        "/params" => {
            *input.pending_params_input = true;
            input
                .logs
                .push(runtime_config::params_settings_summary(input.active_run));
            input.logs.push(
                "editable keys: max_steps, max_context_chars, compaction_mode(off|summary), compaction_keep_last, tool_result_persist(all|digest|none), max_tool_output_bytes, max_read_bytes, stream(on|off), allow_shell(on|off), allow_write(on|off), enable_write_tools(on|off), allow_shell_in_workdir(on|off)"
                    .to_string(),
            );
            input
                .logs
                .push("enter '<key> <value>' or 'cancel' on the next line".to_string());
            *input.show_logs = true;
        }
        "/dismiss" => {
            if *input.timeout_notice_active {
                *input.timeout_notice_active = false;
                input.logs.retain(|l| !l.starts_with("[timeout-notice]"));
                input
                    .logs
                    .push("timeout notification dismissed".to_string());
            } else {
                input
                    .logs
                    .push("no active timeout notification".to_string());
            }
            *input.show_logs = true;
        }
        "/tool docs" => {
            input
                .logs
                .push("usage: /tool docs <name> (example: /tool docs mcp.stub.echo)".to_string());
            *input.show_logs = true;
        }
        "/project guidance" => {
            match project_guidance::resolve_project_guidance(
                &input.active_run.workdir,
                project_guidance::ProjectGuidanceLimits::default(),
            ) {
                Ok(g) => input
                    .logs
                    .push(project_guidance::render_project_guidance_text(&g)),
                Err(e) => input
                    .logs
                    .push(format!("project guidance unavailable: {e}")),
            }
            *input.show_logs = true;
        }
        "/clear" => {
            if input.active_run.no_session {
                input.transcript.clear();
                input.transcript_thinking.clear();
                input.ui_state.tool_calls.clear();
                input.streaming_assistant.clear();
                *input.transcript_scroll = 0;
                *input.follow_output = true;
                input.logs.push("cleared chat transcript".to_string());
            } else {
                let session_path = input
                    .paths
                    .sessions_dir
                    .join(format!("{}.json", input.active_run.session));
                let store = SessionStore::new(session_path, input.active_run.session.clone());
                store.reset()?;
                input.transcript.clear();
                input.transcript_thinking.clear();
                input.ui_state.tool_calls.clear();
                input.streaming_assistant.clear();
                *input.transcript_scroll = 0;
                *input.follow_output = true;
                input.logs.push(format!(
                    "session '{}' and transcript cleared",
                    input.active_run.session
                ));
            }
        }
        "/hide tools" => *input.show_tools = false,
        "/hide approvals" => *input.show_approvals = false,
        "/hide logs" => *input.show_logs = false,
        "/show tools" => *input.show_tools = true,
        "/show approvals" => *input.show_approvals = true,
        "/show logs" => *input.show_logs = true,
        "/show all" => {
            *input.show_tools = true;
            *input.show_approvals = true;
            *input.show_logs = true;
        }
        _ if resolved.starts_with("/mode ") => {
            let mode = resolved["/mode ".len()..].trim();
            if runtime_config::apply_chat_mode(input.active_run, mode).is_some() {
                input.logs.push(format!(
                    "mode switched to {}",
                    chat_runtime::chat_mode_display_label(input.active_run)
                ));
            } else {
                input.logs.push(format!(
                    "unknown mode: {mode}. expected safe|coding|web|custom"
                ));
            }
            *input.show_logs = true;
        }
        _ if resolved.starts_with("/timeout ") => {
            let value = resolved["/timeout ".len()..].trim();
            match runtime_config::apply_timeout_input(input.active_run, value) {
                Ok(msg) => {
                    input.logs.push(msg);
                    *input.show_logs = false;
                }
                Err(msg) => {
                    input.logs.push(msg);
                    *input.show_logs = true;
                }
            }
        }
        _ if resolved.starts_with("/params ") => {
            let value = resolved["/params ".len()..].trim();
            match runtime_config::apply_params_input(input.active_run, value) {
                Ok(msg) => input.logs.push(msg),
                Err(msg) => input.logs.push(msg),
            }
            *input.show_logs = true;
        }
        _ if line.starts_with("/tool docs ") => {
            let tool_name = line["/tool docs ".len()..].trim();
            if tool_name.is_empty() {
                input.logs.push(
                    "usage: /tool docs <name> (example: /tool docs mcp.stub.echo)".to_string(),
                );
                *input.show_logs = true;
                return Ok(SlashCommandDispatchOutcome::Handled);
            }
            if input.active_run.mcp.is_empty() {
                input.logs.push(
                    "MCP registry unavailable: no MCP servers enabled for this chat session"
                        .to_string(),
                );
                *input.show_logs = true;
                return Ok(SlashCommandDispatchOutcome::Handled);
            }
            if input.shared_chat_mcp_registry.is_none() {
                let mcp_config_path = runtime_paths::resolved_mcp_config_path(
                    input.active_run,
                    &input.paths.state_dir,
                );
                match McpRegistry::from_config_path(
                    &mcp_config_path,
                    &input.active_run.mcp,
                    Duration::from_secs(30),
                )
                .await
                {
                    Ok(reg) => {
                        *input.shared_chat_mcp_registry = Some(std::sync::Arc::new(reg));
                    }
                    Err(e) => {
                        input
                            .logs
                            .push(format!("failed to initialize MCP session: {e}"));
                        *input.show_logs = true;
                        return Ok(SlashCommandDispatchOutcome::Handled);
                    }
                }
            }
            if let Some(reg) = input.shared_chat_mcp_registry.as_ref() {
                input.logs.push(reg.render_tool_docs_text(tool_name));
            } else {
                input
                    .logs
                    .push("MCP registry unavailable: failed to initialize".to_string());
            }
            *input.show_logs = true;
        }
        _ => input.logs.push(format!("unknown command: {}", line)),
    }
    Ok(SlashCommandDispatchOutcome::Handled)
}

use std::time::Duration;

use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::chat_commands;
use crate::chat_runtime;
use crate::chat_tui::overlay::LearnOverlayState;
use crate::chat_tui::slash_commands::{
    handle_tui_slash_command, SlashCommandDispatchOutcome, TuiSlashCommandDispatchInput,
};
use crate::chat_tui::text::render_with_optional_caret;
use crate::chat_ui;
use crate::events::Event;
use crate::gate::ProviderKind;
use crate::mcp::registry::McpRegistry;
use crate::provider_runtime;
use crate::providers::mock::MockProvider;
use crate::providers::ollama::OllamaProvider;
use crate::providers::openai_compat::OpenAiCompatProvider;
use crate::run_agent_with_ui;
use crate::runtime_paths;
use crate::store;
use crate::tui::state::UiState;
use crate::RunArgs;

pub(crate) enum TuiNormalSubmitPrepOutcome {
    ContinueToRun,
    HandledNoRun,
}

pub(crate) enum TuiEnterSubmitOutcome {
    Launched(TuiSubmitLaunch),
    ContinueLoop,
    ExitRequested,
}

pub(crate) type TuiRunFuture = std::pin::Pin<
    Box<dyn std::future::Future<Output = anyhow::Result<crate::RunExecutionResult>> + Send>,
>;

pub(crate) struct TuiSubmitLaunch {
    pub(crate) rx: std::sync::mpsc::Receiver<Event>,
    pub(crate) queue_tx: std::sync::mpsc::Sender<crate::operator_queue::QueueSubmitRequest>,
    pub(crate) fut: TuiRunFuture,
}

pub(crate) struct TuiNormalSubmitPrepInput<'a> {
    pub(crate) line: &'a str,
    pub(crate) prompt_history: &'a mut Vec<String>,
    pub(crate) transcript: &'a mut Vec<(String, String)>,
    pub(crate) show_thinking_panel: &'a mut bool,
    pub(crate) show_logs: &'a mut bool,
    pub(crate) follow_output: &'a mut bool,
    pub(crate) transcript_scroll: &'a mut usize,
    pub(crate) status: &'a mut String,
    pub(crate) status_detail: &'a mut String,
    pub(crate) streaming_assistant: &'a mut String,
    pub(crate) think_tick: &'a mut u64,
}

pub(crate) fn prepare_tui_normal_submit_state(
    input: TuiNormalSubmitPrepInput<'_>,
) -> TuiNormalSubmitPrepOutcome {
    let first_prompt = input.transcript.is_empty();
    input.prompt_history.push(input.line.to_string());
    *input.follow_output = true;
    *input.transcript_scroll = usize::MAX;
    input
        .transcript
        .push(("user".to_string(), input.line.to_string()));
    if input.line.starts_with('?') {
        *input.show_logs = true;
        return TuiNormalSubmitPrepOutcome::HandledNoRun;
    }
    *input.status = "running".to_string();
    input.status_detail.clear();
    input.streaming_assistant.clear();
    *input.think_tick = 0;
    if first_prompt {
        *input.show_thinking_panel = true;
    }
    TuiNormalSubmitPrepOutcome::ContinueToRun
}

pub(crate) struct TuiNormalSubmitLaunchInput<'a> {
    pub(crate) provider_kind: ProviderKind,
    pub(crate) base_url: &'a str,
    pub(crate) model: &'a str,
    pub(crate) line: &'a str,
    pub(crate) active_run: &'a RunArgs,
    pub(crate) paths: &'a store::StatePaths,
    pub(crate) logs: &'a mut Vec<String>,
    pub(crate) show_logs: &'a mut bool,
    pub(crate) transcript: &'a mut Vec<(String, String)>,
    pub(crate) status: &'a mut String,
    pub(crate) status_detail: &'a mut String,
    pub(crate) follow_output: &'a bool,
    pub(crate) transcript_scroll: &'a mut usize,
    pub(crate) shared_chat_mcp_registry: &'a mut Option<std::sync::Arc<McpRegistry>>,
}

fn prepare_tui_turn_args(active_run: &RunArgs, line: &str) -> RunArgs {
    let mut turn_args = active_run.clone();
    turn_args.prompt = Some(line.to_string());
    // Keep TUI rendering outside the shared agent loop, but preserve the configured
    // stream mode so interactive runs can match one-shot eval settings.
    turn_args.tui = false;
    turn_args
}

pub(crate) async fn build_tui_normal_submit_launch(
    input: TuiNormalSubmitLaunchInput<'_>,
) -> anyhow::Result<Option<TuiSubmitLaunch>> {
    let (tx, rx) = std::sync::mpsc::channel::<Event>();
    let (queue_tx, queue_rx) =
        std::sync::mpsc::channel::<crate::operator_queue::QueueSubmitRequest>();
    let mut queue_rx_opt = Some(queue_rx);

    let turn_args = prepare_tui_turn_args(input.active_run, input.line);

    if !turn_args.mcp.is_empty() && input.shared_chat_mcp_registry.is_none() {
        let mcp_config_path =
            runtime_paths::resolved_mcp_config_path(&turn_args, &input.paths.state_dir);
        match McpRegistry::from_config_path(
            &mcp_config_path,
            &turn_args.mcp,
            Duration::from_secs(30),
        )
        .await
        {
            Ok(reg) => {
                *input.shared_chat_mcp_registry = Some(std::sync::Arc::new(reg));
            }
            Err(e) => {
                let msg = format!("failed to initialize MCP session: {e}");
                input.logs.push(msg.clone());
                *input.show_logs = true;
                input.transcript.push(("system".to_string(), msg));
                *input.status = "idle".to_string();
                *input.status_detail = "mcp init failed".to_string();
                if *input.follow_output {
                    *input.transcript_scroll = usize::MAX;
                }
                return Ok(None);
            }
        }
    }

    let provider_kind = input.provider_kind;
    let base_url = input.base_url.to_string();
    let model = input.model.to_string();
    let line = input.line.to_string();
    let paths = input.paths.clone();
    let shared_chat_mcp_registry = input.shared_chat_mcp_registry.clone();
    let queue_rx = queue_rx_opt.take().expect("queue rx once");
    let fut: TuiRunFuture = Box::pin(async move {
        match provider_kind {
            ProviderKind::Lmstudio | ProviderKind::Llamacpp => {
                let provider = OpenAiCompatProvider::new(
                    provider_kind,
                    base_url.clone(),
                    turn_args.api_key.clone(),
                    provider_runtime::http_config_from_run_args(&turn_args),
                )?;
                run_agent_with_ui(
                    provider,
                    provider_kind,
                    &base_url,
                    &model,
                    &line,
                    &turn_args,
                    &paths,
                    Some(tx),
                    Some(queue_rx),
                    None,
                    shared_chat_mcp_registry,
                    true,
                )
                .await
            }
            ProviderKind::Ollama => {
                let provider = OllamaProvider::new(
                    base_url.clone(),
                    provider_runtime::http_config_from_run_args(&turn_args),
                )?;
                run_agent_with_ui(
                    provider,
                    provider_kind,
                    &base_url,
                    &model,
                    &line,
                    &turn_args,
                    &paths,
                    Some(tx),
                    Some(queue_rx),
                    None,
                    shared_chat_mcp_registry,
                    true,
                )
                .await
            }
            ProviderKind::Mock => {
                let provider = MockProvider::new();
                run_agent_with_ui(
                    provider,
                    provider_kind,
                    &base_url,
                    &model,
                    &line,
                    &turn_args,
                    &paths,
                    Some(tx),
                    Some(queue_rx),
                    None,
                    shared_chat_mcp_registry,
                    true,
                )
                .await
            }
        }
    });

    Ok(Some(TuiSubmitLaunch { rx, queue_tx, fut }))
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::prepare_tui_turn_args;
    use crate::RunArgs;

    #[test]
    fn prepare_tui_turn_args_preserves_disabled_streaming() {
        let base = RunArgs::parse_from(["localagent"]);
        let prepared = prepare_tui_turn_args(&base, "test prompt");
        assert_eq!(prepared.prompt.as_deref(), Some("test prompt"));
        assert!(!prepared.stream);
        assert!(!prepared.tui);
    }

    #[test]
    fn prepare_tui_turn_args_preserves_enabled_streaming() {
        let base = RunArgs::parse_from(["localagent", "--stream"]);
        let prepared = prepare_tui_turn_args(&base, "test prompt");
        assert_eq!(prepared.prompt.as_deref(), Some("test prompt"));
        assert!(prepared.stream);
        assert!(!prepared.tui);
    }
}

pub(crate) struct TuiEnterSubmitInput<'a> {
    pub(crate) terminal: &'a mut Terminal<CrosstermBackend<std::io::Stdout>>,
    pub(crate) input: &'a mut String,
    pub(crate) history_idx: &'a mut Option<usize>,
    pub(crate) slash_menu_index: &'a mut usize,
    pub(crate) pending_timeout_input: &'a mut bool,
    pub(crate) pending_params_input: &'a mut bool,
    pub(crate) timeout_notice_active: &'a mut bool,
    pub(crate) active_run: &'a mut RunArgs,
    pub(crate) base_run: &'a RunArgs,
    pub(crate) paths: &'a store::StatePaths,
    pub(crate) provider_kind: ProviderKind,
    pub(crate) provider_connected: &'a mut bool,
    pub(crate) base_url: &'a str,
    pub(crate) model: &'a str,
    pub(crate) cwd_label: &'a str,
    pub(crate) logs: &'a mut Vec<String>,
    pub(crate) show_logs: &'a mut bool,
    pub(crate) show_tools: &'a mut bool,
    pub(crate) show_approvals: &'a mut bool,
    pub(crate) show_tool_details: &'a mut bool,
    pub(crate) tools_focus: &'a mut bool,
    pub(crate) visible_tool_count: usize,
    pub(crate) prompt_history: &'a mut Vec<String>,
    pub(crate) transcript: &'a mut Vec<(String, String)>,
    pub(crate) transcript_thinking: &'a mut std::collections::BTreeMap<usize, String>,
    pub(crate) show_thinking_panel: &'a mut bool,
    pub(crate) streaming_assistant: &'a mut String,
    pub(crate) status: &'a mut String,
    pub(crate) status_detail: &'a mut String,
    pub(crate) think_tick: &'a mut u64,
    pub(crate) ui_tick: &'a mut u64,
    pub(crate) follow_output: &'a mut bool,
    pub(crate) transcript_scroll: &'a mut usize,
    pub(crate) ui_state: &'a mut UiState,
    pub(crate) tools_selected: &'a mut usize,
    pub(crate) approvals_selected: &'a mut usize,
    pub(crate) compact_tools: bool,
    pub(crate) show_banner: bool,
    pub(crate) palette_open: bool,
    pub(crate) palette_items: &'a [&'a str],
    pub(crate) palette_selected: usize,
    pub(crate) search_mode: bool,
    pub(crate) search_query: &'a str,
    pub(crate) shared_chat_mcp_registry: &'a mut Option<std::sync::Arc<McpRegistry>>,
    pub(crate) learn_overlay: &'a mut Option<LearnOverlayState>,
    pub(crate) input_cursor: &'a mut usize,
    pub(crate) search_input_cursor: &'a mut usize,
    pub(crate) learn_overlay_cursor: &'a mut usize,
}

pub(crate) async fn handle_tui_enter_submit(
    input: TuiEnterSubmitInput<'_>,
) -> anyhow::Result<TuiEnterSubmitOutcome> {
    let TuiEnterSubmitInput {
        terminal,
        input: input_buf,
        history_idx,
        slash_menu_index,
        pending_timeout_input,
        pending_params_input,
        timeout_notice_active,
        active_run,
        base_run,
        paths,
        provider_kind,
        provider_connected,
        base_url,
        model,
        cwd_label,
        logs,
        show_logs,
        show_tools,
        show_approvals,
        show_tool_details,
        tools_focus,
        visible_tool_count,
        prompt_history,
        transcript,
        transcript_thinking,
        show_thinking_panel,
        streaming_assistant,
        status,
        status_detail,
        think_tick,
        ui_tick,
        follow_output,
        transcript_scroll,
        ui_state,
        tools_selected,
        approvals_selected,
        compact_tools,
        show_banner,
        palette_open,
        palette_items,
        palette_selected,
        search_mode,
        search_query,
        shared_chat_mcp_registry,
        learn_overlay,
        input_cursor,
        search_input_cursor,
        learn_overlay_cursor,
    } = input;

    let line = input_buf.trim().to_string();
    input_buf.clear();
    *input_cursor = 0;
    *history_idx = None;
    *slash_menu_index = 0;
    if line.is_empty() {
        return Ok(TuiEnterSubmitOutcome::ContinueLoop);
    }
    if *pending_params_input && !line.starts_with('/') {
        if line.eq_ignore_ascii_case("cancel") {
            *pending_params_input = false;
            logs.push("params update cancelled".to_string());
        } else {
            match crate::runtime_config::apply_params_input(active_run, &line) {
                Ok(msg) => {
                    *pending_params_input = false;
                    logs.push(msg);
                }
                Err(msg) => logs.push(msg),
            }
        }
        *show_logs = true;
        return Ok(TuiEnterSubmitOutcome::ContinueLoop);
    }
    if *pending_timeout_input && !line.starts_with('/') {
        if line.eq_ignore_ascii_case("cancel") {
            *pending_timeout_input = false;
            logs.push("timeout update cancelled".to_string());
            *show_logs = false;
        } else {
            match crate::runtime_config::apply_timeout_input(active_run, &line) {
                Ok(msg) => {
                    *pending_timeout_input = false;
                    logs.push(msg);
                    *show_logs = false;
                }
                Err(msg) => {
                    logs.push(msg);
                    *show_logs = true;
                }
            }
        }
        return Ok(TuiEnterSubmitOutcome::ContinueLoop);
    }
    if line.starts_with('/') {
        match handle_tui_slash_command(TuiSlashCommandDispatchInput {
            line: &line,
            slash_menu_index: *slash_menu_index,
            run_busy: status.as_str() == "running",
            active_run,
            paths,
            logs,
            show_logs,
            show_tools,
            show_approvals,
            timeout_notice_active,
            pending_timeout_input,
            pending_params_input,
            transcript,
            transcript_thinking,
            ui_state,
            streaming_assistant,
            transcript_scroll,
            follow_output,
            shared_chat_mcp_registry,
            learn_overlay,
            learn_overlay_cursor,
        })
        .await?
        {
            SlashCommandDispatchOutcome::ExitRequested => {
                return Ok(TuiEnterSubmitOutcome::ExitRequested)
            }
            SlashCommandDispatchOutcome::Handled => return Ok(TuiEnterSubmitOutcome::ContinueLoop),
        }
    }

    if line.is_empty() && *show_tools && (!*show_approvals || *tools_focus) {
        if visible_tool_count > 0 {
            *show_tool_details = !*show_tool_details;
            if *show_tool_details {
                *show_logs = false;
            }
        }
        return Ok(TuiEnterSubmitOutcome::ContinueLoop);
    }

    match prepare_tui_normal_submit_state(TuiNormalSubmitPrepInput {
        line: &line,
        prompt_history,
        transcript,
        show_thinking_panel,
        show_logs,
        follow_output,
        transcript_scroll,
        status,
        status_detail,
        streaming_assistant,
        think_tick,
    }) {
        TuiNormalSubmitPrepOutcome::HandledNoRun => return Ok(TuiEnterSubmitOutcome::ContinueLoop),
        TuiNormalSubmitPrepOutcome::ContinueToRun => {}
    }

    terminal.draw(|f| {
        chat_ui::draw_chat_frame(
            f,
            &chat_runtime::chat_mode_display_label(active_run),
            provider_runtime::provider_cli_name(provider_kind),
            *provider_connected,
            model,
            status,
            status_detail,
            transcript,
            transcript_thinking,
            *show_thinking_panel,
            streaming_assistant,
            ui_state,
            *tools_selected,
            *tools_focus,
            *show_tool_details,
            *approvals_selected,
            cwd_label,
            input_buf,
            *input_cursor,
            true,
            logs,
            *think_tick,
            base_run.tui_refresh_ms,
            *show_tools,
            *show_approvals,
            *show_logs,
            *transcript_scroll,
            compact_tools,
            show_banner,
            *ui_tick,
            if palette_open {
                Some(format!(
                    "⌘ {}  (Up/Down, Enter, Esc)",
                    palette_items[palette_selected]
                ))
            } else if search_mode {
                Some(format!(
                    "🔎 {}  (Enter next, Esc close)",
                    render_with_optional_caret(search_query, *search_input_cursor, true)
                ))
            } else if input_buf.starts_with('/') {
                chat_commands::slash_overlay_text(input_buf, *slash_menu_index)
            } else if input_buf.starts_with('?') {
                chat_commands::keybinds_overlay_text()
            } else {
                None
            },
            None,
        );
    })?;
    *ui_tick = ui_tick.saturating_add(1);

    let launch = match build_tui_normal_submit_launch(TuiNormalSubmitLaunchInput {
        provider_kind,
        base_url,
        model,
        line: &line,
        active_run,
        paths,
        logs,
        show_logs,
        transcript,
        status,
        status_detail,
        follow_output,
        transcript_scroll,
        shared_chat_mcp_registry,
    })
    .await?
    {
        Some(launch) => launch,
        None => return Ok(TuiEnterSubmitOutcome::ContinueLoop),
    };

    Ok(TuiEnterSubmitOutcome::Launched(launch))
}

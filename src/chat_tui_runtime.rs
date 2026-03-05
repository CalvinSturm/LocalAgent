use std::time::{Duration, Instant};

use anyhow::anyhow;
use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::chat_runtime;
use crate::chat_tui::active_turn::{
    drive_tui_active_turn_loop, TuiActiveTurnLoopInput,
};
use crate::chat_tui::approvals::refresh_approvals_with_auto_open;
use crate::chat_tui::event_dispatch::{
    handle_tui_outer_event_dispatch, TuiOuterEventDispatchInput, TuiOuterEventDispatchOutcome,
};
use crate::chat_tui::key_dispatch::handle_tui_outer_key_dispatch;
use crate::chat_tui::overlay::{
    overlay_pending_message_for_submit, push_overlay_log_dedup, LearnOverlayState,
};
use crate::chat_tui::render_model::{build_tui_render_frame_input, TuiRenderFrameBuildInput};
use crate::chat_tui::submit::{
    handle_tui_enter_submit, TuiEnterSubmitInput, TuiEnterSubmitOutcome,
    TuiSubmitLaunch,
};
use crate::chat_tui::text::{
    char_len,
};
use crate::chat_ui;
use crate::mcp::registry::McpRegistry;
use crate::provider_runtime;
use crate::store;
use crate::tui::state::UiState;
use crate::{ChatArgs, RunArgs};

pub(crate) async fn run_chat_tui(
    chat: &ChatArgs,
    base_run: &RunArgs,
    paths: &store::StatePaths,
) -> anyhow::Result<()> {
    let provider_kind = base_run
        .provider
        .ok_or_else(|| anyhow!("--provider is required in chat mode"))?;
    let model = base_run
        .model
        .clone()
        .ok_or_else(|| anyhow!("--model is required in chat mode"))?;
    let base_url = base_run
        .base_url
        .clone()
        .unwrap_or_else(|| provider_runtime::default_base_url(provider_kind).to_string());
    let cwd_label = chat_runtime::normalize_path_for_display(
        std::fs::canonicalize(&base_run.workdir)
            .or_else(|_| std::env::current_dir())
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| base_run.workdir.display().to_string()),
    );
    let mut active_run = base_run.clone();

    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    if chat.plain_tui {
        execute!(stdout, DisableMouseCapture, EnableBracketedPaste)?;
    } else {
        execute!(
            stdout,
            EnterAlternateScreen,
            EnableMouseCapture,
            EnableBracketedPaste
        )?;
    }
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut input = String::new();
    let mut prompt_history: Vec<String> = Vec::new();
    let mut history_idx: Option<usize> = None;
    let mut transcript: Vec<(String, String)> = vec![];
    let mut transcript_thinking: std::collections::BTreeMap<usize, String> =
        std::collections::BTreeMap::new();
    let mut show_thinking_panel = false;
    let show_banner = !chat.no_banner;
    let mut logs: Vec<String> = Vec::new();
    let max_logs = base_run.tui_max_log_lines;
    let mut status = "idle".to_string();
    let mut status_detail = String::new();
    let mut provider_connected = true;
    let mut think_tick: u64 = 0;
    let mut ui_tick: u64 = 0;
    let mut approvals_selected = 0usize;
    let mut show_tools = false;
    let mut show_approvals = false;
    let mut show_logs = false;
    let mut transcript_scroll: usize = 0;
    let mut follow_output = true;
    let mut compact_tools = true;
    let mut tools_selected = 0usize;
    let mut tools_focus = true;
    let mut show_tool_details = false;
    let palette_items = [
        "toggle tools pane",
        "toggle approvals pane",
        "toggle logs pane",
        "toggle tool row density",
        "clear transcript",
        "jump to latest",
    ];
    let mut palette_open = false;
    let mut palette_selected = 0usize;
    let mut search_mode = false;
    let mut search_query = String::new();
    let mut search_line_cursor = 0usize;
    let mut search_input_cursor = 0usize;
    let mut slash_menu_index: usize = 0;
    let mut learn_overlay: Option<LearnOverlayState> = None;
    let mut learn_overlay_cursor = 0usize;
    let mut shared_chat_mcp_registry: Option<std::sync::Arc<McpRegistry>> = None;
    let mut pending_timeout_input = false;
    let mut pending_params_input = false;
    let mut timeout_notice_active = false;
    let mut ui_state = UiState::new(max_logs);
    ui_state.provider = provider_runtime::provider_cli_name(provider_kind).to_string();
    ui_state.model = model.clone();
    ui_state.caps_source = format!("{:?}", base_run.caps).to_lowercase();
    ui_state.policy_hash = "-".to_string();
    let mut previous_pending_approvals = ui_state.pending_approval_count();
    let mut streaming_assistant = String::new();
    let mut input_cursor = char_len(&input);

    let run_result: anyhow::Result<()> = async {
        loop {
            ui_state.on_tick(Instant::now());
            refresh_approvals_with_auto_open(
                &mut ui_state,
                &paths.approvals_path,
                &mut show_approvals,
                &mut previous_pending_approvals,
                &mut logs,
            );
            let frame = build_tui_render_frame_input(TuiRenderFrameBuildInput {
                active_run: &active_run,
                provider_kind,
                provider_connected,
                model: &model,
                status: &status,
                status_detail: &status_detail,
                transcript: &transcript,
                transcript_thinking: &transcript_thinking,
                show_thinking_panel,
                streaming_assistant: &streaming_assistant,
                ui_state: &ui_state,
                tools_selected: &mut tools_selected,
                tools_focus: &mut tools_focus,
                show_tool_details: &mut show_tool_details,
                approvals_selected: &mut approvals_selected,
                cwd_label: &cwd_label,
                input: &input,
                input_cursor,
                logs: &logs,
                think_tick,
                tui_refresh_ms: base_run.tui_refresh_ms,
                show_tools: &mut show_tools,
                show_approvals: &mut show_approvals,
                show_logs,
                transcript_scroll,
                compact_tools,
                show_banner,
                ui_tick,
                palette_open,
                palette_items: &palette_items,
                palette_selected,
                search_mode,
                search_query: &search_query,
                search_input_cursor,
                slash_menu_index,
                learn_overlay: &learn_overlay,
                learn_overlay_cursor,
            });
            let visible_tool_count =
                frame
                    .ui_state
                    .tool_calls
                    .len()
                    .min(if frame.compact_tools { 20 } else { 12 });

            terminal.draw(|f| {
                chat_ui::draw_chat_frame(
                    f,
                    frame.mode_label.as_str(),
                    frame.provider_label,
                    frame.provider_connected,
                    frame.model,
                    frame.status,
                    frame.status_detail,
                    frame.transcript,
                    frame.transcript_thinking,
                    frame.show_thinking_panel,
                    frame.streaming_assistant,
                    frame.ui_state,
                    frame.tools_selected,
                    frame.tools_focus,
                    frame.show_tool_details,
                    frame.approvals_selected,
                    frame.cwd_label,
                    frame.input,
                    frame.input_cursor,
                    frame.input_cursor_visible,
                    frame.logs,
                    frame.think_tick,
                    frame.tui_refresh_ms,
                    frame.show_tools,
                    frame.show_approvals,
                    frame.show_logs,
                    frame.transcript_scroll,
                    frame.compact_tools,
                    frame.show_banner,
                    frame.ui_tick,
                    frame.overlay_text.clone(),
                    frame.learn_overlay.as_ref(),
                );
            })?;

            if event::poll(Duration::from_millis(base_run.tui_refresh_ms))? {
                match handle_tui_outer_event_dispatch(
                    TuiOuterEventDispatchInput {
                        event: event::read()?,
                        status: &status,
                        prompt_history: &mut prompt_history,
                        transcript: &mut transcript,
                        transcript_thinking: &mut transcript_thinking,
                        show_thinking_panel: &mut show_thinking_panel,
                        streaming_assistant: &mut streaming_assistant,
                        transcript_scroll: &mut transcript_scroll,
                        follow_output: &mut follow_output,
                        input: &mut input,
                        input_cursor: &mut input_cursor,
                        history_idx: &mut history_idx,
                        slash_menu_index: &mut slash_menu_index,
                        palette_open: &mut palette_open,
                        palette_items: &palette_items,
                        palette_selected: &mut palette_selected,
                        search_mode: &mut search_mode,
                        search_query: &mut search_query,
                        search_line_cursor: &mut search_line_cursor,
                        search_input_cursor: &mut search_input_cursor,
                        ui_state: &mut ui_state,
                        visible_tool_count,
                        show_tools: &mut show_tools,
                        show_approvals: &mut show_approvals,
                        show_logs: &mut show_logs,
                        compact_tools: &mut compact_tools,
                        tools_selected: &mut tools_selected,
                        tools_focus: &mut tools_focus,
                        approvals_selected: &mut approvals_selected,
                        paths,
                        logs: &mut logs,
                        learn_overlay: &mut learn_overlay,
                        learn_overlay_cursor: &mut learn_overlay_cursor,
                    },
                    handle_tui_outer_key_dispatch,
                ) {
                    TuiOuterEventDispatchOutcome::BreakLoop => break,
                    TuiOuterEventDispatchOutcome::ContinueLoop => continue,
                    TuiOuterEventDispatchOutcome::HandledKey => {
                        let pending_line = learn_overlay
                            .as_mut()
                            .and_then(|overlay| overlay.pending_submit_line.take());
                        if let Some(line) = pending_line {
                            if let Some(msg) = overlay_pending_message_for_submit(&line) {
                                if let Some(overlay) = learn_overlay.as_mut() {
                                    overlay.inline_message = Some(msg.to_string());
                                }
                            }
                            let mut submit_fut = Box::pin(
                                crate::chat_tui_learn_adapter::parse_and_dispatch_learn_slash(
                                    &line,
                                    &active_run,
                                    paths,
                                ),
                            );
                            let submit_result = loop {
                                tokio::select! {
                                    r = &mut submit_fut => break r,
                                    _ = tokio::time::sleep(Duration::from_millis(base_run.tui_refresh_ms)) => {
                                        ui_state.on_tick(Instant::now());
                                        ui_tick = ui_tick.saturating_add(1);
                                        let frame = build_tui_render_frame_input(TuiRenderFrameBuildInput {
                                            active_run: &active_run,
                                            provider_kind,
                                            provider_connected,
                                            model: &model,
                                            status: &status,
                                            status_detail: &status_detail,
                                            transcript: &transcript,
                                            transcript_thinking: &transcript_thinking,
                                            show_thinking_panel,
                                            streaming_assistant: &streaming_assistant,
                                            ui_state: &ui_state,
                                            tools_selected: &mut tools_selected,
                                            tools_focus: &mut tools_focus,
                                            show_tool_details: &mut show_tool_details,
                                            approvals_selected: &mut approvals_selected,
                                            cwd_label: &cwd_label,
                                            input: &input,
                                            input_cursor,
                                            logs: &logs,
                                            think_tick,
                                            tui_refresh_ms: base_run.tui_refresh_ms,
                                            show_tools: &mut show_tools,
                                            show_approvals: &mut show_approvals,
                                            show_logs,
                                            transcript_scroll,
                                            compact_tools,
                                            show_banner,
                                            ui_tick,
                                            palette_open,
                                            palette_items: &palette_items,
                                            palette_selected,
                                            search_mode,
                                            search_query: &search_query,
                                            search_input_cursor,
                                            slash_menu_index,
                                            learn_overlay: &learn_overlay,
                                            learn_overlay_cursor,
                                        });
                                        terminal.draw(|f| {
                                            chat_ui::draw_chat_frame(
                                                f,
                                                frame.mode_label.as_str(),
                                                frame.provider_label,
                                                frame.provider_connected,
                                                frame.model,
                                                frame.status,
                                                frame.status_detail,
                                                frame.transcript,
                                                frame.transcript_thinking,
                                                frame.show_thinking_panel,
                                                frame.streaming_assistant,
                                                frame.ui_state,
                                                frame.tools_selected,
                                                frame.tools_focus,
                                                frame.show_tool_details,
                                                frame.approvals_selected,
                                                frame.cwd_label,
                                                frame.input,
                                                frame.input_cursor,
                                                frame.input_cursor_visible,
                                                frame.logs,
                                                frame.think_tick,
                                                frame.tui_refresh_ms,
                                                frame.show_tools,
                                                frame.show_approvals,
                                                frame.show_logs,
                                                frame.transcript_scroll,
                                                frame.compact_tools,
                                                frame.show_banner,
                                                frame.ui_tick,
                                                frame.overlay_text.clone(),
                                                frame.learn_overlay.as_ref(),
                                            );
                                        })?;
                                    }
                                }
                            };

                            if let Some(overlay) = learn_overlay.as_mut() {
                                match submit_result {
                                    Ok(output) => {
                                        if !output.is_empty() {
                                            push_overlay_log_dedup(overlay, &output);
                                        }
                                        overlay.inline_message = Some(
                                            "Enhancement complete. Review output, adjust if needed, then press Enter to run again."
                                                .to_string(),
                                        );
                                    }
                                    Err(e) => {
                                        push_overlay_log_dedup(
                                            overlay,
                                            &format!("learn command failed: {e}"),
                                        );
                                        overlay.inline_message = Some(
                                            "Enhancement failed. Review logs, fix inputs, then press Enter to retry."
                                                .to_string(),
                                        );
                                    }
                                }
                            }
                        }
                        if logs.len() > max_logs {
                            let drop_n = logs.len() - max_logs;
                            logs.drain(0..drop_n);
                        }
                        ui_tick = ui_tick.saturating_add(1);
                    }
                    TuiOuterEventDispatchOutcome::Noop => {}
                    TuiOuterEventDispatchOutcome::EnterInline => {
                        match handle_tui_enter_submit(TuiEnterSubmitInput {
                            terminal: &mut terminal,
                            input: &mut input,
                            history_idx: &mut history_idx,
                            slash_menu_index: &mut slash_menu_index,
                            pending_timeout_input: &mut pending_timeout_input,
                            pending_params_input: &mut pending_params_input,
                            timeout_notice_active: &mut timeout_notice_active,
                            active_run: &mut active_run,
                            base_run,
                            paths,
                            provider_kind,
                            provider_connected: &mut provider_connected,
                            base_url: &base_url,
                            model: &model,
                            cwd_label: &cwd_label,
                            logs: &mut logs,
                            show_logs: &mut show_logs,
                            show_tools: &mut show_tools,
                            show_approvals: &mut show_approvals,
                            show_tool_details: &mut show_tool_details,
                            tools_focus: &mut tools_focus,
                            visible_tool_count,
                            prompt_history: &mut prompt_history,
                            transcript: &mut transcript,
                            transcript_thinking: &mut transcript_thinking,
                            show_thinking_panel: &mut show_thinking_panel,
                            streaming_assistant: &mut streaming_assistant,
                            status: &mut status,
                            status_detail: &mut status_detail,
                            think_tick: &mut think_tick,
                            ui_tick: &mut ui_tick,
                            follow_output: &mut follow_output,
                            transcript_scroll: &mut transcript_scroll,
                            ui_state: &mut ui_state,
                            tools_selected: &mut tools_selected,
                            approvals_selected: &mut approvals_selected,
                            compact_tools,
                            show_banner,
                            palette_open,
                            palette_items: &palette_items,
                            palette_selected,
                            search_mode,
                            search_query: &search_query,
                            shared_chat_mcp_registry: &mut shared_chat_mcp_registry,
                            learn_overlay: &mut learn_overlay,
                            input_cursor: &mut input_cursor,
                            search_input_cursor: &mut search_input_cursor,
                            learn_overlay_cursor: &mut learn_overlay_cursor,
                        })
                        .await?
                        {
                            TuiEnterSubmitOutcome::ContinueLoop => continue,
                            TuiEnterSubmitOutcome::ExitRequested => break,
                            TuiEnterSubmitOutcome::Launched(TuiSubmitLaunch {
                                rx,
                                queue_tx,
                                fut,
                            }) => {
                                drive_tui_active_turn_loop(TuiActiveTurnLoopInput {
                                    terminal: &mut terminal,
                                    fut,
                                    rx,
                                    queue_tx,
                                    ui_state: &mut ui_state,
                                    paths,
                                    active_run: &active_run,
                                    base_run,
                                    provider_kind,
                                    provider_connected: &mut provider_connected,
                                    model: &model,
                                    cwd_label: &cwd_label,
                                    input: &mut input,
                                    logs: &mut logs,
                                    transcript: &mut transcript,
                                    transcript_thinking: &mut transcript_thinking,
                                    show_thinking_panel: &mut show_thinking_panel,
                                    streaming_assistant: &mut streaming_assistant,
                                    status: &mut status,
                                    status_detail: &mut status_detail,
                                    think_tick: &mut think_tick,
                                    ui_tick: &mut ui_tick,
                                    approvals_selected: &mut approvals_selected,
                                    show_tools: &mut show_tools,
                                    show_approvals: &mut show_approvals,
                                    show_logs: &mut show_logs,
                                    timeout_notice_active: &mut timeout_notice_active,
                                    transcript_scroll: &mut transcript_scroll,
                                    follow_output: &mut follow_output,
                                    compact_tools,
                                    tools_selected: &mut tools_selected,
                                    tools_focus: &mut tools_focus,
                                    show_tool_details: &mut show_tool_details,
                                    show_banner,
                                    palette_open,
                                    palette_items: &palette_items,
                                    palette_selected,
                                    search_mode,
                                    search_query: &search_query,
                                    slash_menu_index: &mut slash_menu_index,
                                    learn_overlay: &mut learn_overlay,
                                    input_cursor: &mut input_cursor,
                                    learn_overlay_cursor: &mut learn_overlay_cursor,
                                })
                                .await?;
                                if logs.len() > max_logs {
                                    let drop_n = logs.len() - max_logs;
                                    logs.drain(0..drop_n);
                                }
                                ui_tick = ui_tick.saturating_add(1);
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
    .await;

    disable_raw_mode()?;
    if chat.plain_tui {
        execute!(
            terminal.backend_mut(),
            DisableBracketedPaste,
            DisableMouseCapture
        )?;
    } else {
        execute!(
            terminal.backend_mut(),
            DisableBracketedPaste,
            DisableMouseCapture,
            LeaveAlternateScreen
        )?;
    }
    terminal.show_cursor()?;
    run_result
}

#[cfg(test)]
#[cfg(test)]
mod tests;

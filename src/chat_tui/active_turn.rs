use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event as CEvent, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::agent::AgentExitReason;
use crate::chat_commands;
use crate::chat_runtime;
use crate::chat_tui::approvals::{refresh_approvals_with_auto_open, ActiveQueueRow};
use crate::chat_tui::event_dispatch::TuiOuterKeyDispatchInput;
use crate::chat_tui::key_dispatch::handle_tui_outer_key_dispatch;
use crate::chat_tui::overlay::LearnOverlayState;
use crate::chat_tui::render_model::build_learn_overlay_render_model_with_cursor;
use crate::chat_tui::submit::TuiRunFuture;
use crate::chat_tui::text::{
    char_len, delete_char_before_cursor, insert_text_bounded, render_with_optional_caret,
};
use crate::chat_tui::transcript::push_assistant_transcript_entry;
use crate::chat_ui;
use crate::chat_view_utils;
use crate::events::{Event, EventKind};
use crate::gate::ProviderKind;
use crate::provider_runtime;
use crate::runtime_config;
use crate::store;
use crate::trust::approvals::ApprovalsStore;
use crate::tui::state::UiState;
use crate::RunArgs;

fn should_render_final_assistant_entry(exit_reason: AgentExitReason, final_text: &str) -> bool {
    matches!(exit_reason, AgentExitReason::Ok) && !final_text.trim().is_empty()
}

pub(crate) struct TuiActiveTurnLoopInput<'a> {
    pub(crate) terminal: &'a mut Terminal<CrosstermBackend<std::io::Stdout>>,
    pub(crate) fut: TuiRunFuture,
    pub(crate) rx: std::sync::mpsc::Receiver<Event>,
    pub(crate) queue_tx: std::sync::mpsc::Sender<crate::operator_queue::QueueSubmitRequest>,
    pub(crate) ui_state: &'a mut UiState,
    pub(crate) paths: &'a store::StatePaths,
    pub(crate) active_run: &'a RunArgs,
    pub(crate) base_run: &'a RunArgs,
    pub(crate) provider_kind: ProviderKind,
    pub(crate) provider_connected: &'a mut bool,
    pub(crate) model: &'a str,
    pub(crate) cwd_label: &'a str,
    pub(crate) input: &'a mut String,
    pub(crate) logs: &'a mut Vec<String>,
    pub(crate) transcript: &'a mut Vec<(String, String)>,
    pub(crate) transcript_thinking: &'a mut std::collections::BTreeMap<usize, String>,
    pub(crate) show_thinking_panel: &'a mut bool,
    pub(crate) streaming_assistant: &'a mut String,
    pub(crate) status: &'a mut String,
    pub(crate) status_detail: &'a mut String,
    pub(crate) think_tick: &'a mut u64,
    pub(crate) ui_tick: &'a mut u64,
    pub(crate) approvals_selected: &'a mut usize,
    pub(crate) show_tools: &'a mut bool,
    pub(crate) show_approvals: &'a mut bool,
    pub(crate) show_logs: &'a mut bool,
    pub(crate) timeout_notice_active: &'a mut bool,
    pub(crate) transcript_scroll: &'a mut usize,
    pub(crate) follow_output: &'a mut bool,
    pub(crate) compact_tools: bool,
    pub(crate) tools_selected: &'a mut usize,
    pub(crate) tools_focus: &'a mut bool,
    pub(crate) show_tool_details: &'a mut bool,
    pub(crate) show_banner: bool,
    pub(crate) palette_open: bool,
    pub(crate) palette_items: &'a [&'a str],
    pub(crate) palette_selected: usize,
    pub(crate) search_mode: bool,
    pub(crate) search_query: &'a str,
    pub(crate) slash_menu_index: &'a mut usize,
    pub(crate) learn_overlay: &'a mut Option<LearnOverlayState>,
    pub(crate) input_cursor: &'a mut usize,
    pub(crate) learn_overlay_cursor: &'a mut usize,
}

pub(crate) async fn drive_tui_active_turn_loop(
    input: TuiActiveTurnLoopInput<'_>,
) -> anyhow::Result<()> {
    let TuiActiveTurnLoopInput {
        terminal,
        mut fut,
        rx,
        queue_tx,
        ui_state,
        paths,
        active_run,
        base_run,
        provider_kind,
        provider_connected,
        model,
        cwd_label,
        input: input_buf,
        logs,
        transcript,
        transcript_thinking,
        show_thinking_panel,
        streaming_assistant,
        status,
        status_detail,
        think_tick,
        ui_tick,
        approvals_selected,
        show_tools,
        show_approvals,
        show_logs,
        timeout_notice_active,
        transcript_scroll,
        follow_output,
        compact_tools,
        tools_selected,
        tools_focus,
        show_tool_details,
        show_banner,
        palette_open,
        palette_items,
        palette_selected,
        search_mode,
        search_query,
        slash_menu_index,
        learn_overlay,
        input_cursor,
        learn_overlay_cursor,
    } = input;

    let tool_row_count = if compact_tools { 20 } else { 12 };
    let mut active_queue_rows: BTreeMap<String, ActiveQueueRow> = std::collections::BTreeMap::new();
    let mut previous_pending_approvals = ui_state.pending_approval_count();

    loop {
        ui_state.on_tick(Instant::now());
        refresh_approvals_with_auto_open(
            ui_state,
            &paths.approvals_path,
            show_approvals,
            &mut previous_pending_approvals,
            logs,
        );
        while let Ok(ev) = rx.try_recv() {
            ui_state.apply_event(&ev);
            if matches!(ev.kind, EventKind::ToolDecision)
                && ev.data.get("decision").and_then(|v| v.as_str()) == Some("require_approval")
            {
                refresh_approvals_with_auto_open(
                    ui_state,
                    &paths.approvals_path,
                    show_approvals,
                    &mut previous_pending_approvals,
                    logs,
                );
            }
            match ev.kind {
                EventKind::QueueSubmitted => {
                    if let Some(queue_id) = ev.data.get("queue_id").and_then(|v| v.as_str()) {
                        let sequence_no = ev
                            .data
                            .get("sequence_no")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        let kind = ev
                            .data
                            .get("kind")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let delivery_phrase = ev
                            .data
                            .get("next_delivery")
                            .and_then(|v| v.as_str())
                            .unwrap_or("pending")
                            .to_string();
                        active_queue_rows.insert(
                            queue_id.to_string(),
                            ActiveQueueRow {
                                sequence_no,
                                kind,
                                status: "pending".to_string(),
                                delivery_phrase,
                            },
                        );
                    }
                }
                EventKind::QueueDelivered => {
                    if let Some(queue_id) = ev.data.get("queue_id").and_then(|v| v.as_str()) {
                        if let Some(row) = active_queue_rows.get_mut(queue_id) {
                            row.status = "delivered".to_string();
                            row.delivery_phrase =
                                match ev.data.get("delivery_boundary").and_then(|v| v.as_str()) {
                                    Some("post_tool") => "after current tool finishes".to_string(),
                                    Some("post_step") => "after current step finishes".to_string(),
                                    Some("turn_idle") => "after this turn completes".to_string(),
                                    _ => "delivered".to_string(),
                                };
                        }
                    }
                }
                EventKind::QueueInterrupt => {
                    if let Some(queue_id) = ev.data.get("queue_id").and_then(|v| v.as_str()) {
                        if let Some(row) = active_queue_rows.get_mut(queue_id) {
                            row.status = "interrupted".to_string();
                        }
                    }
                }
                _ => {}
            }
            match ev.kind {
                EventKind::ModelDelta => {
                    if let Some(d) = ev.data.get("delta").and_then(|v| v.as_str()) {
                        streaming_assistant.push_str(d);
                        if *follow_output {
                            *transcript_scroll = usize::MAX;
                        }
                    }
                }
                EventKind::ModelResponseEnd => {
                    if streaming_assistant.is_empty() {
                        if let Some(c) = ev.data.get("content").and_then(|v| v.as_str()) {
                            streaming_assistant.push_str(c);
                            if *follow_output {
                                *transcript_scroll = usize::MAX;
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        while event::poll(Duration::from_millis(0))? {
            match event::read()? {
                CEvent::Mouse(me) => {
                    if let Some(delta) = chat_runtime::mouse_scroll_delta(&me) {
                        let max_scroll = chat_runtime::transcript_max_scroll_lines(
                            transcript,
                            streaming_assistant,
                        );
                        *transcript_scroll = chat_runtime::adjust_transcript_scroll(
                            *transcript_scroll,
                            delta,
                            max_scroll,
                        );
                        *follow_output = false;
                    }
                }
                CEvent::Paste(pasted) => {
                    insert_text_bounded(
                        input_buf,
                        input_cursor,
                        &chat_runtime::normalize_pasted_text(&pasted),
                        usize::MAX,
                    );
                    *slash_menu_index = 0;
                }
                CEvent::Key(key)
                    if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) =>
                {
                    if learn_overlay.is_some()
                        && !(matches!(key.code, KeyCode::Char('c'))
                            && key.modifiers.contains(KeyModifiers::CONTROL))
                    {
                        let mut prompt_history_dummy = Vec::new();
                        let mut history_idx_dummy = None;
                        let mut palette_open_dummy = false;
                        let palette_items_dummy = ["overlay"];
                        let mut palette_selected_dummy = 0usize;
                        let mut search_mode_dummy = false;
                        let mut search_query_dummy = String::new();
                        let mut search_line_cursor_dummy = 0usize;
                        let mut search_input_cursor_dummy = 0usize;
                        let mut compact_tools_dummy = false;
                        let visible_tool_count_dummy =
                            ui_state.tool_calls.len().min(tool_row_count);
                        let _ = handle_tui_outer_key_dispatch(TuiOuterKeyDispatchInput {
                            key,
                            learn_overlay,
                            run_busy: true,
                            input: input_buf,
                            input_cursor,
                            prompt_history: &mut prompt_history_dummy,
                            history_idx: &mut history_idx_dummy,
                            slash_menu_index,
                            palette_open: &mut palette_open_dummy,
                            palette_items: &palette_items_dummy,
                            palette_selected: &mut palette_selected_dummy,
                            search_mode: &mut search_mode_dummy,
                            search_query: &mut search_query_dummy,
                            search_line_cursor: &mut search_line_cursor_dummy,
                            search_input_cursor: &mut search_input_cursor_dummy,
                            transcript,
                            transcript_thinking,
                            show_thinking_panel,
                            streaming_assistant,
                            transcript_scroll,
                            follow_output,
                            ui_state,
                            visible_tool_count: visible_tool_count_dummy,
                            show_tools,
                            show_approvals,
                            show_logs,
                            compact_tools: &mut compact_tools_dummy,
                            tools_selected,
                            tools_focus,
                            approvals_selected,
                            paths,
                            logs,
                            learn_overlay_cursor,
                        });
                        continue;
                    }
                    match key.code {
                        KeyCode::Esc => {
                            let cancelled_text = format!("{streaming_assistant}\n\n[cancelled]");
                            push_assistant_transcript_entry(
                                transcript,
                                transcript_thinking,
                                &cancelled_text,
                            );
                            logs.push("run cancelled by user (Esc/Ctrl+C)".to_string());
                            *show_logs = true;
                            streaming_assistant.clear();
                            *status = "idle".to_string();
                            *status_detail = "cancelled by user".to_string();
                            if *follow_output {
                                *transcript_scroll = usize::MAX;
                            }
                            break;
                        }
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            let cancelled_text = format!("{streaming_assistant}\n\n[cancelled]");
                            push_assistant_transcript_entry(
                                transcript,
                                transcript_thinking,
                                &cancelled_text,
                            );
                            logs.push("run cancelled by user (Esc/Ctrl+C)".to_string());
                            *show_logs = true;
                            streaming_assistant.clear();
                            *status = "idle".to_string();
                            *status_detail = "cancelled by user".to_string();
                            if *follow_output {
                                *transcript_scroll = usize::MAX;
                            }
                            break;
                        }
                        KeyCode::PageUp => {
                            let max_scroll = chat_runtime::transcript_max_scroll_lines(
                                transcript,
                                streaming_assistant,
                            );
                            *transcript_scroll = chat_runtime::adjust_transcript_scroll(
                                *transcript_scroll,
                                -12,
                                max_scroll,
                            );
                            *follow_output = false;
                        }
                        KeyCode::PageDown => {
                            let max_scroll = chat_runtime::transcript_max_scroll_lines(
                                transcript,
                                streaming_assistant,
                            );
                            *transcript_scroll = chat_runtime::adjust_transcript_scroll(
                                *transcript_scroll,
                                12,
                                max_scroll,
                            );
                            *follow_output = false;
                        }
                        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            let max_scroll = chat_runtime::transcript_max_scroll_lines(
                                transcript,
                                streaming_assistant,
                            );
                            *transcript_scroll = chat_runtime::adjust_transcript_scroll(
                                *transcript_scroll,
                                -10,
                                max_scroll,
                            );
                            *follow_output = false;
                        }
                        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            let max_scroll = chat_runtime::transcript_max_scroll_lines(
                                transcript,
                                streaming_assistant,
                            );
                            *transcript_scroll = chat_runtime::adjust_transcript_scroll(
                                *transcript_scroll,
                                10,
                                max_scroll,
                            );
                            *follow_output = false;
                        }
                        KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            *show_tools = !*show_tools;
                        }
                        KeyCode::Char('y') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            *show_approvals = !*show_approvals;
                        }
                        KeyCode::Char('g') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            *show_logs = !*show_logs;
                        }
                        KeyCode::Tab => {
                            if *show_tools && *show_approvals {
                                *tools_focus = !*tools_focus;
                            }
                        }
                        KeyCode::Char('1') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            *show_tools = !*show_tools;
                        }
                        KeyCode::Char('2') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            *show_approvals = !*show_approvals;
                        }
                        KeyCode::Char('3') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            *show_logs = !*show_logs;
                        }
                        KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            let visible_tool_count = ui_state.tool_calls.len().min(tool_row_count);
                            if *show_tools && (!*show_approvals || *tools_focus) {
                                if *tools_selected + 1 < visible_tool_count {
                                    *tools_selected += 1;
                                }
                            } else if *approvals_selected + 1 < ui_state.pending_approvals.len() {
                                *approvals_selected += 1;
                            }
                        }
                        KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            if *show_tools && (!*show_approvals || *tools_focus) {
                                *tools_selected = tools_selected.saturating_sub(1);
                            } else {
                                *approvals_selected = approvals_selected.saturating_sub(1);
                            }
                        }
                        KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            refresh_approvals_with_auto_open(
                                ui_state,
                                &paths.approvals_path,
                                show_approvals,
                                &mut previous_pending_approvals,
                                logs,
                            );
                        }
                        KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            if let Some(row) = ui_state.pending_approvals.get(*approvals_selected) {
                                let store = ApprovalsStore::new(paths.approvals_path.clone());
                                if let Err(e) = store.approve(&row.id, None, None) {
                                    logs.push(format!("approve failed: {e}"));
                                } else {
                                    logs.push(format!("approved {}", row.id));
                                }
                                refresh_approvals_with_auto_open(
                                    ui_state,
                                    &paths.approvals_path,
                                    show_approvals,
                                    &mut previous_pending_approvals,
                                    logs,
                                );
                            }
                        }
                        KeyCode::Char('x') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            if let Some(row) = ui_state.pending_approvals.get(*approvals_selected) {
                                let store = ApprovalsStore::new(paths.approvals_path.clone());
                                if let Err(e) = store.deny(&row.id) {
                                    logs.push(format!("deny failed: {e}"));
                                } else {
                                    logs.push(format!("denied {}", row.id));
                                }
                                refresh_approvals_with_auto_open(
                                    ui_state,
                                    &paths.approvals_path,
                                    show_approvals,
                                    &mut previous_pending_approvals,
                                    logs,
                                );
                            }
                        }
                        KeyCode::Backspace => {
                            delete_char_before_cursor(input_buf, input_cursor);
                            *slash_menu_index = 0;
                        }
                        KeyCode::Left => {
                            *input_cursor = input_cursor.saturating_sub(1);
                        }
                        KeyCode::Right => {
                            *input_cursor = (*input_cursor + 1).min(char_len(input_buf));
                        }
                        KeyCode::Char(c) if chat_runtime::is_text_input_mods(key.modifiers) => {
                            insert_text_bounded(
                                input_buf,
                                input_cursor,
                                &c.to_string(),
                                usize::MAX,
                            );
                            *slash_menu_index = 0;
                        }
                        KeyCode::Enter => {
                            if !chat_runtime::is_tui_main_input_submit_key(key) {
                                insert_text_bounded(input_buf, input_cursor, "\n", usize::MAX);
                                *slash_menu_index = 0;
                                continue;
                            }
                            let line = input_buf.trim().to_string();
                            if let Some(rest) = line.strip_prefix("/interrupt ") {
                                let msg = rest.trim();
                                if msg.is_empty() {
                                    logs.push("usage: /interrupt <message>".to_string());
                                } else {
                                    let req = crate::operator_queue::QueueSubmitRequest {
                                        kind: crate::operator_queue::QueueMessageKind::Steer,
                                        content: msg.to_string(),
                                    };
                                    match queue_tx.send(req) {
                                        Ok(_) => logs.push(
                                            "queued Interrupt: will apply after current tool finishes"
                                                .to_string(),
                                        ),
                                        Err(_) => logs.push(
                                            "queue unavailable: run is ending".to_string(),
                                        ),
                                    }
                                    input_buf.clear();
                                    *input_cursor = 0;
                                    *slash_menu_index = 0;
                                }
                            } else if let Some(rest) = line.strip_prefix("/next ") {
                                let msg = rest.trim();
                                if msg.is_empty() {
                                    logs.push("usage: /next <message>".to_string());
                                } else {
                                    let req = crate::operator_queue::QueueSubmitRequest {
                                        kind: crate::operator_queue::QueueMessageKind::FollowUp,
                                        content: msg.to_string(),
                                    };
                                    match queue_tx.send(req) {
                                        Ok(_) => logs.push(
                                            "queued Next: will apply after this turn completes"
                                                .to_string(),
                                        ),
                                        Err(_) => logs
                                            .push("queue unavailable: run is ending".to_string()),
                                    }
                                    input_buf.clear();
                                    *input_cursor = 0;
                                    *slash_menu_index = 0;
                                }
                            } else if line == "/queue" {
                                let mut rows = active_queue_rows
                                    .iter()
                                    .map(|(id, row)| {
                                        (
                                            row.sequence_no,
                                            id.clone(),
                                            row.kind.clone(),
                                            row.status.clone(),
                                            row.delivery_phrase.clone(),
                                        )
                                    })
                                    .collect::<Vec<_>>();
                                rows.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
                                if rows.is_empty() {
                                    logs.push("queue: empty".to_string());
                                } else {
                                    logs.push(format!("queue: {} item(s)", rows.len()));
                                    for (seq, id, kind, status_row, when) in
                                        rows.into_iter().take(8)
                                    {
                                        let label = match kind.as_str() {
                                            "steer" => "Interrupt",
                                            "follow_up" => "Next",
                                            _ => "Unknown",
                                        };
                                        logs.push(format!(
                                            "  #{seq} {label} [{status_row}] id={id} ({when})"
                                        ));
                                    }
                                }
                                input_buf.clear();
                                *input_cursor = 0;
                                *slash_menu_index = 0;
                            } else if line == "/help" {
                                logs.push(
                                    "active-run commands: /interrupt <message>, /next <message>, /queue ; /learn opens overlay but submit stays blocked while run is active"
                                        .to_string(),
                                );
                                input_buf.clear();
                                *input_cursor = 0;
                                *slash_menu_index = 0;
                            } else if line.starts_with("/learn") {
                                if line == "/learn" {
                                    *learn_overlay = Some(LearnOverlayState::default());
                                    *learn_overlay_cursor = 0;
                                } else {
                                    logs.push("System busy. Operation deferred.".to_string());
                                    logs.push("ERR_TUI_BUSY_TRY_AGAIN".to_string());
                                }
                                input_buf.clear();
                                *input_cursor = 0;
                                *slash_menu_index = 0;
                            } else if !line.is_empty() {
                                logs.push(
                                    "during an active run, supported commands are: /interrupt <message>, /next <message>, /queue, /help"
                                        .to_string(),
                                );
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        if *status == "idle" {
            break;
        }

        let cursor_visible = (*ui_tick / 6) % 2 == 0;
        let learn_overlay_model = learn_overlay.as_ref().map(|s| {
            build_learn_overlay_render_model_with_cursor(s, *learn_overlay_cursor, *ui_tick)
        });
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
                cursor_visible,
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
                if learn_overlay_model.is_some() {
                    None
                } else if palette_open {
                    Some(format!(
                        "⌘ {}  (Up/Down, Enter, Esc)",
                        palette_items[palette_selected]
                    ))
                } else if search_mode {
                    Some(format!(
                        "🔎 {}  (Enter next, Esc close)",
                        render_with_optional_caret(search_query, 0, cursor_visible)
                    ))
                } else if input_buf.starts_with('/') {
                    chat_commands::slash_overlay_text(input_buf, *slash_menu_index)
                } else if input_buf.starts_with('?') {
                    chat_commands::keybinds_overlay_text()
                } else {
                    None
                },
                learn_overlay_model.as_ref(),
            );
        })?;

        let maybe_res = tokio::select! {
            r = &mut fut => Some(r),
            _ = tokio::time::sleep(Duration::from_millis(base_run.tui_refresh_ms)) => None,
        };
        if let Some(res) = maybe_res {
            // Drain any events that arrived between the last try_recv and future completion,
            // so late tool rows (e.g. apply_patch) are not lost from ui_state.
            while let Ok(ev) = rx.try_recv() {
                ui_state.apply_event(&ev);
            }
            match res {
                Ok(out) => {
                    let outcome = out.outcome;
                    let exit_reason = outcome.exit_reason;
                    let outcome_error = outcome.error.unwrap_or_else(String::new);
                    let final_text = if outcome.final_output.is_empty() {
                        streaming_assistant.clone()
                    } else {
                        outcome.final_output
                    };
                    if matches!(exit_reason, AgentExitReason::ProviderError) {
                        let err = if outcome_error.trim().is_empty() {
                            "provider error".to_string()
                        } else {
                            outcome_error.clone()
                        };
                        *provider_connected = false;
                        logs.push(err.clone());
                        if runtime_config::is_timeout_error_text(&err) && !*timeout_notice_active {
                            *timeout_notice_active = true;
                            logs.push(runtime_config::timeout_notice_text(active_run));
                        }
                        *show_logs = true;
                        *status_detail = format!(
                            "{}: {}",
                            exit_reason.as_str(),
                            chat_view_utils::compact_status_detail(&err, 120)
                        );
                        transcript.push(("system".to_string(), format!("Provider error: {err}")));
                        if let Some(hint) = runtime_config::protocol_remediation_hint(&err) {
                            logs.push(hint.clone());
                            transcript.push(("system".to_string(), hint));
                            *show_logs = true;
                        }
                    } else {
                        *provider_connected = true;
                        if matches!(exit_reason, AgentExitReason::Ok) {
                            status_detail.clear();
                        } else {
                            let final_visible =
                                crate::agent_output_sanitize::split_user_visible_and_thinking(
                                    &final_text,
                                )
                                .0;
                            let reason_text = if !outcome_error.trim().is_empty() {
                                outcome_error.clone()
                            } else if !final_visible.trim().is_empty() {
                                final_visible
                            } else {
                                exit_reason.as_str().to_string()
                            };
                            let reason_short =
                                chat_view_utils::compact_status_detail(&reason_text, 120);
                            *status_detail = format!("{}: {}", exit_reason.as_str(), reason_short);
                            transcript.push((
                                "system".to_string(),
                                format!(
                                    "Run ended with {}: {}",
                                    exit_reason.as_str(),
                                    chat_view_utils::compact_status_detail(&reason_text, 220)
                                ),
                            ));
                            if let Some(hint) =
                                runtime_config::protocol_remediation_hint(&reason_text)
                            {
                                logs.push(hint.clone());
                                transcript.push(("system".to_string(), hint));
                                *show_logs = true;
                            }
                        }
                    }
                    if should_render_final_assistant_entry(exit_reason, &final_text) {
                        push_assistant_transcript_entry(
                            transcript,
                            transcript_thinking,
                            &final_text,
                        );
                    }
                    if *follow_output {
                        *transcript_scroll = usize::MAX;
                    }
                }
                Err(e) => {
                    let msg = format!("run failed: {e}");
                    if runtime_config::is_timeout_error_text(&msg) {
                        *provider_connected = false;
                    }
                    logs.push(msg.clone());
                    *show_logs = true;
                    transcript.push(("system".to_string(), msg));
                    *status_detail = format!(
                        "run failed: {}",
                        chat_view_utils::compact_status_detail(&e.to_string(), 120)
                    );
                    if let Some(hint) = runtime_config::protocol_remediation_hint(&format!("{e}")) {
                        logs.push(hint.clone());
                        transcript.push(("system".to_string(), hint));
                        *show_logs = true;
                    }
                    if *follow_output {
                        *transcript_scroll = usize::MAX;
                    }
                }
            }
            streaming_assistant.clear();
            *status = "idle".to_string();
            break;
        }
        *think_tick = think_tick.saturating_add(1);
        *ui_tick = ui_tick.saturating_add(1);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::should_render_final_assistant_entry;
    use crate::agent::AgentExitReason;

    #[test]
    fn renders_final_assistant_entry_only_for_successful_runs() {
        assert!(should_render_final_assistant_entry(
            AgentExitReason::Ok,
            "verified=yes"
        ));
        assert!(!should_render_final_assistant_entry(
            AgentExitReason::PlannerError,
            "verified=yes"
        ));
        assert!(!should_render_final_assistant_entry(
            AgentExitReason::Ok,
            ""
        ));
    }
}

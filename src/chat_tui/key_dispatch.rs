use crossterm::event::{KeyCode, KeyModifiers};

use crate::chat_commands;
use crate::chat_runtime;
use crate::chat_tui::approvals::push_approvals_refresh_error_once;
use crate::chat_tui::event_dispatch::{TuiOuterKeyDispatchInput, TuiOuterKeyDispatchOutcome};
use crate::chat_tui::overlay::{
    assist_summary_stub, build_overlay_promote_submit_line, cycle_overlay_focus,
    overlay_effective_summary, push_overlay_log_dedup, push_overlay_log_unique,
    set_overlay_next_steps_capture, set_overlay_next_steps_promote, LearnOverlayInputFocus,
};
use crate::chat_tui::overlay_input::{overlay_field_mut_and_max, sync_overlay_cursor_to_focus};
use crate::chat_tui::text::{char_len, delete_char_before_cursor, insert_text_bounded};
use crate::trust::approvals::ApprovalsStore;

pub(crate) fn handle_tui_outer_key_dispatch(
    input: TuiOuterKeyDispatchInput<'_>,
) -> TuiOuterKeyDispatchOutcome {
    if (matches!(input.key.code, KeyCode::Char('4'))
        && input.key.modifiers.contains(KeyModifiers::CONTROL))
        || matches!(input.key.code, KeyCode::Char('\u{1c}'))
    {
        *input.show_thinking_panel = !*input.show_thinking_panel;
        return TuiOuterKeyDispatchOutcome::Handled;
    }

    if let Some(overlay) = input.learn_overlay.as_mut() {
        match input.key.code {
            KeyCode::Esc => {
                *input.learn_overlay = None;
                return TuiOuterKeyDispatchOutcome::Handled;
            }
            KeyCode::Char('c') if input.key.modifiers.contains(KeyModifiers::CONTROL) => {
                *input.learn_overlay = None;
                return TuiOuterKeyDispatchOutcome::Handled;
            }
            KeyCode::Char('1') if input.key.modifiers.contains(KeyModifiers::CONTROL) => {
                overlay.tab = crate::chat_ui::LearnOverlayTab::Capture;
                overlay.input_focus = LearnOverlayInputFocus::CaptureSummary;
                *input.learn_overlay_cursor = char_len(&overlay.summary);
                overlay.inline_message = None;
                return TuiOuterKeyDispatchOutcome::Handled;
            }
            KeyCode::Char('2') if input.key.modifiers.contains(KeyModifiers::CONTROL) => {
                overlay.tab = crate::chat_ui::LearnOverlayTab::Review;
                overlay.input_focus = LearnOverlayInputFocus::ReviewId;
                *input.learn_overlay_cursor = char_len(&overlay.review_id);
                overlay.inline_message = None;
                return TuiOuterKeyDispatchOutcome::Handled;
            }
            KeyCode::Char('3') if input.key.modifiers.contains(KeyModifiers::CONTROL) => {
                overlay.tab = crate::chat_ui::LearnOverlayTab::Promote;
                overlay.input_focus = LearnOverlayInputFocus::PromoteId;
                *input.learn_overlay_cursor = char_len(&overlay.promote_id);
                overlay.inline_message = None;
                return TuiOuterKeyDispatchOutcome::Handled;
            }
            KeyCode::Tab => {
                let reverse = input.key.modifiers.contains(KeyModifiers::SHIFT);
                cycle_overlay_focus(overlay, reverse);
                sync_overlay_cursor_to_focus(overlay, input.learn_overlay_cursor);
                return TuiOuterKeyDispatchOutcome::Handled;
            }
            KeyCode::Up => {
                if overlay.tab == crate::chat_ui::LearnOverlayTab::Capture {
                    overlay.category_idx = overlay.category_idx.saturating_sub(1);
                } else if overlay.tab == crate::chat_ui::LearnOverlayTab::Review
                    && !overlay.review_rows.is_empty()
                {
                    overlay.review_selected_idx = overlay.review_selected_idx.saturating_sub(1);
                    if let Some(row) = overlay.review_rows.get(overlay.review_selected_idx) {
                        overlay.review_id = row.split(" | ").next().unwrap_or("").to_string();
                        *input.learn_overlay_cursor = char_len(&overlay.review_id);
                    }
                } else if overlay.tab == crate::chat_ui::LearnOverlayTab::Promote {
                    overlay.promote_target_idx = overlay.promote_target_idx.saturating_sub(1);
                }
                return TuiOuterKeyDispatchOutcome::Handled;
            }
            KeyCode::Down => {
                if overlay.tab == crate::chat_ui::LearnOverlayTab::Capture {
                    overlay.category_idx = (overlay.category_idx + 1).min(2);
                } else if overlay.tab == crate::chat_ui::LearnOverlayTab::Review
                    && !overlay.review_rows.is_empty()
                {
                    overlay.review_selected_idx =
                        (overlay.review_selected_idx + 1).min(overlay.review_rows.len() - 1);
                    if let Some(row) = overlay.review_rows.get(overlay.review_selected_idx) {
                        overlay.review_id = row.split(" | ").next().unwrap_or("").to_string();
                        *input.learn_overlay_cursor = char_len(&overlay.review_id);
                    }
                } else if overlay.tab == crate::chat_ui::LearnOverlayTab::Promote {
                    overlay.promote_target_idx = (overlay.promote_target_idx + 1).min(2);
                }
                return TuiOuterKeyDispatchOutcome::Handled;
            }
            KeyCode::Left => {
                *input.learn_overlay_cursor = input.learn_overlay_cursor.saturating_sub(1);
                return TuiOuterKeyDispatchOutcome::Handled;
            }
            KeyCode::Right => {
                sync_overlay_cursor_to_focus(overlay, input.learn_overlay_cursor);
                *input.learn_overlay_cursor = input.learn_overlay_cursor.saturating_add(1);
                sync_overlay_cursor_to_focus(overlay, input.learn_overlay_cursor);
                return TuiOuterKeyDispatchOutcome::Handled;
            }
            KeyCode::Home => {
                *input.learn_overlay_cursor = 0;
                return TuiOuterKeyDispatchOutcome::Handled;
            }
            KeyCode::End => {
                *input.learn_overlay_cursor = usize::MAX;
                sync_overlay_cursor_to_focus(overlay, input.learn_overlay_cursor);
                return TuiOuterKeyDispatchOutcome::Handled;
            }
            KeyCode::Backspace => {
                let (field, _max_chars, reset_review_select) = overlay_field_mut_and_max(overlay);
                delete_char_before_cursor(field, input.learn_overlay_cursor);
                if reset_review_select {
                    overlay.review_selected_idx = usize::MAX;
                }
                return TuiOuterKeyDispatchOutcome::Handled;
            }
            KeyCode::Char('a') if input.key.modifiers.contains(KeyModifiers::CONTROL) => {
                if overlay.tab == crate::chat_ui::LearnOverlayTab::Capture {
                    overlay.assist_on = !overlay.assist_on;
                    set_overlay_next_steps_capture(overlay);
                } else {
                    overlay.inline_message = None;
                }
                return TuiOuterKeyDispatchOutcome::Handled;
            }
            KeyCode::Char('g') if input.key.modifiers.contains(KeyModifiers::CONTROL) => {
                if overlay.summary.trim().is_empty() {
                    overlay.inline_message =
                        Some("Enter a summary first before asking for an assist.".to_string());
                } else {
                    overlay.assist_summary = Some(assist_summary_stub(&overlay.summary));
                    overlay.summary_choice = crate::chat_ui::LearnOverlaySummaryChoice::Assist;
                    overlay.inline_message =
                        Some("Assist rewrite ready; use Ctrl+O or Ctrl+R to compare.".to_string());
                }
                return TuiOuterKeyDispatchOutcome::Handled;
            }
            KeyCode::Char('o') if input.key.modifiers.contains(KeyModifiers::CONTROL) => {
                overlay.summary_choice = crate::chat_ui::LearnOverlaySummaryChoice::Original;
                overlay.inline_message = Some("Original summary selected.".to_string());
                return TuiOuterKeyDispatchOutcome::Handled;
            }
            KeyCode::Char('r') if input.key.modifiers.contains(KeyModifiers::CONTROL) => {
                if overlay.assist_summary.is_some() {
                    overlay.summary_choice = crate::chat_ui::LearnOverlaySummaryChoice::Assist;
                    overlay.inline_message = Some("Assist summary selected.".to_string());
                } else {
                    overlay.inline_message = Some(
                        "Generate an assist summary with Ctrl+G before selecting it.".to_string(),
                    );
                }
                return TuiOuterKeyDispatchOutcome::Handled;
            }
            KeyCode::Char('w') if input.key.modifiers.contains(KeyModifiers::CONTROL) => {
                overlay.inline_message =
                    Some("Beginner mode: no arm step needed. Press Enter to run.".to_string());
                return TuiOuterKeyDispatchOutcome::Handled;
            }
            KeyCode::Enter => {
                if input.run_busy {
                    push_overlay_log_unique(overlay, "System busy. Operation deferred.");
                    push_overlay_log_unique(overlay, "ERR_TUI_BUSY_TRY_AGAIN");
                    overlay.inline_message = Some("System busy. Operation deferred.".to_string());
                    return TuiOuterKeyDispatchOutcome::Handled;
                }
                return match overlay.tab {
                    crate::chat_ui::LearnOverlayTab::Capture => {
                        if overlay.summary.trim().is_empty() {
                            push_overlay_log_dedup(overlay, "summary: <required>");
                            overlay.inline_message = Some("summary: <required>".to_string());
                            return TuiOuterKeyDispatchOutcome::Handled;
                        }
                        let effective_summary = overlay_effective_summary(overlay);
                        overlay.selected_summary = Some(effective_summary.clone());
                        let category = match overlay.category_idx {
                            0 => "workflow-hint",
                            1 => "prompt-guidance",
                            _ => "check-candidate",
                        };
                        let assist = if overlay.assist_on { " --assist" } else { "" };
                        let write = if overlay.assist_on { " --write" } else { "" };
                        overlay.pending_submit_line = Some(format!(
                            "/learn capture --category {category} --summary \"{}\"{assist}{write}",
                            effective_summary.replace('"', "\\\"")
                        ));
                        set_overlay_next_steps_capture(overlay);
                        TuiOuterKeyDispatchOutcome::Handled
                    }
                    crate::chat_ui::LearnOverlayTab::Review => {
                        if overlay.review_id.trim().is_empty() {
                            let entries =
                                crate::learning::list_learning_entries(&input.paths.state_dir)
                                    .unwrap_or_default();
                            overlay.review_rows = entries
                                .iter()
                                .map(|e| {
                                    format!(
                                        "{} | {} | {}",
                                        e.id,
                                        match e.status {
                                            crate::learning::LearningStatusV1::Captured => {
                                                "captured"
                                            }
                                            crate::learning::LearningStatusV1::Promoted => {
                                                "promoted"
                                            }
                                            crate::learning::LearningStatusV1::Archived => {
                                                "archived"
                                            }
                                        },
                                        e.summary
                                    )
                                })
                                .collect();
                            overlay.review_selected_idx = 0;
                            if let Some(row) = overlay.review_rows.first() {
                                overlay.review_id =
                                    row.split(" | ").next().unwrap_or("").to_string();
                                *input.learn_overlay_cursor = char_len(&overlay.review_id);
                            }
                            overlay.pending_submit_line = Some("/learn list".to_string());
                            overlay.inline_message = Some(
                                "Step 1: Review rows. Step 2: Set review ID (optional). Step 3: Enter to preview."
                                    .to_string(),
                            );
                        } else {
                            overlay.pending_submit_line =
                                Some(format!("/learn show {}", overlay.review_id));
                            overlay.inline_message = Some(
                                "Step 1: Review output in logs. Step 2: adjust ID if needed. Step 3: Enter again."
                                    .to_string(),
                            );
                        }
                        TuiOuterKeyDispatchOutcome::Handled
                    }
                    crate::chat_ui::LearnOverlayTab::Promote => {
                        match build_overlay_promote_submit_line(overlay) {
                            Ok(line) => {
                                overlay.pending_submit_line = Some(line);
                                set_overlay_next_steps_promote(overlay);
                            }
                            Err(msg) => {
                                push_overlay_log_dedup(overlay, &msg);
                                overlay.inline_message = Some(msg);
                                return TuiOuterKeyDispatchOutcome::Handled;
                            }
                        }
                        TuiOuterKeyDispatchOutcome::Handled
                    }
                };
            }
            KeyCode::Char(c) if chat_runtime::is_text_input_mods(input.key.modifiers) => {
                let (field, max_chars, reset_review_select) = overlay_field_mut_and_max(overlay);
                insert_text_bounded(field, input.learn_overlay_cursor, &c.to_string(), max_chars);
                if overlay.input_focus == LearnOverlayInputFocus::CaptureSummary {
                    overlay.assist_summary = None;
                    overlay.summary_choice = crate::chat_ui::LearnOverlaySummaryChoice::Original;
                    overlay.selected_summary = None;
                }
                if reset_review_select {
                    overlay.review_selected_idx = usize::MAX;
                }
                return TuiOuterKeyDispatchOutcome::Handled;
            }
            _ => return TuiOuterKeyDispatchOutcome::Handled,
        }
    }

    if *input.palette_open {
        match input.key.code {
            KeyCode::Esc => *input.palette_open = false,
            KeyCode::Up => {
                *input.palette_selected = input.palette_selected.saturating_sub(1);
            }
            KeyCode::Down => {
                if *input.palette_selected + 1 < input.palette_items.len() {
                    *input.palette_selected += 1;
                }
            }
            KeyCode::Enter => {
                match *input.palette_selected {
                    0 => *input.show_tools = !*input.show_tools,
                    1 => *input.show_approvals = !*input.show_approvals,
                    2 => *input.show_logs = !*input.show_logs,
                    3 => *input.compact_tools = !*input.compact_tools,
                    4 => {
                        input.transcript.clear();
                        input.transcript_thinking.clear();
                        input.ui_state.tool_calls.clear();
                        input.streaming_assistant.clear();
                        *input.transcript_scroll = 0;
                        *input.follow_output = true;
                    }
                    5 => {
                        *input.follow_output = true;
                        *input.transcript_scroll = usize::MAX;
                    }
                    _ => {}
                }
                *input.palette_open = false;
            }
            _ => {}
        }
        return TuiOuterKeyDispatchOutcome::ContinueLoop;
    }
    if *input.search_mode {
        let mut do_search = false;
        match input.key.code {
            KeyCode::Esc => *input.search_mode = false,
            KeyCode::Backspace => {
                delete_char_before_cursor(input.search_query, input.search_input_cursor);
                *input.search_line_cursor = 0;
                do_search = true;
            }
            KeyCode::Left => {
                *input.search_input_cursor = input.search_input_cursor.saturating_sub(1);
            }
            KeyCode::Right => {
                *input.search_input_cursor =
                    (*input.search_input_cursor + 1).min(char_len(input.search_query));
            }
            KeyCode::Enter => {
                do_search = true;
                *input.search_line_cursor = input.search_line_cursor.saturating_add(1);
            }
            KeyCode::Char(c) if chat_runtime::is_text_input_mods(input.key.modifiers) => {
                insert_text_bounded(
                    input.search_query,
                    input.search_input_cursor,
                    &c.to_string(),
                    usize::MAX,
                );
                *input.search_line_cursor = 0;
                do_search = true;
            }
            _ => {}
        }
        if do_search && !input.search_query.is_empty() {
            let hay = input
                .transcript
                .iter()
                .map(|(role, text)| format!("{}: {}", role.to_uppercase(), text))
                .collect::<Vec<_>>()
                .join("\n\n");
            let lines: Vec<&str> = hay.lines().collect();
            let query = input.search_query.to_lowercase();
            let mut found = None;
            for (idx, line) in lines.iter().enumerate().skip(*input.search_line_cursor) {
                if line.to_lowercase().contains(&query) {
                    found = Some(idx);
                    break;
                }
            }
            if found.is_none() {
                for (idx, line) in lines.iter().enumerate().take(*input.search_line_cursor) {
                    if line.to_lowercase().contains(&query) {
                        found = Some(idx);
                        break;
                    }
                }
            }
            if let Some(idx) = found {
                *input.transcript_scroll = idx;
                *input.follow_output = false;
                *input.search_line_cursor = idx;
            }
        }
        return TuiOuterKeyDispatchOutcome::ContinueLoop;
    }

    match input.key.code {
        KeyCode::Char('c') if input.key.modifiers.contains(KeyModifiers::CONTROL) => {
            TuiOuterKeyDispatchOutcome::BreakLoop
        }
        KeyCode::Up => {
            if input.input.starts_with('/') {
                let matches_len = chat_commands::slash_match_count(input.input);
                if matches_len > 0 {
                    *input.slash_menu_index = if *input.slash_menu_index == 0 {
                        matches_len - 1
                    } else {
                        *input.slash_menu_index - 1
                    };
                }
                return TuiOuterKeyDispatchOutcome::ContinueLoop;
            }
            if !input.prompt_history.is_empty() {
                let next = match *input.history_idx {
                    None => input.prompt_history.len().saturating_sub(1),
                    Some(i) => i.saturating_sub(1),
                };
                *input.history_idx = Some(next);
                *input.input = input.prompt_history[next].clone();
                *input.input_cursor = char_len(input.input);
            }
            TuiOuterKeyDispatchOutcome::Handled
        }
        KeyCode::Down => {
            if input.input.starts_with('/') {
                let matches_len = chat_commands::slash_match_count(input.input);
                if matches_len > 0 {
                    *input.slash_menu_index = (*input.slash_menu_index + 1) % matches_len;
                }
                return TuiOuterKeyDispatchOutcome::ContinueLoop;
            }
            if !input.prompt_history.is_empty() {
                if let Some(i) = *input.history_idx {
                    let next = (i + 1).min(input.prompt_history.len());
                    if next >= input.prompt_history.len() {
                        *input.history_idx = None;
                        input.input.clear();
                        *input.input_cursor = 0;
                    } else {
                        *input.history_idx = Some(next);
                        *input.input = input.prompt_history[next].clone();
                        *input.input_cursor = char_len(input.input);
                    }
                }
            }
            TuiOuterKeyDispatchOutcome::Handled
        }
        KeyCode::PageUp => {
            let max_scroll = chat_runtime::transcript_max_scroll_lines(
                input.transcript,
                input.streaming_assistant,
            );
            *input.transcript_scroll =
                chat_runtime::adjust_transcript_scroll(*input.transcript_scroll, -12, max_scroll);
            *input.follow_output = false;
            TuiOuterKeyDispatchOutcome::Handled
        }
        KeyCode::PageDown => {
            let max_scroll = chat_runtime::transcript_max_scroll_lines(
                input.transcript,
                input.streaming_assistant,
            );
            *input.transcript_scroll =
                chat_runtime::adjust_transcript_scroll(*input.transcript_scroll, 12, max_scroll);
            *input.follow_output = false;
            TuiOuterKeyDispatchOutcome::Handled
        }
        KeyCode::Char('u') if input.key.modifiers.contains(KeyModifiers::CONTROL) => {
            let max_scroll = chat_runtime::transcript_max_scroll_lines(
                input.transcript,
                input.streaming_assistant,
            );
            *input.transcript_scroll =
                chat_runtime::adjust_transcript_scroll(*input.transcript_scroll, -10, max_scroll);
            *input.follow_output = false;
            TuiOuterKeyDispatchOutcome::Handled
        }
        KeyCode::Char('d') if input.key.modifiers.contains(KeyModifiers::CONTROL) => {
            let max_scroll = chat_runtime::transcript_max_scroll_lines(
                input.transcript,
                input.streaming_assistant,
            );
            *input.transcript_scroll =
                chat_runtime::adjust_transcript_scroll(*input.transcript_scroll, 10, max_scroll);
            *input.follow_output = false;
            TuiOuterKeyDispatchOutcome::Handled
        }
        KeyCode::Char('t') if input.key.modifiers.contains(KeyModifiers::CONTROL) => {
            *input.show_tools = !*input.show_tools;
            TuiOuterKeyDispatchOutcome::Handled
        }
        KeyCode::Char('y') if input.key.modifiers.contains(KeyModifiers::CONTROL) => {
            *input.show_approvals = !*input.show_approvals;
            TuiOuterKeyDispatchOutcome::Handled
        }
        KeyCode::Char('g') if input.key.modifiers.contains(KeyModifiers::CONTROL) => {
            *input.show_logs = !*input.show_logs;
            TuiOuterKeyDispatchOutcome::Handled
        }
        KeyCode::Tab => {
            if *input.show_tools && (*input.show_approvals) {
                *input.tools_focus = !*input.tools_focus;
            }
            TuiOuterKeyDispatchOutcome::Handled
        }
        KeyCode::Char('1') if input.key.modifiers.contains(KeyModifiers::CONTROL) => {
            *input.show_tools = !*input.show_tools;
            TuiOuterKeyDispatchOutcome::Handled
        }
        KeyCode::Char('2') if input.key.modifiers.contains(KeyModifiers::CONTROL) => {
            *input.show_approvals = !*input.show_approvals;
            TuiOuterKeyDispatchOutcome::Handled
        }
        KeyCode::Char('3') if input.key.modifiers.contains(KeyModifiers::CONTROL) => {
            *input.show_logs = !*input.show_logs;
            TuiOuterKeyDispatchOutcome::Handled
        }
        KeyCode::Char('j') if input.key.modifiers.contains(KeyModifiers::CONTROL) => {
            if *input.show_tools && (!*input.show_approvals || *input.tools_focus) {
                if *input.tools_selected + 1 < input.visible_tool_count {
                    *input.tools_selected += 1;
                }
            } else if *input.approvals_selected + 1 < input.ui_state.pending_approvals.len() {
                *input.approvals_selected += 1;
            }
            TuiOuterKeyDispatchOutcome::Handled
        }
        KeyCode::Char('k') if input.key.modifiers.contains(KeyModifiers::CONTROL) => {
            if *input.show_tools && (!*input.show_approvals || *input.tools_focus) {
                *input.tools_selected = input.tools_selected.saturating_sub(1);
            } else {
                *input.approvals_selected = input.approvals_selected.saturating_sub(1);
            }
            TuiOuterKeyDispatchOutcome::Handled
        }
        KeyCode::Char('r') if input.key.modifiers.contains(KeyModifiers::CONTROL) => {
            let before_pending = input.ui_state.pending_approval_count();
            if let Err(e) = input
                .ui_state
                .refresh_approvals(&input.paths.approvals_path)
            {
                push_approvals_refresh_error_once(input.logs, &e);
            } else {
                let now_pending = input.ui_state.pending_approval_count();
                if before_pending == 0 && now_pending > 0 {
                    *input.show_approvals = true;
                }
            }
            TuiOuterKeyDispatchOutcome::Handled
        }
        KeyCode::Char('a') if input.key.modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(row) = input
                .ui_state
                .pending_approvals
                .get(*input.approvals_selected)
            {
                let store = ApprovalsStore::new(input.paths.approvals_path.clone());
                if let Err(e) = store.approve(&row.id, None, None) {
                    input.logs.push(format!("approve failed: {e}"));
                } else {
                    input.logs.push(format!("approved {}", row.id));
                }
                let before_pending = input.ui_state.pending_approval_count();
                if let Err(e) = input
                    .ui_state
                    .refresh_approvals(&input.paths.approvals_path)
                {
                    push_approvals_refresh_error_once(input.logs, &e);
                } else {
                    let now_pending = input.ui_state.pending_approval_count();
                    if before_pending == 0 && now_pending > 0 {
                        *input.show_approvals = true;
                    }
                }
            }
            TuiOuterKeyDispatchOutcome::Handled
        }
        KeyCode::Char('x') if input.key.modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(row) = input
                .ui_state
                .pending_approvals
                .get(*input.approvals_selected)
            {
                let store = ApprovalsStore::new(input.paths.approvals_path.clone());
                if let Err(e) = store.deny(&row.id) {
                    input.logs.push(format!("deny failed: {e}"));
                } else {
                    input.logs.push(format!("denied {}", row.id));
                }
                let before_pending = input.ui_state.pending_approval_count();
                if let Err(e) = input
                    .ui_state
                    .refresh_approvals(&input.paths.approvals_path)
                {
                    push_approvals_refresh_error_once(input.logs, &e);
                } else {
                    let now_pending = input.ui_state.pending_approval_count();
                    if before_pending == 0 && now_pending > 0 {
                        *input.show_approvals = true;
                    }
                }
            }
            TuiOuterKeyDispatchOutcome::Handled
        }
        KeyCode::Enter => TuiOuterKeyDispatchOutcome::EnterInline,
        KeyCode::Left => {
            *input.input_cursor = input.input_cursor.saturating_sub(1);
            TuiOuterKeyDispatchOutcome::Handled
        }
        KeyCode::Right => {
            *input.input_cursor = (*input.input_cursor + 1).min(char_len(input.input));
            TuiOuterKeyDispatchOutcome::Handled
        }
        KeyCode::Backspace => {
            delete_char_before_cursor(input.input, input.input_cursor);
            *input.slash_menu_index = 0;
            TuiOuterKeyDispatchOutcome::Handled
        }
        KeyCode::Char(c) => {
            if chat_runtime::is_text_input_mods(input.key.modifiers) {
                insert_text_bounded(input.input, input.input_cursor, &c.to_string(), usize::MAX);
                if c == '/' && input.input.len() == 1 {
                    *input.slash_menu_index = 0;
                }
            }
            TuiOuterKeyDispatchOutcome::Handled
        }
        _ => TuiOuterKeyDispatchOutcome::Handled,
    }
}

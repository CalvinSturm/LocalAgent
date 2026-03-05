use crossterm::event::{Event as CEvent, KeyCode, KeyEventKind, KeyModifiers};

use crate::chat_tui::overlay::LearnOverlayState;
use crate::chat_tui::overlay_input::{
    handle_tui_outer_mouse_event, handle_tui_outer_paste_event, TuiOuterMouseInput,
    TuiOuterPasteInput,
};
use crate::chat_tui::text::char_len;
use crate::store;
use crate::tui::state::UiState;

pub(crate) enum TuiOuterKeyPreludeOutcome {
    BreakLoop,
    ContinueLoop,
    Proceed,
}

pub(crate) enum TuiOuterKeyDispatchOutcome {
    BreakLoop,
    ContinueLoop,
    Handled,
    EnterInline,
}

pub(crate) enum TuiOuterEventDispatchOutcome {
    BreakLoop,
    ContinueLoop,
    EnterInline,
    HandledKey,
    Noop,
}

pub(crate) struct TuiOuterKeyPreludeInput<'a> {
    pub(crate) key: crossterm::event::KeyEvent,
    pub(crate) learn_overlay: &'a mut Option<LearnOverlayState>,
    pub(crate) palette_open: &'a mut bool,
    pub(crate) search_mode: &'a mut bool,
    pub(crate) search_query: &'a String,
    pub(crate) search_input_cursor: &'a mut usize,
    pub(crate) follow_output: &'a mut bool,
    pub(crate) transcript_scroll: &'a mut usize,
}

pub(crate) fn handle_tui_outer_key_prelude(
    input: TuiOuterKeyPreludeInput<'_>,
) -> TuiOuterKeyPreludeOutcome {
    if !matches!(input.key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return TuiOuterKeyPreludeOutcome::ContinueLoop;
    }
    if input.learn_overlay.is_some() {
        return TuiOuterKeyPreludeOutcome::Proceed;
    }
    if input.key.code == KeyCode::Esc {
        return TuiOuterKeyPreludeOutcome::BreakLoop;
    }
    if input.key.code == KeyCode::End {
        *input.follow_output = true;
        *input.transcript_scroll = usize::MAX;
        return TuiOuterKeyPreludeOutcome::ContinueLoop;
    }
    if input.key.code == KeyCode::Char('p') && input.key.modifiers.contains(KeyModifiers::CONTROL) {
        *input.palette_open = !*input.palette_open;
        *input.search_mode = false;
        return TuiOuterKeyPreludeOutcome::ContinueLoop;
    }
    if input.key.code == KeyCode::Char('f') && input.key.modifiers.contains(KeyModifiers::CONTROL) {
        *input.search_mode = true;
        *input.search_input_cursor = char_len(input.search_query);
        *input.palette_open = false;
        return TuiOuterKeyPreludeOutcome::ContinueLoop;
    }
    TuiOuterKeyPreludeOutcome::Proceed
}

pub(crate) struct TuiOuterKeyDispatchInput<'a> {
    pub(crate) key: crossterm::event::KeyEvent,
    pub(crate) learn_overlay: &'a mut Option<LearnOverlayState>,
    pub(crate) run_busy: bool,
    pub(crate) input: &'a mut String,
    pub(crate) input_cursor: &'a mut usize,
    pub(crate) prompt_history: &'a mut Vec<String>,
    pub(crate) history_idx: &'a mut Option<usize>,
    pub(crate) slash_menu_index: &'a mut usize,
    pub(crate) palette_open: &'a mut bool,
    pub(crate) palette_items: &'a [&'a str],
    pub(crate) palette_selected: &'a mut usize,
    pub(crate) search_mode: &'a mut bool,
    pub(crate) search_query: &'a mut String,
    pub(crate) search_line_cursor: &'a mut usize,
    pub(crate) search_input_cursor: &'a mut usize,
    pub(crate) transcript: &'a mut Vec<(String, String)>,
    pub(crate) transcript_thinking: &'a mut std::collections::BTreeMap<usize, String>,
    pub(crate) show_thinking_panel: &'a mut bool,
    pub(crate) streaming_assistant: &'a mut String,
    pub(crate) transcript_scroll: &'a mut usize,
    pub(crate) follow_output: &'a mut bool,
    pub(crate) ui_state: &'a mut UiState,
    pub(crate) visible_tool_count: usize,
    pub(crate) show_tools: &'a mut bool,
    pub(crate) show_approvals: &'a mut bool,
    pub(crate) show_logs: &'a mut bool,
    pub(crate) compact_tools: &'a mut bool,
    pub(crate) tools_selected: &'a mut usize,
    pub(crate) tools_focus: &'a mut bool,
    pub(crate) approvals_selected: &'a mut usize,
    pub(crate) paths: &'a store::StatePaths,
    pub(crate) logs: &'a mut Vec<String>,
    pub(crate) learn_overlay_cursor: &'a mut usize,
}

pub(crate) struct TuiOuterEventDispatchInput<'a> {
    pub(crate) event: CEvent,
    pub(crate) status: &'a str,
    pub(crate) prompt_history: &'a mut Vec<String>,
    pub(crate) transcript: &'a mut Vec<(String, String)>,
    pub(crate) transcript_thinking: &'a mut std::collections::BTreeMap<usize, String>,
    pub(crate) show_thinking_panel: &'a mut bool,
    pub(crate) streaming_assistant: &'a mut String,
    pub(crate) transcript_scroll: &'a mut usize,
    pub(crate) follow_output: &'a mut bool,
    pub(crate) input: &'a mut String,
    pub(crate) input_cursor: &'a mut usize,
    pub(crate) history_idx: &'a mut Option<usize>,
    pub(crate) slash_menu_index: &'a mut usize,
    pub(crate) palette_open: &'a mut bool,
    pub(crate) palette_items: &'a [&'a str],
    pub(crate) palette_selected: &'a mut usize,
    pub(crate) search_mode: &'a mut bool,
    pub(crate) search_query: &'a mut String,
    pub(crate) search_line_cursor: &'a mut usize,
    pub(crate) search_input_cursor: &'a mut usize,
    pub(crate) ui_state: &'a mut UiState,
    pub(crate) visible_tool_count: usize,
    pub(crate) show_tools: &'a mut bool,
    pub(crate) show_approvals: &'a mut bool,
    pub(crate) show_logs: &'a mut bool,
    pub(crate) compact_tools: &'a mut bool,
    pub(crate) tools_selected: &'a mut usize,
    pub(crate) tools_focus: &'a mut bool,
    pub(crate) approvals_selected: &'a mut usize,
    pub(crate) paths: &'a store::StatePaths,
    pub(crate) logs: &'a mut Vec<String>,
    pub(crate) learn_overlay: &'a mut Option<LearnOverlayState>,
    pub(crate) learn_overlay_cursor: &'a mut usize,
}

pub(crate) fn handle_tui_outer_event_dispatch(
    input: TuiOuterEventDispatchInput<'_>,
    key_dispatch: impl FnOnce(TuiOuterKeyDispatchInput<'_>) -> TuiOuterKeyDispatchOutcome,
) -> TuiOuterEventDispatchOutcome {
    match input.event {
        CEvent::Mouse(me) => {
            handle_tui_outer_mouse_event(TuiOuterMouseInput {
                me: &me,
                transcript: input.transcript,
                streaming_assistant: input.streaming_assistant,
                transcript_scroll: input.transcript_scroll,
                follow_output: input.follow_output,
            });
            TuiOuterEventDispatchOutcome::Noop
        }
        CEvent::Paste(pasted) => {
            handle_tui_outer_paste_event(TuiOuterPasteInput {
                pasted: &pasted,
                input: input.input,
                input_cursor: input.input_cursor,
                history_idx: input.history_idx,
                slash_menu_index: input.slash_menu_index,
                learn_overlay: input.learn_overlay,
                learn_overlay_cursor: input.learn_overlay_cursor,
            });
            TuiOuterEventDispatchOutcome::Noop
        }
        CEvent::Key(key) => {
            match handle_tui_outer_key_prelude(TuiOuterKeyPreludeInput {
                key,
                learn_overlay: input.learn_overlay,
                palette_open: input.palette_open,
                search_mode: input.search_mode,
                search_query: input.search_query,
                search_input_cursor: input.search_input_cursor,
                follow_output: input.follow_output,
                transcript_scroll: input.transcript_scroll,
            }) {
                TuiOuterKeyPreludeOutcome::BreakLoop => {
                    return TuiOuterEventDispatchOutcome::BreakLoop
                }
                TuiOuterKeyPreludeOutcome::ContinueLoop => {
                    return TuiOuterEventDispatchOutcome::ContinueLoop
                }
                TuiOuterKeyPreludeOutcome::Proceed => {}
            }
            match key_dispatch(TuiOuterKeyDispatchInput {
                key,
                learn_overlay: input.learn_overlay,
                run_busy: input.status == "running",
                input: input.input,
                input_cursor: input.input_cursor,
                prompt_history: input.prompt_history,
                history_idx: input.history_idx,
                slash_menu_index: input.slash_menu_index,
                palette_open: input.palette_open,
                palette_items: input.palette_items,
                palette_selected: input.palette_selected,
                search_mode: input.search_mode,
                search_query: input.search_query,
                search_line_cursor: input.search_line_cursor,
                search_input_cursor: input.search_input_cursor,
                transcript: input.transcript,
                transcript_thinking: input.transcript_thinking,
                show_thinking_panel: input.show_thinking_panel,
                streaming_assistant: input.streaming_assistant,
                transcript_scroll: input.transcript_scroll,
                follow_output: input.follow_output,
                ui_state: input.ui_state,
                visible_tool_count: input.visible_tool_count,
                show_tools: input.show_tools,
                show_approvals: input.show_approvals,
                show_logs: input.show_logs,
                compact_tools: input.compact_tools,
                tools_selected: input.tools_selected,
                tools_focus: input.tools_focus,
                approvals_selected: input.approvals_selected,
                paths: input.paths,
                logs: input.logs,
                learn_overlay_cursor: input.learn_overlay_cursor,
            }) {
                TuiOuterKeyDispatchOutcome::BreakLoop => TuiOuterEventDispatchOutcome::BreakLoop,
                TuiOuterKeyDispatchOutcome::ContinueLoop => {
                    TuiOuterEventDispatchOutcome::ContinueLoop
                }
                TuiOuterKeyDispatchOutcome::Handled => TuiOuterEventDispatchOutcome::HandledKey,
                TuiOuterKeyDispatchOutcome::EnterInline => {
                    TuiOuterEventDispatchOutcome::EnterInline
                }
            }
        }
        _ => TuiOuterEventDispatchOutcome::Noop,
    }
}

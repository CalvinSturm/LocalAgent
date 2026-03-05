use crate::chat_tui::overlay::{learn_overlay_focus_label, LearnOverlayState};
use crate::chat_tui::text::render_with_optional_caret;
use crate::gate::ProviderKind;
use crate::tui::state::UiState;
use crate::RunArgs;

pub(crate) struct TuiRenderFrameInput<'a> {
    pub(crate) mode_label: String,
    pub(crate) provider_label: &'a str,
    pub(crate) provider_connected: bool,
    pub(crate) model: &'a str,
    pub(crate) status: &'a str,
    pub(crate) status_detail: &'a str,
    pub(crate) transcript: &'a Vec<(String, String)>,
    pub(crate) transcript_thinking: &'a std::collections::BTreeMap<usize, String>,
    pub(crate) show_thinking_panel: bool,
    pub(crate) streaming_assistant: &'a str,
    pub(crate) ui_state: &'a UiState,
    pub(crate) tools_selected: usize,
    pub(crate) tools_focus: bool,
    pub(crate) show_tool_details: bool,
    pub(crate) approvals_selected: usize,
    pub(crate) cwd_label: &'a str,
    pub(crate) input: &'a str,
    pub(crate) input_cursor: usize,
    pub(crate) input_cursor_visible: bool,
    pub(crate) logs: &'a Vec<String>,
    pub(crate) think_tick: u64,
    pub(crate) tui_refresh_ms: u64,
    pub(crate) show_tools: bool,
    pub(crate) show_approvals: bool,
    pub(crate) show_logs: bool,
    pub(crate) transcript_scroll: usize,
    pub(crate) compact_tools: bool,
    pub(crate) show_banner: bool,
    pub(crate) ui_tick: u64,
    pub(crate) overlay_text: Option<String>,
    pub(crate) learn_overlay: Option<crate::chat_ui::LearnOverlayRenderModel>,
}

pub(crate) struct TuiRenderFrameBuildInput<'a> {
    pub(crate) active_run: &'a RunArgs,
    pub(crate) provider_kind: ProviderKind,
    pub(crate) provider_connected: bool,
    pub(crate) model: &'a str,
    pub(crate) status: &'a str,
    pub(crate) status_detail: &'a str,
    pub(crate) transcript: &'a Vec<(String, String)>,
    pub(crate) transcript_thinking: &'a std::collections::BTreeMap<usize, String>,
    pub(crate) show_thinking_panel: bool,
    pub(crate) streaming_assistant: &'a str,
    pub(crate) ui_state: &'a UiState,
    pub(crate) tools_selected: &'a mut usize,
    pub(crate) tools_focus: &'a mut bool,
    pub(crate) show_tool_details: &'a mut bool,
    pub(crate) approvals_selected: &'a mut usize,
    pub(crate) cwd_label: &'a str,
    pub(crate) input: &'a str,
    pub(crate) input_cursor: usize,
    pub(crate) logs: &'a Vec<String>,
    pub(crate) think_tick: u64,
    pub(crate) tui_refresh_ms: u64,
    pub(crate) show_tools: &'a mut bool,
    pub(crate) show_approvals: &'a mut bool,
    pub(crate) show_logs: bool,
    pub(crate) transcript_scroll: usize,
    pub(crate) compact_tools: bool,
    pub(crate) show_banner: bool,
    pub(crate) ui_tick: u64,
    pub(crate) palette_open: bool,
    pub(crate) palette_items: &'a [&'a str],
    pub(crate) palette_selected: usize,
    pub(crate) search_mode: bool,
    pub(crate) search_query: &'a str,
    pub(crate) search_input_cursor: usize,
    pub(crate) slash_menu_index: usize,
    pub(crate) learn_overlay: &'a Option<LearnOverlayState>,
    pub(crate) learn_overlay_cursor: usize,
}

pub(crate) fn build_tui_render_frame_input(
    input: TuiRenderFrameBuildInput<'_>,
) -> TuiRenderFrameInput<'_> {
    let tool_row_count = if input.compact_tools { 20 } else { 12 };
    let visible_tool_count = input.ui_state.tool_calls.len().min(tool_row_count);
    if visible_tool_count == 0 {
        *input.tools_selected = 0;
        *input.show_tool_details = false;
    } else {
        *input.tools_selected = (*input.tools_selected).min(visible_tool_count.saturating_sub(1));
    }
    if input.ui_state.pending_approvals.is_empty() {
        *input.approvals_selected = 0;
    } else {
        *input.approvals_selected = (*input.approvals_selected)
            .min(input.ui_state.pending_approvals.len().saturating_sub(1));
    }
    if *input.show_tools && !*input.show_approvals {
        *input.tools_focus = true;
    } else if *input.show_approvals && !*input.show_tools {
        *input.tools_focus = false;
    }
    if !*input.show_tools {
        *input.show_tool_details = false;
    }

    let overlay_text = if input.learn_overlay.is_some() {
        None
    } else if input.palette_open {
        Some(format!(
            "⌘ {}  (Up/Down, Enter, Esc)",
            input.palette_items[input.palette_selected]
        ))
    } else if input.search_mode {
        Some(format!(
            "🔎 {}  (Enter next, Esc close)",
            render_with_optional_caret(input.search_query, input.search_input_cursor, true)
        ))
    } else if input.input.starts_with('/') {
        crate::chat_commands::slash_overlay_text(input.input, input.slash_menu_index)
    } else if input.input.starts_with('?') {
        crate::chat_commands::keybinds_overlay_text()
    } else {
        None
    };

    let learn_overlay = input.learn_overlay.as_ref().map(|s| {
        build_learn_overlay_render_model_with_cursor(s, input.learn_overlay_cursor, input.ui_tick)
    });

    TuiRenderFrameInput {
        mode_label: crate::chat_runtime::chat_mode_display_label(input.active_run),
        provider_label: crate::provider_runtime::provider_cli_name(input.provider_kind),
        provider_connected: input.provider_connected,
        model: input.model,
        status: input.status,
        status_detail: input.status_detail,
        transcript: input.transcript,
        transcript_thinking: input.transcript_thinking,
        show_thinking_panel: input.show_thinking_panel,
        streaming_assistant: input.streaming_assistant,
        ui_state: input.ui_state,
        tools_selected: *input.tools_selected,
        tools_focus: *input.tools_focus,
        show_tool_details: *input.show_tool_details,
        approvals_selected: *input.approvals_selected,
        cwd_label: input.cwd_label,
        input: input.input,
        input_cursor: input.input_cursor,
        input_cursor_visible: (input.ui_tick / 6).is_multiple_of(2),
        logs: input.logs,
        think_tick: input.think_tick,
        tui_refresh_ms: input.tui_refresh_ms,
        show_tools: *input.show_tools,
        show_approvals: *input.show_approvals,
        show_logs: input.show_logs,
        transcript_scroll: input.transcript_scroll,
        compact_tools: input.compact_tools,
        show_banner: input.show_banner,
        ui_tick: input.ui_tick,
        overlay_text,
        learn_overlay,
    }
}

#[cfg(test)]
pub(crate) fn build_learn_overlay_render_model(
    s: &LearnOverlayState,
) -> crate::chat_ui::LearnOverlayRenderModel {
    build_learn_overlay_render_model_with_cursor(s, 0, 0)
}

pub(crate) fn build_learn_overlay_render_model_with_cursor(
    s: &LearnOverlayState,
    active_input_cursor: usize,
    ui_tick: u64,
) -> crate::chat_ui::LearnOverlayRenderModel {
    let (equivalent_cli, target_path) = match s.tab {
        crate::chat_ui::LearnOverlayTab::Capture => {
            let category = match s.category_idx {
                0 => "workflow_hint",
                1 => "prompt_guidance",
                _ => "check_candidate",
            };
            let mut cli = format!("learn capture --category {category} --summary ");
            if s.summary.trim().is_empty() {
                cli.push_str("<required>");
            } else {
                cli.push('"');
                cli.push_str(&s.summary);
                cli.push('"');
            }
            if s.assist_on {
                cli.push_str(" --assist");
            }
            (cli, "N/A".to_string())
        }
        crate::chat_ui::LearnOverlayTab::Review => {
            let cli = if s.review_id.trim().is_empty() {
                "learn list".to_string()
            } else {
                format!("learn show {}", s.review_id)
            };
            (cli, "N/A".to_string())
        }
        crate::chat_ui::LearnOverlayTab::Promote => {
            let target = match s.promote_target_idx {
                0 => "check",
                1 => "pack",
                _ => "agents",
            };
            let mut cli = if s.promote_id.trim().is_empty() {
                format!("learn promote <required_id> --to {target}")
            } else {
                format!("learn promote {} --to {target}", s.promote_id)
            };
            if target == "check" {
                if s.promote_slug.trim().is_empty() {
                    cli.push_str(" --slug <required>");
                } else {
                    cli.push_str(&format!(" --slug {}", s.promote_slug));
                }
            }
            if target == "pack" {
                if s.promote_pack_id.trim().is_empty() {
                    cli.push_str(" --pack-id <required>");
                } else {
                    cli.push_str(&format!(" --pack-id {}", s.promote_pack_id));
                }
            }
            if s.promote_force {
                cli.push_str(" --force");
            }
            let target_path = match target {
                "check" => ".localagent/checks/<slug>.md",
                "pack" => ".localagent/packs/<pack_id>/PACK.md",
                _ => "AGENTS.md",
            };
            (cli, target_path.to_string())
        }
    };
    crate::chat_ui::LearnOverlayRenderModel {
        tab: s.tab,
        selected_category_idx: s.category_idx,
        summary: s.summary.clone(),
        review_id: s.review_id.clone(),
        promote_id: s.promote_id.clone(),
        promote_target_idx: s.promote_target_idx,
        promote_slug: s.promote_slug.clone(),
        promote_pack_id: s.promote_pack_id.clone(),
        promote_force: s.promote_force,
        input_focus: learn_overlay_focus_label(s.input_focus).to_string(),
        inline_message: s.inline_message.clone(),
        review_rows: s.review_rows.clone(),
        review_selected_idx: s.review_selected_idx,
        assist_on: s.assist_on,
        equivalent_cli,
        target_path,
        overlay_logs: s.logs.clone(),
        assist_summary: s.assist_summary.clone(),
        summary_choice: s.summary_choice,
        selected_summary: s.selected_summary.clone(),
        active_input_cursor,
        cursor_visible: (ui_tick / 6).is_multiple_of(2),
    }
}

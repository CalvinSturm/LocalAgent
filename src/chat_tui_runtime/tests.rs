    use crate::chat_tui::approvals::refresh_approvals_with_auto_open;
    use crate::chat_tui::event_dispatch::{TuiOuterKeyDispatchInput, TuiOuterKeyDispatchOutcome};
    use crate::chat_tui::key_dispatch::handle_tui_outer_key_dispatch;
    use crate::chat_tui::overlay::{
        build_overlay_promote_submit_line, cycle_overlay_focus, LearnOverlayInputFocus,
        LearnOverlayState,
    };
    use crate::chat_tui::overlay_input::{
        handle_tui_outer_paste_event, TuiOuterPasteInput, OVERLAY_CAPTURE_SUMMARY_MAX_CHARS,
    };
    use crate::chat_tui::render_model::build_learn_overlay_render_model;
    use crate::trust::approvals::ApprovalsStore;
    use crate::chat_ui::LearnOverlayTab;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use tempfile::tempdir;

    #[test]
    fn learn_overlay_preview_mode_shows_no_writes() {
        let s = LearnOverlayState {
            tab: LearnOverlayTab::Capture,
            category_idx: 0,
            summary: String::new(),
            review_id: String::new(),
            promote_id: String::new(),
            promote_target_idx: 0,
            promote_slug: String::new(),
            promote_pack_id: String::new(),
            promote_force: false,
            input_focus: LearnOverlayInputFocus::CaptureSummary,
            inline_message: None,
            review_rows: Vec::new(),
            review_selected_idx: 0,
            assist_on: true,
            write_armed: false,
            logs: vec![],
            pending_submit_line: None,
            assist_summary: None,
            summary_choice: crate::chat_ui::LearnOverlaySummaryChoice::Original,
            selected_summary: None,
        };
        let model = build_learn_overlay_render_model(&s);
        assert!(model.equivalent_cli.contains("<required>"));
    }

    #[test]
    fn learn_overlay_assist_mode_updates_cli_preview() {
        let s = LearnOverlayState {
            tab: LearnOverlayTab::Capture,
            category_idx: 1,
            summary: "hello".to_string(),
            review_id: String::new(),
            promote_id: String::new(),
            promote_target_idx: 0,
            promote_slug: String::new(),
            promote_pack_id: String::new(),
            promote_force: false,
            input_focus: LearnOverlayInputFocus::CaptureSummary,
            inline_message: None,
            review_rows: Vec::new(),
            review_selected_idx: 0,
            assist_on: true,
            write_armed: true,
            logs: vec![],
            pending_submit_line: None,
            assist_summary: None,
            summary_choice: crate::chat_ui::LearnOverlaySummaryChoice::Original,
            selected_summary: None,
        };
        let model = build_learn_overlay_render_model(&s);
        assert!(model.equivalent_cli.contains("prompt_guidance"));
        assert!(model.equivalent_cli.contains("--assist"));
    }

    #[test]
    fn learn_overlay_promote_preview_shows_no_writes() {
        let s = LearnOverlayState {
            tab: LearnOverlayTab::Promote,
            category_idx: 0,
            summary: String::new(),
            review_id: String::new(),
            promote_id: "01ABC".to_string(),
            promote_target_idx: 0,
            promote_slug: "my_slug".to_string(),
            promote_pack_id: String::new(),
            promote_force: false,
            input_focus: LearnOverlayInputFocus::PromoteId,
            inline_message: None,
            review_rows: Vec::new(),
            review_selected_idx: 0,
            assist_on: false,
            write_armed: false,
            logs: vec![],
            pending_submit_line: None,
            assist_summary: None,
            summary_choice: crate::chat_ui::LearnOverlaySummaryChoice::Original,
            selected_summary: None,
        };
        let model = build_learn_overlay_render_model(&s);
        assert!(model
            .equivalent_cli
            .contains("learn promote 01ABC --to check"));
        assert_eq!(model.target_path, ".localagent/checks/<slug>.md");
    }

    #[test]
    fn learn_overlay_promote_pack_sets_target_path() {
        let s = LearnOverlayState {
            tab: LearnOverlayTab::Promote,
            category_idx: 0,
            summary: String::new(),
            review_id: String::new(),
            promote_id: "01ABC".to_string(),
            promote_target_idx: 1,
            promote_slug: String::new(),
            promote_pack_id: "core".to_string(),
            promote_force: true,
            input_focus: LearnOverlayInputFocus::PromotePackId,
            inline_message: None,
            review_rows: Vec::new(),
            review_selected_idx: 0,
            assist_on: false,
            write_armed: true,
            logs: vec![],
            pending_submit_line: None,
            assist_summary: None,
            summary_choice: crate::chat_ui::LearnOverlaySummaryChoice::Original,
            selected_summary: None,
        };
        let model = build_learn_overlay_render_model(&s);
        assert_eq!(model.target_path, ".localagent/packs/<pack_id>/PACK.md");
        assert!(model.equivalent_cli.contains("--pack-id core"));
        assert!(model.equivalent_cli.contains("--force"));
    }

    #[test]
    fn promote_submit_line_requires_slug_for_check() {
        let s = LearnOverlayState {
            tab: LearnOverlayTab::Promote,
            category_idx: 0,
            summary: String::new(),
            review_id: String::new(),
            promote_id: "01ABC".to_string(),
            promote_target_idx: 0,
            promote_slug: String::new(),
            promote_pack_id: String::new(),
            promote_force: false,
            input_focus: LearnOverlayInputFocus::PromoteSlug,
            inline_message: None,
            review_rows: Vec::new(),
            review_selected_idx: 0,
            assist_on: false,
            write_armed: true,
            logs: vec![],
            pending_submit_line: None,
            assist_summary: None,
            summary_choice: crate::chat_ui::LearnOverlaySummaryChoice::Original,
            selected_summary: None,
        };
        let err = build_overlay_promote_submit_line(&s).expect_err("slug required");
        assert!(err.contains("slug"));
    }

    #[test]
    fn promote_submit_line_builds_agents_command_with_flags() {
        let s = LearnOverlayState {
            tab: LearnOverlayTab::Promote,
            category_idx: 0,
            summary: String::new(),
            review_id: String::new(),
            promote_id: "01ABC".to_string(),
            promote_target_idx: 2,
            promote_slug: String::new(),
            promote_pack_id: String::new(),
            promote_force: true,
            input_focus: LearnOverlayInputFocus::PromoteId,
            inline_message: None,
            review_rows: Vec::new(),
            review_selected_idx: 0,
            assist_on: false,
            write_armed: true,
            logs: vec![],
            pending_submit_line: None,
            assist_summary: None,
            summary_choice: crate::chat_ui::LearnOverlaySummaryChoice::Original,
            selected_summary: None,
        };
        let line = build_overlay_promote_submit_line(&s).expect("line");
        assert!(line.contains("/learn promote 01ABC --to agents"));
        assert!(line.contains("--force"));
    }

    #[test]
    fn promote_submit_line_requires_pack_id_for_pack() {
        let s = LearnOverlayState {
            tab: LearnOverlayTab::Promote,
            category_idx: 0,
            summary: String::new(),
            review_id: String::new(),
            promote_id: "01ABC".to_string(),
            promote_target_idx: 1,
            promote_slug: String::new(),
            promote_pack_id: String::new(),
            promote_force: false,
            input_focus: LearnOverlayInputFocus::PromotePackId,
            inline_message: None,
            review_rows: Vec::new(),
            review_selected_idx: 0,
            assist_on: false,
            write_armed: true,
            logs: vec![],
            pending_submit_line: None,
            assist_summary: None,
            summary_choice: crate::chat_ui::LearnOverlaySummaryChoice::Original,
            selected_summary: None,
        };
        let err = build_overlay_promote_submit_line(&s).expect_err("pack_id required");
        assert!(err.contains("pack_id"));
    }

    #[test]
    fn learn_overlay_review_preview_shows_no_writes() {
        let s = LearnOverlayState {
            tab: LearnOverlayTab::Review,
            category_idx: 0,
            summary: String::new(),
            review_id: String::new(),
            promote_id: String::new(),
            promote_target_idx: 0,
            promote_slug: String::new(),
            promote_pack_id: String::new(),
            promote_force: false,
            input_focus: LearnOverlayInputFocus::ReviewId,
            inline_message: None,
            review_rows: Vec::new(),
            review_selected_idx: 0,
            assist_on: false,
            write_armed: false,
            logs: vec![],
            pending_submit_line: None,
            assist_summary: None,
            summary_choice: crate::chat_ui::LearnOverlaySummaryChoice::Original,
            selected_summary: None,
        };
        let model = build_learn_overlay_render_model(&s);
        assert_eq!(model.tab, LearnOverlayTab::Review);
        assert_eq!(model.equivalent_cli, "learn list");
    }

    #[test]
    fn focus_cycle_promote_wraps() {
        let mut s = LearnOverlayState {
            tab: LearnOverlayTab::Promote,
            category_idx: 0,
            summary: String::new(),
            review_id: String::new(),
            promote_id: String::new(),
            promote_target_idx: 0,
            promote_slug: String::new(),
            promote_pack_id: String::new(),
            promote_force: false,
            input_focus: LearnOverlayInputFocus::PromotePackId,
            inline_message: None,
            review_rows: Vec::new(),
            review_selected_idx: 0,
            assist_on: false,
            write_armed: false,
            logs: vec![],
            pending_submit_line: None,
            assist_summary: None,
            summary_choice: crate::chat_ui::LearnOverlaySummaryChoice::Original,
            selected_summary: None,
        };
        cycle_overlay_focus(&mut s, false);
        assert_eq!(s.input_focus, LearnOverlayInputFocus::PromoteId);
    }

    #[test]
    fn busy_enter_logs_busy_token_for_review() {
        let tmp = tempdir().expect("tempdir");
        let paths = crate::store::resolve_state_paths(tmp.path(), None, None, None, None);
        let mut overlay = Some(LearnOverlayState {
            tab: LearnOverlayTab::Review,
            category_idx: 0,
            summary: String::new(),
            review_id: String::new(),
            promote_id: String::new(),
            promote_target_idx: 0,
            promote_slug: String::new(),
            promote_pack_id: String::new(),
            promote_force: false,
            input_focus: LearnOverlayInputFocus::ReviewId,
            inline_message: None,
            review_rows: Vec::new(),
            review_selected_idx: 0,
            assist_on: false,
            write_armed: false,
            logs: vec![],
            pending_submit_line: None,
            assist_summary: None,
            summary_choice: crate::chat_ui::LearnOverlaySummaryChoice::Original,
            selected_summary: None,
        });
        let mut input_buf = String::new();
        let mut input_cursor = 0usize;
        let mut prompt_history = Vec::new();
        let mut history_idx = None;
        let mut slash_menu_index = 0usize;
        let mut palette_open = false;
        let palette_items = ["a"];
        let mut palette_selected = 0usize;
        let mut search_mode = false;
        let mut search_query = String::new();
        let mut search_line_cursor = 0usize;
        let mut search_input_cursor = 0usize;
        let mut learn_overlay_cursor = 3usize;
        let mut transcript: Vec<(String, String)> = Vec::new();
        let mut transcript_thinking: std::collections::BTreeMap<usize, String> =
            std::collections::BTreeMap::new();
        let mut show_thinking_panel = false;
        let mut streaming = String::new();
        let mut transcript_scroll = 0usize;
        let mut follow_output = true;
        let mut ui_state = crate::tui::state::UiState::new(100);
        let mut show_tools = false;
        let mut show_approvals = false;
        let mut show_logs = false;
        let mut compact_tools = true;
        let mut tools_selected = 0usize;
        let mut tools_focus = true;
        let mut approvals_selected = 0usize;
        let mut logs = Vec::new();
        let out = handle_tui_outer_key_dispatch(TuiOuterKeyDispatchInput {
            key: KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            learn_overlay: &mut overlay,
            run_busy: true,
            input: &mut input_buf,
            input_cursor: &mut input_cursor,
            prompt_history: &mut prompt_history,
            history_idx: &mut history_idx,
            slash_menu_index: &mut slash_menu_index,
            palette_open: &mut palette_open,
            palette_items: &palette_items,
            palette_selected: &mut palette_selected,
            search_mode: &mut search_mode,
            search_query: &mut search_query,
            search_line_cursor: &mut search_line_cursor,
            search_input_cursor: &mut search_input_cursor,
            transcript: &mut transcript,
            transcript_thinking: &mut transcript_thinking,
            show_thinking_panel: &mut show_thinking_panel,
            streaming_assistant: &mut streaming,
            transcript_scroll: &mut transcript_scroll,
            follow_output: &mut follow_output,
            ui_state: &mut ui_state,
            visible_tool_count: 0,
            show_tools: &mut show_tools,
            show_approvals: &mut show_approvals,
            show_logs: &mut show_logs,
            compact_tools: &mut compact_tools,
            tools_selected: &mut tools_selected,
            tools_focus: &mut tools_focus,
            approvals_selected: &mut approvals_selected,
            paths: &paths,
            logs: &mut logs,
            learn_overlay_cursor: &mut learn_overlay_cursor,
        });
        assert!(matches!(out, TuiOuterKeyDispatchOutcome::Handled));
        let ov = overlay.expect("overlay");
        assert!(ov.logs.iter().any(|l| l.contains("ERR_TUI_BUSY_TRY_AGAIN")));
    }

    #[test]
    fn busy_enter_logs_busy_token_for_promote() {
        let tmp = tempdir().expect("tempdir");
        let paths = crate::store::resolve_state_paths(tmp.path(), None, None, None, None);
        let mut overlay = Some(LearnOverlayState {
            tab: LearnOverlayTab::Promote,
            category_idx: 0,
            summary: String::new(),
            review_id: String::new(),
            promote_id: "01ABC".to_string(),
            promote_target_idx: 2,
            promote_slug: String::new(),
            promote_pack_id: String::new(),
            promote_force: false,
            input_focus: LearnOverlayInputFocus::PromoteId,
            inline_message: None,
            review_rows: Vec::new(),
            review_selected_idx: 0,
            assist_on: false,
            write_armed: true,
            logs: vec![],
            pending_submit_line: None,
            assist_summary: None,
            summary_choice: crate::chat_ui::LearnOverlaySummaryChoice::Original,
            selected_summary: None,
        });
        let mut input_buf = String::new();
        let mut input_cursor = 0usize;
        let mut prompt_history = Vec::new();
        let mut history_idx = None;
        let mut slash_menu_index = 0usize;
        let mut palette_open = false;
        let palette_items = ["a"];
        let mut palette_selected = 0usize;
        let mut search_mode = false;
        let mut search_query = String::new();
        let mut search_line_cursor = 0usize;
        let mut search_input_cursor = 0usize;
        let mut learn_overlay_cursor = 0usize;
        let mut transcript: Vec<(String, String)> = Vec::new();
        let mut transcript_thinking: std::collections::BTreeMap<usize, String> =
            std::collections::BTreeMap::new();
        let mut show_thinking_panel = false;
        let mut streaming = String::new();
        let mut transcript_scroll = 0usize;
        let mut follow_output = true;
        let mut ui_state = crate::tui::state::UiState::new(100);
        let mut show_tools = false;
        let mut show_approvals = false;
        let mut show_logs = false;
        let mut compact_tools = true;
        let mut tools_selected = 0usize;
        let mut tools_focus = true;
        let mut approvals_selected = 0usize;
        let mut logs = Vec::new();
        let out = handle_tui_outer_key_dispatch(TuiOuterKeyDispatchInput {
            key: KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            learn_overlay: &mut overlay,
            run_busy: true,
            input: &mut input_buf,
            input_cursor: &mut input_cursor,
            prompt_history: &mut prompt_history,
            history_idx: &mut history_idx,
            slash_menu_index: &mut slash_menu_index,
            palette_open: &mut palette_open,
            palette_items: &palette_items,
            palette_selected: &mut palette_selected,
            search_mode: &mut search_mode,
            search_query: &mut search_query,
            search_line_cursor: &mut search_line_cursor,
            search_input_cursor: &mut search_input_cursor,
            transcript: &mut transcript,
            transcript_thinking: &mut transcript_thinking,
            show_thinking_panel: &mut show_thinking_panel,
            streaming_assistant: &mut streaming,
            transcript_scroll: &mut transcript_scroll,
            follow_output: &mut follow_output,
            ui_state: &mut ui_state,
            visible_tool_count: 0,
            show_tools: &mut show_tools,
            show_approvals: &mut show_approvals,
            show_logs: &mut show_logs,
            compact_tools: &mut compact_tools,
            tools_selected: &mut tools_selected,
            tools_focus: &mut tools_focus,
            approvals_selected: &mut approvals_selected,
            paths: &paths,
            logs: &mut logs,
            learn_overlay_cursor: &mut learn_overlay_cursor,
        });
        assert!(matches!(out, TuiOuterKeyDispatchOutcome::Handled));
        let ov = overlay.expect("overlay");
        assert!(ov.logs.iter().any(|l| l.contains("ERR_TUI_BUSY_TRY_AGAIN")));
    }

    #[test]
    fn capture_enter_sets_submit_line_without_arm_step() {
        let tmp = tempdir().expect("tempdir");
        let paths = crate::store::resolve_state_paths(tmp.path(), None, None, None, None);
        let mut overlay = Some(LearnOverlayState {
            tab: LearnOverlayTab::Capture,
            category_idx: 0,
            summary: "hello".to_string(),
            review_id: String::new(),
            promote_id: String::new(),
            promote_target_idx: 0,
            promote_slug: String::new(),
            promote_pack_id: String::new(),
            promote_force: false,
            input_focus: LearnOverlayInputFocus::CaptureSummary,
            inline_message: None,
            review_rows: Vec::new(),
            review_selected_idx: 0,
            assist_on: true,
            write_armed: false,
            logs: vec![],
            pending_submit_line: None,
            assist_summary: None,
            summary_choice: crate::chat_ui::LearnOverlaySummaryChoice::Original,
            selected_summary: None,
        });
        let mut input_buf = String::new();
        let mut input_cursor = 0usize;
        let mut prompt_history = Vec::new();
        let mut history_idx = None;
        let mut slash_menu_index = 0usize;
        let mut palette_open = false;
        let palette_items = ["a"];
        let mut palette_selected = 0usize;
        let mut search_mode = false;
        let mut search_query = String::new();
        let mut search_line_cursor = 0usize;
        let mut search_input_cursor = 0usize;
        let mut learn_overlay_cursor = 0usize;
        let mut transcript: Vec<(String, String)> = vec![("user".to_string(), "hi".to_string())];
        let mut transcript_thinking: std::collections::BTreeMap<usize, String> =
            std::collections::BTreeMap::new();
        let mut show_thinking_panel = false;
        let before = transcript.clone();
        let mut streaming = String::new();
        let mut transcript_scroll = 0usize;
        let mut follow_output = true;
        let mut ui_state = crate::tui::state::UiState::new(100);
        let mut show_tools = false;
        let mut show_approvals = false;
        let mut show_logs = false;
        let mut compact_tools = true;
        let mut tools_selected = 0usize;
        let mut tools_focus = true;
        let mut approvals_selected = 0usize;
        let mut logs = Vec::new();
        let out = handle_tui_outer_key_dispatch(TuiOuterKeyDispatchInput {
            key: KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            learn_overlay: &mut overlay,
            run_busy: false,
            input: &mut input_buf,
            input_cursor: &mut input_cursor,
            prompt_history: &mut prompt_history,
            history_idx: &mut history_idx,
            slash_menu_index: &mut slash_menu_index,
            palette_open: &mut palette_open,
            palette_items: &palette_items,
            palette_selected: &mut palette_selected,
            search_mode: &mut search_mode,
            search_query: &mut search_query,
            search_line_cursor: &mut search_line_cursor,
            search_input_cursor: &mut search_input_cursor,
            transcript: &mut transcript,
            transcript_thinking: &mut transcript_thinking,
            show_thinking_panel: &mut show_thinking_panel,
            streaming_assistant: &mut streaming,
            transcript_scroll: &mut transcript_scroll,
            follow_output: &mut follow_output,
            ui_state: &mut ui_state,
            visible_tool_count: 0,
            show_tools: &mut show_tools,
            show_approvals: &mut show_approvals,
            show_logs: &mut show_logs,
            compact_tools: &mut compact_tools,
            tools_selected: &mut tools_selected,
            tools_focus: &mut tools_focus,
            approvals_selected: &mut approvals_selected,
            paths: &paths,
            logs: &mut logs,
            learn_overlay_cursor: &mut learn_overlay_cursor,
        });
        assert!(matches!(out, TuiOuterKeyDispatchOutcome::Handled));
        let ov = overlay.expect("overlay");
        assert!(ov.pending_submit_line.is_some());
        assert!(ov
            .inline_message
            .as_deref()
            .unwrap_or("")
            .contains("Press Enter to save"));
        assert_eq!(transcript, before);
    }

    #[test]
    fn capture_enter_keeps_preflight_log_absent() {
        let tmp = tempdir().expect("tempdir");
        let paths = crate::store::resolve_state_paths(tmp.path(), None, None, None, None);
        let mut overlay = Some(LearnOverlayState {
            tab: LearnOverlayTab::Capture,
            category_idx: 0,
            summary: "hello".to_string(),
            review_id: String::new(),
            promote_id: String::new(),
            promote_target_idx: 0,
            promote_slug: String::new(),
            promote_pack_id: String::new(),
            promote_force: false,
            input_focus: LearnOverlayInputFocus::CaptureSummary,
            inline_message: None,
            review_rows: Vec::new(),
            review_selected_idx: 0,
            assist_on: true,
            write_armed: false,
            logs: vec![],
            pending_submit_line: None,
            assist_summary: None,
            summary_choice: crate::chat_ui::LearnOverlaySummaryChoice::Original,
            selected_summary: None,
        });
        let mut input_buf = String::new();
        let mut input_cursor = 0usize;
        let mut prompt_history = Vec::new();
        let mut history_idx = None;
        let mut slash_menu_index = 0usize;
        let mut palette_open = false;
        let palette_items = ["a"];
        let mut palette_selected = 0usize;
        let mut search_mode = false;
        let mut search_query = String::new();
        let mut search_line_cursor = 0usize;
        let mut search_input_cursor = 0usize;
        let mut learn_overlay_cursor = 0usize;
        let mut transcript: Vec<(String, String)> = vec![("user".to_string(), "hi".to_string())];
        let mut transcript_thinking: std::collections::BTreeMap<usize, String> =
            std::collections::BTreeMap::new();
        let mut show_thinking_panel = false;
        let mut streaming = String::new();
        let mut transcript_scroll = 0usize;
        let mut follow_output = true;
        let mut ui_state = crate::tui::state::UiState::new(100);
        let mut show_tools = false;
        let mut show_approvals = false;
        let mut show_logs = false;
        let mut compact_tools = true;
        let mut tools_selected = 0usize;
        let mut tools_focus = true;
        let mut approvals_selected = 0usize;
        let mut logs = Vec::new();

        for _ in 0..3 {
            let out = handle_tui_outer_key_dispatch(TuiOuterKeyDispatchInput {
                key: KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                learn_overlay: &mut overlay,
                run_busy: false,
                input: &mut input_buf,
                input_cursor: &mut input_cursor,
                prompt_history: &mut prompt_history,
                history_idx: &mut history_idx,
                slash_menu_index: &mut slash_menu_index,
                palette_open: &mut palette_open,
                palette_items: &palette_items,
                palette_selected: &mut palette_selected,
                search_mode: &mut search_mode,
                search_query: &mut search_query,
                search_line_cursor: &mut search_line_cursor,
                search_input_cursor: &mut search_input_cursor,
                transcript: &mut transcript,
                transcript_thinking: &mut transcript_thinking,
                show_thinking_panel: &mut show_thinking_panel,
                streaming_assistant: &mut streaming,
                transcript_scroll: &mut transcript_scroll,
                follow_output: &mut follow_output,
                ui_state: &mut ui_state,
                visible_tool_count: 0,
                show_tools: &mut show_tools,
                show_approvals: &mut show_approvals,
                show_logs: &mut show_logs,
                compact_tools: &mut compact_tools,
                tools_selected: &mut tools_selected,
                tools_focus: &mut tools_focus,
                approvals_selected: &mut approvals_selected,
                paths: &paths,
                logs: &mut logs,
                learn_overlay_cursor: &mut learn_overlay_cursor,
            });
            assert!(matches!(out, TuiOuterKeyDispatchOutcome::Handled));
        }

        let ov = overlay.expect("overlay");
        let preflight = "info: Preflight check complete. Waiting for user action.";
        let count = ov.logs.iter().filter(|l| l.as_str() == preflight).count();
        assert_eq!(count, 0);
    }

    #[test]
    fn esc_closes_overlay_even_when_run_busy() {
        let tmp = tempdir().expect("tempdir");
        let paths = crate::store::resolve_state_paths(tmp.path(), None, None, None, None);
        let mut overlay = Some(LearnOverlayState {
            tab: LearnOverlayTab::Capture,
            category_idx: 0,
            summary: "hello".to_string(),
            review_id: String::new(),
            promote_id: String::new(),
            promote_target_idx: 0,
            promote_slug: String::new(),
            promote_pack_id: String::new(),
            promote_force: false,
            input_focus: LearnOverlayInputFocus::CaptureSummary,
            inline_message: None,
            review_rows: Vec::new(),
            review_selected_idx: 0,
            assist_on: true,
            write_armed: false,
            logs: vec![],
            pending_submit_line: None,
            assist_summary: None,
            summary_choice: crate::chat_ui::LearnOverlaySummaryChoice::Original,
            selected_summary: None,
        });
        let mut input_buf = String::new();
        let mut input_cursor = 0usize;
        let mut prompt_history = Vec::new();
        let mut history_idx = None;
        let mut slash_menu_index = 0usize;
        let mut palette_open = false;
        let palette_items = ["a"];
        let mut palette_selected = 0usize;
        let mut search_mode = false;
        let mut search_query = String::new();
        let mut search_line_cursor = 0usize;
        let mut search_input_cursor = 0usize;
        let mut learn_overlay_cursor = 0usize;
        let mut transcript: Vec<(String, String)> = vec![];
        let mut transcript_thinking: std::collections::BTreeMap<usize, String> =
            std::collections::BTreeMap::new();
        let mut show_thinking_panel = false;
        let mut streaming = String::new();
        let mut transcript_scroll = 0usize;
        let mut follow_output = true;
        let mut ui_state = crate::tui::state::UiState::new(100);
        let mut show_tools = false;
        let mut show_approvals = false;
        let mut show_logs = false;
        let mut compact_tools = true;
        let mut tools_selected = 0usize;
        let mut tools_focus = true;
        let mut approvals_selected = 0usize;
        let mut logs = Vec::new();
        let out = handle_tui_outer_key_dispatch(TuiOuterKeyDispatchInput {
            key: KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            learn_overlay: &mut overlay,
            run_busy: true,
            input: &mut input_buf,
            input_cursor: &mut input_cursor,
            prompt_history: &mut prompt_history,
            history_idx: &mut history_idx,
            slash_menu_index: &mut slash_menu_index,
            palette_open: &mut palette_open,
            palette_items: &palette_items,
            palette_selected: &mut palette_selected,
            search_mode: &mut search_mode,
            search_query: &mut search_query,
            search_line_cursor: &mut search_line_cursor,
            search_input_cursor: &mut search_input_cursor,
            transcript: &mut transcript,
            transcript_thinking: &mut transcript_thinking,
            show_thinking_panel: &mut show_thinking_panel,
            streaming_assistant: &mut streaming,
            transcript_scroll: &mut transcript_scroll,
            follow_output: &mut follow_output,
            ui_state: &mut ui_state,
            visible_tool_count: 0,
            show_tools: &mut show_tools,
            show_approvals: &mut show_approvals,
            show_logs: &mut show_logs,
            compact_tools: &mut compact_tools,
            tools_selected: &mut tools_selected,
            tools_focus: &mut tools_focus,
            approvals_selected: &mut approvals_selected,
            paths: &paths,
            logs: &mut logs,
            learn_overlay_cursor: &mut learn_overlay_cursor,
        });
        assert!(matches!(out, TuiOuterKeyDispatchOutcome::Handled));
        assert!(overlay.is_none());
    }

    #[test]
    fn ctrl_1_switches_to_capture_but_plain_1_is_text_input() {
        let tmp = tempdir().expect("tempdir");
        let paths = crate::store::resolve_state_paths(tmp.path(), None, None, None, None);
        let mut overlay = Some(LearnOverlayState {
            tab: LearnOverlayTab::Promote,
            category_idx: 0,
            summary: String::new(),
            review_id: String::new(),
            promote_id: "abc".to_string(),
            promote_target_idx: 2,
            promote_slug: String::new(),
            promote_pack_id: String::new(),
            promote_force: false,
            input_focus: LearnOverlayInputFocus::PromoteId,
            inline_message: None,
            review_rows: Vec::new(),
            review_selected_idx: 0,
            assist_on: true,
            write_armed: false,
            logs: vec![],
            pending_submit_line: None,
            assist_summary: None,
            summary_choice: crate::chat_ui::LearnOverlaySummaryChoice::Original,
            selected_summary: None,
        });
        let mut input_buf = String::new();
        let mut input_cursor = 0usize;
        let mut prompt_history = Vec::new();
        let mut history_idx = None;
        let mut slash_menu_index = 0usize;
        let mut palette_open = false;
        let palette_items = ["a"];
        let mut palette_selected = 0usize;
        let mut search_mode = false;
        let mut search_query = String::new();
        let mut search_line_cursor = 0usize;
        let mut search_input_cursor = 0usize;
        let mut learn_overlay_cursor = 3usize;
        let mut transcript: Vec<(String, String)> = vec![];
        let mut transcript_thinking: std::collections::BTreeMap<usize, String> =
            std::collections::BTreeMap::new();
        let mut show_thinking_panel = false;
        let mut streaming = String::new();
        let mut transcript_scroll = 0usize;
        let mut follow_output = true;
        let mut ui_state = crate::tui::state::UiState::new(100);
        let mut show_tools = false;
        let mut show_approvals = false;
        let mut show_logs = false;
        let mut compact_tools = true;
        let mut tools_selected = 0usize;
        let mut tools_focus = true;
        let mut approvals_selected = 0usize;
        let mut logs = Vec::new();

        let out_plain = handle_tui_outer_key_dispatch(TuiOuterKeyDispatchInput {
            key: KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
            learn_overlay: &mut overlay,
            run_busy: false,
            input: &mut input_buf,
            input_cursor: &mut input_cursor,
            prompt_history: &mut prompt_history,
            history_idx: &mut history_idx,
            slash_menu_index: &mut slash_menu_index,
            palette_open: &mut palette_open,
            palette_items: &palette_items,
            palette_selected: &mut palette_selected,
            search_mode: &mut search_mode,
            search_query: &mut search_query,
            search_line_cursor: &mut search_line_cursor,
            search_input_cursor: &mut search_input_cursor,
            transcript: &mut transcript,
            transcript_thinking: &mut transcript_thinking,
            show_thinking_panel: &mut show_thinking_panel,
            streaming_assistant: &mut streaming,
            transcript_scroll: &mut transcript_scroll,
            follow_output: &mut follow_output,
            ui_state: &mut ui_state,
            visible_tool_count: 0,
            show_tools: &mut show_tools,
            show_approvals: &mut show_approvals,
            show_logs: &mut show_logs,
            compact_tools: &mut compact_tools,
            tools_selected: &mut tools_selected,
            tools_focus: &mut tools_focus,
            approvals_selected: &mut approvals_selected,
            paths: &paths,
            logs: &mut logs,
            learn_overlay_cursor: &mut learn_overlay_cursor,
        });
        assert!(matches!(
            out_plain,
            TuiOuterKeyDispatchOutcome::Handled
        ));
        let ov_plain = overlay.as_ref().expect("overlay");
        assert_eq!(ov_plain.tab, LearnOverlayTab::Promote);
        assert_eq!(ov_plain.promote_id, "abc1");

        let out_ctrl = handle_tui_outer_key_dispatch(TuiOuterKeyDispatchInput {
            key: KeyEvent::new(KeyCode::Char('1'), KeyModifiers::CONTROL),
            learn_overlay: &mut overlay,
            run_busy: false,
            input: &mut input_buf,
            input_cursor: &mut input_cursor,
            prompt_history: &mut prompt_history,
            history_idx: &mut history_idx,
            slash_menu_index: &mut slash_menu_index,
            palette_open: &mut palette_open,
            palette_items: &palette_items,
            palette_selected: &mut palette_selected,
            search_mode: &mut search_mode,
            search_query: &mut search_query,
            search_line_cursor: &mut search_line_cursor,
            search_input_cursor: &mut search_input_cursor,
            transcript: &mut transcript,
            transcript_thinking: &mut transcript_thinking,
            show_thinking_panel: &mut show_thinking_panel,
            streaming_assistant: &mut streaming,
            transcript_scroll: &mut transcript_scroll,
            follow_output: &mut follow_output,
            ui_state: &mut ui_state,
            visible_tool_count: 0,
            show_tools: &mut show_tools,
            show_approvals: &mut show_approvals,
            show_logs: &mut show_logs,
            compact_tools: &mut compact_tools,
            tools_selected: &mut tools_selected,
            tools_focus: &mut tools_focus,
            approvals_selected: &mut approvals_selected,
            paths: &paths,
            logs: &mut logs,
            learn_overlay_cursor: &mut learn_overlay_cursor,
        });
        assert!(matches!(
            out_ctrl,
            TuiOuterKeyDispatchOutcome::Handled
        ));
        let ov_ctrl = overlay.as_ref().expect("overlay");
        assert_eq!(ov_ctrl.tab, LearnOverlayTab::Capture);
    }

    #[test]
    fn overlay_paste_is_bounded_and_deduped_for_summary() {
        let mut overlay = Some(LearnOverlayState {
            tab: LearnOverlayTab::Capture,
            category_idx: 0,
            summary: String::new(),
            review_id: String::new(),
            promote_id: String::new(),
            promote_target_idx: 0,
            promote_slug: String::new(),
            promote_pack_id: String::new(),
            promote_force: false,
            input_focus: LearnOverlayInputFocus::CaptureSummary,
            inline_message: None,
            review_rows: Vec::new(),
            review_selected_idx: 0,
            assist_on: true,
            write_armed: false,
            logs: vec![],
            pending_submit_line: None,
            assist_summary: None,
            summary_choice: crate::chat_ui::LearnOverlaySummaryChoice::Original,
            selected_summary: None,
        });
        let mut input = String::new();
        let mut input_cursor = 0usize;
        let mut learn_overlay_cursor = 0usize;
        let mut history_idx = None;
        let mut slash_menu_index = 0usize;
        let spam = "'FastVideoProcessor' object has no attribute '_artifact_manager'";

        for _ in 0..10 {
            handle_tui_outer_paste_event(TuiOuterPasteInput {
                pasted: spam,
                input: &mut input,
                input_cursor: &mut input_cursor,
                history_idx: &mut history_idx,
                slash_menu_index: &mut slash_menu_index,
                learn_overlay: &mut overlay,
                learn_overlay_cursor: &mut learn_overlay_cursor,
            });
        }

        let ov = overlay.expect("overlay");
        assert!(ov.summary.len() <= OVERLAY_CAPTURE_SUMMARY_MAX_CHARS);
        assert!(ov.summary.contains("_artifact_manager"));
    }

    #[test]
    fn overlay_text_input_allows_consecutive_same_chars() {
        let tmp = tempdir().expect("tempdir");
        let paths = crate::store::resolve_state_paths(tmp.path(), None, None, None, None);
        let mut overlay = Some(LearnOverlayState {
            tab: LearnOverlayTab::Capture,
            category_idx: 0,
            summary: String::new(),
            review_id: String::new(),
            promote_id: String::new(),
            promote_target_idx: 0,
            promote_slug: String::new(),
            promote_pack_id: String::new(),
            promote_force: false,
            input_focus: LearnOverlayInputFocus::CaptureSummary,
            inline_message: None,
            review_rows: Vec::new(),
            review_selected_idx: 0,
            assist_on: true,
            write_armed: false,
            logs: vec![],
            pending_submit_line: None,
            assist_summary: None,
            summary_choice: crate::chat_ui::LearnOverlaySummaryChoice::Original,
            selected_summary: None,
        });
        let mut input_buf = String::new();
        let mut input_cursor = 0usize;
        let mut prompt_history = Vec::new();
        let mut history_idx = None;
        let mut slash_menu_index = 0usize;
        let mut palette_open = false;
        let palette_items = ["a"];
        let mut palette_selected = 0usize;
        let mut search_mode = false;
        let mut search_query = String::new();
        let mut search_line_cursor = 0usize;
        let mut search_input_cursor = 0usize;
        let mut learn_overlay_cursor = 0usize;
        let mut transcript: Vec<(String, String)> = vec![];
        let mut transcript_thinking: std::collections::BTreeMap<usize, String> =
            std::collections::BTreeMap::new();
        let mut show_thinking_panel = false;
        let mut streaming = String::new();
        let mut transcript_scroll = 0usize;
        let mut follow_output = true;
        let mut ui_state = crate::tui::state::UiState::new(100);
        let mut show_tools = false;
        let mut show_approvals = false;
        let mut show_logs = false;
        let mut compact_tools = true;
        let mut tools_selected = 0usize;
        let mut tools_focus = true;
        let mut approvals_selected = 0usize;
        let mut logs = Vec::new();

        let _ = handle_tui_outer_key_dispatch(TuiOuterKeyDispatchInput {
            key: KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
            learn_overlay: &mut overlay,
            run_busy: false,
            input: &mut input_buf,
            input_cursor: &mut input_cursor,
            prompt_history: &mut prompt_history,
            history_idx: &mut history_idx,
            slash_menu_index: &mut slash_menu_index,
            palette_open: &mut palette_open,
            palette_items: &palette_items,
            palette_selected: &mut palette_selected,
            search_mode: &mut search_mode,
            search_query: &mut search_query,
            search_line_cursor: &mut search_line_cursor,
            search_input_cursor: &mut search_input_cursor,
            transcript: &mut transcript,
            transcript_thinking: &mut transcript_thinking,
            show_thinking_panel: &mut show_thinking_panel,
            streaming_assistant: &mut streaming,
            transcript_scroll: &mut transcript_scroll,
            follow_output: &mut follow_output,
            ui_state: &mut ui_state,
            visible_tool_count: 0,
            show_tools: &mut show_tools,
            show_approvals: &mut show_approvals,
            show_logs: &mut show_logs,
            compact_tools: &mut compact_tools,
            tools_selected: &mut tools_selected,
            tools_focus: &mut tools_focus,
            approvals_selected: &mut approvals_selected,
            paths: &paths,
            logs: &mut logs,
            learn_overlay_cursor: &mut learn_overlay_cursor,
        });
        let _ = handle_tui_outer_key_dispatch(TuiOuterKeyDispatchInput {
            key: KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
            learn_overlay: &mut overlay,
            run_busy: false,
            input: &mut input_buf,
            input_cursor: &mut input_cursor,
            prompt_history: &mut prompt_history,
            history_idx: &mut history_idx,
            slash_menu_index: &mut slash_menu_index,
            palette_open: &mut palette_open,
            palette_items: &palette_items,
            palette_selected: &mut palette_selected,
            search_mode: &mut search_mode,
            search_query: &mut search_query,
            search_line_cursor: &mut search_line_cursor,
            search_input_cursor: &mut search_input_cursor,
            transcript: &mut transcript,
            transcript_thinking: &mut transcript_thinking,
            show_thinking_panel: &mut show_thinking_panel,
            streaming_assistant: &mut streaming,
            transcript_scroll: &mut transcript_scroll,
            follow_output: &mut follow_output,
            ui_state: &mut ui_state,
            visible_tool_count: 0,
            show_tools: &mut show_tools,
            show_approvals: &mut show_approvals,
            show_logs: &mut show_logs,
            compact_tools: &mut compact_tools,
            tools_selected: &mut tools_selected,
            tools_focus: &mut tools_focus,
            approvals_selected: &mut approvals_selected,
            paths: &paths,
            logs: &mut logs,
            learn_overlay_cursor: &mut learn_overlay_cursor,
        });

        let ov = overlay.expect("overlay");
        assert_eq!(ov.summary, "aa");
    }

    #[test]
    fn overlay_plain_q_is_text_not_close_shortcut() {
        let tmp = tempdir().expect("tempdir");
        let paths = crate::store::resolve_state_paths(tmp.path(), None, None, None, None);
        let mut overlay = Some(LearnOverlayState {
            tab: LearnOverlayTab::Capture,
            category_idx: 0,
            summary: String::new(),
            review_id: String::new(),
            promote_id: String::new(),
            promote_target_idx: 0,
            promote_slug: String::new(),
            promote_pack_id: String::new(),
            promote_force: false,
            input_focus: LearnOverlayInputFocus::CaptureSummary,
            inline_message: None,
            review_rows: Vec::new(),
            review_selected_idx: 0,
            assist_on: true,
            write_armed: false,
            logs: vec![],
            pending_submit_line: None,
            assist_summary: None,
            summary_choice: crate::chat_ui::LearnOverlaySummaryChoice::Original,
            selected_summary: None,
        });
        let mut input_buf = String::new();
        let mut input_cursor = 0usize;
        let mut prompt_history = Vec::new();
        let mut history_idx = None;
        let mut slash_menu_index = 0usize;
        let mut palette_open = false;
        let palette_items = ["a"];
        let mut palette_selected = 0usize;
        let mut search_mode = false;
        let mut search_query = String::new();
        let mut search_line_cursor = 0usize;
        let mut search_input_cursor = 0usize;
        let mut learn_overlay_cursor = 0usize;
        let mut transcript: Vec<(String, String)> = vec![];
        let mut transcript_thinking: std::collections::BTreeMap<usize, String> =
            std::collections::BTreeMap::new();
        let mut show_thinking_panel = false;
        let mut streaming = String::new();
        let mut transcript_scroll = 0usize;
        let mut follow_output = true;
        let mut ui_state = crate::tui::state::UiState::new(100);
        let mut show_tools = false;
        let mut show_approvals = false;
        let mut show_logs = false;
        let mut compact_tools = true;
        let mut tools_selected = 0usize;
        let mut tools_focus = true;
        let mut approvals_selected = 0usize;
        let mut logs = Vec::new();

        let _ = handle_tui_outer_key_dispatch(TuiOuterKeyDispatchInput {
            key: KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
            learn_overlay: &mut overlay,
            run_busy: false,
            input: &mut input_buf,
            input_cursor: &mut input_cursor,
            prompt_history: &mut prompt_history,
            history_idx: &mut history_idx,
            slash_menu_index: &mut slash_menu_index,
            palette_open: &mut palette_open,
            palette_items: &palette_items,
            palette_selected: &mut palette_selected,
            search_mode: &mut search_mode,
            search_query: &mut search_query,
            search_line_cursor: &mut search_line_cursor,
            search_input_cursor: &mut search_input_cursor,
            transcript: &mut transcript,
            transcript_thinking: &mut transcript_thinking,
            show_thinking_panel: &mut show_thinking_panel,
            streaming_assistant: &mut streaming,
            transcript_scroll: &mut transcript_scroll,
            follow_output: &mut follow_output,
            ui_state: &mut ui_state,
            visible_tool_count: 0,
            show_tools: &mut show_tools,
            show_approvals: &mut show_approvals,
            show_logs: &mut show_logs,
            compact_tools: &mut compact_tools,
            tools_selected: &mut tools_selected,
            tools_focus: &mut tools_focus,
            approvals_selected: &mut approvals_selected,
            paths: &paths,
            logs: &mut logs,
            learn_overlay_cursor: &mut learn_overlay_cursor,
        });

        let ov = overlay.expect("overlay still open");
        assert_eq!(ov.summary, "q");
    }

    #[test]
    fn ctrl_4_toggles_thinking_visibility_in_chat_mode() {
        let tmp = tempdir().expect("tempdir");
        let paths = crate::store::resolve_state_paths(tmp.path(), None, None, None, None);
        let mut overlay = None;
        let mut input_buf = String::new();
        let mut input_cursor = 0usize;
        let mut prompt_history = Vec::new();
        let mut history_idx = None;
        let mut slash_menu_index = 0usize;
        let mut palette_open = false;
        let palette_items = ["a"];
        let mut palette_selected = 0usize;
        let mut search_mode = false;
        let mut search_query = String::new();
        let mut search_line_cursor = 0usize;
        let mut search_input_cursor = 0usize;
        let mut learn_overlay_cursor = 0usize;
        let mut transcript: Vec<(String, String)> = vec![];
        let mut transcript_thinking: std::collections::BTreeMap<usize, String> =
            std::collections::BTreeMap::new();
        let mut show_thinking_panel = false;
        let mut streaming = String::new();
        let mut transcript_scroll = 0usize;
        let mut follow_output = true;
        let mut ui_state = crate::tui::state::UiState::new(100);
        let mut show_tools = false;
        let mut show_approvals = false;
        let mut show_logs = false;
        let mut compact_tools = true;
        let mut tools_selected = 0usize;
        let mut tools_focus = true;
        let mut approvals_selected = 0usize;
        let mut logs = Vec::new();
        let out = handle_tui_outer_key_dispatch(TuiOuterKeyDispatchInput {
            key: KeyEvent::new(KeyCode::Char('4'), KeyModifiers::CONTROL),
            learn_overlay: &mut overlay,
            run_busy: false,
            input: &mut input_buf,
            input_cursor: &mut input_cursor,
            prompt_history: &mut prompt_history,
            history_idx: &mut history_idx,
            slash_menu_index: &mut slash_menu_index,
            palette_open: &mut palette_open,
            palette_items: &palette_items,
            palette_selected: &mut palette_selected,
            search_mode: &mut search_mode,
            search_query: &mut search_query,
            search_line_cursor: &mut search_line_cursor,
            search_input_cursor: &mut search_input_cursor,
            transcript: &mut transcript,
            transcript_thinking: &mut transcript_thinking,
            show_thinking_panel: &mut show_thinking_panel,
            streaming_assistant: &mut streaming,
            transcript_scroll: &mut transcript_scroll,
            follow_output: &mut follow_output,
            ui_state: &mut ui_state,
            visible_tool_count: 0,
            show_tools: &mut show_tools,
            show_approvals: &mut show_approvals,
            show_logs: &mut show_logs,
            compact_tools: &mut compact_tools,
            tools_selected: &mut tools_selected,
            tools_focus: &mut tools_focus,
            approvals_selected: &mut approvals_selected,
            paths: &paths,
            logs: &mut logs,
            learn_overlay_cursor: &mut learn_overlay_cursor,
        });
        assert!(matches!(out, TuiOuterKeyDispatchOutcome::Handled));
        assert!(show_thinking_panel);
    }

    #[test]
    fn ctrl_4_toggles_thinking_panel_even_when_overlay_open() {
        let tmp = tempdir().expect("tempdir");
        let paths = crate::store::resolve_state_paths(tmp.path(), None, None, None, None);
        let mut overlay = Some(LearnOverlayState::default());
        let mut input_buf = String::new();
        let mut input_cursor = 0usize;
        let mut prompt_history = Vec::new();
        let mut history_idx = None;
        let mut slash_menu_index = 0usize;
        let mut palette_open = false;
        let palette_items = ["a"];
        let mut palette_selected = 0usize;
        let mut search_mode = false;
        let mut search_query = String::new();
        let mut search_line_cursor = 0usize;
        let mut search_input_cursor = 0usize;
        let mut learn_overlay_cursor = 0usize;
        let mut transcript: Vec<(String, String)> = vec![];
        let mut transcript_thinking: std::collections::BTreeMap<usize, String> =
            std::collections::BTreeMap::new();
        let mut show_thinking_panel = false;
        let mut streaming = String::new();
        let mut transcript_scroll = 0usize;
        let mut follow_output = true;
        let mut ui_state = crate::tui::state::UiState::new(100);
        let mut show_tools = false;
        let mut show_approvals = false;
        let mut show_logs = false;
        let mut compact_tools = true;
        let mut tools_selected = 0usize;
        let mut tools_focus = true;
        let mut approvals_selected = 0usize;
        let mut logs = Vec::new();
        let out = handle_tui_outer_key_dispatch(TuiOuterKeyDispatchInput {
            key: KeyEvent::new(KeyCode::Char('4'), KeyModifiers::CONTROL),
            learn_overlay: &mut overlay,
            run_busy: false,
            input: &mut input_buf,
            input_cursor: &mut input_cursor,
            prompt_history: &mut prompt_history,
            history_idx: &mut history_idx,
            slash_menu_index: &mut slash_menu_index,
            palette_open: &mut palette_open,
            palette_items: &palette_items,
            palette_selected: &mut palette_selected,
            search_mode: &mut search_mode,
            search_query: &mut search_query,
            search_line_cursor: &mut search_line_cursor,
            search_input_cursor: &mut search_input_cursor,
            transcript: &mut transcript,
            transcript_thinking: &mut transcript_thinking,
            show_thinking_panel: &mut show_thinking_panel,
            streaming_assistant: &mut streaming,
            transcript_scroll: &mut transcript_scroll,
            follow_output: &mut follow_output,
            ui_state: &mut ui_state,
            visible_tool_count: 0,
            show_tools: &mut show_tools,
            show_approvals: &mut show_approvals,
            show_logs: &mut show_logs,
            compact_tools: &mut compact_tools,
            tools_selected: &mut tools_selected,
            tools_focus: &mut tools_focus,
            approvals_selected: &mut approvals_selected,
            paths: &paths,
            logs: &mut logs,
            learn_overlay_cursor: &mut learn_overlay_cursor,
        });
        assert!(matches!(out, TuiOuterKeyDispatchOutcome::Handled));
        assert!(show_thinking_panel);
        assert!(overlay.is_some());
    }

    #[test]
    fn ctrl_4_control_char_toggles_thinking_visibility_in_chat_mode() {
        let tmp = tempdir().expect("tempdir");
        let paths = crate::store::resolve_state_paths(tmp.path(), None, None, None, None);
        let mut overlay = None;
        let mut input_buf = String::new();
        let mut input_cursor = 0usize;
        let mut prompt_history = Vec::new();
        let mut history_idx = None;
        let mut slash_menu_index = 0usize;
        let mut palette_open = false;
        let palette_items = ["a"];
        let mut palette_selected = 0usize;
        let mut search_mode = false;
        let mut search_query = String::new();
        let mut search_line_cursor = 0usize;
        let mut search_input_cursor = 0usize;
        let mut learn_overlay_cursor = 0usize;
        let mut transcript: Vec<(String, String)> = vec![];
        let mut transcript_thinking: std::collections::BTreeMap<usize, String> =
            std::collections::BTreeMap::new();
        let mut show_thinking_panel = false;
        let mut streaming = String::new();
        let mut transcript_scroll = 0usize;
        let mut follow_output = true;
        let mut ui_state = crate::tui::state::UiState::new(100);
        let mut show_tools = false;
        let mut show_approvals = false;
        let mut show_logs = false;
        let mut compact_tools = true;
        let mut tools_selected = 0usize;
        let mut tools_focus = true;
        let mut approvals_selected = 0usize;
        let mut logs = Vec::new();
        let out = handle_tui_outer_key_dispatch(TuiOuterKeyDispatchInput {
            key: KeyEvent::new(KeyCode::Char('\u{1c}'), KeyModifiers::empty()),
            learn_overlay: &mut overlay,
            run_busy: false,
            input: &mut input_buf,
            input_cursor: &mut input_cursor,
            prompt_history: &mut prompt_history,
            history_idx: &mut history_idx,
            slash_menu_index: &mut slash_menu_index,
            palette_open: &mut palette_open,
            palette_items: &palette_items,
            palette_selected: &mut palette_selected,
            search_mode: &mut search_mode,
            search_query: &mut search_query,
            search_line_cursor: &mut search_line_cursor,
            search_input_cursor: &mut search_input_cursor,
            transcript: &mut transcript,
            transcript_thinking: &mut transcript_thinking,
            show_thinking_panel: &mut show_thinking_panel,
            streaming_assistant: &mut streaming,
            transcript_scroll: &mut transcript_scroll,
            follow_output: &mut follow_output,
            ui_state: &mut ui_state,
            visible_tool_count: 0,
            show_tools: &mut show_tools,
            show_approvals: &mut show_approvals,
            show_logs: &mut show_logs,
            compact_tools: &mut compact_tools,
            tools_selected: &mut tools_selected,
            tools_focus: &mut tools_focus,
            approvals_selected: &mut approvals_selected,
            paths: &paths,
            logs: &mut logs,
            learn_overlay_cursor: &mut learn_overlay_cursor,
        });
        assert!(matches!(out, TuiOuterKeyDispatchOutcome::Handled));
        assert!(show_thinking_panel);
    }

    #[test]
    fn refresh_approvals_auto_opens_on_first_pending_transition() {
        let tmp = tempdir().expect("tempdir");
        let approvals_path = tmp.path().join("approvals.json");
        let store = ApprovalsStore::new(approvals_path.clone());
        let _id = store
            .create_pending("shell", &serde_json::json!({"cmd":"echo"}), None, None)
            .expect("pending");

        let mut ui_state = crate::tui::state::UiState::new(20);
        let mut show_approvals = false;
        let mut previous_pending = 0usize;
        let mut logs = Vec::new();

        refresh_approvals_with_auto_open(
            &mut ui_state,
            &approvals_path,
            &mut show_approvals,
            &mut previous_pending,
            &mut logs,
        );

        assert!(show_approvals);
        assert_eq!(previous_pending, 1);
        assert!(logs.is_empty());
    }

    #[test]
    fn refresh_approvals_failure_logs_once_and_preserves_rows() {
        let tmp = tempdir().expect("tempdir");
        let approvals_path = tmp.path().join("approvals.json");
        std::fs::write(&approvals_path, "{not-json").expect("write broken json");

        let mut ui_state = crate::tui::state::UiState::new(20);
        ui_state.pending_approvals = vec![crate::tui::state::ApprovalRow {
            id: "existing".to_string(),
            tool: "shell".to_string(),
            status: "pending".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
        }];
        let mut show_approvals = false;
        let mut previous_pending = 1usize;
        let mut logs = Vec::new();

        refresh_approvals_with_auto_open(
            &mut ui_state,
            &approvals_path,
            &mut show_approvals,
            &mut previous_pending,
            &mut logs,
        );
        refresh_approvals_with_auto_open(
            &mut ui_state,
            &approvals_path,
            &mut show_approvals,
            &mut previous_pending,
            &mut logs,
        );

        assert_eq!(logs.len(), 1);
        assert!(logs[0].starts_with("approvals refresh failed:"));
        assert_eq!(ui_state.pending_approvals.len(), 1);
        assert_eq!(ui_state.pending_approvals[0].id, "existing");
    }


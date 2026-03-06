use crate::chat_tui::overlay::{LearnOverlayInputFocus, LearnOverlayState};
use crate::chat_tui::text::{char_len, insert_text_bounded, normalize_overlay_paste};

pub(crate) const OVERLAY_CAPTURE_SUMMARY_MAX_CHARS: usize = 360;
pub(crate) const OVERLAY_ID_MAX_CHARS: usize = 96;

pub(crate) struct TuiOuterMouseInput<'a> {
    pub(crate) me: &'a crossterm::event::MouseEvent,
    pub(crate) transcript: &'a Vec<(String, String)>,
    pub(crate) streaming_assistant: &'a str,
    pub(crate) transcript_scroll: &'a mut usize,
    pub(crate) follow_output: &'a mut bool,
}

pub(crate) fn handle_tui_outer_mouse_event(input: TuiOuterMouseInput<'_>) {
    if let Some(delta) = crate::chat_runtime::mouse_scroll_delta(input.me) {
        let max_scroll = crate::chat_runtime::transcript_max_scroll_lines(
            input.transcript,
            input.streaming_assistant,
        );
        *input.transcript_scroll = crate::chat_runtime::adjust_transcript_scroll(
            *input.transcript_scroll,
            delta,
            max_scroll,
        );
        *input.follow_output = false;
    }
}

pub(crate) struct TuiOuterPasteInput<'a> {
    pub(crate) pasted: &'a str,
    pub(crate) input: &'a mut String,
    pub(crate) input_cursor: &'a mut usize,
    pub(crate) history_idx: &'a mut Option<usize>,
    pub(crate) slash_menu_index: &'a mut usize,
    pub(crate) learn_overlay: &'a mut Option<LearnOverlayState>,
    pub(crate) learn_overlay_cursor: &'a mut usize,
}

pub(crate) fn handle_tui_outer_paste_event(input: TuiOuterPasteInput<'_>) {
    if let Some(overlay) = input.learn_overlay.as_mut() {
        match overlay.input_focus {
            LearnOverlayInputFocus::CaptureSummary => {
                let s = normalize_overlay_paste(input.pasted, false);
                insert_text_bounded(
                    &mut overlay.summary,
                    input.learn_overlay_cursor,
                    &s,
                    OVERLAY_CAPTURE_SUMMARY_MAX_CHARS,
                );
            }
            LearnOverlayInputFocus::ReviewId => {
                let s = normalize_overlay_paste(input.pasted, true);
                if !s.is_empty() && !overlay.review_id.ends_with(&s) {
                    insert_text_bounded(
                        &mut overlay.review_id,
                        input.learn_overlay_cursor,
                        &s,
                        OVERLAY_ID_MAX_CHARS,
                    );
                    overlay.review_selected_idx = usize::MAX;
                }
            }
            LearnOverlayInputFocus::PromoteId => {
                let s = normalize_overlay_paste(input.pasted, true);
                if !s.is_empty() && !overlay.promote_id.ends_with(&s) {
                    insert_text_bounded(
                        &mut overlay.promote_id,
                        input.learn_overlay_cursor,
                        &s,
                        OVERLAY_ID_MAX_CHARS,
                    );
                }
            }
            LearnOverlayInputFocus::PromoteSlug => {
                let s = normalize_overlay_paste(input.pasted, true);
                if !s.is_empty() && !overlay.promote_slug.ends_with(&s) {
                    insert_text_bounded(
                        &mut overlay.promote_slug,
                        input.learn_overlay_cursor,
                        &s,
                        OVERLAY_ID_MAX_CHARS,
                    );
                }
            }
            LearnOverlayInputFocus::PromotePackId => {
                let s = normalize_overlay_paste(input.pasted, true);
                if !s.is_empty() && !overlay.promote_pack_id.ends_with(&s) {
                    insert_text_bounded(
                        &mut overlay.promote_pack_id,
                        input.learn_overlay_cursor,
                        &s,
                        OVERLAY_ID_MAX_CHARS,
                    );
                }
            }
        }
        return;
    }
    let normalized = crate::chat_runtime::normalize_pasted_text(input.pasted);
    insert_text_bounded(input.input, input.input_cursor, &normalized, usize::MAX);
    *input.history_idx = None;
    *input.slash_menu_index = 0;
}

pub(crate) fn overlay_field_mut_and_max(
    overlay: &mut LearnOverlayState,
) -> (&mut String, usize, bool) {
    match overlay.input_focus {
        LearnOverlayInputFocus::CaptureSummary => (
            &mut overlay.summary,
            OVERLAY_CAPTURE_SUMMARY_MAX_CHARS,
            false,
        ),
        LearnOverlayInputFocus::ReviewId => (&mut overlay.review_id, OVERLAY_ID_MAX_CHARS, true),
        LearnOverlayInputFocus::PromoteId => (&mut overlay.promote_id, OVERLAY_ID_MAX_CHARS, false),
        LearnOverlayInputFocus::PromoteSlug => {
            (&mut overlay.promote_slug, OVERLAY_ID_MAX_CHARS, false)
        }
        LearnOverlayInputFocus::PromotePackId => {
            (&mut overlay.promote_pack_id, OVERLAY_ID_MAX_CHARS, false)
        }
    }
}

pub(crate) fn sync_overlay_cursor_to_focus(overlay: &LearnOverlayState, cursor: &mut usize) {
    let len = match overlay.input_focus {
        LearnOverlayInputFocus::CaptureSummary => char_len(&overlay.summary),
        LearnOverlayInputFocus::ReviewId => char_len(&overlay.review_id),
        LearnOverlayInputFocus::PromoteId => char_len(&overlay.promote_id),
        LearnOverlayInputFocus::PromoteSlug => char_len(&overlay.promote_slug),
        LearnOverlayInputFocus::PromotePackId => char_len(&overlay.promote_pack_id),
    };
    *cursor = (*cursor).min(len);
}

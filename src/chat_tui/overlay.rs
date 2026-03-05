#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LearnOverlayInputFocus {
    CaptureSummary,
    ReviewId,
    PromoteId,
    PromoteSlug,
    PromotePackId,
}

#[derive(Debug, Clone)]
pub(crate) struct LearnOverlayState {
    pub(crate) tab: crate::chat_ui::LearnOverlayTab,
    pub(crate) category_idx: usize,
    pub(crate) summary: String,
    pub(crate) review_id: String,
    pub(crate) promote_id: String,
    pub(crate) promote_target_idx: usize,
    pub(crate) promote_slug: String,
    pub(crate) promote_pack_id: String,
    pub(crate) promote_force: bool,
    pub(crate) input_focus: LearnOverlayInputFocus,
    pub(crate) inline_message: Option<String>,
    pub(crate) review_rows: Vec<String>,
    pub(crate) review_selected_idx: usize,
    pub(crate) assist_on: bool,
    #[allow(dead_code)]
    pub(crate) write_armed: bool,
    pub(crate) logs: Vec<String>,
    pub(crate) pending_submit_line: Option<String>,
    pub(crate) assist_summary: Option<String>,
    pub(crate) summary_choice: crate::chat_ui::LearnOverlaySummaryChoice,
    pub(crate) selected_summary: Option<String>,
}

impl Default for LearnOverlayState {
    fn default() -> Self {
        Self {
            tab: crate::chat_ui::LearnOverlayTab::Capture,
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
            logs: vec!["info: Preflight check complete. Waiting for user action.".to_string()],
            pending_submit_line: None,
            assist_summary: None,
            summary_choice: crate::chat_ui::LearnOverlaySummaryChoice::Original,
            selected_summary: None,
        }
    }
}

pub(crate) fn cycle_overlay_focus(overlay: &mut LearnOverlayState, reverse: bool) {
    use LearnOverlayInputFocus as F;
    overlay.input_focus = match overlay.tab {
        crate::chat_ui::LearnOverlayTab::Capture => F::CaptureSummary,
        crate::chat_ui::LearnOverlayTab::Review => F::ReviewId,
        crate::chat_ui::LearnOverlayTab::Promote => {
            let order = [F::PromoteId, F::PromoteSlug, F::PromotePackId];
            let idx = order
                .iter()
                .position(|v| *v == overlay.input_focus)
                .unwrap_or(0);
            let next = if reverse {
                if idx == 0 {
                    order.len() - 1
                } else {
                    idx - 1
                }
            } else {
                (idx + 1) % order.len()
            };
            order[next]
        }
    };
}

pub(crate) fn push_overlay_log_dedup(overlay: &mut LearnOverlayState, msg: &str) {
    if overlay.logs.last().map(|s| s.as_str()) != Some(msg) {
        overlay.logs.push(msg.to_string());
    }
}

pub(crate) fn push_overlay_log_unique(overlay: &mut LearnOverlayState, msg: &str) {
    if !overlay.logs.iter().any(|s| s == msg) {
        overlay.logs.push(msg.to_string());
    }
}

pub(crate) fn set_overlay_next_steps_capture(overlay: &mut LearnOverlayState) {
    let step_2 = if overlay.assist_on {
        "Press Enter to save using Assist (calls LLM, uses tokens). Ctrl+A disables Assist."
    } else {
        "Press Enter to save locally (no LLM assist call)."
    };
    let assist = if overlay.assist_on { "ON" } else { "OFF" };
    overlay.inline_message = Some(format!("Assist {assist}. {step_2} Esc closes."));
}

pub(crate) fn set_overlay_next_steps_promote(overlay: &mut LearnOverlayState) {
    let step_2 = "Step 2: Press Enter to publish.";
    overlay.inline_message = Some(format!(
        "Step 1: Confirm target + required fields. {step_2} Step 3: Press Esc to close."
    ));
}

pub(crate) fn assist_summary_stub(summary: &str) -> String {
    if summary.trim().is_empty() {
        String::new()
    } else {
        format!(
            "Refined summary: {}",
            summary.trim().replace('"', "'").replace("\\", "")
        )
    }
}

pub(crate) fn overlay_pending_message_for_submit(line: &str) -> Option<&'static str> {
    if line.starts_with("/learn capture") && line.contains("--assist") {
        Some("Enhancing summary")
    } else {
        None
    }
}

pub(crate) fn overlay_effective_summary(overlay: &LearnOverlayState) -> String {
    match overlay.summary_choice {
        crate::chat_ui::LearnOverlaySummaryChoice::Assist => overlay
            .assist_summary
            .as_ref()
            .cloned()
            .unwrap_or_else(|| overlay.summary.clone()),
        _ => overlay.summary.clone(),
    }
}

pub(crate) fn learn_overlay_focus_label(focus: LearnOverlayInputFocus) -> &'static str {
    match focus {
        LearnOverlayInputFocus::CaptureSummary => "capture.summary",
        LearnOverlayInputFocus::ReviewId => "review.id",
        LearnOverlayInputFocus::PromoteId => "promote.id",
        LearnOverlayInputFocus::PromoteSlug => "promote.slug",
        LearnOverlayInputFocus::PromotePackId => "promote.pack_id",
    }
}

pub(crate) fn build_overlay_promote_submit_line(
    overlay: &LearnOverlayState,
) -> Result<String, String> {
    if overlay.promote_id.trim().is_empty() {
        return Err("promote id: <required>".to_string());
    }
    let target = match overlay.promote_target_idx {
        0 => "check",
        1 => "pack",
        _ => "agents",
    };
    let mut line = format!("/learn promote {} --to {target}", overlay.promote_id);
    if target == "check" {
        if overlay.promote_slug.trim().is_empty() {
            return Err("slug: <required for check>".to_string());
        }
        line.push_str(&format!(" --slug {}", overlay.promote_slug));
    }
    if target == "pack" {
        if overlay.promote_pack_id.trim().is_empty() {
            return Err("pack_id: <required for pack>".to_string());
        }
        line.push_str(&format!(" --pack-id {}", overlay.promote_pack_id));
    }
    if overlay.promote_force {
        line.push_str(" --force");
    }
    Ok(line)
}

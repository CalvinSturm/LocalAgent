use crate::tui::state::UiState;

#[derive(Clone)]
pub(crate) struct ActiveQueueRow {
    pub(crate) sequence_no: u64,
    pub(crate) kind: String,
    pub(crate) status: String,
    pub(crate) delivery_phrase: String,
}

pub(crate) fn push_approvals_refresh_error_once(logs: &mut Vec<String>, err: &anyhow::Error) {
    let msg = format!("approvals refresh failed: {err}");
    if !logs.iter().any(|l| l == &msg) {
        logs.push(msg);
    }
}

pub(crate) fn refresh_approvals_with_auto_open(
    ui_state: &mut UiState,
    approvals_path: &std::path::Path,
    show_approvals: &mut bool,
    previous_pending: &mut usize,
    logs: &mut Vec<String>,
) {
    match ui_state.refresh_approvals(approvals_path) {
        Ok(()) => {
            let now_pending = ui_state.pending_approval_count();
            if *previous_pending == 0 && now_pending > 0 {
                *show_approvals = true;
            }
            *previous_pending = now_pending;
        }
        Err(e) => push_approvals_refresh_error_once(logs, &e),
    }
}

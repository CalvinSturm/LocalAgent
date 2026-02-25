use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};

use crate::RunArgs;

pub(crate) fn is_text_input_mods(mods: KeyModifiers) -> bool {
    mods.is_empty() || mods == KeyModifiers::SHIFT
}

pub(crate) fn normalize_pasted_text(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

pub(crate) fn mouse_scroll_delta(me: &MouseEvent) -> Option<isize> {
    let step = if me.modifiers.contains(KeyModifiers::SHIFT) {
        12
    } else {
        3
    };
    match me.kind {
        MouseEventKind::ScrollUp => Some(-(step as isize)),
        MouseEventKind::ScrollDown => Some(step as isize),
        _ => None,
    }
}

pub(crate) fn transcript_max_scroll_lines(
    transcript: &[(String, String)],
    streaming_assistant: &str,
) -> usize {
    let mut chat_text = transcript
        .iter()
        .map(|(role, text)| format!("{}: {}", role.to_uppercase(), text))
        .collect::<Vec<_>>()
        .join("\n\n");
    if !streaming_assistant.is_empty() {
        if !chat_text.is_empty() {
            chat_text.push_str("\n\n");
        }
        chat_text.push_str(&format!("ASSISTANT: {}", streaming_assistant));
    }
    chat_text.lines().count().saturating_sub(1)
}

pub(crate) fn adjust_transcript_scroll(current: usize, delta: isize, max_scroll: usize) -> usize {
    let base = if current == usize::MAX {
        max_scroll
    } else {
        current.min(max_scroll)
    };
    if delta < 0 {
        base.saturating_sub((-delta) as usize)
    } else {
        base.saturating_add(delta as usize).min(max_scroll)
    }
}

pub(crate) fn normalize_path_for_display(path: String) -> String {
    if cfg!(windows) {
        if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
            return format!(r"\\{}", rest);
        }
        if let Some(rest) = path.strip_prefix(r"\\?\") {
            return rest.to_string();
        }
    }
    path
}

pub(crate) fn chat_mode_label(run: &RunArgs) -> &'static str {
    let web_enabled = run.mcp.iter().any(|m| m == "playwright");
    let is_safe = !web_enabled && !run.enable_write_tools && !run.allow_write && !run.allow_shell;
    let is_code = !web_enabled && run.enable_write_tools && run.allow_write && run.allow_shell;
    let is_web = web_enabled && !run.enable_write_tools && !run.allow_write && !run.allow_shell;
    if is_safe {
        "Safe"
    } else if is_code {
        "Code"
    } else if is_web {
        "Web"
    } else {
        "Custom"
    }
}

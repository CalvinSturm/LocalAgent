use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};

use crate::tui::state::UiState;

pub(crate) fn activity_status_hint(ui_state: &UiState, status: &str) -> Option<String> {
    if status != "running" {
        return None;
    }
    if let Some(tool) = ui_state
        .tool_calls
        .iter()
        .rev()
        .find(|t| t.status == "running" || t.status == "STALL")
    {
        let secs = (tool.running_for_ms / 1000).max(1);
        if tool.status == "STALL" {
            return Some(format!(
                "stalled on {} ({}s • esc to interrupt)",
                tool.tool_name, secs
            ));
        }
        return Some(format!(
            "running {} ({}s • esc to interrupt)",
            tool.tool_name, secs
        ));
    }
    if ui_state.net_status == "SLOW" {
        return Some("waiting on provider retry (esc to interrupt)".to_string());
    }
    Some("generating response (esc to interrupt)".to_string())
}

fn is_diff_addition_line(line: &str) -> bool {
    line.starts_with('+') && !line.starts_with("+++")
}

fn is_diff_deletion_line(line: &str) -> bool {
    line.starts_with('-') && !line.starts_with("---")
}

pub(crate) fn styled_chat_text(chat_text: &str, base_style: Style) -> (Text<'static>, String) {
    let mut lines = Vec::<Line<'static>>::new();
    let mut plain = String::new();
    let mut change_line_no = 1usize;

    for raw in chat_text.lines() {
        let (content, style) = if is_diff_addition_line(raw) {
            let numbered = format!("{:>4} | {raw}", change_line_no);
            change_line_no = change_line_no.saturating_add(1);
            (numbered, Style::default().fg(Color::Green))
        } else if is_diff_deletion_line(raw) {
            let numbered = format!("{:>4} | {raw}", change_line_no);
            change_line_no = change_line_no.saturating_add(1);
            (numbered, Style::default().fg(Color::Red))
        } else {
            (raw.to_string(), base_style)
        };
        if !plain.is_empty() {
            plain.push('\n');
        }
        plain.push_str(&content);
        lines.push(Line::from(Span::styled(content, style)));
    }

    (Text::from(lines), plain)
}

pub(crate) fn localagent_banner(_tick: u64) -> String {
    let version = format!("v{}", env!("CARGO_PKG_VERSION"));
    let raw = format!(
        r#"
██╗      █████╗  █████╗  █████╗ ██╗      █████╗  ██████╗ ███████╗███╗  ██╗████████╗
██║     ██╔══██╗██╔══██╗██╔══██╗██║     ██╔══██╗██╔════╝ ██╔════╝████╗ ██║╚══██╔══╝
██║     ██║  ██║██║  ╚═╝███████║██║     ███████║██║  ██╗ █████╗  ██╔██╗██║   ██║   
██║     ██║  ██║██║  ██╗██╔══██║██║     ██╔══██║██║  ╚██╗██╔══╝  ██║╚████║   ██║   
███████╗╚█████╔╝╚█████╔╝██║  ██║███████╗██║  ██║╚██████╔╝███████╗██║ ╚███║   ██║   
╚══════╝ ╚════╝  ╚════╝ ╚═╝  ╚═╝╚══════╝╚═╝  ╚═╝ ╚═════╝ ╚══════╝╚═╝  ╚══╝   ╚═╝   
                                                                            {version}"#
    );
    raw.lines().collect::<Vec<_>>().join("\n")
}

pub(crate) fn horizontal_rule(width: u16) -> String {
    "─".repeat(width as usize)
}

pub(crate) fn wrapped_line_count(text: &str, width: usize) -> usize {
    if width == 0 {
        return 1;
    }
    let mut total = 0usize;
    for line in text.split('\n') {
        let chars = line.chars().count();
        let line_count = if chars == 0 {
            1
        } else {
            (chars - 1) / width + 1
        };
        total = total.saturating_add(line_count);
    }
    total.max(1)
}

pub(crate) fn compact_status_detail(s: &str, max_chars: usize) -> String {
    let compact = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= max_chars {
        return compact;
    }
    let mut out = compact;
    out.truncate(max_chars.saturating_sub(3));
    out.push_str("...");
    out
}

pub(crate) fn centered_multiline(text: &str, width: u16, top_pad: usize) -> String {
    let width = width as usize;
    let lines = text.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for _ in 0..top_pad {
        out.push('\n');
    }
    for (idx, line) in lines.iter().enumerate() {
        let line_width = line.chars().count();
        let left_pad = width.saturating_sub(line_width) / 2;
        out.push_str(&" ".repeat(left_pad));
        out.push_str(line);
        if idx + 1 < lines.len() {
            out.push('\n');
        }
    }
    out
}

pub(crate) fn centered_left_block(text: &str, width: u16, top_pad: usize) -> String {
    let width = width as usize;
    let lines = text.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        return String::new();
    }
    let block_width = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let left_pad = width.saturating_sub(block_width) / 2;
    let mut out = String::new();
    for _ in 0..top_pad {
        out.push('\n');
    }
    for (idx, line) in lines.iter().enumerate() {
        out.push_str(&" ".repeat(left_pad));
        out.push_str(line);
        if idx + 1 < lines.len() {
            out.push('\n');
        }
    }
    out
}

pub(crate) fn rotating_status_word<'a>(
    words: &'a [&'a str],
    think_tick: u64,
    refresh_ms: u64,
    salt: u64,
) -> &'a str {
    if words.is_empty() {
        return "";
    }
    let ticks_per_step = (15_000u64 / refresh_ms.max(1)).max(1);
    let bucket = think_tick / ticks_per_step;
    let mut x = bucket ^ salt;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    words[(x as usize) % words.len()]
}

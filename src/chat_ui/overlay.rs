use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LearnOverlayTab {
    Capture,
    Review,
    Promote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LearnOverlaySummaryChoice {
    Original,
    Assist,
}

#[derive(Debug, Clone)]
pub(crate) struct LearnOverlayRenderModel {
    pub(crate) tab: LearnOverlayTab,
    pub(crate) selected_category_idx: usize,
    pub(crate) summary: String,
    pub(crate) review_id: String,
    pub(crate) promote_id: String,
    pub(crate) promote_target_idx: usize,
    pub(crate) promote_slug: String,
    pub(crate) promote_pack_id: String,
    pub(crate) promote_force: bool,
    pub(crate) input_focus: String,
    pub(crate) inline_message: Option<String>,
    pub(crate) review_rows: Vec<String>,
    pub(crate) review_selected_idx: usize,
    pub(crate) assist_on: bool,
    #[allow(dead_code)]
    pub(crate) equivalent_cli: String,
    #[allow(dead_code)]
    pub(crate) target_path: String,
    pub(crate) overlay_logs: Vec<String>,
    pub(crate) assist_summary: Option<String>,
    pub(crate) summary_choice: LearnOverlaySummaryChoice,
    pub(crate) selected_summary: Option<String>,
    pub(crate) active_input_cursor: usize,
    pub(crate) cursor_visible: bool,
}

pub(crate) fn draw_learn_overlay(
    f: &mut ratatui::Frame<'_>,
    overlay: &LearnOverlayRenderModel,
    ui_tick: u64,
) {
    let area = centered_rect(92, 86, f.area());
    f.render_widget(Clear, area);
    f.render_widget(
        Block::default()
            .title(" Learn Overlay ")
            .borders(Borders::ALL)
            .style(Style::default().fg(Color::Yellow)),
        area,
    );

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(14),
            Constraint::Length(1),
        ])
        .split(area);

    let tabs = format!(
        "{}  {}  {}",
        tab_label(1, LearnOverlayTab::Capture, overlay.tab),
        tab_label(2, LearnOverlayTab::Review, overlay.tab),
        tab_label(3, LearnOverlayTab::Promote, overlay.tab)
    );
    let target = match overlay.tab {
        LearnOverlayTab::Capture => "Target: Capture",
        LearnOverlayTab::Review => "Target: Review",
        LearnOverlayTab::Promote => "Target: Promote",
    };
    let pad = outer[0]
        .width
        .saturating_sub((tabs.chars().count() + target.chars().count()) as u16)
        as usize;
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(tabs, Style::default().fg(Color::Yellow)),
            Span::raw(" ".repeat(pad)),
            Span::styled(target, Style::default().fg(Color::Gray)),
        ]))
        .wrap(Wrap { trim: false }),
        outer[0],
    );
    f.render_widget(
        Paragraph::new(crate::chat_view_utils::horizontal_rule(outer[1].width))
            .style(Style::default().fg(Color::DarkGray))
            .wrap(Wrap { trim: false }),
        outer[1],
    );

    match overlay.tab {
        LearnOverlayTab::Capture => draw_learn_capture_form(f, outer[2], overlay),
        LearnOverlayTab::Review => draw_learn_review_form(f, outer[2], overlay),
        LearnOverlayTab::Promote => draw_learn_promote_form(f, outer[2], overlay),
    }

    let action_hint = match overlay.tab {
        LearnOverlayTab::Capture => {
            if overlay.assist_on {
                "Capture: Enter Save+Enhance | Ctrl+A Assist:ON | Ctrl+G Generate | Ctrl+O/R Pick | Tab Field | Esc Close"
                    .to_string()
            } else {
                "Capture: Enter Save | Ctrl+A Assist:OFF | Ctrl+G Generate | Ctrl+O/R Pick | Tab Field | Esc Close"
                    .to_string()
            }
        }
        LearnOverlayTab::Review => {
            "Review: Enter List/Show | Up/Down Rows | Tab Field | Esc Close".to_string()
        }
        LearnOverlayTab::Promote => {
            "Promote: Up/Down Target | Enter Publish | Ctrl+F Force | Tab Field | Esc Close"
                .to_string()
        }
    };
    let last_log = overlay
        .overlay_logs
        .last()
        .cloned()
        .unwrap_or_else(|| "learn ready".to_string());
    if overlay.inline_message.as_deref() == Some("Enhancing summary") {
        let wave = ["▁", "▂", "▃", "▄", "▅", "▄", "▃", "▂"];
        let phase = ((ui_tick / 3) % wave.len() as u64) as usize;
        let dots = ".".repeat(((ui_tick / 6) % 4) as usize);
        let glow_style = Style::default().fg(match phase {
            0 | 1 => Color::Blue,
            2 | 3 => Color::Cyan,
            4 | 5 => Color::White,
            _ => Color::Blue,
        });
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(wave[phase], glow_style),
                Span::raw(" "),
                Span::styled(
                    format!("Enhancing summary{dots}"),
                    Style::default().fg(Color::Blue),
                ),
                Span::raw("  |  "),
                Span::styled(
                    format!("Last: {last_log}"),
                    Style::default().fg(Color::Gray),
                ),
            ]))
            .wrap(Wrap { trim: false }),
            outer[3],
        );
    } else {
        let primary = overlay.inline_message.as_deref().unwrap_or(&action_hint);
        let status_line = format!("{primary}  |  Last: {last_log}");
        let status_line_wrapped = soft_break_long_tokens(&status_line, outer[3].width as usize);
        f.render_widget(
            Paragraph::new(status_line_wrapped)
                .wrap(Wrap { trim: true })
                .style(Style::default().fg(Color::Gray)),
            outer[3],
        );
    }
}

fn draw_learn_capture_form(
    f: &mut ratatui::Frame<'_>,
    area: ratatui::layout::Rect,
    overlay: &LearnOverlayRenderModel,
) {
    let block = Block::default()
        .title(" - CAPTURE FORM ")
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(8),
            Constraint::Length(5),
        ])
        .split(inner);
    let step_lines = [
        "1) Enter summary",
        "2) Enter saves draft",
        "3) Promote publishes",
    ]
    .join("\n");
    let category_section = sections[0];
    let category_inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(3)])
        .split(category_section);
    f.render_widget(
        Paragraph::new(step_lines.clone())
            .style(Style::default().fg(Color::Gray))
            .wrap(Wrap { trim: false }),
        category_inner[0],
    );
    let categories = [
        ("workflow_hint", "workflow"),
        ("prompt_guidance", "guidance"),
        ("check_candidate", "check"),
    ];
    let mut category_lines: Vec<Line<'static>> = Vec::new();
    for (idx, (_value, label)) in categories.iter().enumerate() {
        let selected = idx == overlay.selected_category_idx;
        let prefix = if selected { "> " } else { "  " };
        category_lines.push(Line::from(vec![
            Span::raw(prefix),
            Span::styled(
                format!("[ {label} ]"),
                if selected {
                    Style::default().fg(Color::Black).bg(Color::Yellow)
                } else {
                    Style::default().fg(Color::Gray)
                },
            ),
        ]));
    }
    f.render_widget(
        Paragraph::new(category_lines).wrap(Wrap { trim: false }),
        category_inner[1],
    );
    let summary_section = sections[1];
    let summary_inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Length(2),
        ])
        .split(summary_section);
    let summary_active = overlay.input_focus == "capture.summary";
    let summary_label = if summary_active {
        if overlay.summary.trim().is_empty() {
            "Summary (example: Document dependency upgrades) [active]"
        } else {
            "Summary [active]"
        }
    } else if overlay.summary.trim().is_empty() {
        "Summary (example: Document dependency upgrades)"
    } else {
        "Summary"
    };
    f.render_widget(
        Paragraph::new(summary_label)
            .style(Style::default().fg(if summary_active {
                Color::Yellow
            } else {
                Color::Gray
            }))
            .wrap(Wrap { trim: false }),
        summary_inner[0],
    );
    let summary_text = if overlay.summary.trim().is_empty() {
        "< Document the dependency upgrade process to avoid regressions >".to_string()
    } else {
        let summary_with_caret = if summary_active {
            render_with_caret(
                &overlay.summary,
                overlay.active_input_cursor,
                overlay.cursor_visible,
            )
        } else {
            overlay.summary.clone()
        };
        right_fit_single_line(
            &summary_with_caret,
            summary_inner[1].width.saturating_sub(4) as usize,
        )
    };
    f.render_widget(
        Paragraph::new(summary_text)
            .style(Style::default().fg(if summary_active {
                Color::Yellow
            } else {
                Color::White
            }))
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .style(Style::default().fg(Color::DarkGray)),
            ),
        summary_inner[1],
    );
    f.render_widget(
        Paragraph::new(format!(
            "Agent assist comparison: selected={} | Ctrl+G generate | Ctrl+O original | Ctrl+R assist",
            match overlay.summary_choice {
                LearnOverlaySummaryChoice::Original => "original",
                LearnOverlaySummaryChoice::Assist => "assist",
            }
        ))
        .style(Style::default().fg(Color::Gray))
        .wrap(Wrap { trim: false }),
        summary_inner[2],
    );
    let assist_preview = overlay
        .assist_summary
        .as_deref()
        .map(|s| right_fit_single_line(s, summary_inner[3].width.saturating_sub(2) as usize))
        .unwrap_or_else(|| "<assist not generated>".to_string());
    f.render_widget(
        Paragraph::new(format!("Assist preview: {assist_preview}"))
            .style(Style::default().fg(Color::Gray))
            .wrap(Wrap { trim: false }),
        summary_inner[3],
    );
    let receipt_section = sections[2];
    let receipt_text = overlay
        .selected_summary
        .as_deref()
        .map(|s| format!("Receipt: {s}"))
        .unwrap_or_else(|| "Receipt: Choose summary and press Enter to confirm.".to_string());
    f.render_widget(
        Paragraph::new(receipt_text)
            .style(Style::default().fg(Color::Yellow))
            .wrap(Wrap { trim: false }),
        receipt_section,
    );
}

fn draw_learn_review_form(
    f: &mut ratatui::Frame<'_>,
    area: ratatui::layout::Rect,
    overlay: &LearnOverlayRenderModel,
) {
    let block = Block::default()
        .title(" - REVIEW FORM ")
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let selected = if overlay.review_id.trim().is_empty() {
        "<empty>".to_string()
    } else {
        overlay.review_id.clone()
    };
    let rows = if overlay.review_rows.is_empty() {
        "• (no rows loaded) press Enter to load list".to_string()
    } else {
        overlay
            .review_rows
            .iter()
            .enumerate()
            .map(|(i, r)| {
                if i == overlay.review_selected_idx {
                    format!("> {r}")
                } else {
                    format!("  {r}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let id_label = if overlay.input_focus == "review.id" {
        "Selected ID [active]"
    } else {
        "Selected ID"
    };
    let mut text = format!(
        "Mode: list/show\n\n{id_label}: {selected}\nField focus: {}\n\nRows:\n{rows}\n\nEnter runs read-only list/show.",
        overlay.input_focus
    );
    if overlay.input_focus == "review.id" && !overlay.review_id.trim().is_empty() {
        text = format!(
            "Mode: list/show\n\n{id_label}: {}\nField focus: {}\n\nRows:\n{rows}\n\nEnter runs read-only list/show.",
            render_with_caret(
                &overlay.review_id,
                overlay.active_input_cursor,
                overlay.cursor_visible
            ),
            overlay.input_focus
        );
    }
    if let Some(msg) = overlay.inline_message.as_deref() {
        text.push_str(&format!("\n\n{msg}"));
    }
    let wrapped = soft_break_long_tokens(&text, inner.width.saturating_sub(2) as usize);
    f.render_widget(Paragraph::new(wrapped).wrap(Wrap { trim: false }), inner);
}

fn draw_learn_promote_form(
    f: &mut ratatui::Frame<'_>,
    area: ratatui::layout::Rect,
    overlay: &LearnOverlayRenderModel,
) {
    let block = Block::default()
        .title(" - PROMOTE FORM ")
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let targets = ["check", "pack", "agents"];
    let target = targets[overlay.promote_target_idx.min(2)];
    let id_label = if overlay.input_focus == "promote.id" {
        "ID (required) [active]"
    } else {
        "ID (required)"
    };
    let slug_label = if overlay.input_focus == "promote.slug" {
        "slug [active]"
    } else {
        "slug"
    };
    let pack_label = if overlay.input_focus == "promote.pack_id" {
        "pack_id [active]"
    } else {
        "pack_id"
    };
    let promote_id_display = if overlay.promote_id.trim().is_empty() {
        "<required>".to_string()
    } else if overlay.input_focus == "promote.id" {
        render_with_caret(
            &overlay.promote_id,
            overlay.active_input_cursor,
            overlay.cursor_visible,
        )
    } else {
        overlay.promote_id.clone()
    };
    let promote_slug_display = if overlay.promote_slug.trim().is_empty() {
        "<empty>".to_string()
    } else if overlay.input_focus == "promote.slug" {
        render_with_caret(
            &overlay.promote_slug,
            overlay.active_input_cursor,
            overlay.cursor_visible,
        )
    } else {
        overlay.promote_slug.clone()
    };
    let promote_pack_display = if overlay.promote_pack_id.trim().is_empty() {
        "<empty>".to_string()
    } else if overlay.input_focus == "promote.pack_id" {
        render_with_caret(
            &overlay.promote_pack_id,
            overlay.active_input_cursor,
            overlay.cursor_visible,
        )
    } else {
        overlay.promote_pack_id.clone()
    };
    let mut text = format!(
        "{id_label}: {}\nTarget: {target}\n{slug_label}: {}\n{pack_label}: {}\n\nforce:{}",
        promote_id_display,
        promote_slug_display,
        promote_pack_display,
        if overlay.promote_force { "ON" } else { "off" }
    );
    text.push_str(&format!(
        "\n\nField focus: {}\nTarget switch: [Up/Down]\nField focus cycle: [Tab]/[Shift+Tab]",
        overlay.input_focus
    ));
    if let Some(msg) = overlay.inline_message.as_deref() {
        text.push_str(&format!("\n\n{msg}"));
    }
    let wrapped = soft_break_long_tokens(&text, inner.width.saturating_sub(2) as usize);
    f.render_widget(Paragraph::new(wrapped).wrap(Wrap { trim: false }), inner);
}

fn soft_break_long_tokens(input: &str, width: usize) -> String {
    let maxw = width.max(8);
    let mut out = String::with_capacity(input.len() + input.len() / maxw + 8);
    let mut col = 0usize;
    for ch in input.chars() {
        if ch == '\n' {
            out.push('\n');
            col = 0;
            continue;
        }
        if col >= maxw {
            out.push('\n');
            col = 0;
        }
        out.push(ch);
        col += 1;
    }
    out
}

fn right_fit_single_line(input: &str, width: usize) -> String {
    let maxw = width.max(4);
    let chars: Vec<char> = input.chars().collect();
    if chars.len() <= maxw {
        return input.to_string();
    }
    let keep = maxw.saturating_sub(1);
    let tail: String = chars[chars.len().saturating_sub(keep)..].iter().collect();
    format!("…{tail}")
}

pub(crate) fn render_with_caret(input: &str, cursor: usize, visible: bool) -> String {
    let mut chars: Vec<char> = input.chars().collect();
    let idx = cursor.min(chars.len());
    if visible {
        chars.insert(idx, '|');
    }
    chars.into_iter().collect()
}

fn tab_label(num: u8, tab: LearnOverlayTab, _active: LearnOverlayTab) -> String {
    let label = match tab {
        LearnOverlayTab::Capture => "Capture",
        LearnOverlayTab::Review => "Review",
        LearnOverlayTab::Promote => "Promote",
    };
    format!("[{num}] {label}")
}

fn centered_rect(
    percent_x: u16,
    percent_y: u16,
    r: ratatui::layout::Rect,
) -> ratatui::layout::Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap};
use ratatui::Frame;

use crate::tui::state::{ApprovalRow, PlanRow, ToolRow, UiState};

pub fn draw(frame: &mut Frame<'_>, state: &UiState, approvals_selected: usize) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(10),
            Constraint::Length(5),
        ])
        .split(frame.area());

    let top = Line::from(format!(
        "MODE:{} AUTH:{} PLAN:{} MCP_ENF:{} BUD:T{}/- W{} A{} MCP:{} PIN:{} SCHEMA:{} NET:{} CANCEL:{} run={} step={} provider={} model={} policy={} exit={}",
        state.mode_label,
        state.authority_label,
        state.enforce_plan_tools_effective.to_ascii_uppercase(),
        state.mcp_pin_enforcement,
        state.total_tool_execs,
        state.filesystem_write_execs,
        state.pending_approval_count(),
        state.mcp_status_compact(),
        state.mcp_pin_state,
        if state.schema_repair_seen { "FIX" } else { "OK" },
        state.net_status,
        state.cancel_lifecycle,
        if state.run_id.is_empty() { "-" } else { &state.run_id },
        state.step,
        state.provider,
        state.model,
        state.policy_hash_short(),
        state.exit_reason.as_deref().unwrap_or("-")
    ));
    frame.render_widget(Paragraph::new(top), outer[0]);

    let sticky = Line::from(format!(
        "step={} goal=\"{}\" allow={} next={} view={} (v=toggle)",
        state.current_step_id,
        state.current_step_goal,
        state.step_allowed_tools_compact(),
        state.next_hint,
        if state.show_details {
            "expanded"
        } else {
            "compact"
        }
    ));
    frame.render_widget(Paragraph::new(sticky), outer[1]);

    let mid = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(outer[2]);
    frame.render_widget(
        Paragraph::new(state.assistant_text.clone())
            .block(Block::default().title("Assistant").borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        mid[0],
    );
    let has_plan = !state.plan_items.is_empty();
    let right = if state.show_details && has_plan {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(7),
                Constraint::Length(6),
                Constraint::Percentage(45),
                Constraint::Percentage(55),
            ])
            .split(mid[1])
    } else if state.show_details {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(7),
                Constraint::Percentage(48),
                Constraint::Percentage(52),
            ])
            .split(mid[1])
    } else if has_plan {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(6),
                Constraint::Percentage(50),
                Constraint::Percentage(50),
            ])
            .split(mid[1])
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(mid[1])
    };

    if state.show_details {
        let latest_tool = state.tool_calls.last();
        let diag = format!(
            "latest_status={}\nlatest_decision={}\nlatest_reason={}\neffective_plan_enf={}\nmcp_pin_enf={}\nauthority={}\ncancel={}\nmcp_pin={}\nmcp_lifecycle={}\nmcp_running_for_ms={}\nmcp_stalled={}\nschema_repair={}\nlast_failure_class={}\nlast_retry_count={}\nlast_tool={}\nstep_allowed={}\nusage:r={} w={} sh={} net={} br={}",
            latest_tool.map(|t| t.status.as_str()).unwrap_or("-"),
            latest_tool
                .and_then(|t| t.decision.as_deref())
                .unwrap_or("-"),
            latest_tool
                .and_then(|t| t.decision_reason.as_deref())
                .unwrap_or("-"),
            state.enforce_plan_tools_effective,
            state.mcp_pin_enforcement,
            state.authority_label,
            state.cancel_lifecycle,
            state.mcp_pin_state,
            state.mcp_lifecycle,
            state.mcp_running_for_ms,
            if state.mcp_stalled { "yes" } else { "no" },
            if state.schema_repair_seen {
                "on"
            } else {
                "off"
            },
            state.last_failure_class,
            state.last_tool_retry_count,
            state.last_tool_summary(),
            state.step_allowed_tools_compact(),
            state.filesystem_read_execs,
            state.filesystem_write_execs,
            state.shell_execs,
            state.network_execs,
            state.browser_execs
        );
        frame.render_widget(
            Paragraph::new(diag)
                .block(Block::default().title("Diagnostics").borders(Borders::ALL))
                .wrap(Wrap { trim: false }),
            right[0],
        );
    }

    let mut next_right_idx = usize::from(state.show_details);
    if has_plan {
        draw_plan_table(frame, right[next_right_idx], &state.plan_items);
        next_right_idx += 1;
    }

    draw_tools_table(frame, right[next_right_idx], state);
    draw_approvals_table(frame, right[next_right_idx + 1], state, approvals_selected);

    let logs = state.logs.join("\n");
    frame.render_widget(
        Paragraph::new(logs)
            .block(Block::default().title("Logs").borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        outer[3],
    );
}

fn draw_plan_table(frame: &mut Frame<'_>, area: Rect, items: &[PlanRow]) {
    let completed = items
        .iter()
        .filter(|item| item.status == "completed")
        .count();
    let rows = items.iter().map(|item| {
        let mark = match item.status.as_str() {
            "completed" => "x",
            "in_progress" => ">",
            _ => " ",
        };
        Row::new(vec![Cell::from(mark), Cell::from(fit_cell(&item.step, 48))])
    });
    frame.render_widget(
        Table::new(rows, [Constraint::Length(1), Constraint::Min(8)])
            .header(Row::new(vec!["", "Step"]))
            .block(
                Block::default()
                    .title(format!("Plan {completed}/{}", items.len()))
                    .borders(Borders::ALL),
            ),
        area,
    );
}

fn draw_tools_table(frame: &mut Frame<'_>, area: Rect, state: &UiState) {
    let inner_width = area.width.saturating_sub(2).max(1);
    let compact = inner_width < 52;
    let rows = state.tool_calls.iter().map(|t| {
        if compact {
            Row::new(vec![
                Cell::from(fit_cell(&t.tool_name, 9)),
                Cell::from(status_label(&t.status, 10)),
                Cell::from(decision_label(t.decision.as_deref(), 9)),
                Cell::from(reason_label(t)),
            ])
        } else {
            Row::new(vec![
                Cell::from(fit_cell(&t.tool_name, 12)),
                Cell::from(status_label(&t.status, 14)),
                Cell::from(decision_label(t.decision.as_deref(), 13)),
                Cell::from(
                    t.ok.map(|v| if v { "ok" } else { "fail" })
                        .unwrap_or("-")
                        .to_string(),
                ),
                Cell::from(side_effect_label(&t.side_effects).to_string()),
                Cell::from(reason_label(t)),
            ])
        }
    });

    let table = if compact {
        Table::new(
            rows,
            [
                Constraint::Length(9),
                Constraint::Length(10),
                Constraint::Length(9),
                Constraint::Min(4),
            ],
        )
        .header(Row::new(vec!["Tool", "State", "Decision", "Why"]))
        .column_spacing(0)
    } else {
        Table::new(
            rows,
            [
                Constraint::Length(12),
                Constraint::Length(14),
                Constraint::Length(13),
                Constraint::Length(3),
                Constraint::Length(7),
                Constraint::Min(8),
            ],
        )
        .header(Row::new(vec![
            "Tool", "State", "Decision", "OK", "Risk", "Reason",
        ]))
        .column_spacing(0)
    };
    frame.render_widget(
        table.block(Block::default().title("Tools").borders(Borders::ALL)),
        area,
    );
}

fn draw_approvals_table(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &UiState,
    approvals_selected: usize,
) {
    let show_detail = state.show_details && !state.pending_approvals.is_empty() && area.height >= 9;
    let layout = if show_detail {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(7)])
            .split(area)
            .to_vec()
    } else {
        vec![area, Rect::default()]
    };

    let inner_width = layout[0].width.saturating_sub(2).max(1);
    let compact = inner_width < 52;
    let approv_rows = state.pending_approvals.iter().enumerate().map(|(i, a)| {
        let style = if i == approvals_selected {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };
        if compact {
            Row::new(vec![
                Cell::from(short_id(&a.id, 8)),
                Cell::from(approval_status_label(&a.status, 7)),
                Cell::from(fit_cell(&a.tool, 9)),
                Cell::from(a.risk.clone()),
            ])
            .style(style)
        } else {
            Row::new(vec![
                Cell::from(short_id(&a.id, 12)),
                Cell::from(approval_status_label(&a.status, 8)),
                Cell::from(fit_cell(&a.tool, 14)),
                Cell::from(a.risk.clone()),
                Cell::from(fit_cell(&a.created_at, 16)),
            ])
            .style(style)
        }
    });
    let table = if compact {
        Table::new(
            approv_rows,
            [
                Constraint::Length(8),
                Constraint::Length(7),
                Constraint::Length(9),
                Constraint::Min(4),
            ],
        )
        .header(Row::new(vec!["ID", "State", "Tool", "Risk"]))
        .column_spacing(0)
    } else {
        Table::new(
            approv_rows,
            [
                Constraint::Length(12),
                Constraint::Length(8),
                Constraint::Length(14),
                Constraint::Length(7),
                Constraint::Min(14),
            ],
        )
        .header(Row::new(vec![
            "Approval", "State", "Tool", "Risk", "Created",
        ]))
        .column_spacing(0)
    };
    frame.render_widget(
        table.block(
            Block::default()
                .title("Approvals a=approve d=deny r=refresh v=details q=cancel qq=quit")
                .borders(Borders::ALL),
        ),
        layout[0],
    );

    if show_detail {
        if let Some(row) = state.pending_approvals.get(approvals_selected) {
            let detail = approval_detail(row);
            frame.render_widget(
                Paragraph::new(detail)
                    .block(
                        Block::default()
                            .title("Approval Detail")
                            .borders(Borders::ALL),
                    )
                    .wrap(Wrap { trim: false }),
                layout[1],
            );
        }
    }
}

fn reason_label(t: &ToolRow) -> String {
    if t.reason_token == "-" {
        t.decision_reason.clone().unwrap_or_default()
    } else {
        t.reason_token.clone()
    }
}

fn status_label(raw: &str, width: usize) -> String {
    let normalized = match raw {
        "PEND:approval" => {
            if width >= 13 {
                "PEND:APPROVAL"
            } else {
                "PEND:APPR"
            }
        }
        "running" => "RUNNING",
        "detected" => "DETECTED",
        "decided" => "DECIDED",
        other => other,
    };
    fit_cell(normalized, width)
}

fn decision_label(raw: Option<&str>, width: usize) -> String {
    let normalized = match raw.unwrap_or("-") {
        "require_approval" => {
            if width >= 12 {
                "REQ_APPROVAL"
            } else {
                "REQ_APPR"
            }
        }
        "allow" => "ALLOW",
        "deny" => "DENY",
        "-" | "" => "-",
        other => other,
    };
    fit_cell(normalized, width)
}

fn approval_status_label(raw: &str, width: usize) -> String {
    let normalized = match raw {
        "pending" => "PENDING",
        "approved" => "APPROVED",
        "denied" => "DENIED",
        other => other,
    };
    fit_cell(normalized, width)
}

fn side_effect_label(raw: &str) -> &'static str {
    match raw {
        "filesystem_read" => "fs-read",
        "filesystem_write" => "fs-write",
        "shell_exec" => "shell",
        "network" => "network",
        "browser" => "browser",
        _ => "-",
    }
}

fn approval_detail(row: &ApprovalRow) -> String {
    format!(
        "id: {}\ntool: {}  risk: {}  decision: {}\nkey: {} ({})  target: {}\nargs: {}",
        row.id,
        row.tool,
        row.risk,
        approval_status_label(&row.status, 16),
        row.approval_key_short,
        row.approval_key_version,
        row.exec_target,
        row.arguments
    )
}

fn short_id(id: &str, width: usize) -> String {
    if id.chars().count() <= width {
        return id.to_string();
    }
    id.chars().take(width).collect()
}

fn fit_cell(input: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if input.chars().count() <= width {
        return input.to_string();
    }
    if width <= 3 {
        return input.chars().take(width).collect();
    }
    let mut out = input
        .chars()
        .take(width.saturating_sub(3))
        .collect::<String>();
    out.push_str("...");
    out
}

#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, Terminal};

    use super::draw;
    use crate::tui::state::{ApprovalRow, ToolRow, UiState};

    fn rendered_frame(width: u16, height: u16, state: &UiState) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(|f| draw(f, state, 0)).expect("draw");
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
    }

    fn approval_state() -> UiState {
        let mut state = UiState::new(20);
        state.provider = "lmstudio".to_string();
        state.model = "local-model".to_string();
        state.assistant_text = "waiting for approval".to_string();
        state.tool_calls.push(ToolRow {
            tool_call_id: "tc1".to_string(),
            tool_name: "shell".to_string(),
            side_effects: "shell_exec".to_string(),
            decision: Some("require_approval".to_string()),
            decision_source: Some("policy".to_string()),
            reason_token: "policy".to_string(),
            decision_reason: Some("shell requires approval".to_string()),
            status: "PEND:approval".to_string(),
            running_since: None,
            running_for_ms: 0,
            ok: None,
            short_result: String::new(),
        });
        state.pending_approvals.push(ApprovalRow {
            id: "approval-1234567890".to_string(),
            tool: "shell".to_string(),
            status: "pending".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            arguments: r#"{"args":["/c","echo","hi"],"cmd":"cmd"}"#.to_string(),
            risk: "shell".to_string(),
            approval_key_short: "abcdef12".to_string(),
            approval_key_version: "v1".to_string(),
            exec_target: "host".to_string(),
        });
        state
    }

    #[test]
    fn tail_normal_width_preserves_approval_meaning() {
        let state = approval_state();
        let rendered = rendered_frame(120, 30, &state);

        assert!(rendered.contains("PEND:APPROVAL"));
        assert!(rendered.contains("REQ_APPROVAL"));
        assert!(rendered.contains("PEND:APPROVAL REQ_APPROVAL"));
        assert!(!rendered.contains("PEND:APPROVALREQ_APPROVAL"));
        assert!(!rendered.contains("PEND:appro"));
        assert!(!rendered.contains("REQUIRE_A"));
    }

    #[test]
    fn tail_72x20_uses_intentional_approval_abbreviations() {
        let state = approval_state();
        let rendered = rendered_frame(72, 20, &state);

        assert!(rendered.contains("PEND:APPR"));
        assert!(rendered.contains("REQ_APPR"));
        assert!(rendered.contains("PEND:APPR REQ_APPR policy"));
        assert!(!rendered.contains("PEND:APPRREQ_APPRpolicy"));
        assert!(rendered.contains("PENDING"));
        assert!(!rendered.contains("PEND:appro"));
        assert!(!rendered.contains("REQUIRE_A"));
    }

    #[test]
    fn tail_details_include_full_status_decision_reason_and_args() {
        let mut state = approval_state();
        state.show_details = true;
        let rendered = rendered_frame(120, 34, &state);

        assert!(rendered.contains("latest_status=PEND:approval"));
        assert!(rendered.contains("latest_decision=require_approval"));
        assert!(rendered.contains("latest_reason=shell requires approval"));
        assert!(rendered.contains(r#"args: {"args":["/c","echo","hi"],"cmd":"cmd"}"#));
    }
}

use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;
use std::time::Duration;

use anyhow::anyhow;
use crossterm::event::{self, Event as CEvent};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::events::Event;
use crate::tui::input::{map_key, UiAction};
use crate::tui::render::draw;
use crate::tui::state::UiState;

pub fn run_tail(path: &Path, refresh_ms: u64) -> anyhow::Result<()> {
    if !path.exists() {
        return Err(anyhow!("events file not found: {}", path.display()));
    }
    let file = std::fs::OpenOptions::new().read(true).open(path)?;
    let mut reader = BufReader::new(file);
    reader.seek(SeekFrom::Start(0))?;

    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut state = UiState::new(200);
    let mut selected = 0usize;
    loop {
        for line in read_available_lines(&mut reader)? {
            match serde_json::from_str::<Event>(&line) {
                Ok(ev) => state.apply_event(&ev),
                Err(e) => state.push_log(format!("tail parse error: {e}")),
            }
        }
        terminal.draw(|f| draw(f, &state, selected))?;
        if event::poll(Duration::from_millis(refresh_ms))? {
            if let CEvent::Key(k) = event::read()? {
                if let Some(act) = map_key(k) {
                    match act {
                        UiAction::Quit => break,
                        UiAction::Up => {
                            selected = selected.saturating_sub(1);
                        }
                        UiAction::Down => {
                            if selected + 1 < state.pending_approvals.len() {
                                selected += 1;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

#[cfg(test)]
pub fn parse_jsonl_into_state(input: &str, max_logs: usize) -> UiState {
    let mut state = UiState::new(max_logs);
    for line in input.lines().filter(|l| !l.trim().is_empty()) {
        match serde_json::from_str::<Event>(line) {
            Ok(ev) => state.apply_event(&ev),
            Err(e) => state.push_log(format!("tail parse error: {e}")),
        }
    }
    state
}

fn read_available_lines<R: BufRead>(reader: &mut R) -> anyhow::Result<Vec<String>> {
    let mut lines = Vec::new();
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            break;
        }
        lines.push(line.trim_end_matches(['\r', '\n']).to_string());
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use crate::events::{Event, EventKind};

    use super::parse_jsonl_into_state;

    #[test]
    fn parse_jsonl_updates_state() {
        let e1 = serde_json::to_string(&Event::new(
            "r1".to_string(),
            1,
            EventKind::ModelDelta,
            serde_json::json!({"delta":"hi"}),
        ))
        .expect("e1");
        let e2 = serde_json::to_string(&Event::new(
            "r1".to_string(),
            2,
            EventKind::RunEnd,
            serde_json::json!({"exit_reason":"ok"}),
        ))
        .expect("e2");
        let s = parse_jsonl_into_state(&(e1 + "\n" + &e2), 20);
        assert_eq!(s.assistant_text, "hi");
        assert_eq!(s.exit_reason.as_deref(), Some("ok"));
    }
}

use crossterm::event::{KeyCode, KeyEvent};

#[derive(Debug, Clone, Copy)]
pub enum UiAction {
    Quit,
    Up,
    Down,
    Approve,
    Deny,
    Refresh,
    Tab,
}

pub fn map_key(key: KeyEvent) -> Option<UiAction> {
    match key.code {
        KeyCode::Char('q') => Some(UiAction::Quit),
        KeyCode::Char('j') | KeyCode::Down => Some(UiAction::Down),
        KeyCode::Char('k') | KeyCode::Up => Some(UiAction::Up),
        KeyCode::Char('a') => Some(UiAction::Approve),
        KeyCode::Char('d') => Some(UiAction::Deny),
        KeyCode::Char('r') => Some(UiAction::Refresh),
        KeyCode::Tab => Some(UiAction::Tab),
        _ => None,
    }
}

pub(crate) fn char_len(s: &str) -> usize {
    s.chars().count()
}

pub(crate) fn byte_index_for_char(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(idx, _)| idx)
        .unwrap_or(s.len())
}

pub(crate) fn clamp_cursor(s: &str, cursor: &mut usize) {
    *cursor = (*cursor).min(char_len(s));
}

pub(crate) fn insert_text_bounded(
    dst: &mut String,
    cursor: &mut usize,
    src: &str,
    max_chars: usize,
) {
    if src.is_empty() {
        return;
    }
    let used = char_len(dst);
    if used >= max_chars {
        return;
    }
    clamp_cursor(dst, cursor);
    let take_n = max_chars - used;
    let chunk: String = src.chars().take(take_n).collect();
    let at = byte_index_for_char(dst, *cursor);
    dst.insert_str(at, &chunk);
    *cursor += char_len(&chunk);
}

pub(crate) fn delete_char_before_cursor(dst: &mut String, cursor: &mut usize) {
    clamp_cursor(dst, cursor);
    if *cursor == 0 {
        return;
    }
    let end = byte_index_for_char(dst, *cursor);
    let start = byte_index_for_char(dst, *cursor - 1);
    dst.replace_range(start..end, "");
    *cursor = cursor.saturating_sub(1);
}

pub(crate) fn normalize_overlay_paste(pasted: &str, single_token: bool) -> String {
    let normalized = crate::chat_runtime::normalize_pasted_text(pasted);
    let first_line = normalized
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_string();
    if first_line.is_empty() {
        return String::new();
    }
    let cooked = if single_token {
        first_line
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_string()
    } else {
        first_line
    };
    cooked.chars().take(180).collect::<String>()
}

pub(crate) fn render_with_optional_caret(input: &str, cursor: usize, visible: bool) -> String {
    if !visible {
        return input.to_string();
    }
    let mut chars: Vec<char> = input.chars().collect();
    let idx = cursor.min(chars.len());
    chars.insert(idx, '|');
    chars.into_iter().collect()
}

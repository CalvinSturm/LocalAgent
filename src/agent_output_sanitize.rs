pub(crate) fn sanitize_user_visible_output(raw: &str) -> String {
    split_user_visible_and_thinking(raw).0
}

pub(crate) fn split_user_visible_and_thinking(raw: &str) -> (String, Option<String>) {
    let (without_think, think_blocks) = strip_tag_block_with_capture(raw, "think", false);
    split_visible_and_thinking_from_parts(&without_think, think_blocks)
}

#[allow(dead_code)]
pub(crate) fn split_user_visible_and_thinking_streaming(raw: &str) -> (String, Option<String>) {
    let (without_think, think_blocks) = strip_tag_block_with_capture(raw, "think", true);
    split_visible_and_thinking_from_parts(&without_think, think_blocks)
}

fn split_visible_and_thinking_from_parts(
    without_think: &str,
    think_blocks: Vec<String>,
) -> (String, Option<String>) {
    let trimmed_owned = strip_orphan_think_closers(without_think);
    let trimmed = trimmed_owned.trim();
    let upper = trimmed.to_uppercase();
    if let Some(thought_idx) = upper.find("THOUGHT:") {
        if let Some(response_rel) = upper[thought_idx..].find("RESPONSE:") {
            let start = thought_idx + response_rel + "RESPONSE:".len();
            let visible = trimmed[start..].trim().to_string();
            let thinking = if think_blocks.is_empty() {
                None
            } else {
                Some(think_blocks.join("\n\n"))
            };
            return (visible, thinking);
        }
    }
    let thinking = if think_blocks.is_empty() {
        None
    } else {
        Some(think_blocks.join("\n\n"))
    };
    (trimmed.to_string(), thinking)
}

fn strip_orphan_think_closers(input: &str) -> String {
    input.replace("</think>", "")
}

fn strip_tag_block_with_capture(
    input: &str,
    tag: &str,
    capture_unclosed: bool,
) -> (String, Vec<String>) {
    let mut out = String::with_capacity(input.len());
    let mut captured = Vec::new();
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut i = 0usize;
    while i < input.len() {
        let rest = &input[i..];
        if rest.starts_with(&open) {
            if let Some(end_rel) = rest.find(&close) {
                let block_start = open.len();
                let block_end = end_rel;
                let inner = rest[block_start..block_end].trim();
                if !inner.is_empty() {
                    captured.push(inner.to_string());
                }
                i += end_rel + close.len();
                continue;
            }
            if capture_unclosed {
                let inner = rest[open.len()..].trim();
                if !inner.is_empty() {
                    captured.push(inner.to_string());
                }
            }
            break;
        }
        if let Some(ch) = rest.chars().next() {
            out.push(ch);
            i += ch.len_utf8();
        } else {
            break;
        }
    }
    (out, captured)
}

#[cfg(test)]
mod tests {
    use super::sanitize_user_visible_output;

    #[test]
    fn strips_orphan_think_closer_from_visible_output() {
        assert_eq!(
            sanitize_user_visible_output("\n</think>\n\nvalidated: src/lib.rs"),
            "validated: src/lib.rs"
        );
    }
}

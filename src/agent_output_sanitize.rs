pub(crate) fn sanitize_user_visible_output(raw: &str) -> String {
    let without_think = strip_tag_block(raw, "think");
    let trimmed = without_think.trim();
    let upper = trimmed.to_uppercase();
    if let Some(thought_idx) = upper.find("THOUGHT:") {
        if let Some(response_rel) = upper[thought_idx..].find("RESPONSE:") {
            let start = thought_idx + response_rel + "RESPONSE:".len();
            return trimmed[start..].trim().to_string();
        }
    }
    trimmed.to_string()
}

fn strip_tag_block(input: &str, tag: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut i = 0usize;
    while i < input.len() {
        let rest = &input[i..];
        if rest.starts_with(&open) {
            if let Some(end_rel) = rest.find(&close) {
                i += end_rel + close.len();
                continue;
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
    out
}

use serde_json::{json, Value};

use super::{ToolArgsStrict, ToolErrorCode, ToolErrorDetail};

pub fn compact_builtin_schema(tool_name: &str) -> Option<Value> {
    match tool_name {
        "list_dir" | "read_file" => Some(json!({
            "type":"object",
            "required":["path"],
            "properties":{"path":{"type":"string"}}
        })),
        "glob" => Some(json!({
            "type":"object",
            "required":["pattern"],
            "properties":{
                "pattern":{"type":"string"},
                "path":{"type":"string"},
                "max_results":{"type":"integer","minimum":1,"maximum":1000}
            }
        })),
        "grep" => Some(json!({
            "type":"object",
            "required":["pattern"],
            "properties":{
                "pattern":{"type":"string"},
                "path":{"type":"string"},
                "max_results":{"type":"integer","minimum":1,"maximum":1000},
                "ignore_case":{"type":"boolean"}
            }
        })),
        "shell" => Some(json!({
            "type":"object",
            "required":["cmd"],
            "properties":{
                "cmd":{"type":"string"},
                "args":{"type":"array","items":{"type":"string"}},
                "cwd":{"type":"string"}
            }
        })),
        "write_file" => Some(json!({
            "type":"object",
            "required":["path","content"],
            "properties":{
                "path":{"type":"string"},
                "content":{"type":"string"},
                "create_parents":{"type":"boolean"},
                "overwrite_existing":{"type":"boolean"}
            }
        })),
        "apply_patch" => Some(json!({
            "type":"object",
            "required":["path","patch"],
            "properties":{"path":{"type":"string"},"patch":{"type":"string"}}
        })),
        "str_replace" => Some(json!({
            "type":"object",
            "required":["path","old_string","new_string"],
            "properties":{
                "path":{"type":"string"},
                "old_string":{"type":"string"},
                "new_string":{"type":"string"}
            }
        })),
        _ => None,
    }
}

pub fn minimal_builtin_example(tool_name: &str) -> Option<Value> {
    match tool_name {
        "list_dir" => Some(json!({"path":"."})),
        "read_file" => Some(json!({"path":"src/main.rs"})),
        "glob" => Some(json!({"pattern":"src/**/*.rs","path":".","max_results":200})),
        "grep" => Some(json!({"pattern":"TODO","path":".","max_results":200,"ignore_case":false})),
        "shell" => Some(json!({"cmd":"echo","args":["hello"]})),
        "write_file" => Some(json!({"path":"notes.txt","content":"hello"})),
        "apply_patch" => Some(json!({"path":"src/main.rs","patch":"@@ -1 +1 @@\n-a\n+b\n"})),
        "str_replace" => Some(
            json!({"path":"src/main.rs","old_string":"println!(\"helo\")","new_string":"println!(\"hello\")"}),
        ),
        _ => None,
    }
}

pub fn sorted_builtin_tool_names() -> Vec<String> {
    let mut names = vec![
        "apply_patch".to_string(),
        "glob".to_string(),
        "grep".to_string(),
        "list_dir".to_string(),
        "read_file".to_string(),
        "shell".to_string(),
        "str_replace".to_string(),
        "write_file".to_string(),
    ];
    names.sort();
    names
}

pub fn invalid_args_detail(tool_name: &str, args: &Value, err: &str) -> ToolErrorDetail {
    ToolErrorDetail {
        code: ToolErrorCode::ToolArgsInvalid,
        message: format!("Invalid arguments: {err}"),
        expected_schema: compact_builtin_schema(tool_name),
        received_args: Some(args.clone()),
        minimal_example: minimal_builtin_example(tool_name),
        available_tools: None,
    }
}

pub fn validate_builtin_tool_args(
    tool_name: &str,
    args: &Value,
    strict: ToolArgsStrict,
) -> Result<(), String> {
    let obj = args
        .as_object()
        .ok_or_else(|| "arguments must be a JSON object".to_string())?;
    if !strict.is_enabled() {
        return Ok(());
    }
    match tool_name {
        "list_dir" | "read_file" => require_non_empty_string(obj, "path")?,
        "glob" => {
            require_non_empty_string(obj, "pattern")?;
            if let Some(v) = obj.get("path") {
                if v.as_str().is_none() {
                    return Err("path must be a string".to_string());
                }
            }
            if let Some(v) = obj.get("max_results") {
                let n = v
                    .as_u64()
                    .ok_or_else(|| "max_results must be an integer".to_string())?;
                if !(1..=1000).contains(&n) {
                    return Err("max_results must be between 1 and 1000".to_string());
                }
            }
        }
        "grep" => {
            require_non_empty_string(obj, "pattern")?;
            if let Some(v) = obj.get("path") {
                if v.as_str().is_none() {
                    return Err("path must be a string".to_string());
                }
            }
            if let Some(v) = obj.get("max_results") {
                let n = v
                    .as_u64()
                    .ok_or_else(|| "max_results must be an integer".to_string())?;
                if !(1..=1000).contains(&n) {
                    return Err("max_results must be between 1 and 1000".to_string());
                }
            }
            if let Some(v) = obj.get("ignore_case") {
                if v.as_bool().is_none() {
                    return Err("ignore_case must be a boolean".to_string());
                }
            }
        }
        "shell" => {
            require_non_empty_string(obj, "cmd")?;
            if let Some(v) = obj.get("args") {
                let arr = v
                    .as_array()
                    .ok_or_else(|| "args must be an array of strings".to_string())?;
                if arr.iter().any(|x| x.as_str().is_none()) {
                    return Err("args must be an array of strings".to_string());
                }
            }
            if let Some(v) = obj.get("cwd") {
                if v.as_str().is_none() {
                    return Err("cwd must be a string".to_string());
                }
            }
        }
        "write_file" => {
            require_non_empty_string(obj, "path")?;
            require_string(obj, "content")?;
            if let Some(v) = obj.get("create_parents") {
                if v.as_bool().is_none() {
                    return Err("create_parents must be a boolean".to_string());
                }
            }
            if let Some(v) = obj.get("overwrite_existing") {
                if v.as_bool().is_none() {
                    return Err("overwrite_existing must be a boolean".to_string());
                }
            }
        }
        "apply_patch" => {
            require_non_empty_string(obj, "path")?;
            require_non_empty_string(obj, "patch")?;
        }
        "str_replace" => {
            require_non_empty_string(obj, "path")?;
            require_string(obj, "old_string")?;
            require_string(obj, "new_string")?;
        }
        _ => {}
    }
    Ok(())
}

pub fn validate_schema_args(
    args: &Value,
    schema: Option<&Value>,
    strict: ToolArgsStrict,
) -> Result<(), String> {
    let schema = match schema {
        Some(v) => v,
        None => {
            let obj = args
                .as_object()
                .ok_or_else(|| "arguments must be a JSON object".to_string())?;
            if obj.is_empty() {
                return Ok(());
            }
            return Err("arguments not allowed for tool with unknown schema".to_string());
        }
    };
    let obj = args
        .as_object()
        .ok_or_else(|| "arguments must be a JSON object".to_string())?;
    if !strict.is_enabled() {
        return Ok(());
    }
    let Some(sobj) = schema.as_object() else {
        return Ok(());
    };
    if let Some(req) = sobj.get("required").and_then(|v| v.as_array()) {
        for it in req {
            if let Some(key) = it.as_str() {
                if !obj.contains_key(key) {
                    return Err(format!("missing required field: {key}"));
                }
            }
        }
    }
    let props = sobj
        .get("properties")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    for (k, v) in obj {
        if let Some(schema) = props.get(k) {
            validate_value_type(v, schema).map_err(|e| format!("field '{k}' {e}"))?;
        } else if sobj.get("additionalProperties").and_then(|v| v.as_bool()) == Some(false) {
            return Err(format!("unknown field not allowed: {k}"));
        }
    }
    Ok(())
}

fn validate_value_type(value: &Value, schema: &Value) -> Result<(), String> {
    let Some(kind) = schema.get("type").and_then(|v| v.as_str()) else {
        return Ok(());
    };
    match kind {
        "string" if value.is_string() => Ok(()),
        "number" if value.is_number() => Ok(()),
        "integer" if value.as_i64().is_some() || value.as_u64().is_some() => Ok(()),
        "boolean" if value.is_boolean() => Ok(()),
        "object" if value.is_object() => Ok(()),
        "array" if value.is_array() => {
            if let Some(item_schema) = schema.get("items") {
                if let Some(arr) = value.as_array() {
                    for item in arr {
                        validate_value_type(item, item_schema)?;
                    }
                }
            }
            Ok(())
        }
        "null" if value.is_null() => Ok(()),
        other => Err(format!("has invalid type (expected {other})")),
    }
}

fn require_string(obj: &serde_json::Map<String, Value>, key: &str) -> Result<(), String> {
    match obj.get(key) {
        Some(v) if v.is_string() => Ok(()),
        Some(_) => Err(format!("{key} must be a string")),
        None => Err(format!("missing required field: {key}")),
    }
}

fn require_non_empty_string(obj: &serde_json::Map<String, Value>, key: &str) -> Result<(), String> {
    let v = obj
        .get(key)
        .ok_or_else(|| format!("missing required field: {key}"))?;
    let s = v
        .as_str()
        .ok_or_else(|| format!("{key} must be a string"))?;
    if s.is_empty() {
        return Err(format!("{key} must be a non-empty string"));
    }
    Ok(())
}

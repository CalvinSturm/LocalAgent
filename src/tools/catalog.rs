use serde_json::{json, Value};

use crate::types::{SideEffects, ToolDef};

pub fn tool_side_effects(tool_name: &str) -> SideEffects {
    match tool_name {
        "list_dir" | "read_file" | "glob" | "grep" => SideEffects::FilesystemRead,
        "shell" => SideEffects::ShellExec,
        "write_file" | "apply_patch" | "edit" | "str_replace" => SideEffects::FilesystemWrite,
        _ if tool_name.starts_with("mcp.playwright.") => SideEffects::Browser,
        _ if tool_name.starts_with("mcp.") => SideEffects::Network,
        _ => SideEffects::None,
    }
}

pub fn builtin_tools_enabled(enable_write_tools: bool, enable_shell_tool: bool) -> Vec<ToolDef> {
    let mut tools = vec![
        ToolDef {
            name: "list_dir".to_string(),
            description: "List entries in a directory.".to_string(),
            parameters: json!({
                "type":"object",
                "properties":{"path":{"type":"string"}},
                "required":["path"]
            }),
            side_effects: SideEffects::FilesystemRead,
        },
        ToolDef {
            name: "read_file".to_string(),
            description: "Read a UTF-8 text file (lossy decode allowed).".to_string(),
            parameters: json!({
                "type":"object",
                "properties":{"path":{"type":"string"}},
                "required":["path"]
            }),
            side_effects: SideEffects::FilesystemRead,
        },
        ToolDef {
            name: "glob".to_string(),
            description: "Find files matching a glob pattern under a scoped path.".to_string(),
            parameters: json!({
                "type":"object",
                "properties":{
                    "pattern":{"type":"string"},
                    "path":{"type":"string"},
                    "max_results":{"type":"integer","minimum":1,"maximum":1000}
                },
                "required":["pattern"]
            }),
            side_effects: SideEffects::FilesystemRead,
        },
        ToolDef {
            name: "grep".to_string(),
            description: "Search text files with a regex pattern under a scoped path.".to_string(),
            parameters: json!({
                "type":"object",
                "properties":{
                    "pattern":{"type":"string"},
                    "path":{"type":"string"},
                    "max_results":{"type":"integer","minimum":1,"maximum":1000},
                    "ignore_case":{"type":"boolean"}
                },
                "required":["pattern"]
            }),
            side_effects: SideEffects::FilesystemRead,
        },
    ];
    if enable_shell_tool {
        tools.push(ToolDef {
            name: "shell".to_string(),
            description: "Run a shell command with optional args and cwd.".to_string(),
            parameters: json!({
                "type":"object",
                "properties":{
                    "cmd":{"type":"string"},
                    "args":{"type":"array","items":{"type":"string"}},
                    "cwd":{"type":"string"}
                },
                "required":["cmd"]
            }),
            side_effects: SideEffects::ShellExec,
        });
    }
    if enable_write_tools {
        tools.push(ToolDef {
            name: "write_file".to_string(),
            description: "Write UTF-8 text content to a file.".to_string(),
            parameters: json!({
                "type":"object",
                "properties":{
                    "path":{"type":"string"},
                    "content":{"type":"string"},
                    "create_parents":{"type":"boolean"},
                    "overwrite_existing":{"type":"boolean"}
                },
                "required":["path","content"]
            }),
            side_effects: SideEffects::FilesystemWrite,
        });
        tools.push(ToolDef {
            name: "apply_patch".to_string(),
            description: "Apply a unified diff patch to an existing file using a workdir-relative path. Prefer this for larger edits or after str_replace exact-match repair fails.".to_string(),
            parameters: json!({
                "type":"object",
                "properties":{"path":{"type":"string"},"patch":{"type":"string"}},
                "required":["path","patch"]
            }),
            side_effects: SideEffects::FilesystemWrite,
        });
        tools.push(ToolDef {
            name: "edit".to_string(),
            description: "Edit an existing file by replacing exactly one matching string with a new string using a workdir-relative path. Prefer this for small in-place edits. Accepts path/old_string/new_string and OpenCode-style aliases filePath/oldString/newString.".to_string(),
            parameters: json!({
                "type":"object",
                "properties":{
                    "path":{"type":"string"},
                    "old_string":{"type":"string"},
                    "new_string":{"type":"string"},
                    "filePath":{"type":"string"},
                    "oldString":{"type":"string"},
                    "newString":{"type":"string"}
                },
                "required":["path","old_string","new_string"]
            }),
            side_effects: SideEffects::FilesystemWrite,
        });
        tools.push(ToolDef {
            name: "str_replace".to_string(),
            description: "Replace an exact string occurrence in a file using a workdir-relative path. The old_string must match exactly once; include surrounding lines for uniqueness if needed. If exact matching is brittle, switch to apply_patch.".to_string(),
            parameters: json!({
                "type":"object",
                "properties":{
                    "path":{"type":"string"},
                    "old_string":{"type":"string"},
                    "new_string":{"type":"string"}
                },
                "required":["path","old_string","new_string"]
            }),
            side_effects: SideEffects::FilesystemWrite,
        });
    }
    tools
}

pub(super) fn normalize_builtin_tool_args(tool_name: &str, args: &Value) -> Value {
    let Some(obj) = args.as_object() else {
        return args.clone();
    };
    if tool_name == "shell" {
        if obj.contains_key("cmd") {
            return args.clone();
        }
        let Some(command) = obj.get("command").and_then(|v| v.as_str()) else {
            return args.clone();
        };
        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.is_empty() {
            return args.clone();
        }
        let mut normalized = obj.clone();
        normalized.insert("cmd".to_string(), Value::String(parts[0].to_string()));
        let arg_list = parts[1..]
            .iter()
            .map(|s| Value::String((*s).to_string()))
            .collect::<Vec<_>>();
        normalized.insert("args".to_string(), Value::Array(arg_list));
        normalized.remove("command");
        return Value::Object(normalized);
    }
    if tool_name == "edit" {
        let mut normalized = obj.clone();
        if !normalized.contains_key("path") {
            if let Some(v) = normalized.get("filePath").cloned() {
                normalized.insert("path".to_string(), v);
            }
        }
        if !normalized.contains_key("old_string") {
            if let Some(v) = normalized.get("oldString").cloned() {
                normalized.insert("old_string".to_string(), v);
            }
        }
        if !normalized.contains_key("new_string") {
            if let Some(v) = normalized.get("newString").cloned() {
                normalized.insert("new_string".to_string(), v);
            }
        }
        return Value::Object(normalized);
    }
    args.clone()
}

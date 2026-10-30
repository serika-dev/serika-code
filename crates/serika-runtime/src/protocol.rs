use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Deserialize)]
struct RawCall {
    #[serde(alias = "tool")]
    name: String,
    #[serde(default, alias = "input")]
    arguments: Value,
}

/// Split assistant text into the prose the user should see and any tool calls.
pub fn extract_tool_calls(text: &str) -> (String, Vec<ToolCall>) {
    let mut visible = String::new();
    let mut calls = Vec::new();
    let mut rest = text;
    while let Some(start) = find_fence(rest) {
        visible.push_str(&rest[..start.0]);
        let after = &rest[start.1..];
        if let Some(end) = after.find("```") {
            let body = after[..end].trim();
            match parse_call_body(body) {
                Ok(found) => calls.extend(found),
                Err(err) => calls.push(ToolCall {
                    name: String::new(),
                    arguments: Value::String(format!("invalid tool block: {err}")),
                }),
            }
            rest = &after[end + 3..];
            if rest.starts_with('\n') {
                rest = &rest[1..];
            }
        } else {
            visible.push_str(&rest[start.0..]);
            rest = "";
            break;
        }
    }
    visible.push_str(rest);
    let visible = visible.trim().to_string();
    (visible, calls)
}

fn find_fence(text: &str) -> Option<(usize, usize)> {
    const MARKERS: &[&str] = &["```serika-tool", "```serika_tool"];
    let mut best: Option<(usize, usize)> = None;
    for marker in MARKERS {
        if let Some(index) = text.find(marker) {
            let body_at = index + marker.len();
            if best.map(|(at, _)| index < at).unwrap_or(true) {
                best = Some((index, body_at));
            }
        }
    }
    best
}

fn parse_call_body(body: &str) -> std::result::Result<Vec<ToolCall>, String> {
    let value: Value = serde_json::from_str(body).map_err(|err| err.to_string())?;
    if let Some(items) = value.as_array() {
        return items.iter().map(one_call).collect();
    }
    one_call(&value).map(|call| vec![call])
}

fn one_call(value: &Value) -> std::result::Result<ToolCall, String> {
    let raw: RawCall = serde_json::from_value(value.clone()).map_err(|err| err.to_string())?;
    let arguments = if raw.arguments.is_null() {
        Value::Object(Default::default())
    } else {
        raw.arguments
    };
    if !arguments.is_object() {
        return Err("arguments must be a JSON object".into());
    }
    if raw.name.trim().is_empty() {
        return Err("tool name is empty".into());
    }
    Ok(ToolCall {
        name: raw.name,
        arguments,
    })
}

pub fn tool_instructions() -> &'static str {
    r#"You are Serika Code, a coding agent inside the user's project. A host program executes tools. It does not see hidden tool-call channels. Emit a tool only as a fenced block in this exact shape:

```serika-tool
{"name":"tool_name","arguments":{}}
```

You may emit several blocks. Wait for the tool results before claiming anything changed. Do not invent files, command output, or test results. Prefer replace_in_file or apply_patch over rewriting a whole file. Keep edits limited to the request. When the task is finished, or you are blocked, reply in plain prose and do not emit a serika-tool block.

Tools:
- list_dir {"path":".","depth":2}
- read_file {"path":"src/lib.rs","offset":1,"limit":400}
- search_text {"query":"fn main","path":".","regex":false,"max_results":30}
- search_files {"pattern":"*.rs"}
- write_file {"path":"src/new.rs","content":"..."}
- replace_in_file {"path":"src/lib.rs","search":"exact existing text","replace":"new text","all":false}
- apply_patch {"patch":"--- a/file\n+++ b/file\n@@ ..."}
- delete_file {"path":"src/old.rs"}
- move_file {"from":"a.rs","to":"b.rs"}
- run_command {"command":"cargo test","timeout_secs":120}
- git_status {}
- git_diff {"path":"src/lib.rs"}
- git_log {"limit":10}
- git_commit {"message":"why this change","paths":["src/lib.rs"]}

search must be copied exactly from the file, including whitespace. Do not commit, push, reset, or delete unless the user asked. git_commit only stages the paths you list."#
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_calls_and_keeps_prose() {
        let text = "I'll read it.\n```serika-tool\n{\"name\":\"read_file\",\"arguments\":{\"path\":\"a.rs\"}}\n```\nThen I will edit.";
        let (visible, calls) = extract_tool_calls(text);
        assert!(visible.contains("I'll read it."));
        assert!(visible.contains("Then I will edit."));
        assert!(!visible.contains("read_file"));
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(calls[0].arguments["path"], "a.rs");
    }

    #[test]
    fn accepts_an_array_and_rejects_garbage() {
        let text = "```serika-tool\n[{\"tool\":\"git_status\",\"input\":{}}]\n```";
        let (_, calls) = extract_tool_calls(text);
        assert_eq!(calls[0].name, "git_status");
        let (_, bad) = extract_tool_calls("```serika-tool\nnot-json\n```");
        assert!(bad[0].name.is_empty());
    }
}

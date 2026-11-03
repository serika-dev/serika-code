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
    #[serde(default, alias = "input", alias = "args", alias = "parameters")]
    arguments: Value,
}

const KNOWN_TOOLS: &[&str] = &[
    "list_dir",
    "list_files",
    "read_file",
    "search_text",
    "grep",
    "ripgrep",
    "search_files",
    "find_files",
    "glob",
    "write_file",
    "replace_in_file",
    "apply_patch",
    "delete_file",
    "move_file",
    "run_command",
    "git_status",
    "git_diff",
    "git_log",
    "git_commit",
    "browser_open",
    "browser_snapshot",
    "browser_click",
    "browser_type",
    "browser_press",
    "browser_scroll",
    "browser_back",
    "browser_new_tab",
    "browser_switch_tab",
    "delegate_task",
    "use_skill",
];

/// Split assistant text into `(visible_prose, extracted_reasoning, tool_calls)`.
pub fn extract_reasoning_and_tool_calls(text: &str) -> (String, String, Vec<ToolCall>) {
    let (without_think, reasoning) = extract_think_blocks(text);
    let (visible, calls) = extract_tool_fences(&without_think);
    (visible, reasoning, calls)
}

/// Split assistant text into the prose the user should see and any tool calls.
pub fn extract_tool_calls(text: &str) -> (String, Vec<ToolCall>) {
    let (visible, _reasoning, calls) = extract_reasoning_and_tool_calls(text);
    (visible, calls)
}

/// Extract `<think>...</think>` and `<thinking>...</thinking>` blocks from text.
pub fn extract_think_blocks(text: &str) -> (String, String) {
    let mut visible = String::new();
    let mut reasoning = String::new();
    let mut rest = text;

    while let Some((start_idx, open_len, close_tag)) = find_think_open(rest) {
        visible.push_str(&rest[..start_idx]);
        let after_open = &rest[start_idx + open_len..];
        if let Some(end_idx) = find_case_insensitive(after_open, close_tag) {
            let chunk = after_open[..end_idx].trim();
            if !chunk.is_empty() {
                if !reasoning.is_empty() {
                    reasoning.push_str("\n\n");
                }
                reasoning.push_str(chunk);
            }
            rest = &after_open[end_idx + close_tag.len()..];
            if rest.starts_with('\n') {
                rest = &rest[1..];
            }
        } else {
            // Unclosed <think> block (e.g. still streaming)
            let chunk = after_open.trim();
            if !chunk.is_empty() {
                if !reasoning.is_empty() {
                    reasoning.push_str("\n\n");
                }
                reasoning.push_str(chunk);
            }
            rest = "";
            break;
        }
    }
    visible.push_str(rest);
    (visible.trim().to_string(), reasoning.trim().to_string())
}

fn find_think_open(text: &str) -> Option<(usize, usize, &'static str)> {
    let candidates = [
        ("<think>", "</think>"),
        ("<thinking>", "</thinking>"),
        ("<thought>", "</thought>"),
    ];
    let mut best: Option<(usize, usize, &'static str)> = None;
    for (open, close) in candidates {
        if let Some(idx) = find_case_insensitive(text, open) {
            if best.map(|(b, _, _)| idx < b).unwrap_or(true) {
                best = Some((idx, open.len(), close));
            }
        }
    }
    best
}

fn find_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    let lower = haystack.to_ascii_lowercase();
    lower.find(&needle.to_ascii_lowercase())
}

fn extract_tool_fences(text: &str) -> (String, Vec<ToolCall>) {
    let mut visible = String::new();
    let mut calls = Vec::new();
    let mut rest = text;

    while let Some((start, body_at, close_marker, is_generic_json)) = find_fence(rest) {
        visible.push_str(&rest[..start]);
        let after = &rest[body_at..];
        if let Some(end) = after.find(close_marker) {
            let body = after[..end].trim();
            match parse_call_body(body, is_generic_json) {
                Ok(found) => calls.extend(found),
                Err(err) => {
                    if is_generic_json {
                        // A regular ```json code block for the user, not a tool call
                        let slice_end = body_at + end + close_marker.len();
                        visible.push_str(&rest[start..slice_end]);
                    } else {
                        calls.push(ToolCall {
                            name: String::new(),
                            arguments: Value::String(format!("invalid tool block: {err}")),
                        });
                    }
                }
            }
            rest = &after[end + close_marker.len()..];
            if rest.starts_with('\n') {
                rest = &rest[1..];
            }
        } else {
            visible.push_str(&rest[start..]);
            rest = "";
            break;
        }
    }
    visible.push_str(rest);
    let visible = visible.trim().to_string();
    (visible, calls)
}

fn find_fence(text: &str) -> Option<(usize, usize, &'static str, bool)> {
    const MARKERS: &[(&str, &str, bool)] = &[
        ("```serika-tool", "```", false),
        ("```serika_tool", "```", false),
        ("```tool_call", "```", false),
        ("```tool", "```", false),
        ("<tool_call>", "</tool_call>", false),
        ("```json", "```", true),
    ];
    let mut best: Option<(usize, usize, &'static str, bool)> = None;
    for &(marker, close, is_json) in MARKERS {
        if let Some(index) = text.find(marker) {
            // Ensure ```tool doesn't falsely match ```tool_something_else unless followed by whitespace/newline
            let after_marker = &text[index + marker.len()..];
            if marker.starts_with("```")
                && !after_marker.is_empty()
                && !after_marker.starts_with('\n')
                && !after_marker.starts_with('\r')
                && !after_marker.starts_with(' ')
            {
                continue;
            }
            let body_at = index + marker.len();
            if best.map(|(at, _, _, _)| index < at).unwrap_or(true) {
                best = Some((index, body_at, close, is_json));
            }
        }
    }
    best
}

fn parse_call_body(
    body: &str,
    require_known_tool: bool,
) -> std::result::Result<Vec<ToolCall>, String> {
    let value: Value = serde_json::from_str(body).map_err(|err| err.to_string())?;
    let parsed = if let Some(items) = value.as_array() {
        items
            .iter()
            .map(one_call)
            .collect::<std::result::Result<Vec<_>, _>>()?
    } else {
        vec![one_call(&value)?]
    };
    if require_known_tool
        && (parsed.is_empty()
            || !parsed
                .iter()
                .all(|c| KNOWN_TOOLS.contains(&c.name.as_str()) || c.name.starts_with("mcp__")))
    {
        return Err("not a known serika tool".into());
    }
    Ok(parsed)
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

/// Streaming filter that routes `<think>...</think>` content to `on_reasoning`
/// and normal content to `on_content` as tokens arrive.
#[derive(Default)]
pub struct ThinkStreamSplitter {
    pending: String,
    in_think: bool,
}

impl ThinkStreamSplitter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn feed(
        &mut self,
        chunk: &str,
        mut on_content: impl FnMut(&str),
        mut on_reasoning: impl FnMut(&str),
    ) {
        self.pending.push_str(chunk);
        loop {
            if self.in_think {
                if let Some(close_idx) = find_think_close(&self.pending) {
                    let (idx, tag_len) = close_idx;
                    let thought = &self.pending[..idx];
                    if !thought.is_empty() {
                        on_reasoning(thought);
                    }
                    self.pending.replace_range(..idx + tag_len, "");
                    self.in_think = false;
                    continue;
                }
                // Keep a small tail in case `</think>` is split across chunks
                if self.pending.len() > 16 {
                    let safe_len = floor_char_boundary(&self.pending, self.pending.len() - 12);
                    if safe_len > 0 {
                        let emit_part = self.pending[..safe_len].to_string();
                        self.pending.replace_range(..safe_len, "");
                        on_reasoning(&emit_part);
                    }
                }
                break;
            } else {
                if let Some((idx, open_len, _)) = find_think_open(&self.pending) {
                    let before = &self.pending[..idx];
                    if !before.is_empty() {
                        on_content(before);
                    }
                    self.pending.replace_range(..idx + open_len, "");
                    self.in_think = true;
                    continue;
                }
                if self.pending.contains('<') && self.pending.len() < 16 {
                    break;
                }
                if let Some(lt_idx) = self.pending.rfind('<') {
                    if self.pending.len() - lt_idx < 12 {
                        if lt_idx > 0 {
                            let emit_part = self.pending[..lt_idx].to_string();
                            self.pending.replace_range(..lt_idx, "");
                            on_content(&emit_part);
                        }
                        break;
                    }
                }
                if !self.pending.is_empty() {
                    let emit_part = std::mem::take(&mut self.pending);
                    on_content(&emit_part);
                }
                break;
            }
        }
    }

    pub fn flush(&mut self, mut on_content: impl FnMut(&str), mut on_reasoning: impl FnMut(&str)) {
        if self.pending.is_empty() {
            return;
        }
        let rest = std::mem::take(&mut self.pending);
        if self.in_think {
            on_reasoning(&rest);
        } else {
            on_content(&rest);
        }
        self.in_think = false;
    }
}

fn find_think_close(text: &str) -> Option<(usize, usize)> {
    let tags = ["</think>", "</thinking>", "</thought>"];
    let mut best: Option<(usize, usize)> = None;
    for tag in tags {
        if let Some(idx) = find_case_insensitive(text, tag) {
            if best.map(|(b, _)| idx < b).unwrap_or(true) {
                best = Some((idx, tag.len()));
            }
        }
    }
    best
}

fn floor_char_boundary(s: &str, index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    let mut i = index;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

pub fn tool_instructions() -> &'static str {
    r#"You are Serika Code, a fast native coding agent inside the user's project. A local host program executes workspace tools on your behalf.

Reasoning Trace Requirement:
- Start EVERY turn with a `<think>...</think>` block containing your concrete step-by-step reasoning: what the user asked, what you see in the workspace, which files/patterns to search or read next, and why.

Tool Execution Requirement:
- Emit workspace tool calls ONLY as a fenced block in your text response in this exact shape:

```serika-tool
{"name":"tool_name","arguments":{}}
```

- Never use native API function/tool calls and NEVER call `generate_image` or `edit_image` or mention stray pictures.
- You may emit several `serika-tool` blocks in one turn (for example, `search_files` and `search_text` together).
- Wait for the tool results before claiming anything changed. Do not invent files, command output, or test results.
- Prefer `replace_in_file` or `apply_patch` over rewriting a whole file. Keep edits limited to the request.
- When the task is finished, reply in clear Markdown after your `<think>...</think>` block and do not emit a `serika-tool` block.

Tools:
- list_dir {"path":".","depth":2}
- read_file {"path":"src/lib.rs","offset":1,"limit":400}
- search_text {"query":"ticket","path":".","regex":false,"max_results":40}
- search_files {"pattern":"*ticket*"}
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

Browser (a real browser with JavaScript; the user watches it in a panel):
- browser_open {"url":"https://example.com"} (a plain phrase searches the web)
- browser_snapshot {} returns the page text and numbered interactive elements
- browser_click {"id":3} clicks element 3 from the latest snapshot
- browser_type {"id":2,"text":"hello","submit":true} types into element 2 (omit id to type into the focused field)
- browser_press {"key":"Enter"} (Enter, Tab, Escape, Backspace, ArrowDown, ArrowUp, PageDown, PageUp, Home, End)
- browser_scroll {"amount":600,"direction":"down"}
- browser_back {}
- browser_new_tab {"url":"https://example.com"} opens a tab (url optional); links that open a new window become tabs too
- browser_switch_tab {"tab":2} shows another tab by its number in the tab list
Every browser tool returns a fresh snapshot. Use it to read documentation, check a running dev server, or follow links. Element numbers change after each action, so use the latest ones.

search must be copied exactly from the file, including whitespace. Do not commit, push, reset, or delete unless the user asked. git_commit only stages the paths you list."#
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_calls_and_keeps_prose() {
        let text = "<think>Check a.rs first.</think>\nI'll read it.\n```serika-tool\n{\"name\":\"read_file\",\"arguments\":{\"path\":\"a.rs\"}}\n```\nThen I will edit.";
        let (visible, reasoning, calls) = extract_reasoning_and_tool_calls(text);
        assert_eq!(reasoning, "Check a.rs first.");
        assert!(visible.contains("I'll read it."));
        assert!(visible.contains("Then I will edit."));
        assert!(!visible.contains("read_file"));
        assert!(!visible.contains("<think>"));
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

    #[test]
    fn splits_streaming_think_tags() {
        let mut splitter = ThinkStreamSplitter::new();
        let mut content = String::new();
        let mut reasoning = String::new();
        for chunk in ["<thi", "nk>Step 1: ", "search files</th", "ink>Hello ", "world!"] {
            splitter.feed(
                chunk,
                |c| content.push_str(c),
                |r| reasoning.push_str(r),
            );
        }
        splitter.flush(|c| content.push_str(c), |r| reasoning.push_str(r));
        assert_eq!(reasoning, "Step 1: search files");
        assert_eq!(content, "Hello world!");
    }
}

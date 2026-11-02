//! Context handed to a model that joins a conversation midway.
//!
//! A chat can move between Serika models, Claude Code, and Codex at any message. Serika's own
//! models read the stored history directly. A subscription CLI keeps its own session, so when
//! it starts fresh, or resumes after other models took turns, it gets a budgeted selection of
//! what it missed: the original request, the most recent turns, and abridged tool results,
//! attributed to who said them. The new request is never shortened to make room.

use crate::sessions::{is_tool_result, StoredMessage};

/// Characters of history a handoff may carry.
pub const BUDGET: usize = 16_000;
const ENTRY_MAX: usize = 4_000;
const TOOL_MAX: usize = 600;

pub struct Handoff {
    pub text: String,
    /// Messages included.
    pub messages: usize,
    /// Messages in range that did not fit.
    pub omitted: usize,
}

fn clip(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        return text.to_string();
    }
    let kept: String = text.chars().take(max).collect();
    format!("{kept}… [{} more characters]", count - max)
}

fn entry(message: &StoredMessage, original: bool) -> Option<String> {
    if is_tool_result(&message.role, &message.content) {
        let body = message.content.trim_start_matches("Tool results:\n").trim();
        return (!body.is_empty()).then(|| format!("Tool results (abridged):\n{}", clip(body, TOOL_MAX)));
    }
    let (text, _) = crate::protocol::extract_tool_calls(&message.content);
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let who = match (message.role.as_str(), original) {
        ("user", true) => "User (original request)".to_string(),
        ("user", false) => "User".to_string(),
        _ => match &message.model {
            Some(model) if !model.is_empty() => format!("Assistant ({model})"),
            _ => "Assistant".to_string(),
        },
    };
    Some(format!("{who}:\n{}", clip(text, ENTRY_MAX)))
}

/// Messages with `after < id < before`, newest kept first, the opening request always kept.
pub fn build(messages: &[StoredMessage], after: i64, before: i64, budget: usize) -> Handoff {
    let range: Vec<&StoredMessage> = messages
        .iter()
        .filter(|m| m.id > after && m.id < before && (m.role == "user" || m.role == "assistant"))
        .collect();
    let first_user = range
        .iter()
        .position(|m| m.role == "user" && !is_tool_result(&m.role, &m.content));
    let mut chosen: Vec<(usize, String)> = Vec::new();
    let mut used = 0;
    if after == 0 {
        if let Some(i) = first_user {
            if let Some(text) = entry(range[i], true) {
                used += text.len();
                chosen.push((i, text));
            }
        }
    }
    let mut considered = 0;
    for (i, message) in range.iter().enumerate().rev() {
        if chosen.iter().any(|(j, _)| *j == i) {
            continue;
        }
        let Some(text) = entry(message, false) else { continue };
        considered += 1;
        if used + text.len() > budget {
            break;
        }
        used += text.len();
        chosen.push((i, text));
    }
    chosen.sort_by_key(|(i, _)| *i);
    let total = range
        .iter()
        .filter(|m| entry(m, false).is_some())
        .count();
    let included = chosen.len();
    let omitted = total.saturating_sub(included);
    let _ = considered;
    if included == 0 {
        return Handoff { text: String::new(), messages: 0, omitted };
    }
    let mut text = String::new();
    if omitted > 0 {
        text.push_str(&format!(
            "[{omitted} older message(s) of this conversation are not shown; ask the user if you need them.]\n\n"
        ));
    }
    text.push_str(&chosen.into_iter().map(|(_, t)| t).collect::<Vec<_>>().join("\n\n"));
    Handoff { text, messages: included, omitted }
}

/// The model that wrote the last reply before message `before`.
pub fn previous_model(messages: &[StoredMessage], before: i64) -> Option<String> {
    messages
        .iter()
        .rev()
        .filter(|m| m.id < before && m.role == "assistant")
        .find_map(|m| m.model.clone())
        .filter(|m| !m.is_empty())
}

/// Wrap a request with the history a joining model missed.
pub fn wrap(handoff: &Handoff, fresh: bool, prompt: &str) -> String {
    if handoff.text.is_empty() {
        return prompt.to_string();
    }
    let heading = if fresh {
        "You are joining a conversation that started with another assistant. Here is what was said so far:"
    } else {
        "While you were away, this conversation continued with another assistant:"
    };
    format!("{heading}\n\n{}\n\n---\n\nThe user's new request:\n{prompt}", handoff.text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(id: i64, role: &str, content: &str, model: Option<&str>) -> StoredMessage {
        StoredMessage {
            id,
            role: role.into(),
            content: content.into(),
            reasoning: None,
            attachments: None,
            model: model.map(str::to_string),
            created_at: id,
        }
    }

    #[test]
    fn keeps_the_original_request_and_recent_turns() {
        let mut messages = vec![msg(1, "user", "Build a timer app", None)];
        for i in 0..40 {
            messages.push(msg(2 + i * 2, "assistant", &"x".repeat(900), Some("openrouter/free")));
            messages.push(msg(3 + i * 2, "user", &format!("step {i}"), None));
        }
        let h = build(&messages, 0, 1000, 8_000);
        assert!(h.text.contains("User (original request):\nBuild a timer app"));
        assert!(h.text.contains("step 39"));
        assert!(!h.text.contains("step 0\n"));
        assert!(h.omitted > 0);
        assert!(h.text.starts_with("[") && h.text.contains("not shown"));
        assert!(h.text.contains("Assistant (openrouter/free)"));
    }

    #[test]
    fn catch_up_covers_only_the_missed_range() {
        let messages = vec![
            msg(1, "user", "first", None),
            msg(2, "assistant", "claude answer", Some("claude-code/opus")),
            msg(3, "user", "second", None),
            msg(4, "assistant", "serika answer", Some("openrouter/free")),
            msg(5, "user", "third", None),
        ];
        let h = build(&messages, 2, 5, BUDGET);
        assert_eq!(h.messages, 2);
        assert!(!h.text.contains("claude answer"));
        assert!(h.text.contains("serika answer"));
        assert_eq!(previous_model(&messages, 5).as_deref(), Some("openrouter/free"));
        let wrapped = wrap(&h, false, "third");
        assert!(wrapped.ends_with("The user's new request:\nthird"));
    }

    #[test]
    fn nothing_to_hand_over_leaves_the_prompt_alone() {
        let h = build(&[], 0, 10, BUDGET);
        assert_eq!(wrap(&h, true, "hi"), "hi");
    }
}

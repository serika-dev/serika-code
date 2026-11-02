use std::collections::BTreeMap;
use std::path::Path;

use crate::client::ChatTurn;
use crate::error::Result;
use crate::workspace::{language_of, list_files, looks_like_manifest, RelFile};

pub fn snapshot(root: &Path, exclusions: &[String]) -> Result<Vec<RelFile>> {
    list_files(root, exclusions, None)
}

pub fn brief(root: &Path, files: &[RelFile], changed: &[String]) -> String {
    let mut languages: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut manifests = Vec::new();
    for file in files {
        if let Some(language) = language_of(&file.relative) {
            *languages.entry(language).or_insert(0) += 1;
        }
        if looks_like_manifest(&file.relative) {
            manifests.push(file.relative.as_str());
        }
    }
    let mut text = String::new();
    // Notes the project keeps for agents come first, so the length limit never cuts them.
    if let Some((name, notes)) = project_notes(root) {
        text.push_str(&format!("Project notes from {name} (follow them):\n{notes}\n\n"));
    }
    text.push_str(&format!("Project: {}\nFiles indexed: {}\n", root.display(), files.len()));
    if !languages.is_empty() {
        text.push_str("Languages: ");
        text.push_str(
            &languages
                .iter()
                .map(|(name, count)| format!("{name} {count}"))
                .collect::<Vec<_>>()
                .join(", "),
        );
        text.push('\n');
    }
    if !manifests.is_empty() {
        text.push_str("Manifests: ");
        text.push_str(&manifests.join(", "));
        text.push('\n');
    }
    if !changed.is_empty() {
        text.push_str("Changed during this task:\n");
        for path in changed {
            text.push_str("- ");
            text.push_str(path);
            text.push('\n');
        }
    }
    text.push_str("Tree:\n");
    for file in files.iter().take(200) {
        text.push_str(&file.relative);
        text.push('\n');
    }
    if files.len() > 200 {
        text.push_str(&format!("… {} more files\n", files.len() - 200));
    }
    if let Some(readme) = readme_excerpt(root) {
        text.push_str("\nREADME excerpt:\n");
        text.push_str(&readme);
        text.push('\n');
    }
    text
}

/// SERIKA.md, else AGENTS.md, else CLAUDE.md in the project root, up to 6,000 characters.
pub fn project_notes(root: &Path) -> Option<(&'static str, String)> {
    for name in ["SERIKA.md", "AGENTS.md", "CLAUDE.md"] {
        let Ok(text) = std::fs::read_to_string(root.join(name)) else { continue };
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        let clipped: String = text.chars().take(6_000).collect();
        return Some((name, clipped));
    }
    None
}

fn readme_excerpt(root: &Path) -> Option<String> {
    for name in ["README.md", "README.txt", "README"] {
        let path = root.join(name);
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        if text.len() > 8_000 {
            continue;
        }
        let excerpt: String = text.lines().take(40).collect::<Vec<_>>().join("\n");
        return Some(excerpt);
    }
    None
}

pub fn trim_history(mut turns: Vec<ChatTurn>, max_chars: usize) -> Vec<ChatTurn> {
    let system = if turns.first().is_some_and(|turn| turn.role == "system") {
        Some(turns.remove(0))
    } else {
        None
    };
    while chars_of(&turns) + system.as_ref().map(|turn| turn.content.len()).unwrap_or(0) > max_chars
        && turns.len() > 1
    {
        turns.remove(0);
    }
    if let Some(system) = system {
        turns.insert(0, system);
    }
    turns
}

fn chars_of(turns: &[ChatTurn]) -> usize {
    turns.iter().map(|turn| turn.content.len()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_old_turns_and_keeps_the_instruction() {
        let turns = vec![
            ChatTurn { role: "system".into(), content: "rules".into(), attachments: Vec::new() },
            ChatTurn { role: "user".into(), content: "a".repeat(100), attachments: Vec::new() },
            ChatTurn { role: "assistant".into(), content: "b".repeat(100), attachments: Vec::new() },
            ChatTurn { role: "user".into(), content: "latest".into(), attachments: Vec::new() },
        ];
        let trimmed = trim_history(turns, 120);
        assert_eq!(trimmed.first().unwrap().role, "system");
        assert_eq!(trimmed.last().unwrap().content, "latest");
        assert!(trimmed.len() < 4);
    }

    #[test]
    fn project_notes_lead_the_brief() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("AGENTS.md"), "Use tabs.").unwrap();
        let text = brief(dir.path(), &[], &[]);
        assert!(text.starts_with("Project notes from AGENTS.md (follow them):\nUse tabs."));
        std::fs::write(dir.path().join("SERIKA.md"), "Prefer SERIKA.").unwrap();
        assert_eq!(project_notes(dir.path()).unwrap().0, "SERIKA.md");
    }
}

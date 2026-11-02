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
    let mut text = format!("Project: {}\nFiles indexed: {}\n", root.display(), files.len());
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
}

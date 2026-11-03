//! Facts about a project folder for its settings page, and the instruction files the agent
//! reads there (AGENTS.md, CLAUDE.md, .serika/instructions.md), the same files other coding
//! agents use, so one file works everywhere.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// Files Serika reads as project instructions, in this order.
pub const INSTRUCTION_FILES: &[&str] = &["AGENTS.md", "CLAUDE.md", ".serika/instructions.md"];

/// Instructions longer than this are cut so they cannot crowd out the conversation.
const MAX_INSTRUCTIONS: usize = 16_000;

#[derive(Debug, Clone, Serialize)]
pub struct InstructionFile {
    pub name: String,
    pub path: String,
    pub exists: bool,
    pub bytes: u64,
}

pub fn instruction_files(project: &Path) -> Vec<InstructionFile> {
    INSTRUCTION_FILES
        .iter()
        .map(|name| {
            let path = project.join(name);
            let meta = std::fs::metadata(&path).ok().filter(|m| m.is_file());
            InstructionFile {
                name: name.to_string(),
                path: path.display().to_string(),
                exists: meta.is_some(),
                bytes: meta.map(|m| m.len()).unwrap_or(0),
            }
        })
        .collect()
}

/// The project's instruction files as a block for the system prompt ("" when there are none).
pub fn instructions_prompt(project: &Path) -> String {
    let mut out = String::new();
    for file in instruction_files(project).into_iter().filter(|f| f.exists) {
        let Ok(text) = std::fs::read_to_string(&file.path) else { continue };
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        let room = MAX_INSTRUCTIONS.saturating_sub(out.len());
        if room < 200 {
            break;
        }
        let mut body = text.to_string();
        if body.len() > room {
            let mut end = room;
            while !body.is_char_boundary(end) {
                end -= 1;
            }
            body.truncate(end);
            body.push_str("\n… (cut)");
        }
        out.push_str(&format!("\n\n## {}\n{}", file.name, body));
    }
    if out.is_empty() {
        out
    } else {
        format!("Project instructions (follow them unless the user says otherwise):{out}")
    }
}

/// Create AGENTS.md with a short starting template, unless it exists. Returns its path.
pub fn create_agents_md(project: &Path) -> crate::Result<PathBuf> {
    let path = project.join("AGENTS.md");
    if !path.exists() {
        let name = project.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "this project".into());
        std::fs::write(
            &path,
            format!(
                "# {name}\n\nInstructions for coding agents working in this repository.\n\n\
                 ## Build and test\n\n- \n\n## Conventions\n\n- \n"
            ),
        )?;
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_instruction_files_in_order() {
        let dir = tempfile::tempdir().unwrap();
        assert!(instructions_prompt(dir.path()).is_empty());
        std::fs::write(dir.path().join("CLAUDE.md"), "Use tabs.").unwrap();
        let path = create_agents_md(dir.path()).unwrap();
        assert!(path.exists());
        let prompt = instructions_prompt(dir.path());
        assert!(prompt.find("## AGENTS.md").unwrap() < prompt.find("## CLAUDE.md").unwrap());
        assert!(prompt.contains("Use tabs."));
        assert_eq!(instruction_files(dir.path()).iter().filter(|f| f.exists).count(), 2);
    }
}

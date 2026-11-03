//! Skills: folders with a `SKILL.md` that teach the agent a task.
//!
//! The format is the one Claude Code and Codex use: YAML frontmatter with `name` and
//! `description`, then instructions; the folder may hold scripts and references. Skills are
//! found in the project, in Serika's data folder, and — imported automatically — wherever
//! Claude Code, its plugins, and Codex keep theirs. The agent sees each skill's name and
//! description and loads one with `use_skill` when it applies.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    /// Folder that holds SKILL.md.
    pub dir: PathBuf,
    /// Where it came from: "Project", "Serika", "Claude Code", "Claude plugin", "Codex".
    pub source: String,
    pub enabled: bool,
}

const MAX_SKILLS: usize = 200;

/// Folders to search, most specific first: a project skill overrides a user skill of the same name.
pub fn roots(project: &Path, data_dir: &Path) -> Vec<(PathBuf, &'static str)> {
    let mut roots = vec![
        (project.join(".serika").join("skills"), "Project"),
        (project.join(".claude").join("skills"), "Project"),
        (project.join(".agents").join("skills"), "Project"),
        (project.join(".codex").join("skills"), "Project"),
        (data_dir.join("skills"), "Serika"),
    ];
    if let Some(home) = dirs::home_dir() {
        let claude = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from).unwrap_or_else(|| home.join(".claude"));
        roots.push((claude.join("skills"), "Claude Code"));
        roots.push((claude.join("plugins"), "Claude plugin"));
        let codex = std::env::var_os("CODEX_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".codex"));
        roots.push((codex.join("skills"), "Codex"));
        roots.push((home.join(".agents").join("skills"), "Codex"));
    }
    roots
}

/// Every skill visible from `project`, with the user's on/off choices applied.
pub fn discover(project: &Path, data_dir: &Path) -> Vec<Skill> {
    let disabled = disabled_names(data_dir);
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (root, source) in roots(project, data_dir) {
        if !root.is_dir() {
            continue;
        }
        // Plugins keep skills deeper (plugins/<marketplace>/<plugin>/skills/<skill>).
        let depth = if source == "Claude plugin" { 6 } else { 3 };
        let mut files = Vec::new();
        find_skill_files(&root, depth, &mut files);
        files.sort();
        for file in files {
            let Some(skill) = read_skill(&file, source) else { continue };
            if !seen.insert(skill.name.to_ascii_lowercase()) {
                continue;
            }
            let enabled = !disabled.contains(&skill.name.to_ascii_lowercase());
            out.push(Skill { enabled, ..skill });
            if out.len() >= MAX_SKILLS {
                return out;
            }
        }
    }
    out
}

fn find_skill_files(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_file() && name.eq_ignore_ascii_case("SKILL.md") {
            out.push(path);
        } else if path.is_dir() && depth > 0 && !name.starts_with('.') && name != "node_modules" {
            find_skill_files(&path, depth - 1, out);
        }
    }
}

fn read_skill(file: &Path, source: &str) -> Option<Skill> {
    let text = std::fs::read_to_string(file).ok()?;
    let (meta, _) = split_frontmatter(&text);
    let dir = file.parent()?.to_path_buf();
    let folder = dir.file_name()?.to_string_lossy().to_string();
    let name = field(&meta, "name").filter(|n| !n.is_empty()).unwrap_or(folder);
    let description = field(&meta, "description").unwrap_or_default();
    Some(Skill { name, description, dir, source: source.to_string(), enabled: true })
}

/// Frontmatter between leading `---` lines, and the body after it.
pub fn split_frontmatter(text: &str) -> (String, String) {
    let text = text.trim_start_matches('\u{feff}');
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return (String::new(), text.to_string());
    }
    let mut meta = Vec::new();
    let mut body_start = None;
    let mut offset = text.find('\n').map(|i| i + 1).unwrap_or(text.len());
    for line in lines {
        let next = offset + line.len() + 1;
        if line.trim() == "---" {
            body_start = Some(next.min(text.len()));
            break;
        }
        meta.push(line);
        offset = next;
    }
    match body_start {
        Some(start) => (meta.join("\n"), text[start..].to_string()),
        None => (String::new(), text.to_string()),
    }
}

/// One frontmatter value: `key: value`, quoted values, and `key: >` / `key: |` blocks.
pub fn field(meta: &str, key: &str) -> Option<String> {
    let lines: Vec<&str> = meta.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let Some(rest) = line.strip_prefix(key).and_then(|r| r.strip_prefix(':')) else { continue };
        let value = rest.trim();
        if value == ">" || value == "|" || value == ">-" || value == "|-" {
            let block: Vec<&str> = lines[i + 1..]
                .iter()
                .take_while(|l| l.starts_with(' ') || l.starts_with('\t') || l.trim().is_empty())
                .map(|l| l.trim())
                .collect();
            let joiner = if value.starts_with('>') { " " } else { "\n" };
            return Some(block.join(joiner).trim().to_string());
        }
        if let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
            return Some(unescape_double(inner));
        }
        if let Some(inner) = value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
            return Some(inner.replace("''", "'"));
        }
        return Some(value.to_string());
    }
    None
}

/// YAML double-quoted escapes: \" \\ \n \t (anything else is kept as written).
fn unescape_double(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other @ ('"' | '\\' | '/')) => out.push(other),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

fn prefs_path(data_dir: &Path) -> PathBuf {
    data_dir.join("skills.json")
}

fn disabled_names(data_dir: &Path) -> HashSet<String> {
    std::fs::read_to_string(prefs_path(data_dir))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("disabled").cloned())
        .and_then(|v| serde_json::from_value::<Vec<String>>(v).ok())
        .unwrap_or_default()
        .into_iter()
        .map(|n| n.to_ascii_lowercase())
        .collect()
}

/// Turn one skill on or off for every project.
pub fn set_enabled(data_dir: &Path, name: &str, enabled: bool) -> crate::Result<()> {
    let mut disabled: Vec<String> = disabled_names(data_dir).into_iter().collect();
    let key = name.to_ascii_lowercase();
    disabled.retain(|n| *n != key);
    if !enabled {
        disabled.push(key);
    }
    disabled.sort();
    std::fs::create_dir_all(data_dir)?;
    std::fs::write(prefs_path(data_dir), serde_json::json!({ "disabled": disabled }).to_string())?;
    Ok(())
}

/// The part of the system prompt that lists skills.
pub fn instructions(skills: &[Skill]) -> String {
    let usable: Vec<&Skill> = skills.iter().filter(|s| s.enabled).collect();
    if usable.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "Skills (packaged know-how for specific tasks). When the user's request matches one, load it first with use_skill and follow it:\n\
         - use_skill {\"name\":\"skill name\"} returns its instructions and lists its files\n\
         - use_skill {\"name\":\"skill name\",\"file\":\"relative/path\"} reads one of those files\n",
    );
    for skill in usable.iter().take(80) {
        let description: String = skill.description.chars().take(240).collect();
        out.push_str(&format!("- {}: {}\n", skill.name, description.replace('\n', " ")));
    }
    out
}

/// What `use_skill` returns.
pub fn load(skills: &[Skill], name: &str, file: Option<&str>) -> Result<String, String> {
    let skill = skills
        .iter()
        .find(|s| s.name.eq_ignore_ascii_case(name.trim()))
        .ok_or_else(|| format!("there is no skill named \"{name}\""))?;
    if !skill.enabled {
        return Err(format!("the skill \"{}\" is turned off in Settings", skill.name));
    }
    match file.map(str::trim).filter(|f| !f.is_empty()) {
        Some(rel) => {
            let path = skill.dir.join(rel);
            let canonical = dunce::canonicalize(&path).map_err(|_| format!("{rel} is not in the skill"))?;
            let base = dunce::canonicalize(&skill.dir).map_err(|e| e.to_string())?;
            if !canonical.starts_with(&base) {
                return Err("that path is outside the skill folder".into());
            }
            let text = std::fs::read_to_string(&canonical).map_err(|e| format!("could not read {rel}: {e}"))?;
            Ok(clip(&text, 40_000))
        }
        None => {
            let text = std::fs::read_to_string(skill.dir.join("SKILL.md")).map_err(|e| e.to_string())?;
            let (_, body) = split_frontmatter(&text);
            let mut files = Vec::new();
            list_files(&skill.dir, &skill.dir, 3, &mut files);
            files.retain(|f| !f.eq_ignore_ascii_case("SKILL.md"));
            let listing = if files.is_empty() {
                String::new()
            } else {
                format!(
                    "\n\nFiles in this skill (read with use_skill {{\"name\":\"{}\",\"file\":\"...\"}}; scripts run from {}):\n{}",
                    skill.name,
                    skill.dir.display(),
                    files.iter().take(60).map(|f| format!("- {f}")).collect::<Vec<_>>().join("\n")
                )
            };
            Ok(format!("Skill: {}\n\n{}{listing}", skill.name, clip(body.trim(), 40_000)))
        }
    }
}

fn list_files(base: &Path, dir: &Path, depth: usize, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Ok(rel) = path.strip_prefix(base) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        } else if path.is_dir() && depth > 0 {
            list_files(base, &path, depth - 1, out);
        }
    }
    out.sort();
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max).collect::<String>() + "\n… [cut]"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_frontmatter_values() {
        let text = "---\nname: pdf-tools\ndescription: >\n  Fill and merge\n  PDF forms.\nlicense: \"MIT\"\n---\n# Body\n";
        let (meta, body) = split_frontmatter(text);
        assert_eq!(field(&meta, "name").as_deref(), Some("pdf-tools"));
        assert_eq!(field(&meta, "description").as_deref(), Some("Fill and merge PDF forms."));
        assert_eq!(field(&meta, "license").as_deref(), Some("MIT"));
        assert_eq!(body, "# Body\n");
    }

    #[test]
    fn unescapes_quoted_values() {
        let meta = "description: \"Use the \\\"browser\\\" pane\"\ntitle: 'it''s here'";
        assert_eq!(field(meta, "description").as_deref(), Some("Use the \"browser\" pane"));
        assert_eq!(field(meta, "title").as_deref(), Some("it's here"));
    }

    #[test]
    fn finds_loads_and_toggles_skills() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("proj");
        let data = dir.path().join("data");
        let skill = project.join(".claude").join("skills").join("release");
        std::fs::create_dir_all(skill.join("scripts")).unwrap();
        std::fs::write(skill.join("SKILL.md"), "---\nname: release\ndescription: Cut a release\n---\nRun scripts/tag.sh").unwrap();
        std::fs::write(skill.join("scripts").join("tag.sh"), "git tag").unwrap();

        let found: Vec<Skill> = discover(&project, &data).into_iter().filter(|s| s.source == "Project").collect();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].description, "Cut a release");
        let loaded = load(&found, "release", None).unwrap();
        assert!(loaded.contains("Run scripts/tag.sh") && loaded.contains("- scripts/tag.sh"));
        assert_eq!(load(&found, "release", Some("scripts/tag.sh")).unwrap(), "git tag");
        assert!(load(&found, "release", Some("../../../secret")).is_err());
        assert!(instructions(&found).contains("- release: Cut a release"));

        set_enabled(&data, "release", false).unwrap();
        let again: Vec<Skill> = discover(&project, &data).into_iter().filter(|s| s.source == "Project").collect();
        assert!(!again[0].enabled);
        assert!(instructions(&again).is_empty());
    }
}

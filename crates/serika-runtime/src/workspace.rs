use std::path::{Component, Path, PathBuf};

use ignore::WalkBuilder;

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct RelFile {
    pub relative: String,
    pub bytes: u64,
}

const SENSITIVE_NAMES: &[&str] = &[
    ".env",
    ".npmrc",
    ".pypirc",
    "credentials.json",
    "secrets.json",
    "id_rsa",
    "id_dsa",
    "id_ecdsa",
    "id_ed25519",
];

const SENSITIVE_SUFFIXES: &[&str] = &[".pem", ".pfx", ".p12", ".key", ".kdbx"];

pub fn is_sensitive(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let lower = name.to_ascii_lowercase();
    if SENSITIVE_NAMES.iter().any(|item| lower == *item) || lower.starts_with(".env") {
        return true;
    }
    SENSITIVE_SUFFIXES.iter().any(|suffix| lower.ends_with(suffix))
}

pub fn is_binary_sample(bytes: &[u8]) -> bool {
    bytes.contains(&0)
}

/// Resolve `user_path` inside `root`, rejecting `..` escapes and symlink escapes.
pub fn resolve_inside(root: &Path, user_path: &str) -> Result<PathBuf> {
    if user_path.contains('\0') {
        return Err(Error::msg("path contains a null byte"));
    }
    let root = lexical_normalize(&absolute_root(root)?)?;
    let raw = Path::new(user_path);
    let joined = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        root.join(raw)
    };
    let normalized = lexical_normalize(&joined)?;
    if !normalized.starts_with(&root) {
        return Err(Error::msg(format!(
            "path escapes the project: {user_path}"
        )));
    }
    if normalized.exists() {
        let canonical = dunce::canonicalize(&normalized).unwrap_or(normalized.clone());
        let canonical = lexical_normalize(&canonical)?;
        if !canonical.starts_with(&root) {
            return Err(Error::msg(format!(
                "symlink escapes the project: {user_path}"
            )));
        }
        return Ok(canonical);
    }
    // New path: the nearest existing ancestor must also stay inside the project.
    let mut ancestor = normalized.clone();
    let mut missing = Vec::new();
    while !ancestor.exists() {
        let Some(name) = ancestor.file_name().map(|n| n.to_os_string()) else {
            break;
        };
        missing.push(name);
        if !ancestor.pop() {
            break;
        }
    }
    if ancestor.exists() {
        let canonical = dunce::canonicalize(&ancestor).unwrap_or(ancestor);
        let canonical = lexical_normalize(&canonical)?;
        if !canonical.starts_with(&root) {
            return Err(Error::msg(format!(
                "symlink escapes the project: {user_path}"
            )));
        }
        missing.reverse();
        let mut full = canonical;
        for name in missing {
            if name == ".." || name == "." {
                return Err(Error::msg(format!("path escapes the project: {user_path}")));
            }
            full.push(name);
        }
        return Ok(full);
    }
    Ok(normalized)
}

fn absolute_root(root: &Path) -> Result<PathBuf> {
    if root.exists() {
        dunce::canonicalize(root).map_err(Error::from)
    } else {
        Err(Error::msg(format!(
            "project directory does not exist: {}",
            root.display()
        )))
    }
}

fn lexical_normalize(path: &Path) -> Result<PathBuf> {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return Err(Error::msg("path escapes the project"));
                }
            }
            Component::RootDir | Component::Prefix(_) | Component::Normal(_) => {
                out.push(component.as_os_str());
            }
        }
    }
    Ok(out)
}

pub fn list_files(root: &Path, exclusions: &[String], max_depth: Option<usize>) -> Result<Vec<RelFile>> {
    let root = absolute_root(root)?;
    let mut walk = WalkBuilder::new(&root);
    walk.hidden(true)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(false)
        .parents(false)
        .follow_links(false)
        .require_git(false);
    if let Some(depth) = max_depth {
        walk.max_depth(Some(depth));
    }
    let mut files = Vec::new();
    for entry in walk.build() {
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let path = entry.path();
        if excluded(path, &root, exclusions) || is_sensitive(path) {
            continue;
        }
        let Ok(relative) = path.strip_prefix(&root) else { continue };
        let relative = relative.to_string_lossy().replace('\\', "/");
        if relative.is_empty() {
            continue;
        }
        let bytes = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
        files.push(RelFile { relative, bytes });
    }
    files.sort_by(|a, b| a.relative.cmp(&b.relative));
    Ok(files)
}

pub fn excluded(path: &Path, root: &Path, exclusions: &[String]) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    for component in relative.components() {
        let Some(name) = component.as_os_str().to_str() else {
            continue;
        };
        if exclusions.iter().any(|pattern| name == pattern) {
            return true;
        }
    }
    false
}

pub fn language_of(path: &str) -> Option<&'static str> {
    let ext = path.rsplit('.').next().unwrap_or("");
    Some(match ext {
        "rs" => "Rust",
        "py" => "Python",
        "ts" | "tsx" => "TypeScript",
        "js" | "jsx" | "mjs" | "cjs" => "JavaScript",
        "go" => "Go",
        "cpp" | "cc" | "cxx" | "hpp" | "hh" => "C++",
        "c" | "h" => "C",
        "java" => "Java",
        "cs" => "C#",
        "rb" => "Ruby",
        "php" => "PHP",
        "swift" => "Swift",
        "kt" => "Kotlin",
        "toml" => "TOML",
        "json" => "JSON",
        "md" => "Markdown",
        "yml" | "yaml" => "YAML",
        "css" => "CSS",
        "html" => "HTML",
        "sql" => "SQL",
        "ps1" => "PowerShell",
        "sh" => "Shell",
        "xml" => "XML",
        _ => return None,
    })
}

pub fn looks_like_manifest(path: &str) -> bool {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    matches!(
        name,
        "Cargo.toml"
            | "package.json"
            | "pyproject.toml"
            | "go.mod"
            | "CMakeLists.txt"
            | "pom.xml"
            | "build.gradle"
            | "build.gradle.kts"
            | "composer.json"
            | "Gemfile"
            | "requirements.txt"
    ) || name.ends_with(".sln")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_parent_traversal() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("inside.txt"), "ok").unwrap();
        assert!(resolve_inside(dir.path(), "inside.txt").is_ok());
        assert!(resolve_inside(dir.path(), "../outside.txt").is_err());
        assert!(resolve_inside(dir.path(), "sub/../../outside.txt").is_err());
    }

    #[test]
    fn respects_gitignore_and_sensitive_names() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".gitignore"), "ignored.txt\n").unwrap();
        std::fs::write(dir.path().join("keep.rs"), "fn main() {}\n").unwrap();
        std::fs::write(dir.path().join("ignored.txt"), "nope").unwrap();
        std::fs::write(dir.path().join(".env"), "TOKEN=1\n").unwrap();
        let files = list_files(dir.path(), &["target".into()], None).unwrap();
        let names: Vec<_> = files.iter().map(|file| file.relative.as_str()).collect();
        assert!(names.contains(&"keep.rs"));
        assert!(!names.iter().any(|name| name.ends_with("ignored.txt")));
        assert!(!names.iter().any(|name| name.ends_with(".env")));
        assert!(is_sensitive(Path::new(".env.local")));
        assert!(is_sensitive(Path::new("server.pem")));
    }
}

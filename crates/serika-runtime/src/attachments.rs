//! Files and images attached to a message.
//!
//! The limits and the way text files travel match the Serika chat web app: images are uploaded
//! (up to 4, 10 MB each) and sent as `attachments`; text and code files are added to the message
//! as fenced blocks (up to 6, 256 KB each, 512 KB in all). Claude Code and Codex get the staged
//! file paths instead, since they read files themselves.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

pub const MAX_IMAGES: usize = 4;
pub const MAX_IMAGE_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_FILES: usize = 6;
pub const MAX_FILE_BYTES: u64 = 256 * 1024;
pub const MAX_TOTAL_TEXT_BYTES: u64 = 512 * 1024;
/// Anything else (PDF, archive, large text) is passed by path, for providers that can read files.
pub const MAX_OTHER_BYTES: u64 = 100 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AttachmentKind {
    Image,
    Text,
    File,
}

impl AttachmentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Text => "text",
            Self::File => "file",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub name: String,
    pub kind: AttachmentKind,
    pub size: u64,
    /// The staged copy inside the app data folder.
    pub path: String,
    /// Where the Serika server keeps an uploaded image.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Contents of a text file, kept so later turns can include it again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default)]
    pub lang: String,
}

const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp"];

const LANGS: &[(&str, &str)] = &[
    ("js", "javascript"), ("jsx", "jsx"), ("mjs", "javascript"), ("cjs", "javascript"), ("ts", "typescript"),
    ("tsx", "tsx"), ("py", "python"), ("rb", "ruby"), ("go", "go"), ("rs", "rust"), ("java", "java"),
    ("kt", "kotlin"), ("swift", "swift"), ("c", "c"), ("h", "c"), ("cpp", "cpp"), ("cc", "cpp"), ("hpp", "cpp"),
    ("cs", "csharp"), ("php", "php"), ("sh", "bash"), ("bash", "bash"), ("zsh", "bash"), ("ps1", "powershell"),
    ("sql", "sql"), ("html", "html"), ("htm", "html"), ("css", "css"), ("scss", "scss"), ("json", "json"),
    ("yaml", "yaml"), ("yml", "yaml"), ("toml", "toml"), ("xml", "xml"), ("svg", "svg"), ("md", "markdown"),
    ("mdx", "markdown"), ("csv", "csv"), ("tsv", "tsv"), ("txt", "text"), ("log", "text"), ("ini", "ini"),
    ("env", "text"), ("lua", "lua"), ("r", "r"), ("dart", "dart"), ("vue", "vue"), ("svelte", "svelte"),
    ("graphql", "graphql"), ("qml", "qml"), ("cmake", "cmake"), ("bat", "bat"), ("cfg", "ini"), ("lock", "text"),
];

fn extension(name: &str) -> String {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

pub fn lang_for(name: &str) -> &'static str {
    let ext = extension(name);
    LANGS
        .iter()
        .find(|(e, _)| *e == ext)
        .map(|(_, lang)| *lang)
        .unwrap_or("text")
}

pub fn mime_for(name: &str) -> &'static str {
    match extension(name).as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}

fn safe_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ' | '(' | ')') { c } else { '_' })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').to_string();
    if trimmed.is_empty() {
        "file".to_string()
    } else {
        trimmed
    }
}

fn looks_binary(sample: &[u8]) -> bool {
    sample.contains(&0)
}

/// Copy the picked files into `dest` and describe each one. Files that cannot be used come back
/// as messages in the second list. `existing` counts toward the limits.
pub fn stage(sources: &[PathBuf], dest: &Path, existing: &[Attachment]) -> (Vec<Attachment>, Vec<String>) {
    let mut staged: Vec<Attachment> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut images = existing.iter().filter(|a| a.kind == AttachmentKind::Image).count();
    let mut texts = existing.iter().filter(|a| a.kind == AttachmentKind::Text).count();
    let mut text_bytes: u64 = existing
        .iter()
        .filter(|a| a.kind == AttachmentKind::Text)
        .map(|a| a.size)
        .sum();
    if std::fs::create_dir_all(dest).is_err() {
        return (staged, vec!["Could not prepare the attachments folder.".to_string()]);
    }
    for source in sources {
        let name = source
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();
        let Ok(meta) = std::fs::metadata(source) else {
            errors.push(format!("{name}: could not be read."));
            continue;
        };
        if meta.is_dir() {
            errors.push(format!("{name}: folders cannot be attached. Add it as a project instead."));
            continue;
        }
        let size = meta.len();
        let ext = extension(&name);
        let mut kind;
        let mut text: Option<String> = None;
        if IMAGE_EXTS.contains(&ext.as_str()) {
            if size > MAX_IMAGE_BYTES {
                errors.push(format!("{name} is larger than 10 MB."));
                continue;
            }
            if images >= MAX_IMAGES {
                errors.push(format!("Up to {MAX_IMAGES} images per message."));
                continue;
            }
            kind = AttachmentKind::Image;
        } else {
            let is_known_text = LANGS.iter().any(|(e, _)| *e == ext);
            let mut sniffed_text = false;
            if size <= MAX_FILE_BYTES {
                if let Ok(bytes) = std::fs::read(source) {
                    if !looks_binary(&bytes[..bytes.len().min(8000)]) {
                        if let Ok(content) = String::from_utf8(bytes) {
                            sniffed_text = true;
                            text = Some(content);
                        }
                    }
                }
            }
            if sniffed_text && (is_known_text || ext.is_empty() || size <= MAX_FILE_BYTES) {
                if texts >= MAX_FILES {
                    errors.push(format!("Up to {MAX_FILES} text files per message."));
                    continue;
                }
                if text_bytes + size > MAX_TOTAL_TEXT_BYTES {
                    errors.push("Text files are limited to 512 KB per message.".to_string());
                    continue;
                }
                kind = AttachmentKind::Text;
            } else {
                if size > MAX_OTHER_BYTES {
                    errors.push(format!("{name} is too large to attach."));
                    continue;
                }
                kind = AttachmentKind::File;
                text = None;
            }
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        let target = dest.join(format!("{}-{}", &id[..8], safe_name(&name)));
        if let Err(err) = std::fs::copy(source, &target) {
            errors.push(format!("{name}: {err}"));
            continue;
        }
        match kind {
            AttachmentKind::Image => images += 1,
            AttachmentKind::Text => {
                texts += 1;
                text_bytes += size;
            }
            AttachmentKind::File => {}
        }
        if kind == AttachmentKind::Text && text.is_none() {
            kind = AttachmentKind::File;
        }
        staged.push(Attachment {
            lang: lang_for(&name).to_string(),
            name,
            kind,
            size,
            path: target.display().to_string(),
            url: None,
            text,
        });
    }
    (staged, errors)
}

/// The staging folder must contain every path the interface sends back.
pub fn verify(attachments: &[Attachment], root: &Path) -> Result<()> {
    let root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    for attachment in attachments {
        let path = dunce::canonicalize(&attachment.path)
            .map_err(|_| Error::msg(format!("{} is no longer available", attachment.name)))?;
        if !path.starts_with(&root) {
            return Err(Error::msg("attachments must come from the attachments folder"));
        }
    }
    Ok(())
}

/// Read back text for attachments that came from the interface without their contents.
pub fn load_text(attachments: &mut [Attachment]) {
    for attachment in attachments.iter_mut() {
        if attachment.kind == AttachmentKind::Text && attachment.text.is_none() {
            attachment.text = std::fs::read_to_string(&attachment.path).ok();
            if attachment.text.is_none() {
                attachment.kind = AttachmentKind::File;
            }
        }
    }
}

/// The message with each text file appended as a fenced block, as the web app does.
pub fn with_files(message: &str, attachments: &[Attachment]) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !message.trim().is_empty() {
        parts.push(message.trim().to_string());
    }
    for attachment in attachments {
        if attachment.kind != AttachmentKind::Text {
            continue;
        }
        let Some(text) = &attachment.text else { continue };
        let mut longest = 0usize;
        let mut run = 0usize;
        for c in text.chars() {
            if c == '`' {
                run += 1;
                longest = longest.max(run);
            } else {
                run = 0;
            }
        }
        let fence = "`".repeat((longest + 1).max(3));
        let lang = if attachment.lang == "text" { "" } else { attachment.lang.as_str() };
        parts.push(format!(
            "Attached file `{}`:\n\n{fence}{lang}\n{}\n{fence}",
            attachment.name,
            text.trim_end_matches('\n')
        ));
    }
    parts.join("\n\n")
}

/// Lines that tell a file-reading agent where the attachments are.
pub fn path_note(attachments: &[Attachment]) -> String {
    if attachments.is_empty() {
        return String::new();
    }
    let mut out = String::from("\n\nThe user attached these files. Read them from disk when relevant:\n");
    for attachment in attachments {
        out.push_str(&format!("- {} ({}): {}\n", attachment.name, attachment.kind.as_str(), attachment.path));
    }
    out
}

pub fn parse_json(raw: Option<&str>) -> Vec<Attachment> {
    raw.and_then(|text| serde_json::from_str(text).ok()).unwrap_or_default()
}

pub fn to_json(attachments: &[Attachment]) -> Option<String> {
    if attachments.is_empty() {
        None
    } else {
        serde_json::to_string(attachments).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_files_become_fenced_blocks() {
        let attachment = Attachment {
            name: "main.rs".into(),
            kind: AttachmentKind::Text,
            size: 12,
            path: String::new(),
            url: None,
            text: Some("fn main() {}\n".into()),
            lang: lang_for("main.rs").into(),
        };
        let message = with_files("look at this", &[attachment]);
        assert!(message.starts_with("look at this\n\nAttached file `main.rs`:"));
        assert!(message.contains("```rust\nfn main() {}\n```"));
    }

    #[test]
    fn longer_fences_wrap_code_that_contains_fences() {
        let attachment = Attachment {
            name: "a.md".into(),
            kind: AttachmentKind::Text,
            size: 9,
            path: String::new(),
            url: None,
            text: Some("```x```".into()),
            lang: "markdown".into(),
        };
        assert!(with_files("", &[attachment]).contains("````markdown"));
    }

    #[test]
    fn staging_sorts_files_and_enforces_limits() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let mut sources = Vec::new();
        for i in 0..5 {
            let p = src.join(format!("pic{i}.png"));
            std::fs::write(&p, b"\x89PNG\r\n\x1a\nfake").unwrap();
            sources.push(p);
        }
        let notes = src.join("notes.txt");
        std::fs::write(&notes, "hello").unwrap();
        let blob = src.join("data.bin");
        std::fs::write(&blob, [0u8, 1, 2, 3]).unwrap();
        sources.push(notes);
        sources.push(blob);
        let (staged, errors) = stage(&sources, &dir.path().join("out"), &[]);
        assert_eq!(staged.iter().filter(|a| a.kind == AttachmentKind::Image).count(), MAX_IMAGES);
        assert_eq!(errors.len(), 1);
        assert!(staged.iter().any(|a| a.kind == AttachmentKind::Text && a.text.as_deref() == Some("hello")));
        assert!(staged.iter().any(|a| a.kind == AttachmentKind::File && a.name == "data.bin"));
        assert!(verify(&staged, &dir.path().join("out")).is_ok());
    }
}

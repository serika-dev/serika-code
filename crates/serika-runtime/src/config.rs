use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::paths::AppPaths;
use crate::permissions::PermissionMode;

pub const DEFAULT_API_BASE: &str = "https://api.serika.dev";
pub const DEFAULT_ACCOUNTS_BASE: &str = "https://accounts.serika.dev";

fn default_api() -> String {
    DEFAULT_API_BASE.to_string()
}
fn default_accounts() -> String {
    DEFAULT_ACCOUNTS_BASE.to_string()
}
fn default_timeout() -> u64 {
    120
}
fn default_rounds() -> u32 {
    12
}
fn default_retention() -> u32 {
    90
}
fn default_log() -> String {
    "info".to_string()
}
fn default_exclusions() -> Vec<String> {
    [
        "node_modules",
        "target",
        "dist",
        "build",
        ".git",
        "vendor",
        "__pycache__",
        ".next",
        "coverage",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}
fn default_read() -> usize {
    200_000
}
fn default_theme() -> String {
    "dark".to_string()
}
fn default_locale() -> String {
    "en".to_string()
}
fn default_browser_home() -> String {
    "https://duckduckgo.com".to_string()
}
fn default_context_chars() -> usize {
    60_000
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    #[serde(default = "default_api")]
    pub api_base_url: String,
    #[serde(default = "default_accounts")]
    pub accounts_base_url: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub permission: PermissionMode,
    #[serde(default = "default_timeout")]
    pub command_timeout_secs: u64,
    #[serde(default = "default_rounds")]
    pub max_tool_rounds: u32,
    #[serde(default = "default_retention")]
    pub session_retention_days: u32,
    #[serde(default = "default_log")]
    pub log_level: String,
    #[serde(default = "default_exclusions")]
    pub context_exclusions: Vec<String>,
    #[serde(default = "default_read")]
    pub max_read_bytes: usize,
    #[serde(default = "default_theme")]
    pub theme: String,
    /// UI language code (`en`, `nl`, `ja`, ...). English is the canonical default.
    #[serde(default = "default_locale")]
    pub locale: String,
    #[serde(default = "default_context_chars")]
    pub max_context_chars: usize,
    #[serde(default = "default_true")]
    pub stream: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openai_api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anthropic_api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gemini_api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openai_base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anthropic_base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gemini_base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ollama_base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_model: Option<String>,
    /// Claude Code binary when it is not on PATH.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_path: Option<String>,
    /// Sets `CLAUDE_CONFIG_DIR` for Claude Code, so a second account keeps its own login.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_config_dir: Option<String>,
    /// Effort level for Claude Code runs (`--effort`), when the model supports one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_effort: Option<String>,
    /// Codex binary when it is not on PATH.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_path: Option<String>,
    /// Sets `CODEX_HOME` for Codex.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_home: Option<String>,
    /// Reasoning effort for Codex runs (`model_reasoning_effort`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_effort: Option<String>,
    /// Chromium-based browser the agent drives (Edge, Chrome, Chromium, Brave).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser_path: Option<String>,
    #[serde(default = "default_true")]
    pub browser_enabled: bool,
    #[serde(default = "default_browser_home")]
    pub browser_home: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_base_url: default_api(),
            accounts_base_url: default_accounts(),
            model: None,
            permission: PermissionMode::Standard,
            command_timeout_secs: default_timeout(),
            max_tool_rounds: default_rounds(),
            session_retention_days: default_retention(),
            log_level: default_log(),
            context_exclusions: default_exclusions(),
            max_read_bytes: default_read(),
            theme: default_theme(),
            locale: default_locale(),
            max_context_chars: default_context_chars(),
            stream: true,
            openai_api_key: None,
            anthropic_api_key: None,
            gemini_api_key: None,
            openai_base_url: None,
            anthropic_base_url: None,
            gemini_base_url: None,
            ollama_base_url: None,
            local_base_url: None,
            local_model: None,
            claude_path: None,
            claude_config_dir: None,
            codex_path: None,
            codex_home: None,
            claude_effort: None,
            codex_effort: None,
            browser_path: None,
            browser_enabled: true,
            browser_home: default_browser_home(),
        }
    }
}

impl Config {
    pub fn validate(&self) -> Result<()> {
        validate_base_url("api_base_url", &self.api_base_url)?;
        validate_base_url("accounts_base_url", &self.accounts_base_url)?;
        if self.command_timeout_secs == 0 || self.command_timeout_secs > 3600 {
            return Err(Error::msg(
                "command_timeout_secs must be between 1 and 3600",
            ));
        }
        if self.max_tool_rounds == 0 || self.max_tool_rounds > 40 {
            return Err(Error::msg("max_tool_rounds must be between 1 and 40"));
        }
        if self.max_read_bytes < 1024 || self.max_read_bytes > 5_000_000 {
            return Err(Error::msg(
                "max_read_bytes must be between 1024 and 5000000",
            ));
        }
        if self.max_context_chars < 4_000 {
            return Err(Error::msg("max_context_chars must be at least 4000"));
        }
        match self.theme.as_str() {
            "dark" | "light" => {}
            _ => return Err(Error::msg("theme must be \"dark\" or \"light\"")),
        }
        let home = self.browser_home.trim();
        if !(home.starts_with("https://") || home.starts_with("http://")) {
            return Err(Error::msg("browser_home must start with https:// or http://"));
        }
        if !is_supported_locale(&self.locale) {
            return Err(Error::msg(
                "locale must be a supported language code such as en, nl, es, fr, de, ja, or zh-CN",
            ));
        }
        match self.log_level.to_ascii_lowercase().as_str() {
            "error" | "warn" | "info" | "debug" | "trace" => {}
            _ => {
                return Err(Error::msg(
                    "log_level must be error, warn, info, debug, or trace",
                ))
            }
        }
        Ok(())
    }

    pub fn load_or_create(paths: &AppPaths) -> Result<Self> {
        paths.ensure()?;
        if !paths.config_file.exists() {
            let config = Config::default();
            config.save(&paths.config_file)?;
            return Ok(config);
        }
        let text = std::fs::read_to_string(&paths.config_file)?;
        let config: Config = serde_json::from_str(&text).map_err(|err| {
            Error::msg(format!(
                "could not parse {}: {err}",
                paths.config_file.display()
            ))
        })?;
        config.validate()?;
        Ok(config)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(path, text + "\n")?;
        Ok(())
    }

    /// Environment overrides used by tests and `serika config`. Documented, not a secret channel.
    pub fn apply_env_overrides(&mut self) {
        if let Ok(value) = std::env::var("SERIKA_API_BASE_URL") {
            if !value.trim().is_empty() {
                self.api_base_url = value;
            }
        }
        if let Ok(value) = std::env::var("SERIKA_ACCOUNTS_BASE_URL") {
            if !value.trim().is_empty() {
                self.accounts_base_url = value;
            }
        }
    }

    pub fn set_field(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "api_base_url" => self.api_base_url = value.to_string(),
            "accounts_base_url" => self.accounts_base_url = value.to_string(),
            "model" => {
                self.model = if value.is_empty() || value == "auto" {
                    None
                } else {
                    Some(value.to_string())
                }
            }
            "permission" => {
                self.permission = PermissionMode::parse(value).ok_or_else(|| {
                    Error::msg("permission must be read_only, standard, restricted, or full")
                })?;
            }
            "command_timeout_secs" => {
                self.command_timeout_secs = value
                    .parse()
                    .map_err(|_| Error::msg("command_timeout_secs must be a number"))?;
            }
            "max_tool_rounds" => {
                self.max_tool_rounds = value
                    .parse()
                    .map_err(|_| Error::msg("max_tool_rounds must be a number"))?;
            }
            "session_retention_days" => {
                self.session_retention_days = value
                    .parse()
                    .map_err(|_| Error::msg("session_retention_days must be a number"))?;
            }
            "log_level" => self.log_level = value.to_string(),
            "theme" => self.theme = value.to_string(),
            "locale" => self.locale = value.trim().to_string(),
            "claude_path" => self.claude_path = non_empty_value(value),
            "claude_config_dir" => self.claude_config_dir = non_empty_value(value),
            "codex_path" => self.codex_path = non_empty_value(value),
            "codex_home" => self.codex_home = non_empty_value(value),
            "claude_effort" => self.claude_effort = effort_value(value)?,
            "codex_effort" => self.codex_effort = effort_value(value)?,
            "browser_path" => self.browser_path = non_empty_value(value),
            "browser_home" => self.browser_home = value.trim().to_string(),
            "browser_enabled" => {
                self.browser_enabled = matches!(value.trim(), "true" | "1" | "on" | "yes");
            }
            "stream" => {
                self.stream = matches!(value.trim(), "true" | "1" | "on" | "yes");
            }
            "max_read_bytes" => {
                self.max_read_bytes = value
                    .parse()
                    .map_err(|_| Error::msg("max_read_bytes must be a number"))?;
            }
            "max_context_chars" => {
                self.max_context_chars = value
                    .parse()
                    .map_err(|_| Error::msg("max_context_chars must be a number"))?;
            }
            "context_exclusions" => {
                self.context_exclusions = value
                    .split(',')
                    .map(|part| part.trim().to_string())
                    .filter(|part| !part.is_empty())
                    .collect();
            }
            "openai_api_key" => {
                self.openai_api_key = if value.trim().is_empty() {
                    None
                } else {
                    Some(value.trim().to_string())
                };
            }
            "anthropic_api_key" | "claude_api_key" => {
                self.anthropic_api_key = if value.trim().is_empty() {
                    None
                } else {
                    Some(value.trim().to_string())
                };
            }
            "gemini_api_key" => {
                self.gemini_api_key = if value.trim().is_empty() {
                    None
                } else {
                    Some(value.trim().to_string())
                };
            }
            "ollama_base_url" => {
                self.ollama_base_url = if value.trim().is_empty() {
                    None
                } else {
                    Some(value.trim().to_string())
                };
            }
            "local_base_url" => {
                self.local_base_url = if value.trim().is_empty() {
                    None
                } else {
                    Some(value.trim().to_string())
                };
            }
            "local_model" => {
                self.local_model = if value.trim().is_empty() {
                    None
                } else {
                    Some(value.trim().to_string())
                };
            }
            _ => {
                return Err(Error::msg(format!(
                    "unknown setting \"{key}\". See docs/CONFIGURATION.md"
                )))
            }
        }
        self.validate()
    }

    pub fn resolve_openai_key(&self) -> Option<String> {
        std::env::var("OPENAI_API_KEY")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| self.openai_api_key.clone().filter(|s| !s.trim().is_empty()))
    }

    pub fn resolve_anthropic_key(&self) -> Option<String> {
        std::env::var("ANTHROPIC_API_KEY")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                self.anthropic_api_key
                    .clone()
                    .filter(|s| !s.trim().is_empty())
            })
    }

    pub fn resolve_gemini_key(&self) -> Option<String> {
        std::env::var("GEMINI_API_KEY")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                std::env::var("GOOGLE_API_KEY")
                    .ok()
                    .filter(|s| !s.trim().is_empty())
            })
            .or_else(|| self.gemini_api_key.clone().filter(|s| !s.trim().is_empty()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectSettings {
    pub permission: Option<PermissionMode>,
    #[serde(default)]
    pub extra_exclusions: Vec<String>,
}

impl ProjectSettings {
    pub fn load(project: &Path) -> Result<Self> {
        let path = crate::paths::project_settings_path(project);
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)?;
        serde_json::from_str(&text)
            .map_err(|err| Error::msg(format!("could not parse {}: {err}", path.display())))
    }
}

/// An effort level is passed to a CLI as an argument, so only plain words are kept.
fn effort_value(value: &str) -> Result<Option<String>> {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty() || value == "default" {
        return Ok(None);
    }
    if value.len() > 16 || !value.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(Error::msg("effort must be a level such as low, medium or high"));
    }
    Ok(Some(value))
}

fn non_empty_value(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn is_supported_locale(value: &str) -> bool {
    matches!(
        value.trim(),
        "en" | "es"
            | "fr"
            | "de"
            | "pt-BR"
            | "pt"
            | "it"
            | "nl"
            | "pl"
            | "ru"
            | "tr"
            | "ja"
            | "ko"
            | "zh-CN"
            | "zh"
    )
}

fn validate_base_url(name: &str, value: &str) -> Result<()> {
    let value = value.trim().trim_end_matches('/');
    let ok = value.starts_with("https://")
        || value.starts_with("http://127.0.0.1")
        || value.starts_with("http://localhost");
    if !ok || value.contains(' ') {
        return Err(Error::msg(format!(
            "{name} must be an https URL, or http://localhost for tests"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::AppPaths;

    #[test]
    fn roundtrip_and_validation() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(dir.path().to_path_buf()).unwrap();
        let mut config = Config::load_or_create(&paths).unwrap();
        assert_eq!(config.api_base_url, DEFAULT_API_BASE);
        config.set_field("permission", "restricted").unwrap();
        config.set_field("theme", "light").unwrap();
        config.set_field("locale", "nl").unwrap();
        config.save(&paths.config_file).unwrap();
        let loaded = Config::load_or_create(&paths).unwrap();
        assert_eq!(loaded.locale, "nl");
        assert!(config.set_field("locale", "pirate").is_err());
        assert_eq!(loaded.permission, PermissionMode::Restricted);
        assert_eq!(loaded.theme, "light");
        assert!(config.set_field("theme", "neon").is_err());
        assert!(config
            .set_field("api_base_url", "http://evil.example")
            .is_err());
    }
}

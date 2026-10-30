use serde::{Deserialize, Serialize};

/// How much the agent may do without stopping to ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    /// Inspect the project. No writes, no arbitrary commands.
    ReadOnly,
    /// Writes inside the project and ordinary development commands.
    /// Destructive actions still ask.
    Standard,
    /// Every write and every non-inspect command asks first.
    Restricted,
    /// Autonomous inside the project, except destructive actions, which still ask.
    Full,
}

impl Default for PermissionMode {
    fn default() -> Self {
        Self::Standard
    }
}

impl PermissionMode {
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "read_only" | "readonly" | "read" => Some(Self::ReadOnly),
            "standard" | "default" => Some(Self::Standard),
            "restricted" => Some(Self::Restricted),
            "full" | "full_access" => Some(Self::Full),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::Standard => "standard",
            Self::Restricted => "restricted",
            Self::Full => "full",
        }
    }
}

impl std::fmt::Display for PermissionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    Allow,
    /// Ask the user. `destructive` stays interactive even when `--yes` is set.
    Prompt { destructive: bool },
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    Read,
    /// Reading a credential-like file.
    ReadSensitive,
    Write,
    WriteOutside,
    Delete,
    Command,
    /// Ordinary build and test commands.
    CommandDev,
    CommandReadOnly,
    CommandDestructive,
    GitCommit,
    GitDestructive,
}

pub fn gate(mode: PermissionMode, action: ActionKind) -> Gate {
    use ActionKind::*;
    use PermissionMode::*;
    match (mode, action) {
        (_, Read) => Gate::Allow,
        (_, CommandReadOnly) => Gate::Allow,
        (ReadOnly, _) => Gate::Deny,
        (_, ReadSensitive) => Gate::Prompt { destructive: true },
        (_, WriteOutside) => Gate::Prompt { destructive: true },
        (_, GitDestructive | CommandDestructive) => Gate::Prompt { destructive: true },
        (_, GitCommit) => Gate::Prompt { destructive: true },
        (Restricted, Write | Delete | Command | CommandDev) => Gate::Prompt { destructive: false },
        (Standard, Delete) => Gate::Prompt { destructive: true },
        (Standard, Write | CommandDev) => Gate::Allow,
        (Standard, Command) => Gate::Prompt { destructive: false },
        (Full, Write | Command | CommandDev) => Gate::Allow,
        (Full, Delete) => Gate::Prompt { destructive: false },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_blocks_writes_and_allows_inspection() {
        assert_eq!(gate(PermissionMode::ReadOnly, ActionKind::Read), Gate::Allow);
        assert_eq!(gate(PermissionMode::ReadOnly, ActionKind::Write), Gate::Deny);
        assert_eq!(gate(PermissionMode::ReadOnly, ActionKind::Command), Gate::Deny);
        assert_eq!(
            gate(PermissionMode::ReadOnly, ActionKind::CommandReadOnly),
            Gate::Allow
        );
    }

    #[test]
    fn destructive_always_prompts_except_read_only_denies() {
        assert!(matches!(
            gate(PermissionMode::Full, ActionKind::GitDestructive),
            Gate::Prompt { destructive: true }
        ));
        assert_eq!(
            gate(PermissionMode::ReadOnly, ActionKind::GitCommit),
            Gate::Deny
        );
        assert!(matches!(
            gate(PermissionMode::Standard, ActionKind::WriteOutside),
            Gate::Prompt { destructive: true }
        ));
    }

    #[test]
    fn standard_allows_project_writes_and_restricted_asks() {
        assert_eq!(gate(PermissionMode::Standard, ActionKind::Write), Gate::Allow);
        assert!(matches!(
            gate(PermissionMode::Restricted, ActionKind::Write),
            Gate::Prompt { destructive: false }
        ));
        assert_eq!(gate(PermissionMode::Full, ActionKind::Command), Gate::Allow);
    }
}

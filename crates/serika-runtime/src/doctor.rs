use std::process::Command;

use crate::error::Result;
use crate::Runtime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckLevel {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone)]
pub struct Check {
    pub name: String,
    pub level: CheckLevel,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct DoctorReport {
    pub checks: Vec<Check>,
}

impl DoctorReport {
    pub fn ok(&self) -> bool {
        !self.checks.iter().any(|check| check.level == CheckLevel::Fail)
    }

    pub fn render(&self) -> String {
        let mut text = String::from("Serika Code doctor\n");
        for check in &self.checks {
            let mark = match check.level {
                CheckLevel::Pass => "ok",
                CheckLevel::Warn => "warn",
                CheckLevel::Fail => "fail",
            };
            text.push_str(&format!("[{mark}] {}: {}\n", check.name, check.detail));
        }
        text.push_str(if self.ok() {
            "No blocking problems.\n"
        } else {
            "Fix the failed checks and run doctor again.\n"
        });
        text
    }
}

impl Runtime {
    pub async fn doctor(&self) -> Result<DoctorReport> {
        let mut checks = Vec::new();
        checks.push(Check {
            name: "version".into(),
            level: CheckLevel::Pass,
            detail: format!("{} ({})", crate::client::VERSION, std::env::consts::OS),
        });
        match self.config.validate() {
            Ok(()) => checks.push(Check {
                name: "config".into(),
                level: CheckLevel::Pass,
                detail: format!("{}", self.paths.config_file.display()),
            }),
            Err(err) => checks.push(Check {
                name: "config".into(),
                level: CheckLevel::Fail,
                detail: err.to_string(),
            }),
        }
        match self.sessions.integrity_ok() {
            Ok(true) => checks.push(Check {
                name: "database".into(),
                level: CheckLevel::Pass,
                detail: format!("{}", self.paths.database.display()),
            }),
            Ok(false) => checks.push(Check {
                name: "database".into(),
                level: CheckLevel::Fail,
                detail: "integrity check failed".into(),
            }),
            Err(err) => checks.push(Check {
                name: "database".into(),
                level: CheckLevel::Fail,
                detail: err.to_string(),
            }),
        }
        let probe = self.paths.logs_dir.join(".probe");
        match std::fs::write(&probe, b"ok") {
            Ok(()) => {
                let _ = std::fs::remove_file(&probe);
                checks.push(Check {
                    name: "filesystem".into(),
                    level: CheckLevel::Pass,
                    detail: format!("log directory {}", self.paths.logs_dir.display()),
                });
            }
            Err(err) => checks.push(Check {
                name: "filesystem".into(),
                level: CheckLevel::Fail,
                detail: err.to_string(),
            }),
        }
        match self.secrets.load() {
            Ok(Some(session)) => checks.push(Check {
                name: "authentication".into(),
                level: CheckLevel::Pass,
                detail: format!("signed in as {}", session.username),
            }),
            Ok(None) => checks.push(Check {
                name: "authentication".into(),
                level: CheckLevel::Warn,
                detail: "not signed in. Run serika auth login.".into(),
            }),
            Err(err) => checks.push(Check {
                name: "authentication".into(),
                level: CheckLevel::Fail,
                detail: err.to_string(),
            }),
        }
        match self.client.models().await {
            Ok(catalog) => checks.push(Check {
                name: "models".into(),
                level: CheckLevel::Pass,
                detail: format!(
                    "{} free, {} premium from {}",
                    catalog.free.len(),
                    catalog.premium.len(),
                    self.config.api_base_url
                ),
            }),
            Err(err) => checks.push(Check {
                name: "models".into(),
                level: CheckLevel::Fail,
                detail: err.to_string(),
            }),
        }
        match self.secrets.load() {
            Ok(Some(session)) => match self.client.usage(&session.token).await {
                Ok(usage) => checks.push(Check {
                    name: "usage".into(),
                    level: CheckLevel::Pass,
                    detail: usage.summary_line(),
                }),
                Err(err) => checks.push(Check {
                    name: "usage".into(),
                    level: CheckLevel::Fail,
                    detail: err.to_string(),
                }),
            },
            _ => checks.push(Check {
                name: "usage".into(),
                level: CheckLevel::Warn,
                detail: "skipped until you are signed in".into(),
            }),
        }
        checks.push(git_check());
        Ok(DoctorReport { checks })
    }
}

fn git_check() -> Check {
    match Command::new("git").arg("--version").output() {
        Ok(output) if output.status.success() => Check {
            name: "git".into(),
            level: CheckLevel::Pass,
            detail: String::from_utf8_lossy(&output.stdout).trim().to_string(),
        },
        Ok(_) => Check {
            name: "git".into(),
            level: CheckLevel::Warn,
            detail: "git did not run successfully".into(),
        },
        Err(_) => Check {
            name: "git".into(),
            level: CheckLevel::Warn,
            detail: "git is not on PATH. Status and commit tools need it.".into(),
        },
    }
}

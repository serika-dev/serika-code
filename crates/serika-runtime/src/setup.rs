use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::client::VERSION;
use crate::config::Config;
use crate::error::{Error, Result};
use crate::paths::AppPaths;
use crate::updater::replace_binary;

#[derive(Debug, Clone)]
pub struct SetupOptions {
    pub prefix: Option<PathBuf>,
    pub add_to_path: bool,
    pub create_shortcuts: bool,
}

impl Default for SetupOptions {
    fn default() -> Self {
        Self {
            prefix: None,
            add_to_path: true,
            create_shortcuts: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupReport {
    pub install_root: PathBuf,
    pub bin_dir: PathBuf,
    pub installed_binaries: Vec<PathBuf>,
    pub path_updated: bool,
    pub shortcuts_created: Vec<PathBuf>,
    pub notes: Vec<String>,
}

impl SetupReport {
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("Serika Code {VERSION} setup complete\n"));
        out.push_str(&format!("  install root : {}\n", self.install_root.display()));
        out.push_str(&format!("  bin dir      : {}\n", self.bin_dir.display()));
        for bin in &self.installed_binaries {
            out.push_str(&format!("  binary       : {}\n", bin.display()));
        }
        for sc in &self.shortcuts_created {
            out.push_str(&format!("  shortcut     : {}\n", sc.display()));
        }
        if self.path_updated {
            out.push_str("  PATH         : updated (open a new terminal to use `serika`)\n");
        }
        for note in &self.notes {
            out.push_str(&format!("  note         : {note}\n"));
        }
        out
    }
}

pub fn default_install_root() -> Result<PathBuf> {
    if cfg!(windows) {
        let local = dirs::data_local_dir()
            .or_else(dirs::home_dir)
            .ok_or_else(|| Error::msg("could not resolve local app data directory"))?;
        Ok(local.join("Programs").join("Serika Code"))
    } else {
        let home = dirs::home_dir().ok_or_else(|| Error::msg("could not resolve home directory"))?;
        Ok(home.join(".local"))
    }
}

pub fn install_system(paths: &AppPaths, opts: &SetupOptions) -> Result<SetupReport> {
    paths.ensure()?;
    if !paths.config_file.exists() {
        Config::default().save(&paths.config_file)?;
    }

    let install_root = match &opts.prefix {
        Some(p) => p.clone(),
        None => default_install_root()?,
    };
    let bin_dir = install_root.join("bin");
    std::fs::create_dir_all(&bin_dir)?;

    let current_exe = std::env::current_exe()?;
    let source_dir = current_exe
        .parent()
        .ok_or_else(|| Error::msg("could not resolve executable directory"))?;

    let bin_names: &[&str] = if cfg!(windows) {
        &["serika.exe", "serika-desktop.exe"]
    } else {
        &["serika", "serika-desktop"]
    };

    let mut installed_binaries = Vec::new();
    let mut notes = Vec::new();

    for &name in bin_names {
        let src = source_dir.join(name);
        if src.exists() {
            let dst = bin_dir.join(name);
            let same = dunce::canonicalize(&src).ok() == dunce::canonicalize(&dst).ok();
            if !same {
                replace_binary(&src, &dst)?;
            }
            installed_binaries.push(dst);
        }
    }

    // If invoked from a single binary whose sibling wasn't built yet, ensure at least current_exe is installed
    if installed_binaries.is_empty() {
        if let Some(name) = current_exe.file_name().and_then(|n| n.to_str()) {
            let dst = bin_dir.join(name);
            let same = dunce::canonicalize(&current_exe).ok() == dunce::canonicalize(&dst).ok();
            if !same {
                replace_binary(&current_exe, &dst)?;
            }
            installed_binaries.push(dst);
        }
    }

    let path_updated = if opts.add_to_path {
        ensure_in_user_path(&bin_dir).unwrap_or_else(|err| {
            notes.push(format!("could not update PATH automatically: {err}"));
            false
        })
    } else {
        false
    };

    let mut shortcuts_created = Vec::new();
    if opts.create_shortcuts {
        match create_platform_shortcuts(&install_root, &bin_dir) {
            Ok(list) => shortcuts_created.extend(list),
            Err(err) => notes.push(format!("shortcut creation note: {err}")),
        }
    }

    Ok(SetupReport {
        install_root,
        bin_dir,
        installed_binaries,
        path_updated,
        shortcuts_created,
        notes,
    })
}

pub fn uninstall_system(
    paths: &AppPaths,
    prefix: Option<&Path>,
    purge_data: bool,
) -> Result<Vec<String>> {
    let install_root = match prefix {
        Some(p) => p.to_path_buf(),
        None => default_install_root()?,
    };
    let bin_dir = install_root.join("bin");
    let mut removed = Vec::new();

    for name in [
        "serika.exe",
        "serika-desktop.exe",
        "serika.old.exe",
        "serika-desktop.old.exe",
        "serika",
        "serika-desktop",
        "serika.old",
        "serika-desktop.old",
    ] {
        let p = bin_dir.join(name);
        if p.exists() && std::fs::remove_file(&p).is_ok() {
            removed.push(p.display().to_string());
        }
    }

    if prefix.is_none() {
        remove_platform_shortcuts(&mut removed);
    }

    if purge_data {
        if paths.home.exists() && std::fs::remove_dir_all(&paths.home).is_ok() {
            removed.push(paths.home.display().to_string());
        }
        if paths.data_dir.exists() && std::fs::remove_dir_all(&paths.data_dir).is_ok() {
            removed.push(paths.data_dir.display().to_string());
        }
    }

    Ok(removed)
}

fn ensure_in_user_path(bin_dir: &Path) -> Result<bool> {
    let bin_str = bin_dir.to_string_lossy().to_string();
    #[cfg(windows)]
    {
        let script = format!(
            "$target = '{}'; \
             $cur = [Environment]::GetEnvironmentVariable('Path', 'User'); \
             if (-not $cur) {{ $cur = '' }}; \
             $parts = $cur -split ';' | Where-Object {{ $_ -ne '' }}; \
             if ($parts -notcontains $target) {{ \
               $new = ($parts + $target) -join ';'; \
               [Environment]::SetEnvironmentVariable('Path', $new, 'User'); \
               Write-Output 'UPDATED' \
             }} else {{ \
               Write-Output 'ALREADY' \
             }}",
            bin_str.replace('\'', "''")
        );
        let output = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Ok(stdout.contains("UPDATED"));
    }
    #[cfg(not(windows))]
    {
        let home = dirs::home_dir().ok_or_else(|| Error::msg("could not resolve HOME"))?;
        let export_line = format!("\n# Serika Code\nexport PATH=\"{bin_str}:$PATH\"\n");
        let mut updated = false;
        for rc in [".bashrc", ".zshrc", ".profile"] {
            let rc_path = home.join(rc);
            if rc_path.exists() {
                let content = std::fs::read_to_string(&rc_path).unwrap_or_default();
                if !content.contains(&bin_str) {
                    let mut next = content;
                    next.push_str(&export_line);
                    std::fs::write(&rc_path, next)?;
                    updated = true;
                }
            }
        }
        Ok(updated)
    }
}

fn create_platform_shortcuts(_install_root: &Path, bin_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut created = Vec::new();
    #[cfg(windows)]
    {
        let desktop_exe = bin_dir.join("serika-desktop.exe");
        let cli_exe = bin_dir.join("serika.exe");
        let target_exe = if desktop_exe.exists() { &desktop_exe } else { &cli_exe };
        let default_work_dir = dirs::home_dir().unwrap_or_else(|| bin_dir.to_path_buf());
        if let Some(appdata) = dirs::data_dir() {
            let start_menu = appdata
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs");
            if start_menu.is_dir() {
                let lnk = start_menu.join("Serika Code.lnk");
                if create_windows_shortcut(target_exe, &lnk, &default_work_dir).is_ok() {
                    created.push(lnk);
                }
            }
        }
        if let Some(desktop) = dirs::desktop_dir() {
            let lnk = desktop.join("Serika Code.lnk");
            if create_windows_shortcut(target_exe, &lnk, &default_work_dir).is_ok() {
                created.push(lnk);
            }
        }
        let _ = register_windows_uninstall(_install_root, &cli_exe);
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            let app_bundle = home.join("Applications").join("Serika Code.app");
            let contents = app_bundle.join("Contents");
            let macos_dir = contents.join("MacOS");
            std::fs::create_dir_all(&macos_dir)?;
            let plist = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                 <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
                 <plist version=\"1.0\">\n\
                 <dict>\n\
                   <key>CFBundleExecutable</key><string>serika-desktop</string>\n\
                   <key>CFBundleIdentifier</key><string>dev.serika.code</string>\n\
                   <key>CFBundleName</key><string>Serika Code</string>\n\
                   <key>CFBundleShortVersionString</key><string>{VERSION}</string>\n\
                   <key>CFBundlePackageType</key><string>APPL</string>\n\
                   <key>NSHighResolutionCapable</key><true/>\n\
                 </dict>\n\
                 </plist>\n"
            );
            std::fs::write(contents.join("Info.plist"), plist)?;
            let desktop_src = bin_dir.join("serika-desktop");
            let bundle_bin = macos_dir.join("serika-desktop");
            if desktop_src.exists() {
                replace_binary(&desktop_src, &bundle_bin)?;
                created.push(app_bundle);
            }
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(data_dir) = dirs::data_local_dir() {
            let apps = data_dir.join("applications");
            std::fs::create_dir_all(&apps)?;
            let desktop_file = apps.join("serika-code.desktop");
            let exec_path = bin_dir.join("serika-desktop");
            let entry = format!(
                "[Desktop Entry]\n\
                 Type=Application\n\
                 Name=Serika Code\n\
                 Comment=Native AI coding agent for the Serika ecosystem\n\
                 Exec=\"{}\" %F\n\
                 Terminal=false\n\
                 Categories=Development;IDE;\n\
                 StartupWMClass=serika-desktop\n",
                exec_path.display()
            );
            std::fs::write(&desktop_file, entry)?;
            created.push(desktop_file);
        }
    }
    Ok(created)
}

#[cfg(windows)]
fn create_windows_shortcut(target: &Path, lnk: &Path, working_dir: &Path) -> Result<()> {
    let script = format!(
        "$ws = New-Object -ComObject WScript.Shell; \
         $s = $ws.CreateShortcut('{}'); \
         $s.TargetPath = '{}'; \
         $s.WorkingDirectory = '{}'; \
         $s.Description = 'Serika Code — Native AI coding agent'; \
         $s.Save()",
        lnk.display().to_string().replace('\'', "''"),
        target.display().to_string().replace('\'', "''"),
        working_dir.display().to_string().replace('\'', "''")
    );
    let status = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .status()?;
    if !status.success() {
        return Err(Error::msg("WScript.Shell shortcut creation returned non-zero"));
    }
    Ok(())
}

#[cfg(windows)]
fn register_windows_uninstall(install_root: &Path, cli_exe: &Path) -> Result<()> {
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\SerikaCode";
    let uninstall_cmd = format!("\"{}\" setup --uninstall", cli_exe.display());
    for (name, val) in [
        ("DisplayName", "Serika Code"),
        ("DisplayVersion", VERSION),
        ("Publisher", "Serika"),
        ("InstallLocation", &install_root.display().to_string()),
        ("UninstallString", &uninstall_cmd),
    ] {
        let _ = Command::new("reg")
            .args(["add", key, "/v", name, "/t", "REG_SZ", "/d", val, "/f"])
            .output();
    }
    Ok(())
}

fn remove_platform_shortcuts(removed: &mut Vec<String>) {
    #[cfg(windows)]
    {
        if let Some(appdata) = dirs::data_dir() {
            let start_lnk = appdata
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs")
                .join("Serika Code.lnk");
            if start_lnk.exists() && std::fs::remove_file(&start_lnk).is_ok() {
                removed.push(start_lnk.display().to_string());
            }
        }
        if let Some(desktop) = dirs::desktop_dir() {
            let desk_lnk = desktop.join("Serika Code.lnk");
            if desk_lnk.exists() && std::fs::remove_file(&desk_lnk).is_ok() {
                removed.push(desk_lnk.display().to_string());
            }
        }
        let _ = Command::new("reg")
            .args([
                "delete",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\SerikaCode",
                "/f",
            ])
            .output();
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            let app_bundle = home.join("Applications").join("Serika Code.app");
            if app_bundle.exists() && std::fs::remove_dir_all(&app_bundle).is_ok() {
                removed.push(app_bundle.display().to_string());
            }
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(data_dir) = dirs::data_local_dir() {
            let desktop_file = data_dir.join("applications").join("serika-code.desktop");
            if desktop_file.exists() && std::fs::remove_file(&desktop_file).is_ok() {
                removed.push(desktop_file.display().to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installs_and_uninstalls_into_custom_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let prefix = dir.path().join("prefix");
        let paths = AppPaths::under(home).unwrap();
        let report = install_system(
            &paths,
            &SetupOptions {
                prefix: Some(prefix.clone()),
                add_to_path: false,
                create_shortcuts: false,
            },
        )
        .unwrap();
        assert_eq!(report.install_root, prefix);
        assert!(report.bin_dir.is_dir());
        assert!(!report.installed_binaries.is_empty());
        assert!(paths.config_file.exists());

        let removed = uninstall_system(&paths, Some(&prefix), true).unwrap();
        assert!(!removed.is_empty());
        assert!(!paths.home.exists());
    }
}

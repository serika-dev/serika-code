use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::client::VERSION;
use crate::error::{Error, Result};

pub const DEFAULT_RELEASE_API: &str =
    "https://api.github.com/repos/serika-dev/serika-code/releases/latest";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub tag_name: String,
    pub release_url: String,
    pub published_at: String,
    pub notes: String,
    pub asset_name: String,
    pub asset_url: String,
    pub sha256_url: Option<String>,
    pub update_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateOutcome {
    pub previous_version: String,
    pub installed_version: String,
    pub install_dir: PathBuf,
    pub updated_binaries: Vec<String>,
    pub sha256_verified: bool,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    url: String,
}

pub fn target_triple() -> &'static str {
    if cfg!(target_os = "windows") {
        "x86_64-pc-windows-msvc"
    } else if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        "aarch64-apple-darwin"
    } else if cfg!(target_os = "macos") {
        "x86_64-apple-darwin"
    } else {
        "x86_64-unknown-linux-gnu"
    }
}

pub fn expected_asset_name() -> String {
    let triple = target_triple();
    if cfg!(target_os = "windows") {
        format!("serika-code-{triple}.zip")
    } else {
        format!("serika-code-{triple}.tar.gz")
    }
}

/// Remove any leftover `.old` / `.old.exe` binaries from a previous self-update.
pub fn cleanup_old_binaries() {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for name in ["serika.old.exe", "serika-desktop.old.exe", "serika.old", "serika-desktop.old"] {
                let candidate = dir.join(name);
                if candidate.exists() {
                    let _ = std::fs::remove_file(candidate);
                }
            }
        }
    }
}

pub async fn check_for_update() -> Result<UpdateInfo> {
    let url = std::env::var("SERIKA_UPDATE_URL").unwrap_or_else(|_| DEFAULT_RELEASE_API.to_string());
    check_for_update_at(&url).await
}

pub async fn check_for_update_at(url: &str) -> Result<UpdateInfo> {
    let client = build_http_client()?;
    let mut req = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(token) = github_token() {
        req = req.bearer_auth(token);
    }

    let response = req.send().await?;
    if response.status().as_u16() == 404 {
        return Ok(UpdateInfo {
            current_version: VERSION.to_string(),
            latest_version: VERSION.to_string(),
            tag_name: format!("v{VERSION}"),
            release_url: "https://github.com/serika-dev/serika-code/releases".to_string(),
            published_at: String::new(),
            notes: "No published releases found yet; running the latest build.".to_string(),
            asset_name: expected_asset_name(),
            asset_url: String::new(),
            sha256_url: None,
            update_available: false,
        });
    }
    if !response.status().is_success() {
        return Err(Error::msg(format!(
            "update check returned HTTP {}",
            response.status()
        )));
    }

    let release: GithubRelease = response.json().await?;
    let latest_version = release.tag_name.trim_start_matches('v').to_string();
    let wanted_asset = expected_asset_name();
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == wanted_asset)
        .or_else(|| {
            let triple = target_triple();
            release.assets.iter().find(|a| a.name.contains(triple))
        });

    let (asset_name, asset_url) = match asset {
        Some(a) => (
            a.name.clone(),
            if github_token().is_some() && !a.url.is_empty() {
                a.url.clone()
            } else {
                a.browser_download_url.clone()
            },
        ),
        None => (wanted_asset, String::new()),
    };

    let sha256_url = release
        .assets
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case("SHA256SUMS") || a.name.eq_ignore_ascii_case("SHA256SUMS.txt"))
        .map(|a| {
            if github_token().is_some() && !a.url.is_empty() {
                a.url.clone()
            } else {
                a.browser_download_url.clone()
            }
        });

    let update_available = semver_gt(&latest_version, VERSION);

    Ok(UpdateInfo {
        current_version: VERSION.to_string(),
        latest_version,
        tag_name: release.tag_name,
        release_url: release.html_url,
        published_at: release.published_at.unwrap_or_default(),
        notes: release.body.unwrap_or_default(),
        asset_name,
        asset_url,
        sha256_url,
        update_available,
    })
}

pub async fn install_update(info: &UpdateInfo, target_dir: Option<&Path>, force: bool) -> Result<UpdateOutcome> {
    if !info.update_available && !force {
        return Err(Error::msg(format!(
            "Serika Code {} is already up to date",
            info.current_version
        )));
    }
    if info.asset_url.is_empty() {
        return Err(Error::msg(format!(
            "release {} does not contain a prebuilt package for {}",
            info.tag_name,
            target_triple()
        )));
    }

    let install_dir = match target_dir {
        Some(dir) => dir.to_path_buf(),
        None => std::env::current_exe()?
            .parent()
            .ok_or_else(|| Error::msg("could not determine current executable directory"))?
            .to_path_buf(),
    };
    std::fs::create_dir_all(&install_dir)?;

    let client = build_http_client()?;
    let bytes = download_asset(&client, &info.asset_url).await?;

    let mut sha256_verified = false;
    if let Some(sums_url) = &info.sha256_url {
        if let Ok(sums_bytes) = download_asset(&client, sums_url).await {
            let sums_text = String::from_utf8_lossy(&sums_bytes);
            if let Some(expected_hash) = parse_sha256sums(&sums_text, &info.asset_name) {
                let actual_hash = sha256_hex(&bytes);
                if !actual_hash.eq_ignore_ascii_case(&expected_hash) {
                    return Err(Error::msg(format!(
                        "SHA-256 mismatch for {}: expected {expected_hash}, got {actual_hash}",
                        info.asset_name
                    )));
                }
                sha256_verified = true;
            }
        }
    }

    let staging = std::env::temp_dir().join(format!("serika-update-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&staging)?;
    let archive_path = staging.join(&info.asset_name);
    std::fs::write(&archive_path, &bytes)?;

    let extract_dir = staging.join("extracted");
    std::fs::create_dir_all(&extract_dir)?;
    extract_archive(&archive_path, &extract_dir)?;

    let mut updated_binaries = Vec::new();
    let bin_names: &[&str] = if cfg!(windows) {
        &["serika.exe", "serika-desktop.exe"]
    } else {
        &["serika", "serika-desktop"]
    };

    for &bin_name in bin_names {
        if let Some(found) = find_file_recursive(&extract_dir, bin_name) {
            let dest = install_dir.join(bin_name);
            replace_binary(&found, &dest)?;
            updated_binaries.push(bin_name.to_string());
        }
    }

    let _ = std::fs::remove_dir_all(&staging);

    if updated_binaries.is_empty() {
        return Err(Error::msg("downloaded archive did not contain serika binaries"));
    }

    Ok(UpdateOutcome {
        previous_version: info.current_version.clone(),
        installed_version: info.latest_version.clone(),
        install_dir,
        updated_binaries,
        sha256_verified,
    })
}

async fn download_asset(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let mut req = client.get(url);
    if url.contains("api.github.com/") && url.contains("/releases/assets/") {
        req = req.header("Accept", "application/octet-stream");
    }
    if let Some(token) = github_token() {
        req = req.bearer_auth(token);
    }
    let resp = req.send().await?;
    if !resp.status().is_success() {
        return Err(Error::msg(format!("download failed with HTTP {}", resp.status())));
    }
    let bytes = resp.bytes().await?;
    Ok(bytes.to_vec())
}

fn build_http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(format!("serika-code/{VERSION}"))
        .timeout(std::time::Duration::from_secs(60))
        .build()?)
}

fn github_token() -> Option<String> {
    std::env::var("SERIKA_GITHUB_TOKEN")
        .ok()
        .or_else(|| std::env::var("GITHUB_TOKEN").ok())
        .filter(|v| !v.trim().is_empty())
}

pub fn replace_binary(source: &Path, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if dest.exists() {
        let backup = dest.with_extension(if cfg!(windows) { "old.exe" } else { "old" });
        let _ = std::fs::remove_file(&backup);
        let _ = std::fs::rename(dest, &backup);
    }
    std::fs::copy(source, dest)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o755));
    }
    Ok(())
}

fn extract_archive(archive: &Path, dest: &Path) -> Result<()> {
    let name = archive
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    let status = Command::new("tar")
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(dest)
        .status();

    if let Ok(s) = status {
        if s.success() {
            return Ok(());
        }
    }

    if cfg!(windows) && name.ends_with(".zip") {
        let ps = Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &format!(
                    "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
                    archive.display(),
                    dest.display()
                ),
            ])
            .status()?;
        if ps.success() {
            return Ok(());
        }
    }

    Err(Error::msg(format!("failed to extract archive {}", archive.display())))
}

fn find_file_recursive(dir: &Path, file_name: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if path.file_name().and_then(|n| n.to_str()) == Some(file_name) {
                return Some(path);
            }
        } else if path.is_dir() {
            if let Some(found) = find_file_recursive(&path, file_name) {
                return Some(found);
            }
        }
    }
    None
}

pub fn parse_sha256sums(content: &str, asset_name: &str) -> Option<String> {
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        let file = parts.next()?.trim_start_matches('*').trim_start_matches("./");
        let basename = Path::new(file)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(file);
        if basename == asset_name && hash.len() == 64 {
            return Some(hash.to_ascii_lowercase());
        }
    }
    None
}

pub fn semver_gt(candidate: &str, current: &str) -> bool {
    let parse = |s: &str| -> (u64, u64, u64, String) {
        let s = s.trim().trim_start_matches('v');
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) => (c, p.to_string()),
            None => (s, String::new()),
        };
        let mut nums = core.split('.').map(|n| n.parse::<u64>().unwrap_or(0));
        (
            nums.next().unwrap_or(0),
            nums.next().unwrap_or(0),
            nums.next().unwrap_or(0),
            pre,
        )
    };
    let (ma1, mi1, pa1, pre1) = parse(candidate);
    let (ma2, mi2, pa2, pre2) = parse(current);
    match (ma1, mi1, pa1).cmp(&(ma2, mi2, pa2)) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => {
            if pre1.is_empty() && !pre2.is_empty() {
                true
            } else if !pre1.is_empty() && pre2.is_empty() {
                false
            } else {
                pre1 > pre2
            }
        }
    }
}

/// Pure-Rust FIPS 180-4 / RFC 6234 SHA-256 digest formatted as lowercase hex.
pub fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let bit_len = (data.len() as u64) * 8;
    let mut padded = Vec::with_capacity(data.len() + 72);
    padded.extend_from_slice(data);
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0x00);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in chunk.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut hh = h[7];
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = String::with_capacity(64);
    for word in h {
        use std::fmt::Write;
        let _ = write!(&mut out, "{word:08x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_semantic_versions_accurately() {
        assert!(semver_gt("0.2.0", "0.1.0"));
        assert!(semver_gt("v0.1.1", "0.1.0"));
        assert!(!semver_gt("0.1.0", "0.1.0"));
        assert!(!semver_gt("0.1.0-beta.1", "0.1.0"));
        assert!(semver_gt("0.1.0", "0.1.0-beta.1"));
    }

    #[test]
    fn computes_nist_sha256_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn parses_sha256sums_lines() {
        let text = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  dist/serika-code-x86_64-pc-windows-msvc.zip\n";
        let hash = parse_sha256sums(text, "serika-code-x86_64-pc-windows-msvc.zip").unwrap();
        assert_eq!(
            hash,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn replaces_existing_binary_with_backup() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("new.bin");
        let dst = dir.path().join("serika.exe");
        std::fs::write(&dst, b"old-binary").unwrap();
        std::fs::write(&src, b"new-binary").unwrap();
        replace_binary(&src, &dst).unwrap();
        assert_eq!(std::fs::read(&dst).unwrap(), b"new-binary");
    }
}

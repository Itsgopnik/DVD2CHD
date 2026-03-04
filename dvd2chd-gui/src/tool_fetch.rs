use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use reqwest::blocking::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

pub const DEFAULT_MANIFEST_URL: &str = "https://example.com/dvd2chd/tools/manifest.json";

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub updated: Option<String>,
    pub tools: Vec<RemoteTool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RemoteTool {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    #[allow(unused)]
    pub homepage: Option<String>,
    pub platforms: Vec<PlatformArtifact>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlatformArtifact {
    pub os: String,
    pub arch: Option<String>,
    pub url: String,
    /// Optional checksum for integrity verification. Format: `"sha256:<hex>"`.
    pub checksum: Option<String>,
    pub binary_name: Option<String>,
}

pub fn fetch_manifest(url: &str) -> Result<Manifest> {
    let client = Client::builder().user_agent("dvd2chd-gui").build()?;
    let resp = client.get(url).send()?;
    if !resp.status().is_success() {
        return Err(anyhow!("Manifest download failed: {}", resp.status()));
    }
    let manifest = resp.json::<Manifest>()?;
    Ok(manifest)
}

pub fn install_tool(tool: &RemoteTool, dest_dir: &Path) -> Result<PathBuf> {
    let platform = current_platform();
    let artifact = tool
        .platforms
        .iter()
        .find(|p| matches_platform(p, &platform));
    let artifact =
        artifact.ok_or_else(|| anyhow!("No package available for the current platform"))?;

    fs::create_dir_all(dest_dir)
        .with_context(|| format!("Cannot create directory: {}", dest_dir.display()))?;

    let client = Client::builder().user_agent("dvd2chd-gui").build()?;
    let mut resp = client.get(&artifact.url).send()?;
    if !resp.status().is_success() {
        return Err(anyhow!("Download failed: {}", resp.status()));
    }

    let file_name = artifact
        .binary_name
        .clone()
        .unwrap_or_else(|| tool.name.clone());
    let dest_path = dest_dir.join(&file_name);

    if dest_path.exists() {
        fs::remove_file(&dest_path)
            .with_context(|| format!("Cannot remove existing file: {}", dest_path.display()))?;
    }

    let mut tmp = NamedTempFile::new_in(dest_dir)?;
    io::copy(&mut resp, &mut tmp)?;
    tmp.flush()?;

    if let Some(expected) = &artifact.checksum {
        let actual = sha256_file(tmp.path())?;
        let expected_hex = expected
            .strip_prefix("sha256:")
            .ok_or_else(|| anyhow!("Unsupported checksum format (expected 'sha256:<hex>')"))?;
        if actual != expected_hex {
            return Err(anyhow!(
                "Checksum mismatch for {}: expected {}, got {}",
                tool.name,
                expected_hex,
                actual
            ));
        }
    }

    tmp.persist(&dest_path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&dest_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&dest_path, perms)?;
    }

    Ok(dest_path)
}

fn matches_platform(artifact: &PlatformArtifact, current: &(String, String)) -> bool {
    let arch = artifact.arch.as_deref().unwrap_or("*");
    (artifact.os == current.0 || artifact.os == "*") && (arch == current.1 || arch == "*")
}

fn current_platform() -> (String, String) {
    (
        std::env::consts::OS.to_string(),
        std::env::consts::ARCH.to_string(),
    )
}

/// Downloads chdman from a manifest URL into `dest_dir`.
///
/// The manifest must contain a tool entry with `"name": "chdman"` and a
/// platform artifact matching the current OS/arch. Returns the path to the
/// installed binary.
///
/// # Manifest format (manifest.json hosted on your GitHub Releases):
/// ```json
/// {
///   "updated": "2024-01-01",
///   "tools": [{
///     "name": "chdman",
///     "version": "0.272",
///     "platforms": [
///       { "os": "linux",   "arch": "x86_64", "url": "…/chdman-linux-x86_64",  "binary_name": "chdman"     },
///       { "os": "windows", "arch": "x86_64", "url": "…/chdman.exe",            "binary_name": "chdman.exe" }
///     ]
///   }]
/// }
/// ```
pub fn download_chdman(manifest_url: &str, dest_dir: &Path) -> Result<PathBuf> {
    let manifest = fetch_manifest(manifest_url)?;
    let tool = manifest
        .tools
        .into_iter()
        .find(|t| t.name == "chdman")
        .ok_or_else(|| anyhow!("chdman not found in manifest"))?;
    install_tool(&tool, dest_dir)
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file =
        fs::File::open(path).with_context(|| format!("Cannot open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

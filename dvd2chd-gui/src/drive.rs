use std::path::PathBuf;
use std::process::Command;

/// Hide console window on Windows when spawning child processes.
#[cfg(windows)]
fn hide_window(cmd: &mut Command) -> &mut Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW)
}
#[cfg(not(windows))]
fn hide_window(cmd: &mut Command) -> &mut Command {
    cmd
}

#[derive(Debug, Clone)]
pub struct Drive {
    pub path: PathBuf,
    pub model: Option<String>,
    pub vendor: Option<String>,
}

impl Drive {
    pub fn short_label(&self) -> Option<String> {
        match (&self.vendor, &self.model) {
            (Some(v), Some(m)) if !v.is_empty() && !m.is_empty() => Some(format!("{v} {m}")),
            (Some(v), _) if !v.is_empty() => Some(v.clone()),
            (_, Some(m)) if !m.is_empty() => Some(m.clone()),
            _ => None,
        }
    }
}

pub fn probe_drives() -> Vec<Drive> {
    #[cfg(target_os = "linux")]
    {
        return probe_linux();
    }
    #[cfg(target_os = "windows")]
    {
        return probe_windows();
    }
    #[cfg(target_os = "macos")]
    {
        return probe_macos();
    }
    #[allow(unreachable_code)]
    Vec::new()
}

#[cfg(target_os = "linux")]
fn probe_linux() -> Vec<Drive> {
    use serde_json::Value;
    let mut out = Vec::<Drive>::new();

    if let Ok(res) = Command::new("lsblk")
        .args(["-J", "-o", "KNAME,TYPE,MODEL,VENDOR,PATH"])
        .output()
    {
        if res.status.success() {
            if let Ok(v) = serde_json::from_slice::<Value>(&res.stdout) {
                if let Some(arr) = v.get("blockdevices").and_then(|x| x.as_array()) {
                    for d in arr {
                        let ty = d.get("type").and_then(|x| x.as_str());
                        if ty == Some("rom") {
                            let path = d
                                .get("path")
                                .and_then(|x| x.as_str())
                                .map(PathBuf::from)
                                .unwrap_or_else(|| {
                                    let k =
                                        d.get("kname").and_then(|x| x.as_str()).unwrap_or("sr0");
                                    PathBuf::from(format!("/dev/{k}"))
                                });
                            let vendor = d
                                .get("vendor")
                                .and_then(|x| x.as_str())
                                .map(|s| s.trim().to_string());
                            let model = d
                                .get("model")
                                .and_then(|x| x.as_str())
                                .map(|s| s.trim().to_string());
                            out.push(Drive {
                                path,
                                model,
                                vendor,
                            });
                        }
                    }
                }
            }
        }
    }

    // Fallback-Symlinks
    for link in ["/dev/cdrom", "/dev/dvd"] {
        let p = PathBuf::from(link);
        if p.exists() && !out.iter().any(|d| d.path == p) {
            out.push(Drive {
                path: p,
                model: None,
                vendor: None,
            });
        }
    }

    out
}

#[cfg(target_os = "windows")]
fn probe_windows() -> Vec<Drive> {
    // Native Win32: enumerate drive letters, filter for DRIVE_CDROM.
    // No subprocess (powershell/wmic) needed — instant and silent.

    #[link(name = "kernel32")]
    extern "system" {
        fn GetLogicalDriveStringsW(len: u32, buf: *mut u16) -> u32;
        fn GetDriveTypeW(root: *const u16) -> u32;
    }
    const DRIVE_CDROM: u32 = 5;

    let mut buf = [0u16; 256];
    let len = unsafe { GetLogicalDriveStringsW(buf.len() as u32, buf.as_mut_ptr()) };
    if len == 0 {
        return Vec::new();
    }

    let mut out = Vec::new();
    // Buffer contains null-separated root paths: "A:\\\0C:\\\0D:\\\0\0"
    for root in buf[..len as usize].split(|&c| c == 0) {
        if root.is_empty() {
            continue;
        }
        let drive_type = unsafe { GetDriveTypeW(root.as_ptr()) };
        if drive_type == DRIVE_CDROM {
            let root_str = String::from_utf16_lossy(root);
            // "D:\\" → "D:"
            let letter = root_str.trim_end_matches('\\');
            out.push(Drive {
                path: PathBuf::from(letter),
                model: wmi_drive_name(letter),
                vendor: None,
            });
        }
    }

    out
}

/// Query WMI for the friendly name of a CD/DVD drive (e.g. "HL-DT-ST DVDRAM").
/// Uses a hidden wmic subprocess — returns None silently on failure.
#[cfg(target_os = "windows")]
fn wmi_drive_name(drive_letter: &str) -> Option<String> {
    let mut cmd = Command::new("wmic");
    cmd.args(["cdrom", "get", "Drive,Name", "/format:csv"]);
    hide_window(&mut cmd);
    let res = cmd.output().ok()?;
    if !res.status.success() {
        return None;
    }
    let txt = String::from_utf8_lossy(&res.stdout);
    for line in txt.lines().skip(1) {
        let parts: Vec<_> = line.split(',').collect(); // Node,Drive,Name
        if parts.len() >= 3 {
            let d = parts[1].trim();
            let name = parts[2].trim();
            if d.eq_ignore_ascii_case(drive_letter) && !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn probe_macos() -> Vec<Drive> {
    // macOS drive detection is not yet implemented.
    // Optical drives can be enumerated via `diskutil list -plist`, but parsing
    // and filtering for CD/DVD devices requires additional work.
    // For now this returns an empty list — users must enter the device path manually
    // (e.g. /dev/disk2).
    Vec::new()
}

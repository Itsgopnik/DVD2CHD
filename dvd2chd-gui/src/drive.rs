use std::path::PathBuf;
use std::process::Command;

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
    use serde_json::Value;
    let mut out = Vec::<Drive>::new();

    let ps =
        r#"Get-CimInstance Win32_CDROMDrive | Select-Object Drive,Name | ConvertTo-Json -Depth 2"#;
    if let Ok(res) = Command::new("powershell")
        .args(["-NoProfile", "-Command", ps])
        .output()
    {
        if res.status.success() {
            if let Ok(val) = serde_json::from_slice::<Value>(&res.stdout) {
                match val {
                    Value::Array(arr) => {
                        for o in arr {
                            let drive = o.get("Drive").and_then(|x| x.as_str());
                            let name = o
                                .get("Name")
                                .and_then(|x| x.as_str())
                                .map(|s| s.to_string());
                            if let Some(d) = drive {
                                out.push(Drive {
                                    path: PathBuf::from(d),
                                    model: name,
                                    vendor: None,
                                });
                            }
                        }
                        if !out.is_empty() {
                            return out;
                        }
                    }
                    Value::Object(o) => {
                        let drive = o.get("Drive").and_then(|x| x.as_str());
                        let name = o
                            .get("Name")
                            .and_then(|x| x.as_str())
                            .map(|s| s.to_string());
                        if let Some(d) = drive {
                            out.push(Drive {
                                path: PathBuf::from(d),
                                model: name,
                                vendor: None,
                            });
                            return out;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // Fallback: WMIC CSV
    if let Ok(res) = Command::new("wmic")
        .args(["cdrom", "get", "Drive,Name", "/format:csv"])
        .output()
    {
        if res.status.success() {
            let txt = String::from_utf8_lossy(&res.stdout);
            for line in txt.lines().skip(1) {
                let parts: Vec<_> = line.split(',').collect(); // Node,Drive,Name
                if parts.len() >= 3 {
                    let drive = parts[1].trim();
                    let name = parts[2].trim();
                    if !drive.is_empty() {
                        out.push(Drive {
                            path: PathBuf::from(drive),
                            model: Some(name.to_string()),
                            vendor: None,
                        });
                    }
                }
            }
        }
    }

    out
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

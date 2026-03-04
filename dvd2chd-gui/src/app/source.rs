use super::App;
use crate::drive::Drive;
use rust_i18n::t;
use std::path::{Path, PathBuf};
use std::process::Command;

impl App {
    pub(super) fn set_source_file(&mut self, path: PathBuf, log: Option<String>) {
        self.s.source_file = Some(path.clone());
        self.s.device_path = None;
        self.set_out_dir_if_none(&path);
        if let Some(msg) = log {
            self.log_line(&msg);
        }
    }

    pub(super) fn set_device_path(&mut self, path: PathBuf, log: Option<String>) {
        self.s.device_path = Some(path);
        self.s.source_file = None;
        if let Some(msg) = log {
            self.log_line(&msg);
        }
    }

    pub(super) fn set_out_dir_if_none(&mut self, path: &Path) {
        if self.s.out_dir.is_none() {
            if let Some(parent) = path.parent() {
                self.s.out_dir = Some(parent.to_path_buf());
            }
        }
    }

    pub(super) fn read_volume_label_from_source(&self, src: &Path) -> Option<String> {
        let out = Command::new("isoinfo")
            .args(["-d", "-i"])
            .arg(src)
            .stdout(std::process::Stdio::piped())
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let txt = String::from_utf8_lossy(&out.stdout);
        for line in txt.lines() {
            if let Some(v) = line.strip_prefix("Volume id:") {
                let label = v.trim().to_string();
                if !label.is_empty() {
                    return Some(label);
                }
            }
        }
        None
    }

    pub(super) fn ps_id_from_source(&self, src: &Path) -> Option<String> {
        let out = Command::new("isoinfo")
            .args(["-i"])
            .arg(src)
            .args(["-x", "/SYSTEM.CNF;1"])
            .stdout(std::process::Stdio::piped())
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let txt = String::from_utf8_lossy(&out.stdout);
        for line in txt.lines() {
            let line = line.trim();
            let lower = line.to_ascii_lowercase();
            if !lower.starts_with("boot") {
                continue;
            }
            if let Some(pos) = lower.find("cdrom0:") {
                let rest = &line[pos + "cdrom0:".len()..];
                let rest = rest.trim();
                let rest = rest.trim_start_matches('\\');
                let end = rest.find(";1").unwrap_or(rest.len());
                let id = rest[..end].trim();
                if !id.is_empty() {
                    return Some(id.replace('\\', "/"));
                }
            }
        }
        None
    }

    pub(super) fn drive_label(&self, drive: &Drive) -> String {
        drive
            .short_label()
            .unwrap_or_else(|| t!("drive.unknown").to_string())
    }

    pub(super) fn drive_display_label(&self, drive: &Drive) -> String {
        format!("{} — {}", drive.path.display(), self.drive_label(drive))
    }
}

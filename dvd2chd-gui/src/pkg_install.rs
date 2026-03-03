//! Linux system package manager auto-install for missing tools.
//!
//! Uses `pkexec` (polkit) for privilege escalation — shows the desktop
//! authentication dialog without requiring a terminal.

use std::process::{Command, Stdio};

/// Supported system package managers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageManager {
    Apt,    // Debian, Ubuntu, Mint …
    Pacman, // Arch, Manjaro …
    Dnf,    // Fedora, RHEL 8+ …
    Zypper, // openSUSE …
}

impl PackageManager {
    /// Detects the first available package manager on Linux.
    /// Returns `None` on non-Linux platforms or if neither pkexec nor a
    /// known package manager is found.
    pub fn detect() -> Option<Self> {
        #[cfg(target_os = "linux")]
        {
            // pkexec is required for privilege escalation in a GUI context.
            if !std::path::Path::new("/usr/bin/pkexec").exists() {
                return None;
            }
            let candidates: &[(&str, PackageManager)] = &[
                ("/usr/bin/apt", PackageManager::Apt),
                ("/usr/bin/pacman", PackageManager::Pacman),
                ("/usr/bin/dnf", PackageManager::Dnf),
                ("/usr/bin/zypper", PackageManager::Zypper),
            ];
            for (path, pm) in candidates {
                if std::path::Path::new(path).exists() {
                    return Some(*pm);
                }
            }
        }
        None
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Apt => "apt",
            Self::Pacman => "pacman",
            Self::Dnf => "dnf",
            Self::Zypper => "zypper",
        }
    }

    /// Maps a tool name to the distro-specific package that provides it.
    fn package_name(self, tool: &str) -> Option<&'static str> {
        match (self, tool) {
            // chdman
            (Self::Apt, "chdman") => Some("mame-tools"), // Ubuntu/Debian: lightweight subset of MAME
            (Self::Dnf, "chdman") => Some("mame"),
            (Self::Zypper, "chdman") => Some("mame"),
            // Pacman: chdman is AUR-only — cannot auto-install via pacman

            // cdrdao
            (Self::Apt, "cdrdao") => Some("cdrdao"),
            (Self::Pacman, "cdrdao") => Some("cdrdao"),
            (Self::Dnf, "cdrdao") => Some("cdrdao"),
            (Self::Zypper, "cdrdao") => Some("cdrdao"),

            // ddrescue
            (Self::Apt, "ddrescue") => Some("gddrescue"), // Debian/Ubuntu package name differs
            (Self::Pacman, "ddrescue") => Some("ddrescue"),
            (Self::Dnf, "ddrescue") => Some("ddrescue"),
            (Self::Zypper, "ddrescue") => Some("ddrescue"),

            _ => None,
        }
    }

    /// Installs the requested tools via `pkexec <pm> install`.
    /// This is a **blocking** call — run it from a background thread.
    ///
    /// Returns `Ok(installed_packages)` on success, or `Err(message)` on failure.
    pub fn install(self, tool_names: Vec<String>) -> Result<Vec<String>, String> {
        let packages: Vec<&'static str> = tool_names
            .iter()
            .filter_map(|t| self.package_name(t.as_str()))
            .collect();

        if packages.is_empty() {
            return Err(format!(
                "No installable package found for the requested tools via {}. \
                 Please install manually.",
                self.display_name()
            ));
        }

        let mut cmd = Command::new("pkexec");
        match self {
            Self::Apt => {
                cmd.arg("apt").arg("install").arg("-y");
            }
            Self::Pacman => {
                cmd.arg("pacman").arg("-S").arg("--noconfirm");
            }
            Self::Dnf => {
                cmd.arg("dnf").arg("install").arg("-y");
            }
            Self::Zypper => {
                cmd.arg("zypper").arg("install").arg("-y");
            }
        }
        for pkg in &packages {
            cmd.arg(pkg);
        }

        let status = cmd
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| format!("Failed to launch pkexec: {e}"))?;

        if status.success() {
            Ok(packages.iter().map(|s| s.to_string()).collect())
        } else {
            let code = status.code().unwrap_or(-1);
            // pkexec exit 126 = auth dismissed / 127 = pkexec not found
            if code == 126 || code == 127 {
                Err("Authentication cancelled or pkexec unavailable.".to_string())
            } else {
                Err(format!(
                    "{} exited with code {code}",
                    self.display_name()
                ))
            }
        }
    }
}

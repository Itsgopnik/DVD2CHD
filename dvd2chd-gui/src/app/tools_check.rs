use super::App;
use crate::pkg_install::PackageManager;
use dvd2chd_core::tools::probe_all;
use rust_i18n::t;
use std::{path::PathBuf, sync::mpsc};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(super) enum ToolRequirement {
    Chdman,
    Ddrescue,
    Cdrdao,
}

impl ToolRequirement {
    pub(super) fn reason_key(self) -> &'static str {
        match self {
            ToolRequirement::Chdman => "tool.chdman_missing",
            ToolRequirement::Ddrescue => "tool.ddrescue_missing",
            ToolRequirement::Cdrdao => "tool.cdrdao_missing",
        }
    }
}

impl App {
    pub(super) fn reprobe_tools(&mut self) {
        self.tools = probe_all(
            self.s.chdman_path.as_deref(),
            self.s.ddrescue_path.as_deref(),
            self.s.cdrdao_path.as_deref(),
        );
    }

    pub(super) fn missing_tools_for(&self, intent: super::job::JobIntent) -> Vec<ToolRequirement> {
        let mut missing = Vec::new();
        if self.tools.chdman.is_none() {
            missing.push(ToolRequirement::Chdman);
        }
        // On Windows the native Win32 ripper is used — cdrdao/ddrescue not needed.
        #[cfg(not(windows))]
        if matches!(intent, super::job::JobIntent::Device) {
            if self.tools.cdrdao.is_none() {
                missing.push(ToolRequirement::Cdrdao);
            }
            if self.s.use_ddrescue && self.tools.ddrescue.is_none() {
                missing.push(ToolRequirement::Ddrescue);
            }
        }
        missing
    }

    pub(super) fn missing_tool_message(&self, req: ToolRequirement) -> String {
        t!(req.reason_key()).to_string()
    }

    pub(super) fn tool_warnings(&self) -> Vec<String> {
        if let Some(intent) = self.current_job_intent() {
            self.missing_tools_for(intent)
                .into_iter()
                .map(|req| self.missing_tool_message(req))
                .collect()
        } else {
            Vec::new()
        }
    }

    pub(super) fn tool_install_dir(&self) -> PathBuf {
        if let Some(mut dir) = dirs::data_local_dir() {
            dir.push("dvd2chd");
            dir.push("tools");
            dir
        } else {
            PathBuf::from("tools")
        }
    }

    /// Spawns a background thread that installs missing tools via `pkexec`.
    /// Progress is surfaced through `tool_install_running` + `tool_install_rx`.
    pub(super) fn start_tool_install(&mut self, pm: PackageManager) {
        if self.tool_install_running {
            return;
        }

        // Collect names of all currently missing tools.
        let mut to_install: Vec<String> = Vec::new();
        if self.tools.chdman.is_none() {
            to_install.push("chdman".to_string());
        }
        #[cfg(not(windows))]
        {
            if self.tools.cdrdao.is_none() {
                to_install.push("cdrdao".to_string());
            }
            if self.tools.ddrescue.is_none() {
                to_install.push("ddrescue".to_string());
            }
        }

        if to_install.is_empty() {
            return;
        }

        let pm_name = pm.display_name();
        self.log_line(&t!("log.installing_tools", pm = pm_name));

        let (tx, rx) = mpsc::channel();
        self.tool_install_rx = Some(rx);
        self.tool_install_running = true;

        std::thread::spawn(move || {
            let _ = tx.send(pm.install(to_install));
        });
    }

    /// Downloads the chdman binary from the configured manifest URL.
    /// The URL must be set in Settings (`tool_manifest_url`) before calling this.
    pub(super) fn start_chdman_download(&mut self) {
        if self.chdman_dl_running {
            return;
        }
        let url = self.s.tool_manifest_url.trim().to_string();
        if url.is_empty() {
            self.log_line(&t!("log.no_manifest_url"));
            return;
        }
        let dest_dir = self.tool_install_dir();
        self.log_line(&t!("log.downloading_chdman"));
        let (tx, rx) = mpsc::channel();
        self.chdman_dl_rx = Some(rx);
        self.chdman_dl_running = true;
        std::thread::spawn(move || {
            let result =
                crate::tool_fetch::download_chdman(&url, &dest_dir).map_err(|e| e.to_string());
            let _ = tx.send(result);
        });
    }
}

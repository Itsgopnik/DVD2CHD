use std::path::PathBuf;

use dvd2chd_core::Profile;
use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

fn default_language() -> Language {
    Language::German
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Settings {
    pub source_file: Option<PathBuf>,
    pub device_path: Option<PathBuf>,
    pub profile: Profile,
    #[serde(default = "default_language")]
    pub language: Language,
    pub out_dir: Option<PathBuf>,
    pub use_ddrescue: bool,
    pub ddrescue_scrape: bool,
    pub delete_image_after: bool,
    #[serde(default = "default_true")]
    pub prefer_id_rename: bool,
    pub rename_by_label: bool,
    pub cd_speed_x: Option<u32>,
    pub cd_buffers: Option<u32>,
    pub extra_chd_args: String,
    pub run_nice: bool,
    pub run_ionice: bool,
    pub compute_md5: bool,
    pub compute_sha1: bool,
    #[serde(default)]
    pub compute_sha256: bool,
    #[serde(default)]
    pub auto_eject: bool,
    #[serde(default)]
    pub notify_on_done: bool,
    pub chdman_path: Option<PathBuf>,
    pub ddrescue_path: Option<PathBuf>,
    pub cdrdao_path: Option<PathBuf>,
    pub force_createdvd: Option<bool>,
    #[serde(default = "default_true")]
    pub auto_clear_log: bool,
    /// URL pointing to a manifest.json on your own GitHub Releases.
    /// Format: https://github.com/USER/REPO/releases/download/TAG/manifest.json
    #[serde(default)]
    pub tool_manifest_url: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            source_file: None,
            device_path: None,
            profile: Profile::Auto,
            language: Language::German,
            out_dir: None,
            use_ddrescue: false,
            ddrescue_scrape: false,
            delete_image_after: false,
            prefer_id_rename: true,
            rename_by_label: false,
            cd_speed_x: None,
            cd_buffers: None,
            extra_chd_args: String::new(),
            run_nice: false,
            run_ionice: false,
            compute_md5: false,
            compute_sha1: false,
            compute_sha256: false,
            auto_eject: false,
            notify_on_done: false,
            chdman_path: None,
            ddrescue_path: None,
            cdrdao_path: None,
            force_createdvd: None,
            auto_clear_log: true,
            tool_manifest_url: String::new(),
        }
    }
}

impl Settings {
    pub fn snapshot_for_preset(&self) -> Self {
        let mut snap = self.clone();
        snap.source_file = None;
        snap.device_path = None;
        snap
    }
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Preset {
    pub name: String,
    pub settings: Settings,
}

#[derive(Serialize, Deserialize, Default)]
pub struct PresetStore {
    pub presets: Vec<Preset>,
}

#[derive(Serialize, Deserialize)]
pub struct UiPreferences {
    pub theme: Theme,
    pub auto_theme: bool,
    pub zoom: f32,
    pub reduce_motion: bool,
}

impl Default for UiPreferences {
    fn default() -> Self {
        Self {
            theme: Theme::Dark,
            auto_theme: true,
            zoom: 1.0,
            reduce_motion: false,
        }
    }
}

#[derive(Copy, Clone, Default, PartialEq, Serialize, Deserialize)]
pub enum Theme {
    Auto,
    #[default]
    Dark,
    Light,
    HighContrast,
}

#[derive(Copy, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    German,
    English,
}

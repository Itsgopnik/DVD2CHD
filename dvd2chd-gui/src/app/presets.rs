use super::state::{Settings, UiPreferences};
use super::App;
use rust_i18n::t;

impl App {
    pub(super) fn apply_preset_settings(&mut self, preset: &Settings) {
        let mut new_settings = preset.clone();
        new_settings.source_file = self.s.source_file.clone();
        new_settings.device_path = self.s.device_path.clone();
        self.s = new_settings;
        self.reprobe_tools();
    }

    pub(super) fn persist_presets(&mut self, success_msg: &str) {
        if let Err(e) = confy::store("dvd2chd_presets", None::<&str>, &self.presets) {
            self.log_line(&t!("log.presets_save_failed", err = e.to_string()));
        } else {
            self.log_line(success_msg);
            if !self.log_ends_with_newline() {
                self.append_log_text("\n");
            }
        }
    }

    pub(super) fn save_settings(&self) {
        let _ = confy::store("dvd2chd", None::<&str>, &self.s);
    }

    pub(super) fn save_ui_prefs(&mut self) {
        let prefs = UiPreferences {
            theme: self.manual_theme,
            auto_theme: self.auto_theme,
            zoom: self.zoom,
            reduce_motion: self.reduce_motion,
        };
        if let Err(e) = confy::store("dvd2chd", Some("ui_prefs"), prefs) {
            self.log_line(&t!("log.ui_prefs_save_failed", err = e.to_string()));
        }
    }
}

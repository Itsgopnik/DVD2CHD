use super::{palette_for_theme, App};
use egui::{Color32, RichText};
use rust_i18n::t;

impl App {
    pub(super) fn draw_about_window(&mut self, ctx: &egui::Context) {
        if !self.show_about {
            return;
        }
        let mut open = self.show_about;
        let title = t!("toolbar.about");
        egui::Window::new(title.as_ref())
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.heading("DVD2CHD");
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                ui.add_space(8.0);
                ui.hyperlink("https://github.com/itsgopnik/DVD2CHD");
                ui.add_space(8.0);
                ui.label(t!("about.license").as_ref());
            });
        self.show_about = open;
    }

    pub(super) fn draw_custom_name_prompt(&mut self, ctx: &egui::Context) {
        if !self.show_custom_name_prompt {
            return;
        }
        let mut open = self.show_custom_name_prompt;
        let mut close = false;
        let mut action: Option<Option<String>> = None;
        let title = t!("dialog.name_disc");
        egui::Window::new(title.as_ref())
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(t!("dialog.name_disc_hint").as_ref());
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(t!("dialog.name_label").as_ref());
                    ui.text_edit_singleline(&mut self.custom_name_input);
                });
                if let Some(err) = &self.custom_name_error {
                    ui.colored_label(Color32::from_rgb(239, 68, 68), err);
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button(t!("dialog.cancel").as_ref()).clicked() {
                        self.custom_name_error = None;
                        self.custom_name_input.clear();
                        close = true;
                    }
                    if ui.button(t!("dialog.start_unnamed").as_ref()).clicked() {
                        self.custom_name_error = None;
                        self.custom_name_input.clear();
                        close = true;
                        action = Some(None);
                    }
                    if ui.button(t!("dialog.start").as_ref()).clicked() {
                        let name = self.custom_name_input.trim();
                        if name.is_empty() {
                            self.custom_name_error = Some(t!("dialog.name_required").to_string());
                        } else {
                            self.custom_name_error = None;
                            let value = name.to_string();
                            self.custom_name_input.clear();
                            close = true;
                            action = Some(Some(value));
                        }
                    }
                });
            });
        if close {
            open = false;
        }
        self.show_custom_name_prompt = open;
        if let Some(custom_name) = action {
            self.start_job_internal(custom_name);
        }
    }

    pub(super) fn draw_stage_preview(&self, ui: &mut egui::Ui) {
        let palette = palette_for_theme(self.effective_theme());
        #[cfg(debug_assertions)]
        let debug_override_active = self.debug_animation_override.is_some();
        #[cfg(not(debug_assertions))]
        let debug_override_active = false;

        let show_preview = self.running
            || self
                .timeline
                .iter()
                .any(|stage| !matches!(stage.state, super::workflow::StageState::Pending))
            || debug_override_active;
        if !show_preview {
            return;
        }

        let Some(kind) = self.active_stage_kind() else {
            return;
        };
        let visuals = ui.visuals().clone();
        ui.add_space(10.0);
        ui.vertical_centered(|ui| {
            let size = egui::vec2(150.0, 150.0);
            let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
            let painter = ui.painter();
            let center = rect.center();
            let radius = size.x.min(size.y) * 0.45;
            self.animation.draw_stage_graphic(
                painter,
                kind,
                center,
                radius,
                palette.accent,
                &visuals,
            );
        });

        ui.add_space(12.0);
        if let Some(stage) = self.timeline.iter().find(|s| s.kind == kind) {
            let label = stage.label();
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(label)
                        .color(palette.accent)
                        .size(13.0)
                        .strong(),
                );
            });
        }
    }

    pub(super) fn ensure_icon_texture(&mut self, ctx: &egui::Context) {
        if self.icon_texture.is_some() {
            return;
        }
        let bytes = include_bytes!("../../assets/icon.png");
        if let Ok(img) = image::load_from_memory(bytes) {
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            let pixels = rgba.into_raw();
            let color = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &pixels);
            let tex = ctx.load_texture("app-icon-ui", color, egui::TextureOptions::LINEAR);
            self.icon_texture = Some(tex);
        }
    }

    pub(super) fn ensure_log_icon_texture(&mut self, ctx: &egui::Context) {
        if self.log_icon_texture.is_some() {
            return;
        }
        if let Ok(img) = image::load_from_memory(super::LOG_ICON_BYTES) {
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            let pixels = rgba.into_raw();
            let color = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &pixels);
            let tex = ctx.load_texture("app-log-icon", color, egui::TextureOptions::LINEAR);
            self.log_icon_texture = Some(tex);
        }
    }
}

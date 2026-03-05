use super::{breakpoint, my_big_shadow, my_small_shadow, palette_for_theme, App, Breakpoint};
use super::state::{Language, Theme};
#[cfg(debug_assertions)]
use super::workflow::JobStageKind;
use dark_light::{self, Mode as SystemMode};
use egui::{
    Color32, FontId, Frame, Margin, RichText, Rounding, Stroke, TopBottomPanel, Visuals,
};
use rust_i18n::t;

impl App {
    pub(super) fn draw_top_toolbar(&mut self, ctx: &egui::Context) {
        const H: f32 = 34.0;

        let compact = matches!(breakpoint(ctx), Breakpoint::Narrow | Breakpoint::Medium);
        let vis = ctx.style().visuals.clone();
        let start_status = self.start_status();
        let tool_warnings = self.tool_warnings();

        let palette = palette_for_theme(self.effective_theme());
        let top_frame = Frame::none()
            .fill(vis.panel_fill)
            .stroke(Stroke::new(1.0, palette.stroke))
            .inner_margin(Margin {
                left: 14.0,
                right: 14.0,
                top: 10.0,
                bottom: 10.0,
            });

        TopBottomPanel::top("top_toolbar")
            .frame(top_frame)
            .show(ctx, |ui| {
                let old_spacing = ui.style().spacing.clone();
                ui.style_mut().spacing.item_spacing = egui::vec2(12.0, 0.0);
                ui.style_mut().spacing.button_padding = egui::vec2(10.0, 6.0);

                ui.horizontal(|ui| {
                    // ── Logo (klickbar → About) ──
                    let logo_response = if let Some(icon) = &self.icon_texture {
                        let size = if compact { 22.0 } else { 26.0 };
                        ui.add(
                            egui::Image::new(icon)
                                .fit_to_exact_size(egui::vec2(size, size))
                                .sense(egui::Sense::click()),
                        )
                    } else {
                        ui.add(egui::Button::new(RichText::new("DVD2CHD").strong()).frame(true))
                    };
                    if logo_response
                        .on_hover_text(t!("toolbar.about").as_ref())
                        .clicked()
                    {
                        self.show_about = true;
                    }
                    ui.add_space(4.0);
                    ui.add_space(12.0);
                    ui.add(egui::Separator::default().vertical());
                    ui.add_space(12.0);

                    // ── Panel-Toggle ──
                    let panel_label = if compact {
                        "☰".to_string()
                    } else if self.left_open {
                        t!("toolbar.controls").to_string()
                    } else {
                        t!("toolbar.panel").to_string()
                    };
                    if ui
                        .add(
                            egui::Button::new(RichText::new(panel_label).size(18.0))
                                .min_size(egui::vec2(44.0, H))
                                .frame(false),
                        )
                        .on_hover_text(t!("toolbar.toggle_panel").as_ref())
                        .clicked()
                    {
                        self.left_open = !self.left_open;
                    }

                    // ── Start ──
                    let can_start = start_status.can_start;
                    let start_text = if compact {
                        "▶".to_string()
                    } else {
                        t!("toolbar.start").to_string()
                    };
                    let palette = palette_for_theme(self.effective_theme());
                    let start_button =
                        egui::Button::new(RichText::new(start_text).strong().size(18.0))
                            .min_size(egui::vec2(if compact { 54.0 } else { 120.0 }, H))
                            .fill(palette.accent.linear_multiply(0.15))
                            .stroke(Stroke::new(1.0, palette.accent.linear_multiply(0.5)));
                    if ui
                        .add_enabled(can_start, start_button)
                        .on_hover_text(t!("toolbar.start_shortcut").as_ref())
                        .clicked()
                    {
                        let _ = confy::store("dvd2chd", None::<&str>, &self.s);
                        self.start_job();
                    }

                    // ── Stop ──
                    let cancel_text = if compact {
                        "⏹".to_string()
                    } else {
                        t!("toolbar.stop").to_string()
                    };
                    if ui
                        .add_enabled(
                            self.running,
                            egui::Button::new(RichText::new(cancel_text).size(16.0))
                                .min_size(egui::vec2(if compact { 54.0 } else { 110.0 }, H))
                                .frame(false),
                        )
                        .on_hover_text(t!("toolbar.stop_hint").as_ref())
                        .clicked()
                    {
                        if let Some(c) = &self.cancel {
                            dvd2chd_core::core_wiring::request_cancel(c);
                        }
                        self.log_line(&t!("log.abort_requested"));
                    }

                    // ── Kompakte Warnung: nur Icon + Hover-Text ──
                    if !tool_warnings.is_empty() {
                        let warning = tool_warnings.join(" · ");
                        ui.colored_label(Color32::from_rgb(239, 68, 68), "⚠")
                            .on_hover_text(warning);
                        if let Some(pm) = self.detected_pkg_manager {
                            let btn_text = if self.tool_install_running {
                                t!("toolbar.installing")
                            } else {
                                t!("toolbar.install")
                            };
                            let hover = t!("toolbar.install_hint", pm = pm.display_name());
                            if ui
                                .add_enabled(
                                    !self.tool_install_running,
                                    egui::Button::new(btn_text.as_ref()).small(),
                                )
                                .on_hover_text(hover.as_ref())
                                .clicked()
                            {
                                self.start_tool_install(pm);
                            }
                        }
                    }

                    // ── Debug-Steuerung (nur Debug-Builds) ──
                    #[cfg(debug_assertions)]
                    {
                        ui.add_space(12.0);
                        ui.add(egui::Separator::default().vertical());
                        ui.add_space(12.0);

                        let current_label = if let Some(override_stage) =
                            self.debug_animation_override
                        {
                            format!("DBG: {} (override)", Self::debug_stage_name(override_stage))
                        } else if self.debug_animation_indicator.is_empty() {
                            "DBG: idle".to_string()
                        } else {
                            format!("DBG: {}", self.debug_animation_indicator)
                        };
                        ui.label(RichText::new(current_label).monospace());

                        let debug_buttons = [
                            ("Rip", JobStageKind::Input),
                            ("CHD", JobStageKind::Chd),
                            ("Verify", JobStageKind::Verify),
                            ("Hash", JobStageKind::Hash),
                        ];
                        for (label, kind) in debug_buttons {
                            if ui
                                .add(egui::Button::new(label).small())
                                .on_hover_text("Animation erzwingen")
                                .clicked()
                            {
                                self.debug_animation_override = Some(kind);
                                self.debug_apply_override();
                                ui.ctx().request_repaint();
                            }
                        }
                        if ui
                            .add(egui::Button::new("Auto").small())
                            .on_hover_text("Animation dem echten Fortschritt überlassen")
                            .clicked()
                        {
                            self.debug_animation_override = None;
                            self.debug_apply_override();
                            ui.ctx().request_repaint();
                        }
                    }

                    // ── Rechte Seite: Utilities + Progress ──
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // ⚙ Theme/Settings-Menü (ganz rechts)
                        egui::menu::menu_button(ui, "⚙", |ui| {
                            let mut auto = self.auto_theme;
                            if ui
                                .checkbox(&mut auto, t!("toolbar.theme_auto").as_ref())
                                .clicked()
                            {
                                self.auto_theme = auto;
                                if self.auto_theme {
                                    self.theme = Theme::Auto;
                                } else {
                                    self.theme = self.manual_theme;
                                }
                                self.save_ui_prefs();
                            }
                            ui.separator();
                            let theme_options = [
                                (Theme::Dark, t!("theme.dark")),
                                (Theme::Light, t!("theme.light")),
                                (Theme::HighContrast, t!("theme.high_contrast")),
                            ];
                            for (variant, label) in theme_options {
                                let selected = if self.auto_theme {
                                    self.manual_theme == variant
                                } else {
                                    self.theme == variant
                                };
                                if ui.selectable_label(selected, label.as_ref()).clicked() {
                                    self.auto_theme = false;
                                    self.manual_theme = variant;
                                    self.theme = variant;
                                    self.save_ui_prefs();
                                }
                            }
                            ui.separator();
                            let mut reduce_motion = self.reduce_motion;
                            if ui
                                .checkbox(&mut reduce_motion, t!("toolbar.reduce_motion").as_ref())
                                .clicked()
                            {
                                self.reduce_motion = reduce_motion;
                                self.animation.set_reduce_motion(reduce_motion);
                                self.save_ui_prefs();
                            }
                            ui.separator();
                            ui.label(t!("toolbar.language").as_ref());
                            if ui
                                .selectable_value(&mut self.s.language, Language::German, "Deutsch")
                                .clicked()
                            {
                                rust_i18n::set_locale("de");
                                self.save_settings();
                            }
                            if ui
                                .selectable_value(
                                    &mut self.s.language,
                                    Language::English,
                                    "English",
                                )
                                .clicked()
                            {
                                rust_i18n::set_locale("en");
                                self.save_settings();
                            }
                        });

                        // Trennlinie
                        ui.add(egui::Separator::default().vertical());

                        // Zoom: in right_to_left — ➕ zuerst (rechts), dann %, dann ➖ (links)
                        // Visuelles Ergebnis L→R: ➖ {%} ➕
                        if ui
                            .button("➕")
                            .on_hover_text(t!("toolbar.zoom_plus").as_ref())
                            .clicked()
                        {
                            self.zoom = (self.zoom + 0.1).min(2.0);
                            self.save_ui_prefs();
                        }
                        ui.monospace(format!("{:.0}%", (self.zoom * 100.0).round()));
                        if ui
                            .button("➖")
                            .on_hover_text(t!("toolbar.zoom_minus").as_ref())
                            .clicked()
                        {
                            self.zoom = (self.zoom - 0.1).max(0.75);
                            self.save_ui_prefs();
                        }

                        // Trennlinie
                        ui.add(egui::Separator::default().vertical());

                        // Log-Icon
                        let log_button_size = if compact { 24.0 } else { 28.0 };
                        let log_hover_text = if self.log_open {
                            t!("toolbar.log_hide")
                        } else {
                            t!("toolbar.log_show")
                        };
                        if let Some(tex) = &self.log_icon_texture {
                            let tint = if self.log_open {
                                ui.visuals().text_color()
                            } else {
                                ui.visuals().weak_text_color()
                            };
                            let response = ui
                                .add(
                                    egui::ImageButton::new(
                                        egui::Image::new(tex).fit_to_exact_size(egui::vec2(
                                            log_button_size,
                                            log_button_size,
                                        )),
                                    )
                                    .frame(false)
                                    .tint(tint),
                                )
                                .on_hover_text(log_hover_text.as_ref());
                            if response.clicked() {
                                self.log_open = !self.log_open;
                            }
                            if !self.log_open && !self.log.is_empty() {
                                let painter = ui.painter();
                                let dot_center =
                                    response.rect.right_top() - egui::vec2(6.0, -6.0);
                                painter.circle_filled(
                                    dot_center,
                                    4.0,
                                    Color32::from_rgb(167, 139, 250),
                                );
                            }
                        } else {
                            let log_label = if compact { "🗒" } else { "🗒 Log" };
                            if ui
                                .add(
                                    egui::Button::new(RichText::new(log_label).size(16.0))
                                        .min_size(egui::vec2(
                                            if compact { 44.0 } else { 80.0 },
                                            H,
                                        ))
                                        .frame(false),
                                )
                                .on_hover_text(log_hover_text.as_ref())
                                .clicked()
                            {
                                self.log_open = !self.log_open;
                            }
                        }

                    });
                });

                ui.style_mut().spacing = old_spacing;
            });
    }

    pub(super) fn apply_style(&self, ctx: &egui::Context) {
        let effective = self.effective_theme();
        let palette = palette_for_theme(effective);

        let mut v = match effective {
            Theme::Light => Visuals::light(),
            _ => Visuals::dark(),
        };

        // ── Soft Neutral styling ──────────────────────────────────────────────
        let r_window: f32 = 12.0;
        let r_widget: f32 = 8.0;

        v.panel_fill = palette.panel;
        v.extreme_bg_color = palette.extreme;
        v.faint_bg_color = palette.surface;
        v.code_bg_color = palette.extreme;

        // Base widget surfaces
        v.widgets.noninteractive.bg_fill   = palette.surface;
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, palette.stroke);
        v.widgets.noninteractive.rounding  = Rounding::same(r_widget);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, palette.stroke);

        v.widgets.inactive.bg_fill   = palette.surface;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, palette.stroke);
        v.widgets.inactive.rounding  = Rounding::same(r_widget);

        // Hovered: subtle background shift, clean border
        v.widgets.hovered.bg_fill   = palette.surface.linear_multiply(1.15);
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, palette.accent.linear_multiply(0.45));
        v.widgets.hovered.rounding  = Rounding::same(r_widget);
        v.widgets.hovered.fg_stroke = Stroke::new(1.0, palette.accent);

        // Active/pressed
        v.widgets.active.bg_fill   = palette.accent.linear_multiply(0.12);
        v.widgets.active.bg_stroke = Stroke::new(1.0, palette.accent.linear_multiply(0.6));
        v.widgets.active.rounding  = Rounding::same(r_widget);
        v.widgets.active.fg_stroke = Stroke::new(1.0, palette.accent);

        // Open (combo-box / menu open state)
        v.widgets.open.bg_fill   = palette.surface.linear_multiply(1.1);
        v.widgets.open.bg_stroke = Stroke::new(1.0, palette.stroke);
        v.widgets.open.rounding  = Rounding::same(r_widget);

        // Windows & menus
        v.window_fill   = palette.panel;
        v.window_stroke = Stroke::new(1.0, palette.stroke);
        v.window_rounding = Rounding::same(r_window);
        v.menu_rounding   = Rounding::same(r_window - 4.0);

        // Shadows — neutral, subtle
        v.window_shadow = my_big_shadow();
        v.popup_shadow  = my_small_shadow();

        // Selection
        v.selection.bg_fill = palette.accent.linear_multiply(0.15);
        v.selection.stroke  = Stroke::new(1.0, palette.accent);

        // Hyperlinks
        v.hyperlink_color = palette.accent;

        ctx.set_visuals(v);

        let mut s = (*ctx.style()).clone();
        s.spacing.item_spacing   = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(12.0, 7.0);
        s.spacing.window_margin  = egui::Margin::same(16.0);
        s.text_styles
            .insert(egui::TextStyle::Heading, FontId::proportional(18.0));
        s.text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(14.0));
        s.visuals.selection.bg_fill = palette.accent.linear_multiply(0.15);
        s.visuals.selection.stroke  = Stroke::new(1.0, palette.accent);
        ctx.set_style(s);

        ctx.set_pixels_per_point(self.zoom.clamp(0.75, 2.0));
    }

    pub(super) fn effective_theme(&self) -> Theme {
        if self.auto_theme {
            match dark_light::detect() {
                Ok(SystemMode::Light) => Theme::Light,
                Ok(SystemMode::Dark) => Theme::Dark,
                _ => self.manual_theme,
            }
        } else {
            match self.theme {
                Theme::Auto => self.manual_theme,
                other => other,
            }
        }
    }

    pub(super) fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let mods = ctx.input(|i| i.modifiers);

        // Datei öffnen: Ctrl/Command + O
        if ctx.input(|i| i.key_pressed(egui::Key::O)) && mods.command {
            if let Some(p) = rfd::FileDialog::new()
                .add_filter("ISO/CUE", &["iso", "cue"])
                .pick_file()
            {
                self.set_source_file(p, None);
            }
        }
        // Gerät setzen: Ctrl/Command + G
        if ctx.input(|i| i.key_pressed(egui::Key::G)) && mods.command {
            if let Some(p) = rfd::FileDialog::new().pick_folder() {
                self.set_device_path(p, None);
            }
        }
        // Start: Ctrl/Command + S
        if ctx.input(|i| i.key_pressed(egui::Key::S)) && mods.command && self.can_start_job() {
            let _ = confy::store("dvd2chd", None::<&str>, &self.s);
            self.start_job();
        }
    }
}

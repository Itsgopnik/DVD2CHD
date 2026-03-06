use super::{card, open_folder, App, Breakpoint, breakpoint};
use super::state::Preset;
use dvd2chd_core::Profile;
use egui::{Color32, RichText, Rounding, Stroke};
use rust_i18n::t;
use std::mem::discriminant;

impl App {
    pub(super) fn draw_left_column(&mut self, ui: &mut egui::Ui) {
        // --- Quelle ---
        card(ui, t!("panel.source").as_ref(), |ui| {
            ui.horizontal(|ui| {
                if ui
                    .button(t!("source.choose_file").as_ref())
                    .on_hover_text("Ctrl+O")
                    .clicked()
                {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("ISO/CUE", &["iso", "cue"])
                        .pick_file()
                    {
                        self.set_source_file(p, None);
                    }
                }
                let file_label = self
                    .s
                    .source_file
                    .as_ref()
                    .and_then(|f| f.file_name())
                    .and_then(|s| s.to_str())
                    .unwrap_or("—")
                    .to_owned();
                ui.label(file_label.as_str());
            });

            ui.horizontal(|ui| {
                if ui.button(t!("source.set_device").as_ref())
                    .on_hover_text("Ctrl+G")
                    .clicked() {
                    if let Some(p) = rfd::FileDialog::new().pick_folder() {
                        self.set_device_path(p, None);
                    }
                }
                let dev_label = self
                    .s
                    .device_path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "—".into());
                ui.label(dev_label.as_str());
            });

            ui.horizontal(|ui| {
                let can_eject = self.s.device_path.is_some() && !self.running;
                if ui
                    .add_enabled(can_eject, egui::Button::new(t!("source.eject").as_ref()))
                    .on_hover_text(t!("tooltip.eject").as_ref())
                    .clicked()
                {
                    self.do_manual_eject();
                }
                if ui
                    .button(t!("source.detect_drive").as_ref())
                    .on_hover_text(t!("tooltip.detect_drive").as_ref())
                    .clicked()
                {
                    self.detected_drives = crate::drive::probe_drives();
                    match self.detected_drives.len() {
                        0 => self.log_line(&t!("log.no_drive")),
                        1 => {
                            let drive = self.detected_drives[0].clone();
                            self.set_device_path(drive.path.clone(), None);
                            let label = self.drive_label(&drive);
                            let path_display = drive.path.display().to_string();
                            self.log_line(&t!("log.drive_detected", path = path_display, label = label));
                        }
                        _ => {
                            self.selected_drive = Some(0);
                            self.show_drive_picker = true;
                        }
                    }
                }
            });

            ui.horizontal(|ui| {
                ui.label(t!("source.profile_label").as_ref());
                let profile_text = format!("{:?}", self.s.profile);
                egui::ComboBox::from_id_source("profile")
                    .selected_text(profile_text)
                    .show_ui(ui, |ui| {
                        for &(p, label) in &[
                            (Profile::Auto, "Auto"),
                            (Profile::PS1, "PS1"),
                            (Profile::PS2, "PS2"),
                            (Profile::GenericCd, "Generic CD"),
                            (Profile::PC, "PC"),
                        ] {
                            let sel = discriminant(&self.s.profile) == discriminant(&p);
                            if ui.selectable_label(sel, label).clicked() {
                                self.s.profile = p;
                                ui.close_menu();
                            }
                        }
                    });
            });
        });

        ui.add_space(8.0);

        // --- Ziel ---
        card(ui, t!("panel.target").as_ref(), |ui| {
            ui.horizontal(|ui| {
                if ui
                    .button(t!("target.set_folder").as_ref())
                    .on_hover_text(t!("tooltip.set_output").as_ref())
                    .clicked()
                {
                    if let Some(p) = rfd::FileDialog::new().pick_folder() {
                        self.s.out_dir = Some(p);
                    }
                }
                let out_label = self
                    .s
                    .out_dir
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "—".into());
                ui.label(out_label.as_str());
                if let Some(dir) = &self.s.out_dir {
                    if ui.button(t!("target.open_folder").as_ref())
                        .on_hover_text(t!("tooltip.open_output").as_ref())
                        .clicked() {
                        let _ = open_folder(dir);
                    }
                }
            });
        });

        ui.add_space(8.0);

        // --- Optionen ---
        egui::CollapsingHeader::new(t!("panel.options").as_ref())
            .default_open(false)
            .show(ui, |ui| {
                card(ui, t!("panel.ripping").as_ref(), |ui| {
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.s.use_ddrescue, "ddrescue (DVD)");
                        let scrape_text = t!("ripping.rescue_pass");
                        ui.checkbox(&mut self.s.ddrescue_scrape, scrape_text.as_ref());
                    });

                    ui.horizontal(|ui| {
                        ui.label(t!("ripping.cd_speed").as_ref());
                        let cd_speed_text = self
                            .s
                            .cd_speed_x
                            .map(|x| format!("{x}×"))
                            .unwrap_or_else(|| "Auto/Max".into());
                        egui::ComboBox::from_id_source("cdsp")
                            .selected_text(cd_speed_text)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.s.cd_speed_x, None, "Auto/Max");
                                for x in [8, 16, 24, 32, 40, 48] {
                                    ui.selectable_value(
                                        &mut self.s.cd_speed_x,
                                        Some(x),
                                        format!("{x}×"),
                                    );
                                }
                            });

                        ui.label(t!("ripping.buffers").as_ref());
                        let cd_buf_text = self
                            .s
                            .cd_buffers
                            .map(|b| b.to_string())
                            .unwrap_or_else(|| "Auto".into());
                        egui::ComboBox::from_id_source("cdbuf")
                            .selected_text(cd_buf_text)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.s.cd_buffers, None, "Auto");
                                for b in [64, 96, 128, 192, 256] {
                                    ui.selectable_value(
                                        &mut self.s.cd_buffers,
                                        Some(b),
                                        format!("{b}"),
                                    );
                                }
                            });
                    });
                });

                ui.add_space(6.0);

                card(ui, t!("panel.chd_hashes").as_ref(), |ui| {
                    ui.horizontal(|ui| {
                        let delete_after_text = t!("chd.delete_after");
                        ui.checkbox(&mut self.s.delete_image_after, delete_after_text.as_ref())
                            .on_hover_text(t!("tooltip.delete_after").as_ref());
                        ui.checkbox(&mut self.s.auto_eject, t!("chd.auto_eject").as_ref())
                            .on_hover_text(t!("tooltip.auto_eject").as_ref());
                    });
                    ui.horizontal(|ui| {
                        ui.label("Hashes:");
                        ui.checkbox(&mut self.s.compute_md5, "MD5");
                        ui.checkbox(&mut self.s.compute_sha1, "SHA1");
                        ui.checkbox(&mut self.s.compute_sha256, "SHA-256");
                    });
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.s.notify_on_done, t!("chd.notify_on_done").as_ref())
                            .on_hover_text(t!("tooltip.notify").as_ref());
                    });
                    ui.horizontal(|ui| {
                        let rename_id_text = t!("chd.rename_by_id");
                        let rename_label_text = t!("chd.rename_by_label");
                        ui.checkbox(&mut self.s.prefer_id_rename, rename_id_text.as_ref());
                        ui.checkbox(&mut self.s.rename_by_label, rename_label_text.as_ref());
                    });
                    ui.horizontal(|ui| {
                        ui.label(t!("chd.extra_args").as_ref());
                        ui.text_edit_singleline(&mut self.s.extra_chd_args);
                    });
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.s.run_nice, "nice");
                        ui.checkbox(&mut self.s.run_ionice, "ionice");
                    });
                });

                ui.add_space(6.0);

                card(ui, t!("panel.tools").as_ref(), |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("chdman…").clicked() {
                            if let Some(p) = rfd::FileDialog::new().pick_file() {
                                self.s.chdman_path = Some(p);
                                self.reprobe_tools();
                                self.save_settings();
                            }
                        }
                        let lbl = self
                            .s
                            .chdman_path
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|| "PATH".into());
                        ui.label(lbl.as_str());
                    });
                    ui.horizontal(|ui| {
                        if ui.button("ddrescue…").clicked() {
                            if let Some(p) = rfd::FileDialog::new().pick_file() {
                                self.s.ddrescue_path = Some(p);
                                self.reprobe_tools();
                                self.save_settings();
                            }
                        }
                        let lbl = self
                            .s
                            .ddrescue_path
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|| "PATH".into());
                        ui.label(lbl.as_str());
                    });
                    ui.horizontal(|ui| {
                        if ui.button("cdrdao…").clicked() {
                            if let Some(p) = rfd::FileDialog::new().pick_file() {
                                self.s.cdrdao_path = Some(p);
                                self.reprobe_tools();
                                self.save_settings();
                            }
                        }
                        let lbl = self
                            .s
                            .cdrdao_path
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|| "PATH".into());
                        ui.label(lbl.as_str());
                    });

                    ui.add_space(6.0);
                    if ui
                        .button(t!("tools.refresh").as_ref())
                        .clicked()
                    {
                        self.reprobe_tools();
                    }

                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui
                            .button(t!("tools.open_folder").as_ref())
                            .clicked()
                        {
                            let dir = self.tool_install_dir();
                            if let Err(e) = open_folder(&dir) {
                                self.log_line(&t!("log.cannot_open_folder", err = e.to_string()));
                            }
                        }
                    });

                    ui.separator();

                    // chdman binary download via self-hosted GitHub manifest
                    ui.label(
                        egui::RichText::new(t!("tools.chdman_download").as_ref())
                            .small()
                            .weak(),
                    );
                    let hint = "https://github.com/USER/REPO/releases/download/TAG/manifest.json";
                    let url_edit = egui::TextEdit::singleline(&mut self.s.tool_manifest_url)
                        .hint_text(hint)
                        .desired_width(f32::INFINITY);
                    if ui.add(url_edit).changed() {
                        self.save_settings();
                    }
                    ui.horizontal(|ui| {
                        let can_dl = !self.s.tool_manifest_url.trim().is_empty()
                            && self.tools.chdman.is_none()
                            && !self.chdman_dl_running;
                        let btn_text = if self.chdman_dl_running {
                            t!("tools.chdman_loading")
                        } else {
                            std::borrow::Cow::Borrowed("⬇ chdman")
                        };
                        if ui
                            .add_enabled(can_dl, egui::Button::new(btn_text.as_ref()))
                            .on_hover_text(t!("tools.chdman_download_hint").as_ref())
                            .clicked()
                        {
                            self.start_chdman_download();
                        }
                        if self.tools.chdman.is_some() {
                            ui.colored_label(Color32::from_rgb(34, 197, 94), "✔ chdman OK");
                        }
                    });
                });

                ui.add_space(6.0);

                card(ui, t!("panel.file_mode").as_ref(), |ui| {
                    let selected_text = self
                        .s
                        .force_createdvd
                        .map(|flag| if flag { "createdvd" } else { "createcd" })
                        .unwrap_or("Auto")
                        .to_string();
                    egui::ComboBox::from_id_source("force")
                        .selected_text(selected_text)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.s.force_createdvd, None, "Auto");
                            ui.selectable_value(
                                &mut self.s.force_createdvd,
                                Some(true),
                                "createdvd",
                            );
                            ui.selectable_value(
                                &mut self.s.force_createdvd,
                                Some(false),
                                "createcd",
                            );
                        });
                });
            });
    }

    pub(super) fn draw_right_column(&mut self, ui: &mut egui::Ui) {
        let is_wide = matches!(breakpoint(ui.ctx()), Breakpoint::Wide);
        if is_wide {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    self.draw_quickstart_card(ui);
                    ui.add_space(8.0);
                    self.draw_batch_card(ui);
                    ui.add_space(8.0);
                    self.draw_history_card(ui);
                });
                ui.add_space(14.0);
                ui.vertical(|ui| {
                    self.draw_presets_card(ui);
                    ui.add_space(8.0);
                    self.draw_extract_card(ui);
                    ui.add_space(8.0);
                    self.draw_dropzone_card(ui);
                });
            });
        } else {
            self.draw_quickstart_card(ui);
            ui.add_space(8.0);
            self.draw_presets_card(ui);
            ui.add_space(8.0);
            self.draw_extract_card(ui);
            ui.add_space(8.0);
            self.draw_batch_card(ui);
            ui.add_space(8.0);
            self.draw_dropzone_card(ui);
            ui.add_space(8.0);
            self.draw_history_card(ui);
        }
    }

    pub(super) fn draw_quickstart_card(&mut self, ui: &mut egui::Ui) {
        card(ui, t!("panel.quickstart").as_ref(), |ui| {
            let file = self
                .s
                .source_file
                .as_ref()
                .and_then(|p| p.file_name().and_then(|s| s.to_str()))
                .unwrap_or("—");
            let dev = self
                .s
                .device_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "—".into());
            let out = self
                .s
                .out_dir
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "—".into());

            ui.label(t!("quickstart.file", name = file).as_ref());
            ui.label(t!("quickstart.device", path = dev).as_ref());
            ui.label(t!("quickstart.target", path = out).as_ref());

            self.draw_stage_preview(ui);

            if (self.running || self.progress > 0.0) && !self.is_extract_job {
                let sp = self.smooth_progress();
                let eta = self.eta_suffix();
                let bar_text = if self.label.is_empty() {
                    format!("{:.0}%{}", sp * 100.0, eta)
                } else {
                    format!("{}{}", self.label, eta)
                };
                ui.scope(|ui| {
                    ui.visuals_mut().extreme_bg_color = Color32::from_rgb(30, 30, 34);
                    ui.add(
                        egui::ProgressBar::new(sp)
                            .text(bar_text)
                            .animate(self.running),
                    );
                });
            }

            let start_status = self.start_status();
            let can_start = start_status.can_start;

            ui.horizontal(|ui| {
                let compact = matches!(
                    breakpoint(ui.ctx()),
                    Breakpoint::Narrow | Breakpoint::Medium
                );
                let start_text = if compact { "▶".to_string() } else { t!("toolbar.start").to_string() };
                let start_btn = egui::Button::new(
                    RichText::new(start_text)
                    .strong(),
                )
                .fill(ui.visuals().selection.bg_fill)
                .stroke(Stroke::new(1.0, ui.visuals().selection.stroke.color));
                if ui.add_enabled(can_start, start_btn).clicked() {
                    let _ = confy::store("dvd2chd", None::<&str>, &self.s);
                    self.start_job();
                }
                if let Some(dir) = &self.s.out_dir {
                    let open_text = if compact { "📂".to_string() } else { t!("quickstart.open_output").to_string() };
                    if ui.button(open_text).clicked() {
                        let _ = open_folder(dir);
                    }
                }
            });

            if !start_status.reasons.is_empty() && !self.running {
                ui.add_space(6.0);
                let prefix = t!("quickstart.cannot_start");
                let reasons = start_status.reasons.join(" · ");
                ui.colored_label(
                    Color32::from_rgb(239, 68, 68),
                    format!("{} {reasons}", prefix),
                );
            }
        });
    }

    pub(super) fn draw_presets_card(&mut self, ui: &mut egui::Ui) {
        card(ui, t!("panel.presets").as_ref(), |ui| {
            if self.presets.presets.is_empty() {
                ui.label(t!("presets.empty").as_ref());
            } else {
                let len = self.presets.presets.len();
                let current_idx = self.selected_preset.unwrap_or(0).min(len.saturating_sub(1));
                self.selected_preset = Some(current_idx);
                let current_name = self.presets.presets[current_idx].name.clone();
                egui::ComboBox::from_id_source("preset_select")
                    .selected_text(current_name)
                    .show_ui(ui, |ui| {
                        for (idx, preset) in self.presets.presets.iter().enumerate() {
                            if ui
                                .selectable_label(
                                    self.selected_preset == Some(idx),
                                    preset.name.as_str(),
                                )
                                .clicked()
                            {
                                self.selected_preset = Some(idx);
                            }
                        }
                    });

                ui.horizontal(|ui| {
                    let can_load = self
                        .selected_preset
                        .and_then(|idx| self.presets.presets.get(idx))
                        .is_some();
                    if ui
                        .add_enabled(can_load, egui::Button::new(t!("presets.load").as_ref()))
                        .clicked()
                    {
                        if let Some(idx) = self.selected_preset {
                            if let Some(preset) = self.presets.presets.get(idx).cloned() {
                                self.apply_preset_settings(&preset.settings);
                                self.log_line(&t!("log.preset_loaded", name = preset.name));
                            }
                        }
                    }

                    if ui
                        .add_enabled(can_load, egui::Button::new(t!("presets.delete").as_ref()))
                        .clicked()
                    {
                        if let Some(idx) = self.selected_preset {
                            if let Some(preset) = self.presets.presets.get(idx).cloned() {
                                let name = preset.name;
                                self.presets.presets.remove(idx);
                                if self.presets.presets.is_empty() {
                                    self.selected_preset = None;
                                } else {
                                    let clamped =
                                        idx.min(self.presets.presets.len().saturating_sub(1));
                                    self.selected_preset = Some(clamped);
                                }
                                let msg = t!("log.preset_deleted", name = name).to_string();
                                self.persist_presets(&msg);
                            }
                        }
                    }
                });
            }

            ui.separator();
            ui.horizontal(|ui| {
                ui.label(t!("presets.name_label").as_ref());
                ui.text_edit_singleline(&mut self.preset_name);
            });
            let preset_name_empty = self.preset_name.trim().is_empty();
            if ui
                .button(t!("presets.save").as_ref())
                .clicked()
            {
                let name = self.preset_name.trim();
                if name.is_empty() {
                    self.log_line(&t!("log.preset_name_required"));
                } else {
                    let mut snapshot = self.s.snapshot_for_preset();
                    snapshot.force_createdvd = self.s.force_createdvd;
                    if let Some(idx) = self
                        .presets
                        .presets
                        .iter()
                        .position(|p| p.name.eq_ignore_ascii_case(name))
                    {
                        self.presets.presets[idx].settings = snapshot;
                        self.presets.presets[idx].name = name.to_string();
                        self.selected_preset = Some(idx);
                        let msg = t!("log.preset_updated", name = name).to_string();
                        self.persist_presets(&msg);
                    } else {
                        self.presets.presets.push(Preset {
                            name: name.to_string(),
                            settings: snapshot,
                        });
                        self.selected_preset = Some(self.presets.presets.len() - 1);
                        let msg = t!("log.preset_saved", name = name).to_string();
                        self.persist_presets(&msg);
                    }
                }
            }
            if preset_name_empty {
                ui.colored_label(
                    Color32::from_rgb(239, 68, 68),
                    t!("presets.name_required").as_ref(),
                );
            }
        });
    }

    pub(super) fn draw_batch_card(&mut self, ui: &mut egui::Ui) {
        card(ui, t!("panel.batch").as_ref(), |ui| {
            ui.horizontal(|ui| {
                if ui
                    .button(t!("batch.add_files").as_ref())
                    .clicked()
                {
                    if let Some(files) = rfd::FileDialog::new()
                        .add_filter("ISO/CUE", &["iso", "cue"])
                        .set_title(t!("batch.title").as_ref())
                        .pick_files()
                    {
                        for path in files {
                            self.enqueue_batch_file(path);
                        }
                    }
                }
                if ui
                    .add_enabled(
                        !self.batch_queue.is_empty(),
                        egui::Button::new(t!("batch.clear").as_ref()),
                    )
                    .clicked()
                {
                    self.batch_queue.clear();
                    self.log_line(&t!("log.batch_cleared"));
                }
            });

            ui.separator();
            if self.batch_queue.is_empty() {
                ui.label(t!("batch.empty").as_ref());
            } else {
                let mut remove_idx: Option<usize> = None;
                egui::ScrollArea::vertical()
                    .max_height(140.0)
                    .show(ui, |ui| {
                        for (idx, path) in self.batch_queue.iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.label(format!("{:02}. {}", idx + 1, path.display()));
                                if ui.button("✖").clicked() {
                                    remove_idx = Some(idx);
                                }
                            });
                        }
                    });
                if let Some(idx) = remove_idx {
                    self.batch_queue.remove(idx);
                }
            }

            ui.add_space(4.0);
            let can_start_batch = !self.running && !self.batch_queue.is_empty();
            if ui
                .add_enabled(
                    can_start_batch,
                    egui::Button::new(t!("batch.start").as_ref()),
                )
                .clicked()
                && !self.start_next_batch_if_possible()
            {
                self.log_line(&t!("log.batch_failed"));
            }
            if self.batch_queue.is_empty() {
                ui.colored_label(
                    Color32::from_rgb(239, 68, 68),
                    t!("batch.empty_warning").as_ref(),
                );
            }
        });
    }

    pub(super) fn draw_history_card(&mut self, ui: &mut egui::Ui) {
        card(ui, t!("history.heading").as_ref(), |ui| {
            if self.job_history.is_empty() {
                ui.label(t!("history.empty").as_ref());
            } else {
                // Show newest entries first (up to 10)
                let entries: Vec<_> = self.job_history.iter().rev().take(10).collect();
                for (ts, path, size_bytes) in entries {
                    ui.horizontal(|ui| {
                        // Timestamp
                        let time_str = {
                            let duration = ts
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default();
                            let secs = duration.as_secs();
                            let h = (secs / 3600) % 24;
                            let m = (secs / 60) % 60;
                            let s = secs % 60;
                            format!("{h:02}:{m:02}:{s:02}")
                        };
                        ui.label(egui::RichText::new(time_str).monospace().weak());

                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| path.display().to_string());
                        let size_mb = *size_bytes as f64 / 1_048_576.0;
                        ui.label(format!("{name} ({size_mb:.0} MB)"));

                        if ui.small_button(t!("history.open").as_ref()).clicked() {
                            if let Some(dir) = path.parent() {
                                let _ = open_folder(dir);
                            }
                        }
                    });
                }
            }
        });
    }

    pub(super) fn draw_extract_card(&mut self, ui: &mut egui::Ui) {
        card(ui, t!("panel.extract").as_ref(), |ui| {
            // CHD file picker
            ui.horizontal(|ui| {
                if ui.button(t!("extract.choose_chd").as_ref()).clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("CHD", &["chd"])
                        .pick_file()
                    {
                        self.extract_chd_path = Some(p);
                    }
                }
                let chd_label = self
                    .extract_chd_path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .and_then(|s| s.to_str())
                    .unwrap_or("—");
                ui.label(chd_label);
            });

            // Output directory picker
            ui.horizontal(|ui| {
                if ui.button(t!("extract.out_dir").as_ref()).clicked() {
                    if let Some(p) = rfd::FileDialog::new().pick_folder() {
                        self.extract_out_dir = Some(p);
                    }
                }
                let dir_label = self
                    .extract_out_dir
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "—".into());
                ui.label(dir_label.as_str());
            });

            // Mode combo
            ui.horizontal(|ui| {
                ui.label(t!("extract.mode_label").as_ref());
                let mode_selected = match self.extract_mode {
                    dvd2chd_core::ExtractMode::Auto => t!("extract.mode_auto"),
                    dvd2chd_core::ExtractMode::Dvd => t!("extract.mode_dvd"),
                    dvd2chd_core::ExtractMode::Cd => t!("extract.mode_cd"),
                };
                egui::ComboBox::from_id_source("extract_mode")
                    .selected_text(mode_selected.as_ref())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.extract_mode,
                            dvd2chd_core::ExtractMode::Auto,
                            t!("extract.mode_auto").as_ref(),
                        );
                        ui.selectable_value(
                            &mut self.extract_mode,
                            dvd2chd_core::ExtractMode::Dvd,
                            t!("extract.mode_dvd").as_ref(),
                        );
                        ui.selectable_value(
                            &mut self.extract_mode,
                            dvd2chd_core::ExtractMode::Cd,
                            t!("extract.mode_cd").as_ref(),
                        );
                    });
            });

            // Progress (only while an extract job is running)
            if (self.running || self.progress > 0.0) && self.is_extract_job {
                let sp = self.smooth_progress();
                let eta = self.eta_suffix();
                let bar_text = if self.label.is_empty() {
                    format!("{:.0}%{}", sp * 100.0, eta)
                } else {
                    format!("{}{}", self.label, eta)
                };
                ui.scope(|ui| {
                    ui.visuals_mut().extreme_bg_color = Color32::from_rgb(30, 30, 34);
                    ui.add(
                        egui::ProgressBar::new(sp)
                            .text(bar_text)
                            .animate(true),
                    );
                });
            }

            // Start button
            let can_extract = self.extract_chd_path.is_some()
                && self.extract_out_dir.is_some()
                && self.tools.chdman.is_some()
                && !self.running;
            if ui
                .add_enabled(
                    can_extract,
                    egui::Button::new(t!("extract.start").as_ref()),
                )
                .clicked()
            {
                self.start_extract_job();
            }
        });
    }

    pub(super) fn draw_dropzone_card(&mut self, ui: &mut egui::Ui) {
        card(ui, t!("panel.dropzone").as_ref(), |ui| {
                use egui::{CursorIcon, Sense};
                let size = egui::vec2(ui.available_width(), 160.0);

                let (response, painter) = ui.allocate_painter(size, Sense::click());
                let visuals = ui.visuals().clone();
                let rect = response.rect;

                let hovering_file = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
                let stroke_col = visuals.widgets.inactive.fg_stroke.color;
                let stroke_w = if hovering_file || response.hovered() {
                    2.5
                } else {
                    1.0
                };

                painter.rect(
                    rect.shrink(8.0),
                    Rounding::same(10.0),
                    visuals.extreme_bg_color,
                    Stroke::new(stroke_w, stroke_col),
                );

                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    t!("dropzone.hint").as_ref(),
                    egui::TextStyle::Body.resolve(ui.style()),
                    visuals.text_color(),
                );

                if response.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                }
                if response.clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("ISO/CUE", &["iso", "cue"])
                        .pick_file()
                    {
                        self.set_source_file(p, Some(t!("log.file_selected").to_string()));
                    }
                }
            });
    }
}

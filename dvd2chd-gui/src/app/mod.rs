//! DVD2CHD GUI – responsive (seamless), with drive detection

use rust_i18n::t;
use eframe::{egui, NativeOptions};
use egui::{
    Align, CentralPanel, Color32, Frame, Layout, Margin, RichText, Rounding, SidePanel,
    Stroke, TopBottomPanel,
};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    process::Command,
    sync::{atomic::AtomicBool, mpsc, Arc},
};

use dvd2chd_core::core_wiring::UiMsg;
use dvd2chd_core::tools::{probe_all, Tools};

use crate::drive::Drive;
use crate::pkg_install::PackageManager;

mod animation;
mod state;
mod workflow;

mod log;
mod source;
mod tools_check;
mod job;
mod timeline;
mod presets;
mod draw_layout;
mod draw_toolbar;
mod draw_dialogs;
#[cfg(windows)]
mod taskbar;

use self::animation::AnimationState;
#[cfg(debug_assertions)]
use self::animation::StageActivity;
use self::state::{Language, PresetStore, Settings, Theme, UiPreferences};
use self::workflow::{JobStage, StageState};
#[cfg(debug_assertions)]
use self::workflow::JobStageKind;
use self::log::LogEntry;

const LOG_ICON_BYTES: &[u8] = include_bytes!("../../assets/log_icon_32.png");

fn my_small_shadow() -> egui::epaint::Shadow {
    egui::epaint::Shadow {
        offset: egui::vec2(0.0, 2.0),
        blur: 8.0,
        spread: 0.0,
        color: Color32::from_black_alpha(30),
    }
}
fn my_big_shadow() -> egui::epaint::Shadow {
    egui::epaint::Shadow {
        offset: egui::vec2(0.0, 4.0),
        blur: 16.0,
        spread: 0.0,
        color: Color32::from_black_alpha(40),
    }
}

/// Translates well-known English core labels to the active locale.
fn translate_core_label(label: &str) -> String {
    match label {
        "Starting…" => t!("label.starting").to_string(),
        "Verification…" => t!("label.verification_starting").to_string(),
        "Verification complete" => t!("label.verification_done").to_string(),
        _ => {
            if let Some(rest) = label.strip_prefix("Verification ") {
                format!("{} {}", t!("label.verification_prefix"), rest)
            } else if let Some(pct_prefix) = label.strip_suffix("• Creating CHD…") {
                format!("{}• {}", pct_prefix, t!("label.creating_chd"))
            } else {
                label.to_string()
            }
        }
    }
}

fn card(ui: &mut egui::Ui, title: &str, mut body: impl FnMut(&mut egui::Ui)) {
    let visuals = ui.visuals().clone();
    let stroke_color = visuals.widgets.noninteractive.bg_stroke.color;
    let fill = visuals.faint_bg_color;
    Frame::group(ui.style())
        .fill(fill)
        .rounding(Rounding::same(10.0))
        .stroke(Stroke::new(1.0, stroke_color))
        .outer_margin(Margin::symmetric(0.0, 4.0))
        .inner_margin(Margin::symmetric(14.0, 12.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(title)
                        .strong()
                        .color(visuals.text_color())
                        .size(13.0),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_space(0.0);
                });
            });
            ui.separator();
            body(ui);
        });
}

fn open_folder(path: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        Command::new("explorer").arg(path).spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(path).spawn()?;
    }
    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open").arg(path).spawn()?;
    }
    Ok(())
}

fn bundled_tool_path(tool_name: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let tools_dir = dir.join("tools");
    let candidate = tools_dir.join(tool_name);
    if candidate.exists() {
        return Some(candidate);
    }
    #[cfg(windows)]
    {
        let candidate = tools_dir.join(format!("{tool_name}.exe"));
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}


#[derive(Copy, Clone, Debug, PartialEq)]
enum Breakpoint {
    Narrow,
    Medium,
    Wide,
}

fn breakpoint(ctx: &egui::Context) -> Breakpoint {
    let w = ctx.input(|i| i.screen_rect().width());
    if w < 900.0 {
        Breakpoint::Narrow
    } else if w < 1500.0 {
        Breakpoint::Medium
    } else {
        Breakpoint::Wide
    }
}

pub fn run() {
    fn load_icon(bytes: &[u8]) -> egui::IconData {
        let image = image::load_from_memory(bytes)
            .expect("icon decode")
            .to_rgba8();
        let (width, height) = image.dimensions();
        egui::IconData {
            rgba: image.into_raw(),
            width,
            height,
        }
    }

    let icon_bytes = include_bytes!("../../assets/icon.png");
    let viewport = egui::ViewportBuilder {
        icon: Some(Arc::new(load_icon(icon_bytes))),
        title: Some("DVD2CHD (GUI)".to_owned()),
        app_id: Some("dvd2chd-gui".to_owned()),
        ..Default::default()
    };
    let opts = NativeOptions {
        viewport,
        ..NativeOptions::default()
    };
    if let Err(e) = eframe::run_native(
        "DVD2CHD (GUI)",
        opts,
        Box::new(
            |cc| -> Result<Box<dyn eframe::App>, Box<dyn std::error::Error + Send + Sync>> {
                Ok(Box::new(App::new(cc)))
            },
        ),
    ) {
        eprintln!("GUI start failed: {e}");
    }
}

#[derive(Clone, Copy)]
struct ThemePalette {
    panel: Color32,
    surface: Color32,
    extreme: Color32,
    stroke: Color32,
    accent: Color32,
}

struct App {
    s: Settings,
    rx: Option<mpsc::Receiver<UiMsg>>,
    running: bool,
    cancel: Option<Arc<AtomicBool>>,
    log: VecDeque<LogEntry>,
    log_line_count: usize,
    progress: f32,
    label: String,

    theme: Theme,
    zoom: f32,
    reduce_motion: bool,

    tools: Tools,

    animation: AnimationState,

    // Narrow: linke Spalte ein-/ausblendbar
    left_open: bool,

    // Laufwerke
    detected_drives: Vec<Drive>,
    show_drive_picker: bool,
    selected_drive: Option<usize>,

    // Presets
    presets: PresetStore,
    selected_preset: Option<usize>,
    preset_name: String,

    // Batch
    batch_queue: VecDeque<PathBuf>,

    // Timeline
    timeline: Vec<JobStage>,

    layout_initialized: bool,
    manual_theme: Theme,
    auto_theme: bool,
    show_about: bool,
    log_open: bool,
    icon_texture: Option<egui::TextureHandle>,
    log_icon_texture: Option<egui::TextureHandle>,
    #[cfg(debug_assertions)]
    debug_animation_override: Option<JobStageKind>,
    #[cfg(debug_assertions)]
    debug_animation_indicator: String,
    #[cfg(debug_assertions)]
    debug_timeline_backup: Option<Vec<JobStage>>,

    show_custom_name_prompt: bool,
    custom_name_input: String,
    custom_name_error: Option<String>,

    // Tool auto-install via system package manager
    detected_pkg_manager: Option<PackageManager>,
    tool_install_running: bool,
    tool_install_rx: Option<mpsc::Receiver<Result<Vec<String>, String>>>,

    // chdman binary download via manifest URL
    chdman_dl_running: bool,
    chdman_dl_rx: Option<mpsc::Receiver<Result<PathBuf, String>>>,

    // Job timing (for ETA)
    job_start_time: Option<std::time::Instant>,
    progress_hide_at: Option<std::time::Instant>,

    // Session job history (not persisted)
    job_history: Vec<(std::time::SystemTime, PathBuf, u64)>,

    // Smooth progress display (interpolated toward `progress`)
    progress_display: f32,

    // Theme crossfade
    last_effective_theme: Theme,
    theme_fade_start: Option<std::time::Instant>,
    theme_fade_from: Option<ThemePalette>,

    // Windows taskbar progress
    #[cfg(windows)]
    taskbar_progress: Option<taskbar::TaskbarProgress>,

    // CHD extraction
    extract_chd_path: Option<PathBuf>,
    extract_out_dir: Option<PathBuf>,
    extract_mode: dvd2chd_core::ExtractMode,
    is_extract_job: bool,
}

struct StartStatus {
    can_start: bool,
    reasons: Vec<String>,
}

impl Default for App {
    fn default() -> Self {
        let mut s: Settings = confy::load("dvd2chd", None::<&str>).unwrap_or_default();
        rust_i18n::set_locale(match s.language {
            Language::German => "de",
            Language::English => "en",
        });
        if s.chdman_path.is_none() {
            if let Some(path) = bundled_tool_path("chdman") {
                s.chdman_path = Some(path);
            }
        }
        if s.ddrescue_path.is_none() {
            if let Some(path) = bundled_tool_path("ddrescue") {
                s.ddrescue_path = Some(path);
            }
        }
        if s.cdrdao_path.is_none() {
            if let Some(path) = bundled_tool_path("cdrdao") {
                s.cdrdao_path = Some(path);
            }
        }
        let tools = probe_all(
            s.chdman_path.as_deref(),
            s.ddrescue_path.as_deref(),
            s.cdrdao_path.as_deref(),
        );
        let presets: PresetStore = confy::load("dvd2chd_presets", None::<&str>).unwrap_or_default();
        let selected_preset = if presets.presets.is_empty() {
            None
        } else {
            Some(0)
        };
        let ui_prefs: UiPreferences = confy::load("dvd2chd", Some("ui_prefs")).unwrap_or_default();
        let mut zoom = ui_prefs.zoom.clamp(0.75, 2.0);
        if zoom.is_nan() {
            zoom = 1.0;
        }
        let manual_theme = ui_prefs.theme;
        let auto_theme = ui_prefs.auto_theme;
        let reduce_motion = ui_prefs.reduce_motion;
        let current_theme = if auto_theme {
            Theme::Auto
        } else {
            manual_theme
        };
        let mut animation = AnimationState::default();
        animation.set_reduce_motion(reduce_motion);
        Self {
            s,
            rx: None,
            running: false,
            cancel: None,
            log: VecDeque::new(),
            log_line_count: 0,
            progress: 0.0,
            label: String::new(),
            theme: current_theme,
            zoom,
            reduce_motion,
            tools,
            animation,
            left_open: true,
            detected_drives: Vec::new(),
            show_drive_picker: false,
            selected_drive: None,
            presets,
            selected_preset,
            preset_name: String::new(),
            batch_queue: VecDeque::new(),
            timeline: Vec::new(),
            layout_initialized: false,
            manual_theme,
            auto_theme,
            show_about: false,
            log_open: true,
            icon_texture: None,
            log_icon_texture: None,
            #[cfg(debug_assertions)]
            debug_animation_override: None,
            #[cfg(debug_assertions)]
            debug_animation_indicator: String::new(),
            #[cfg(debug_assertions)]
            debug_timeline_backup: None,
            show_custom_name_prompt: false,
            custom_name_input: String::new(),
            custom_name_error: None,
            detected_pkg_manager: PackageManager::detect(),
            tool_install_running: false,
            tool_install_rx: None,
            chdman_dl_running: false,
            chdman_dl_rx: None,
            job_start_time: None,
            progress_hide_at: None,
            job_history: Vec::new(),
            progress_display: 0.0,
            last_effective_theme: current_theme,
            theme_fade_start: None,
            theme_fade_from: None,
            #[cfg(windows)]
            taskbar_progress: None,
            extract_chd_path: None,
            extract_out_dir: None,
            extract_mode: dvd2chd_core::ExtractMode::Auto,
            is_extract_job: false,
        }
    }
}

fn palette_for_theme(theme: Theme) -> ThemePalette {
    match theme {
        Theme::Light => ThemePalette {
            panel:   Color32::from_rgb(255, 255, 255),
            surface: Color32::from_rgb(247, 247, 248),
            extreme: Color32::from_rgb(239, 239, 241),
            stroke:  Color32::from_rgb(225, 225, 228),
            accent:  Color32::from_rgb(124, 93, 250),
        },
        Theme::HighContrast => ThemePalette {
            panel:   Color32::from_rgb(0, 0, 0),
            surface: Color32::from_rgb(18, 18, 18),
            extreme: Color32::from_rgb(0, 0, 0),
            stroke:  Color32::from_rgb(200, 200, 210),
            accent:  Color32::from_rgb(180, 160, 255),
        },
        _ => ThemePalette {
            // Soft Neutral — warm charcoal with soft lavender accent
            panel:   Color32::from_rgb(28, 28, 30),
            surface: Color32::from_rgb(44, 44, 46),
            extreme: Color32::from_rgb(20, 20, 22),
            stroke:  Color32::from_rgb(72, 72, 74),
            accent:  Color32::from_rgb(167, 139, 250),
        },
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Style & Shortcuts & Toolbar
        self.apply_style(ctx);
        self.ensure_log_icon_texture(ctx);
        self.handle_shortcuts(ctx);
        self.draw_top_toolbar(ctx);

        // Worker-Messages
        let mut pending_msgs = Vec::new();
        if let Some(rx) = &self.rx {
            while let Ok(msg) = rx.try_recv() {
                pending_msgs.push(msg);
            }
        }
        for msg in pending_msgs {
            match msg {
                UiMsg::Log(s) => {
                    self.append_log_text(&format!("{s}\n"));
                }
                UiMsg::Progress(p) => self.progress = p.clamp(0.0, 1.0),
                UiMsg::Label(t) => {
                    self.label = translate_core_label(&t);
                }
                UiMsg::Done(res) => {
                    self.running = false;
                    // NOTE: is_extract_job intentionally NOT cleared here —
                    // it stays true for 30 s so the extract card keeps the bar
                    // and the Quick Start card is suppressed during that window.
                    self.cancel = None;
                    self.job_start_time = None;
                    match res {
                        Ok(p) => {
                            self.progress = 1.0;
                            if self.label.is_empty() {
                                self.label = t!("label.done").to_string();
                            }
                            self.log_line(&t!("log.job_done", path = p.display().to_string()));
                            // Record job history entry
                            let chd_size = p.metadata().map(|m| m.len()).unwrap_or(0);
                            self.job_history.push((std::time::SystemTime::now(), p.clone(), chd_size));
                            // System notification (cross-platform via notify-rust)
                            if self.s.notify_on_done {
                                let name = p
                                    .file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| p.display().to_string());
                                let _ = notify_rust::Notification::new()
                                    .summary("DVD2CHD")
                                    .body(&format!("✔ {}", name))
                                    .show();
                            }
                            if !self.batch_queue.is_empty() {
                                let _ = self.start_next_batch_if_possible();
                            }
                            self.mark_all_stages_done();
                            self.progress_hide_at = Some(
                                std::time::Instant::now()
                                    + std::time::Duration::from_secs(30),
                            );
                        }
                        Err(e) => {
                            self.log_line(&t!("log.job_error", err = format!("{e:?}")));
                            self.progress = 0.0;
                            self.label.clear();
                            self.is_extract_job = false;
                        }
                    }
                    if self
                        .timeline
                        .iter()
                        .any(|stage| stage.state == StageState::Active)
                    {
                        self.reset_timeline_after_failure();
                    }
                }
                UiMsg::Stage(ev) => self.handle_stage_event(ev),
            }
        }

        // Poll tool-install result (background thread)
        let install_result = self
            .tool_install_rx
            .as_ref()
            .and_then(|rx| rx.try_recv().ok());
        if let Some(result) = install_result {
            self.tool_install_running = false;
            self.tool_install_rx = None;
            match result {
                Ok(pkgs) => {
                    let pkg_list = pkgs.join(", ");
                    self.log_line(&t!("log.tools_installed", pkgs = pkg_list));
                    self.reprobe_tools();
                }
                Err(e) => {
                    self.log_line(&t!("log.install_failed", err = e));
                }
            }
        }

        // Poll chdman binary download result
        let dl_result = self
            .chdman_dl_rx
            .as_ref()
            .and_then(|rx| rx.try_recv().ok());
        if let Some(result) = dl_result {
            self.chdman_dl_running = false;
            self.chdman_dl_rx = None;
            match result {
                Ok(path) => {
                    let disp = path.display().to_string();
                    self.log_line(&t!("log.chdman_installed", path = disp));
                    self.s.chdman_path = Some(path);
                    self.save_settings();
                    self.reprobe_tools();
                }
                Err(e) => {
                    self.log_line(&t!("log.chdman_dl_failed", err = e));
                }
            }
        }

        // Auto-hide progress bar after 30 s
        if !self.running {
            if let Some(hide_at) = self.progress_hide_at {
                let now = std::time::Instant::now();
                if now >= hide_at {
                    self.progress = 0.0;
                    self.label.clear();
                    self.progress_hide_at = None;
                    self.is_extract_job = false;
                } else {
                    ctx.request_repaint_after(hide_at.duration_since(now));
                }
            }
        }

        #[cfg(debug_assertions)]
        let override_kind = self.debug_animation_override;
        #[cfg(not(debug_assertions))]
        let override_kind = None;

        #[cfg(debug_assertions)]
        let StageActivity {
            compress_active,
            verify_active,
            hash_active,
        } = self
            .animation
            .update(ctx, &self.timeline, override_kind, self.running);
        #[cfg(not(debug_assertions))]
        let _ = self
            .animation
            .update(ctx, &self.timeline, override_kind, self.running);

        #[cfg(debug_assertions)]
        {
            if let Some(kind) = self.debug_animation_override {
                self.debug_update_indicator(Some(kind));
            } else {
                if compress_active {
                    self.debug_update_indicator(Some(JobStageKind::Chd));
                }
                if verify_active {
                    self.debug_update_indicator(Some(JobStageKind::Verify));
                }
                if hash_active {
                    self.debug_update_indicator(Some(JobStageKind::Hash));
                }
            }
        }

        // ── Smooth progress interpolation ──
        {
            let dt = ctx.input(|i| i.stable_dt).min(0.1);
            let diff = self.progress - self.progress_display;
            if diff.abs() < 0.001 || self.progress < self.progress_display - 0.01 {
                // Snap immediately on reset or when close enough
                self.progress_display = self.progress;
            } else {
                self.progress_display += diff * (1.0 - (-10.0_f32 * dt).exp());
            }
        }

        // ── Windows taskbar progress ──
        #[cfg(windows)]
        {
            if self.taskbar_progress.is_none() {
                self.taskbar_progress = taskbar::TaskbarProgress::new("DVD2CHD (GUI)");
            }
            if let Some(tb) = &self.taskbar_progress {
                if self.running || self.progress > 0.0 {
                    tb.set_progress(self.progress);
                } else {
                    tb.clear();
                }
            }
        }

        if !self.layout_initialized {
            if matches!(breakpoint(ctx), Breakpoint::Narrow) {
                self.left_open = false;
            }
            self.layout_initialized = true;
        }

        let control_width = match breakpoint(ctx) {
            Breakpoint::Narrow => 280.0,
            Breakpoint::Medium => 340.0,
            Breakpoint::Wide => 400.0,
        };
        SidePanel::left("control_panel")
            .resizable(true)
            .default_width(control_width)
            .show_animated(ctx, self.left_open, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.draw_left_column(ui);
                    });
            });

        let vis = ctx.style().visuals.clone();
        CentralPanel::default()
            .frame(Frame::none().fill(vis.panel_fill))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.draw_right_column(ui);
                    });
            });

        // === Log unten
        if self.log_open {
            TopBottomPanel::bottom("log_panel")
                .resizable(true)
                .default_height(180.0)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.heading(RichText::new(t!("log.heading").as_ref()).strong());
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button(t!("log.save").as_ref()).clicked() {
                                if let Some(path) = rfd::FileDialog::new()
                                    .set_file_name("dvd2chd.log")
                                    .save_file()
                                {
                                    let _ = std::fs::write(path, self.log_text());
                                }
                            }
                            if ui.button(t!("log.copy").as_ref()).clicked() {
                                let snapshot = self.log_text();
                                ui.output_mut(|o| o.copied_text = snapshot);
                            }
                            if ui.button(t!("log.clear_btn").as_ref()).clicked() {
                                self.clear_log();
                            }
                        });
                    });
                    ui.horizontal(|ui| {
                        let clear_log_text = t!("log.auto_clear");
                        ui.checkbox(&mut self.s.auto_clear_log, clear_log_text.as_ref());
                    });
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .stick_to_bottom(true)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.monospace(self.log_text());
                        });
                });
        }

        self.draw_about_window(ctx);
        self.draw_custom_name_prompt(ctx);

        // Drag&Drop (ISO/CUE)
        let drop_items = ctx.input(|i| i.raw.dropped_files.clone());
        let total_dropped = drop_items.len();
        let mut first_drop = true;
        for file in drop_items {
            if let Some(p) = file.path {
                let ok = p
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|s| s.eq_ignore_ascii_case("iso") || s.eq_ignore_ascii_case("cue"))
                    .unwrap_or(false);
                if ok {
                    if first_drop {
                        first_drop = false;
                        self.set_source_file(
                            p.clone(),
                            Some(t!("log.drop_accepted").to_string()),
                        );
                        if total_dropped > 1 {
                            self.log_line(&t!("log.drop_batch_added"));
                        }
                    } else {
                        self.enqueue_batch_file(p);
                    }
                }
            }
        }

        // Drive-Picker
        if self.show_drive_picker {
            egui::Window::new(t!("drive.picker_title").as_ref())
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    if self.detected_drives.is_empty() {
                        ui.label(t!("drive.no_drive").as_ref());
                        if ui.button(t!("drive.close").as_ref()).clicked() {
                            self.show_drive_picker = false;
                        }
                        return;
                    }

                    let current = self
                        .selected_drive
                        .and_then(|i| self.detected_drives.get(i))
                        .map(|d| self.drive_display_label(d))
                        .unwrap_or_else(|| t!("drive.please_choose").to_string());

                    egui::ComboBox::from_label(t!("drive.detected").as_ref())
                        .selected_text(current)
                        .show_ui(ui, |ui| {
                            for (i, d) in self.detected_drives.iter().enumerate() {
                                let label = self.drive_display_label(d);
                                if ui
                                    .selectable_label(self.selected_drive == Some(i), label)
                                    .clicked()
                                {
                                    self.selected_drive = Some(i);
                                    ui.close_menu();
                                }
                            }
                        });

                    ui.horizontal(|ui| {
                        if ui.button(t!("drive.apply").as_ref()).clicked() {
                            if let Some(i) = self.selected_drive {
                                if let Some(drive) = self.detected_drives.get(i).cloned() {
                                    let path_display = drive.path.display().to_string();
                                    self.set_device_path(drive.path.clone(), None);
                                    self.log_line(&t!("log.drive_set", path = path_display));
                                }
                            }
                            self.show_drive_picker = false;
                        }
                        if ui.button(t!("drive.cancel").as_ref()).clicked() {
                            self.show_drive_picker = false;
                        }
                    });
                });
        }
    }
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut app = Self::default();
        app.ensure_icon_texture(&cc.egui_ctx);
        app.ensure_log_icon_texture(&cc.egui_ctx);
        app
    }

    fn start_status(&self) -> StartStatus {
        let mut reasons = Vec::new();
        if self.s.out_dir.is_none() {
            reasons.push(t!("status.no_output_folder").to_string());
        }
        let intent = self.current_job_intent();
        if intent.is_none() {
            reasons.push(t!("status.no_source").to_string());
        }
        if self.running {
            reasons.push(t!("status.job_running").to_string());
        }
        if let Some(intent) = intent {
            for req in self.missing_tools_for(intent) {
                reasons.push(self.missing_tool_message(req));
            }
        }
        StartStatus {
            can_start: reasons.is_empty(),
            reasons,
        }
    }

    fn can_start_job(&self) -> bool {
        self.start_status().can_start
    }

    /// Computes an ETA suffix like " — ~3m12s" when a job is running.
    /// Returns an empty string when the ETA is not meaningful or the core
    /// label already contains an "ETA" timestamp.
    pub(super) fn eta_suffix(&self) -> String {
        if !self.running || self.progress <= 0.02 || self.label.contains("ETA") {
            return String::new();
        }
        let Some(start) = self.job_start_time else {
            return String::new();
        };
        let elapsed = start.elapsed().as_secs_f64();
        let remaining = elapsed / self.progress as f64 * (1.0 - self.progress as f64);
        if remaining >= 3600.0 {
            format!(
                " — ~{:.0}h{:.0}m",
                remaining / 3600.0,
                (remaining % 3600.0) / 60.0
            )
        } else if remaining >= 60.0 {
            format!(" — ~{:.0}m{:.0}s", remaining / 60.0, remaining % 60.0)
        } else {
            format!(" — ~{:.0}s", remaining)
        }
    }

    /// Smoothly interpolated progress value for display.
    pub(super) fn smooth_progress(&self) -> f32 {
        self.progress_display
    }

    pub(super) fn do_manual_eject(&mut self) {
        let Some(dev) = self.s.device_path.clone() else {
            return;
        };

        #[cfg(windows)]
        {
            match dvd2chd_core::windows_rip::eject_drive_windows(&dev) {
                Ok(()) => {
                    self.log_line(&t!("log.eject_ok", path = dev.display().to_string()));
                }
                Err(e) => {
                    self.log_line(&t!(
                        "log.eject_failed",
                        path = dev.display().to_string(),
                        err = format!("{e}")
                    ));
                }
            }
        }

        #[cfg(not(windows))]
        {
            let udisks_ok = Command::new("udisksctl")
                .args(["eject", "-b"])
                .arg(&dev)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if udisks_ok {
                self.log_line(&t!("log.eject_ok", path = dev.display().to_string()));
                return;
            }
            match Command::new("eject").arg(&dev).status() {
                Ok(s) if s.success() => {
                    self.log_line(&t!("log.eject_ok", path = dev.display().to_string()));
                }
                Ok(s) => {
                    self.log_line(&t!(
                        "log.eject_failed",
                        path = dev.display().to_string(),
                        err = format!("exit {}", s.code().unwrap_or(-1))
                    ));
                }
                Err(e) => {
                    self.log_line(&t!(
                        "log.eject_failed",
                        path = dev.display().to_string(),
                        err = e.to_string()
                    ));
                }
            }
        }
    }
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    Color32::from_rgba_unmultiplied(
        (a.r() as f32 + (b.r() as f32 - a.r() as f32) * t) as u8,
        (a.g() as f32 + (b.g() as f32 - a.g() as f32) * t) as u8,
        (a.b() as f32 + (b.b() as f32 - a.b() as f32) * t) as u8,
        (a.a() as f32 + (b.a() as f32 - a.a() as f32) * t) as u8,
    )
}

fn lerp_palette(a: &ThemePalette, b: &ThemePalette, t: f32) -> ThemePalette {
    ThemePalette {
        panel: lerp_color(a.panel, b.panel, t),
        surface: lerp_color(a.surface, b.surface, t),
        extreme: lerp_color(a.extreme, b.extreme, t),
        stroke: lerp_color(a.stroke, b.stroke, t),
        accent: lerp_color(a.accent, b.accent, t),
    }
}

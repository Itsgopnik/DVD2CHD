use std::{
    f32::consts::TAU,
    time::{Duration, Instant},
};

use eframe::egui::{self, Color32, Pos2, Stroke, Visuals};

use super::workflow::{JobStage, JobStageKind, StageState};

// ── Per-stage animation state ────────────────────────────────────────────────

#[derive(Debug)]
struct PhaseAnim {
    phase: f32,
    phase_smooth: f32,
    velocity: f32,
    offset: f32,
    drive: f32,
}

impl PhaseAnim {
    fn new(offset: f32) -> Self {
        Self {
            phase: 0.0,
            phase_smooth: offset.rem_euclid(1.0),
            velocity: 0.0,
            offset,
            drive: 0.0,
        }
    }

    fn reset(&mut self) {
        self.phase = 0.0;
        self.phase_smooth = self.offset.rem_euclid(1.0);
        self.velocity = 0.0;
        self.drive = 0.0;
    }

    /// Tick the phase forward by `delta` seconds.
    ///
    /// Uses exponential-decay smoothing for both drive and velocity, giving
    /// critically-damped (no-overshoot) easing. Returns `true` while the
    /// animation still needs repainting.
    fn tick(
        &mut self,
        active: bool,
        delta: f32,
        stiffness: f32,
        smooth_k: f32,
        responsiveness: f32,
    ) -> bool {
        const EPS: f32 = 0.0005;
        let target = if active { 1.0 } else { 0.0 };
        // SPEED controls how many full revolutions per second at full drive.
        // 0.38 ≈ one revolution every ~2.6 s — feels calm and readable.
        const SPEED: f32 = 0.38;
        self.drive = lerp_f32(self.drive, target, 1.0 - (-delta * responsiveness).exp());
        self.velocity = lerp_f32(self.velocity, self.drive, 1.0 - (-delta * stiffness).exp());
        self.phase = (self.phase + delta * self.velocity * SPEED).rem_euclid(1.0);

        let target_phase = (self.phase + self.offset).rem_euclid(1.0);
        let prev = self.phase_smooth;
        self.phase_smooth =
            lerp_phase(self.phase_smooth, target_phase, 1.0 - (-delta * smooth_k).exp());

        self.drive > EPS || self.velocity.abs() > EPS || phase_distance(prev, self.phase_smooth) > EPS
    }

    /// Immediately snap to the settled state (for reduce-motion mode).
    fn snap(&mut self, active: bool) {
        self.drive = if active { 1.0 } else { 0.0 };
        self.velocity = self.drive;
        self.phase_smooth = (self.phase + self.offset).rem_euclid(1.0);
    }

    /// Zero velocity and drive (for reduce-motion transition).
    fn pause(&mut self) {
        self.velocity = 0.0;
        self.drive = 0.0;
    }
}

// ── Top-level animation state ────────────────────────────────────────────────

#[derive(Debug)]
pub struct AnimationState {
    spinner_angle: f32,
    spinner_velocity: f32,
    spinner_angle_smooth: f32,
    spinner_drive: f32,

    compress: PhaseAnim,
    verify: PhaseAnim,
    hash: PhaseAnim,
    rip: PhaseAnim,

    last_tick: Instant,
    reduce_motion: bool,
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct StageActivity {
    pub compress_active: bool,
    pub verify_active: bool,
    pub hash_active: bool,
}

fn ease_in_out_smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let t2 = t * t;
    let t3 = t2 * t;
    3.0 * t2 - 2.0 * t3
}

fn lerp_angle(from: f32, to: f32, t: f32) -> f32 {
    let diff = (to - from).rem_euclid(TAU);
    let shortest = if diff > TAU / 2.0 { diff - TAU } else { diff };
    (from + shortest * t).rem_euclid(TAU)
}

fn lerp_phase(from: f32, to: f32, t: f32) -> f32 {
    let diff = (to - from).rem_euclid(1.0);
    let shortest = if diff > 0.5 { diff - 1.0 } else { diff };
    (from + shortest * t).rem_euclid(1.0)
}

fn lerp_f32(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t.clamp(0.0, 1.0)
}

fn phase_distance(a: f32, b: f32) -> f32 {
    let diff = (a - b).abs();
    diff.min(1.0 - diff)
}

fn blend_activity(current: &mut f32, active: bool, delta: f32, responsiveness: f32) -> f32 {
    let target = if active { 1.0 } else { 0.0 };
    let smoothing = 1.0 - (-delta * responsiveness).exp();
    *current = lerp_f32(*current, target, smoothing);
    *current
}

impl Default for AnimationState {
    fn default() -> Self {
        use std::collections::hash_map::RandomState;
        use std::hash::BuildHasher;
        let rand_val = RandomState::new().hash_one(Instant::now()) as f32 / u64::MAX as f32;

        Self {
            spinner_angle: rand_val * TAU,
            spinner_velocity: 0.0,
            spinner_angle_smooth: rand_val * TAU,
            spinner_drive: 0.0,
            compress: PhaseAnim::new((rand_val * 0.7).fract()),
            verify: PhaseAnim::new((rand_val * 0.5).fract()),
            hash: PhaseAnim::new((rand_val * 0.3).fract()),
            rip: PhaseAnim::new(0.0),
            last_tick: Instant::now(),
            reduce_motion: false,
        }
    }
}

impl AnimationState {
    pub fn set_reduce_motion(&mut self, enabled: bool) {
        self.reduce_motion = enabled;
        if enabled {
            self.spinner_velocity = 0.0;
            self.spinner_drive = 0.0;
            self.compress.pause();
            self.verify.pause();
            self.hash.pause();
            self.rip.pause();
        }
    }

    pub fn update(
        &mut self,
        ctx: &egui::Context,
        timeline: &[JobStage],
        override_kind: Option<JobStageKind>,
        running: bool,
    ) -> StageActivity {
        let now = Instant::now();
        let mut delta = now.duration_since(self.last_tick).as_secs_f32();
        self.last_tick = now;
        delta = delta.clamp(0.001, 0.1);

        let mut any_active = timeline.iter().any(|s| s.state == StageState::Active);
        let mut compress_active = timeline
            .iter()
            .any(|s| s.kind == JobStageKind::Chd && s.state == StageState::Active);
        let mut verify_active = timeline
            .iter()
            .any(|s| s.kind == JobStageKind::Verify && s.state == StageState::Active);
        let mut hash_active = timeline
            .iter()
            .any(|s| s.kind == JobStageKind::Hash && s.state == StageState::Active);
        let mut rip_active = timeline
            .iter()
            .any(|s| s.kind == JobStageKind::Input && s.state == StageState::Active);

        if let Some(kind) = override_kind {
            any_active = true;
            compress_active |= matches!(kind, JobStageKind::Chd);
            verify_active |= matches!(kind, JobStageKind::Verify);
            hash_active |= matches!(kind, JobStageKind::Hash);
            rip_active |= matches!(kind, JobStageKind::Input);
        }

        if self.reduce_motion {
            self.spinner_angle_smooth = self.spinner_angle;
            self.spinner_drive = if any_active { 1.0 } else { 0.0 };
            self.compress.snap(compress_active);
            self.verify.snap(verify_active);
            self.hash.snap(hash_active);
            self.rip.snap(rip_active);
            if running || any_active {
                ctx.request_repaint_after(Duration::from_millis(80));
            }
            return StageActivity {
                compress_active,
                verify_active,
                hash_active,
            };
        }

        let mut needs_repaint = false;

        // Spinner — kept as a separate angle accumulator rather than a phase.
        // 0.40 = calm rotation speed (about 1.5 s per revolution at full drive).
        const SPINNER_SPEED: f32 = 0.40;
        let spinner_drive = blend_activity(&mut self.spinner_drive, any_active, delta, 4.0);
        self.spinner_velocity =
            lerp_f32(self.spinner_velocity, spinner_drive, 1.0 - (-delta * 5.0_f32).exp());
        const EPS: f32 = 0.0005;
        if spinner_drive > EPS || self.spinner_velocity.abs() > EPS {
            self.spinner_angle =
                (self.spinner_angle + delta * self.spinner_velocity * SPINNER_SPEED).rem_euclid(TAU);
            self.spinner_angle_smooth = lerp_angle(
                self.spinner_angle_smooth,
                self.spinner_angle,
                1.0 - (-delta * 8.0_f32).exp(),
            );
            needs_repaint = true;
        }

        // Phase animations — stiffness / smooth_k / responsiveness tuned per stage
        needs_repaint |= self.compress.tick(compress_active, delta, 5.0, 7.0, 3.8);
        needs_repaint |= self.verify.tick(verify_active, delta, 4.8, 6.5, 3.5);
        needs_repaint |= self.hash.tick(hash_active, delta, 5.2, 7.5, 4.0);
        needs_repaint |= self.rip.tick(rip_active, delta, 4.5, 6.0, 3.2);

        if running || needs_repaint {
            ctx.request_repaint_after(Duration::from_millis(16));
        }

        StageActivity {
            compress_active,
            verify_active,
            hash_active,
        }
    }

    pub fn restart(&mut self) {
        *self = Self::default();
    }

    pub fn reset_phase_for(&mut self, kind: JobStageKind) {
        match kind {
            JobStageKind::Chd => self.compress.reset(),
            JobStageKind::Verify => self.verify.reset(),
            JobStageKind::Hash => self.hash.reset(),
            JobStageKind::Input => self.rip.reset(),
        }
        self.last_tick = Instant::now();
    }

    pub fn draw_stage_graphic(
        &self,
        painter: &egui::Painter,
        kind: JobStageKind,
        center: Pos2,
        radius: f32,
        accent: Color32,
        visuals: &Visuals,
    ) {
        match kind {
            JobStageKind::Input => self.draw_rip_reader(painter, center, radius, accent, visuals),
            JobStageKind::Chd => {
                self.draw_compression_disc(painter, center, radius, accent, visuals)
            }
            JobStageKind::Verify => {
                self.draw_verify_scanner(painter, center, radius, accent, visuals)
            }
            JobStageKind::Hash => self.draw_hash_pulses(painter, center, radius, accent, visuals),
        }
    }

    /// Disc ripping animation: a spinning disc with concentric track marks,
    /// scanner heads orbiting with glowing trails, inward-flowing data streams,
    /// a pulsing data core, and a sweeping sector read-head.
    fn draw_rip_reader(
        &self,
        painter: &egui::Painter,
        center: Pos2,
        radius: f32,
        _accent: Color32,
        _visuals: &Visuals,
    ) {
        let drive = self.rip.drive;
        let phase = self.rip.phase_smooth;
        let spinner = self.spinner_angle_smooth;

        let disc_r = radius * 0.72; // main disc radius

        // ── Outer ring glow — soft lavender ─────────────────────────────────
        if drive > 0.01 {
            let glow_pulse = (0.3 * (phase * TAU * 2.0).sin() + 0.5) * drive;
            for i in 0..6_usize {
                let r = disc_r + radius * 0.06 + i as f32 * radius * 0.03;
                let alpha = (14.0 * glow_pulse * (1.0 - i as f32 / 6.0)) as u8;
                painter.circle_filled(
                    center,
                    r,
                    Color32::from_rgba_unmultiplied(167, 139, 250, alpha),
                );
            }
        }

        // ── Static frame: outer ring ─────────────────────────────────────────
        painter.circle_stroke(
            center,
            disc_r + radius * 0.08,
            Stroke::new(
                radius * 0.010,
                Color32::from_rgba_unmultiplied(72, 72, 74, 30),
            ),
        );

        // ── Main disc ring — lavender ────────────────────────────────────────
        let ring_color = if drive > 0.01 {
            Color32::from_rgba_unmultiplied(167, 139, 250, (drive * 0.35 * 255.0) as u8)
        } else {
            Color32::from_rgba_unmultiplied(120, 120, 125, 32)
        };
        painter.circle_stroke(center, disc_r, Stroke::new(radius * 0.020, ring_color));

        // ── Track marks (concentric circles on the disc) ──────────────────────
        let track_alpha = if drive > 0.01 {
            (drive * 10.0).min(10.0) as u8
        } else {
            6_u8
        };
        let mut tr = radius * 0.15;
        while tr < disc_r {
            painter.circle_stroke(
                center,
                tr,
                Stroke::new(
                    0.5,
                    Color32::from_rgba_unmultiplied(255, 255, 255, track_alpha),
                ),
            );
            tr += radius * 0.08;
        }

        if drive > 0.01 {
            // ── 1. Pulsing data core with glow ────────────────────────────────
            let pulse = (0.5 * (phase * TAU * 5.0).sin() + 0.5) * drive;
            let core_r = radius * 0.10 + radius * 0.05 * pulse;

            // Core glow rings — soft lavender
            for i in 0..8_usize {
                let glow_r = core_r + i as f32 * radius * 0.025;
                let alpha = (24.0 * pulse * (1.0 - i as f32 / 8.0)) as u8;
                painter.circle_filled(
                    center,
                    glow_r,
                    Color32::from_rgba_unmultiplied(167, 139, 250, alpha),
                );
            }

            // Core body — mid lavender
            painter.circle_filled(
                center,
                core_r,
                Color32::from_rgba_unmultiplied(147, 119, 230, (drive * 0.72 * 255.0) as u8),
            );
            // Bright inner core — pale lavender-white
            painter.circle_filled(
                center,
                core_r * 0.5,
                Color32::from_rgba_unmultiplied(220, 215, 245, (drive * 0.82 * 255.0) as u8),
            );

            // ── 2. Rotating scanner heads with trails ─────────────────────────
            let num_scanners = 3_usize;
            for i in 0..num_scanners {
                let offset = i as f32 * TAU / num_scanners as f32;
                let angle = spinner + offset;

                let scanner_pos = center
                    + egui::vec2(angle.cos() * disc_r, angle.sin() * disc_r);

                // Scanner trail — neutral white
                let trail_steps = 12_usize;
                let trail_arc = 0.4_f32;
                for t in 0..trail_steps {
                    let frac = t as f32 / trail_steps as f32;
                    let trail_angle = angle - trail_arc * frac;
                    let trail_pos = center
                        + egui::vec2(trail_angle.cos() * disc_r, trail_angle.sin() * disc_r);
                    let trail_alpha = ((1.0 - frac) * 140.0 * drive) as u8;
                    let trail_size = radius * (0.010 + 0.010 * (1.0 - frac));
                    painter.circle_filled(
                        trail_pos,
                        trail_size,
                        Color32::from_rgba_unmultiplied(200, 200, 210, trail_alpha),
                    );
                }

                // Scanner glow halo — soft lavender
                for g in 0..5_usize {
                    let glow_r = radius * 0.04 + g as f32 * radius * 0.018;
                    let alpha = (60.0 * drive * (1.0 - g as f32 / 5.0)) as u8;
                    painter.circle_filled(
                        scanner_pos,
                        glow_r,
                        Color32::from_rgba_unmultiplied(180, 170, 230, alpha),
                    );
                }

                // Scanner dot — pale lavender-white
                painter.circle_filled(
                    scanner_pos,
                    radius * 0.032,
                    Color32::from_rgba_unmultiplied(230, 225, 250, (drive * 0.95 * 255.0) as u8),
                );

                // Line from scanner to center — lavender tint
                let line_alpha =
                    (14.0 + 9.0 * (phase * TAU * 8.0 + i as f32).sin()) * drive;
                painter.line_segment(
                    [scanner_pos, center],
                    Stroke::new(
                        0.5,
                        Color32::from_rgba_unmultiplied(167, 139, 250, line_alpha as u8),
                    ),
                );
            }

            // ── 3. Inward-flowing data streams ────────────────────────────────
            let num_streams = 8_usize;
            for i in 0..num_streams {
                let stream_t =
                    (phase * 1.5 + i as f32 * 0.3 / num_streams as f32).rem_euclid(1.0);
                let stream_radius = disc_r * (1.0 - stream_t);
                let angle = i as f32 * TAU / num_streams as f32;

                let stream_pos =
                    center + egui::vec2(angle.cos() * stream_radius, angle.sin() * stream_radius);

                let dot_size = radius * (0.012 + 0.018 * stream_t);
                let alpha = (0.3 + 0.7 * (1.0 - stream_t)).min(1.0) * drive;

                // Glow around data dot
                painter.circle_filled(
                    stream_pos,
                    dot_size * 3.0,
                    Color32::from_rgba_unmultiplied(167, 139, 250, (alpha * 40.0) as u8),
                );
                // Data dot
                painter.circle_filled(
                    stream_pos,
                    dot_size,
                    Color32::from_rgba_unmultiplied(255, 255, 255, (alpha * 200.0) as u8),
                );
            }

            // ── 4. Sector sweep — lavender ────────────────────────────────────
            let sweep_angle = spinner * 0.3;
            let sweep_steps = 15_usize;
            let sweep_arc = 0.5_f32;
            for s in 0..sweep_steps {
                let frac = s as f32 / sweep_steps as f32;
                let a = sweep_angle + sweep_arc * frac;
                let end_pos =
                    center + egui::vec2(a.cos() * (disc_r - 2.0), a.sin() * (disc_r - 2.0));
                let alpha = ((1.0 - frac) * 15.0 * drive) as u8;
                painter.line_segment(
                    [center, end_pos],
                    Stroke::new(
                        0.5,
                        Color32::from_rgba_unmultiplied(167, 139, 250, alpha),
                    ),
                );
            }
        } else {
            // ── Idle: static center dot ───────────────────────────────────────
            for i in (0..6_usize).rev() {
                let r = radius * 0.10 * (1.0 - i as f32 * 0.05);
                let brightness = 60 + (i * 10) as u8;
                let alpha = (150u8).saturating_sub((i * 20) as u8);
                painter.circle_filled(
                    center,
                    r,
                    Color32::from_rgba_unmultiplied(brightness, brightness, brightness, alpha),
                );
            }
        }
    }

    /// Conveyor belt compression: packets travel on a belt, are squeezed
    /// between two counter-rotating shredder rollers, and emerge as small
    /// compressed CHD packages on the output belt.
    fn draw_compression_disc(
        &self,
        painter: &egui::Painter,
        center: Pos2,
        radius: f32,
        accent: Color32,
        visuals: &Visuals,
    ) {
        let mul = |c: Color32, f: f32| c.linear_multiply(f.clamp(0.0, 1.0));
        let bg = visuals.extreme_bg_color;
        let drive = self.compress.drive;
        let cycle = self.compress.phase_smooth;

        // ── Outer panel ───────────────────────────────────────────────────────
        let w = radius * 2.0;
        let h = radius * 1.3;
        let panel = egui::Rect::from_center_size(center, egui::vec2(w, h));
        painter.rect(
            panel,
            egui::Rounding::same(radius * 0.13),
            mul(bg, 0.94),
            Stroke::new(radius * 0.018, mul(accent, 0.32)),
        );

        // ── Layout constants ──────────────────────────────────────────────────
        let belt_y = center.y + radius * 0.26; // top surface of belts
        let belt_h = radius * 0.07; // belt thickness
        let belt_left = panel.left() + radius * 0.14;
        let belt_right = panel.right() - radius * 0.14;

        let roller_cx = center.x; // x-center of roller pair
        let roller_r = radius * 0.20; // roller radius
        let _roller_gap = radius * 0.10; // gap between rollers (squeeze zone)

        let top_roller_y = belt_y - radius * 0.18 - roller_r;
        let bottom_roller_y = belt_y + belt_h + radius * 0.06 + roller_r;
        let squeeze_y = (top_roller_y + roller_r + bottom_roller_y - roller_r) * 0.5;

        let input_belt_end = roller_cx - roller_r - radius * 0.12;
        let output_belt_start = roller_cx + roller_r + radius * 0.12;

        let _belt_speed = drive; // 0..1

        // ── Input conveyor belt ───────────────────────────────────────────────
        painter.rect_filled(
            egui::Rect::from_min_size(
                Pos2::new(belt_left, belt_y),
                egui::vec2(input_belt_end - belt_left, belt_h),
            ),
            egui::Rounding::ZERO,
            mul(accent, 0.06),
        );
        painter.line_segment(
            [Pos2::new(belt_left, belt_y), Pos2::new(input_belt_end, belt_y)],
            Stroke::new(radius * 0.014, mul(accent, 0.35)),
        );
        painter.line_segment(
            [
                Pos2::new(belt_left, belt_y + belt_h),
                Pos2::new(input_belt_end, belt_y + belt_h),
            ],
            Stroke::new(radius * 0.014, mul(accent, 0.35)),
        );

        // ── Output conveyor belt ──────────────────────────────────────────────
        painter.rect_filled(
            egui::Rect::from_min_size(
                Pos2::new(output_belt_start, belt_y),
                egui::vec2(belt_right - output_belt_start, belt_h),
            ),
            egui::Rounding::ZERO,
            mul(accent, 0.06),
        );
        painter.line_segment(
            [Pos2::new(output_belt_start, belt_y), Pos2::new(belt_right, belt_y)],
            Stroke::new(radius * 0.014, mul(accent, 0.35)),
        );
        painter.line_segment(
            [
                Pos2::new(output_belt_start, belt_y + belt_h),
                Pos2::new(belt_right, belt_y + belt_h),
            ],
            Stroke::new(radius * 0.014, mul(accent, 0.35)),
        );

        // ── Belt tick marks (animated) ────────────────────────────────────────
        let tick_spacing = radius * 0.12;
        let tick_offset = (cycle * tick_spacing * 8.0).rem_euclid(tick_spacing);
        // Input belt ticks
        let mut tx = belt_left + tick_offset;
        while tx < input_belt_end {
            painter.line_segment(
                [Pos2::new(tx, belt_y + 1.0), Pos2::new(tx, belt_y + belt_h - 1.0)],
                Stroke::new(radius * 0.008, mul(accent, 0.12)),
            );
            tx += tick_spacing;
        }
        // Output belt ticks
        tx = output_belt_start + tick_offset;
        while tx < belt_right {
            painter.line_segment(
                [Pos2::new(tx, belt_y + 1.0), Pos2::new(tx, belt_y + belt_h - 1.0)],
                Stroke::new(radius * 0.008, mul(accent, 0.12)),
            );
            tx += tick_spacing;
        }

        // ── Belt end wheels ───────────────────────────────────────────────────
        let wheel_r = radius * 0.045;
        let wheel_cy = belt_y + belt_h * 0.5;
        for wx in [belt_left, belt_right] {
            painter.circle_filled(Pos2::new(wx, wheel_cy), wheel_r, mul(bg, 0.85));
            painter.circle_stroke(
                Pos2::new(wx, wheel_cy),
                wheel_r,
                Stroke::new(radius * 0.014, mul(accent, 0.40)),
            );
        }

        // ── Roller housing / frame ────────────────────────────────────────────
        let housing = egui::Rect::from_min_max(
            Pos2::new(roller_cx - roller_r - radius * 0.08, top_roller_y - roller_r - radius * 0.06),
            Pos2::new(roller_cx + roller_r + radius * 0.08, bottom_roller_y + roller_r + radius * 0.06),
        );
        painter.rect(
            housing,
            egui::Rounding::same(radius * 0.04),
            mul(bg, 0.92),
            Stroke::new(radius * 0.010, mul(accent, 0.18)),
        );

        // ── Shredder rollers ──────────────────────────────────────────────────
        let roller_angle = cycle * TAU * 3.0; // rotation based on cycle

        // Helper: draw one roller
        let draw_roller = |cx: f32, cy: f32, angle: f32, direction: f32| {
            // Roller body
            painter.circle_filled(Pos2::new(cx, cy), roller_r, mul(bg, 0.88));
            painter.circle_stroke(
                Pos2::new(cx, cy),
                roller_r,
                Stroke::new(radius * 0.018, mul(accent, 0.45)),
            );

            // Teeth / grip marks
            let num_teeth = 10_usize;
            for t in 0..num_teeth {
                let a = angle * direction + t as f32 * TAU / num_teeth as f32;
                let inner_r = roller_r * 0.55;
                let outer_r = roller_r * 0.92;
                let x1 = cx + a.cos() * inner_r;
                let y1 = cy + a.sin() * inner_r;
                let x2 = cx + a.cos() * outer_r;
                let y2 = cy + a.sin() * outer_r;

                let tooth_color = if drive > 0.01 {
                    Color32::from_rgba_unmultiplied(167, 139, 250, (drive * 0.35 * 255.0) as u8)
                } else {
                    mul(accent, 0.15)
                };
                painter.line_segment(
                    [Pos2::new(x1, y1), Pos2::new(x2, y2)],
                    Stroke::new(radius * 0.022, tooth_color),
                );
            }

            // Center bolt
            painter.circle_filled(Pos2::new(cx, cy), radius * 0.035, mul(accent, 0.35));

            // Rotation indicator dot (visible when active)
            if drive > 0.01 {
                let dot_a = angle * direction;
                let dot_r = roller_r + radius * 0.04;
                painter.circle_filled(
                    Pos2::new(cx + dot_a.cos() * dot_r, cy + dot_a.sin() * dot_r),
                    radius * 0.018,
                    Color32::from_rgba_unmultiplied(167, 139, 250, (drive * 0.3 * 255.0) as u8),
                );
            }
        };

        // Top roller (counter-clockwise → pulls right)
        draw_roller(roller_cx, top_roller_y, roller_angle, -1.0);
        // Bottom roller (clockwise → pulls right)
        draw_roller(roller_cx, bottom_roller_y, roller_angle, 1.0);

        // ── Friction glow between rollers when active ─────────────────────────
        if drive > 0.01 {
            let glow_rect = egui::Rect::from_center_size(
                Pos2::new(roller_cx, squeeze_y),
                egui::vec2(roller_r * 2.0 + radius * 0.08, radius * 0.14),
            );
            painter.rect_filled(
                glow_rect,
                egui::Rounding::same(radius * 0.04),
                Color32::from_rgba_unmultiplied(147, 119, 230, (drive * 0.10 * 255.0) as u8),
            );
        }

        // ── Packages ──────────────────────────────────────────────────────────
        let pkg_spacing = radius * 0.58;
        let belt_len = belt_right - belt_left;
        let entry_x = roller_cx - roller_r - radius * 0.04;
        let exit_x = roller_cx + roller_r + radius * 0.04;
        let squeeze_width = exit_x - entry_x;

        // Raw package dimensions
        let raw_w = radius * 0.22;
        let raw_h = radius * 0.20;
        // Compressed package dimensions
        let chd_w = radius * 0.13;
        let chd_h = radius * 0.08;

        // Neutral gray (input) and lavender (output) colors
        let color_raw = Color32::from_rgba_unmultiplied(152, 152, 157, 170);
        let color_chd = Color32::from_rgba_unmultiplied(167, 139, 250, 180);

        if drive > 0.01 {
            let num_slots = ((belt_len + pkg_spacing) / pkg_spacing) as usize + 2;
            let wrap_len = num_slots as f32 * pkg_spacing;
            // Use wrap_len as the scroll period so that cycle=0 and cycle=1
            // produce identical positions — no visible jump on wrap-around.
            let scroll_offset = cycle * wrap_len;

            for i in 0..num_slots {
                let raw_pos =
                    ((i as f32 * pkg_spacing + scroll_offset) % wrap_len + wrap_len) % wrap_len;
                let pkg_x = belt_left - radius * 0.2 + raw_pos;

                if pkg_x < belt_left - raw_w || pkg_x > belt_right + raw_w {
                    continue;
                }

                // Determine squeeze state
                let (pkg_w, pkg_h, pkg_y, fill_color);
                if pkg_x > entry_x && pkg_x < exit_x {
                    // Inside the roller zone — squeezing
                    let local_progress = (pkg_x - entry_x) / squeeze_width;
                    let squeeze = (local_progress * std::f32::consts::PI).sin();

                    // Deform: gets wider and flatter
                    let mut pw = raw_w + squeeze * (raw_w * 0.5);
                    let mut ph = raw_h - squeeze * (raw_h * 0.65);

                    // After center, blend toward compressed size
                    if local_progress > 0.5 {
                        let blend = ((local_progress - 0.5) * 2.0).powi(2);
                        pw = pw + (chd_w - pw) * blend;
                        ph = ph + (chd_h - ph) * blend;
                    }

                    pkg_w = pw;
                    pkg_h = ph;
                    pkg_y = squeeze_y - ph * 0.5;

                    // Color interpolation: gray → bright → lavender
                    if local_progress < 0.5 {
                        let heat = local_progress * 2.0;
                        fill_color = Color32::from_rgba_unmultiplied(
                            (152.0 + heat * 48.0) as u8,
                            (152.0 - heat * 30.0) as u8,
                            (157.0 + heat * 60.0) as u8,
                            220,
                        );
                    } else {
                        let cool = (local_progress - 0.5) * 2.0;
                        fill_color = Color32::from_rgba_unmultiplied(
                            (200.0 - cool * 33.0) as u8,
                            (122.0 + cool * 17.0) as u8,
                            (217.0 + cool * 33.0) as u8,
                            210,
                        );
                    }

                    // Friction sparks (deterministic, based on index + cycle)
                    if squeeze > 0.5 {
                        for sp in 0..3_usize {
                            let seed = (i * 7 + sp * 13) as f32 * 0.1 + cycle * 17.0;
                            let sx = pkg_x + (seed.sin() * 0.5) * pkg_w;
                            let sy = squeeze_y + (seed.cos() * 0.5) * radius * 0.05;
                            let spark_alpha = (drive * (0.3 + (seed * 3.7).sin().abs() * 0.4) * 255.0) as u8;
                            painter.circle_filled(
                                Pos2::new(sx, sy),
                                radius * 0.010 + (seed * 2.3).sin().abs() * radius * 0.008,
                                Color32::from_rgba_unmultiplied(
                                    (190.0 + (seed * 5.1).sin().abs() * 30.0) as u8,
                                    (170.0 + (seed * 5.1).sin().abs() * 20.0) as u8,
                                    250,
                                    spark_alpha,
                                ),
                            );
                        }
                    }
                } else if pkg_x >= exit_x {
                    // Past rollers — fully compressed
                    pkg_w = chd_w;
                    pkg_h = chd_h;
                    pkg_y = belt_y - chd_h;
                    fill_color = color_chd;
                } else {
                    // Before rollers — raw input
                    pkg_w = raw_w;
                    pkg_h = raw_h;
                    pkg_y = belt_y - raw_h;
                    fill_color = color_raw;
                }

                // Shadow
                painter.rect_filled(
                    egui::Rect::from_min_size(
                        Pos2::new(pkg_x - pkg_w * 0.5 + 1.5, pkg_y + 1.5),
                        egui::vec2(pkg_w, pkg_h),
                    ),
                    egui::Rounding::same(radius * 0.02),
                    Color32::from_black_alpha(30),
                );

                // Package body
                painter.rect_filled(
                    egui::Rect::from_min_size(
                        Pos2::new(pkg_x - pkg_w * 0.5, pkg_y),
                        egui::vec2(pkg_w, pkg_h),
                    ),
                    egui::Rounding::same(radius * 0.02),
                    fill_color,
                );

                // Detail lines
                let detail_alpha = if pkg_x >= exit_x { 0.2 } else { 0.12 };
                if pkg_h > radius * 0.04 {
                    painter.line_segment(
                        [
                            Pos2::new(pkg_x - pkg_w * 0.5, pkg_y + pkg_h * 0.5),
                            Pos2::new(pkg_x + pkg_w * 0.5, pkg_y + pkg_h * 0.5),
                        ],
                        Stroke::new(0.5, Color32::from_white_alpha((detail_alpha * 255.0) as u8)),
                    );
                }
                painter.line_segment(
                    [
                        Pos2::new(pkg_x, pkg_y),
                        Pos2::new(pkg_x, pkg_y + pkg_h),
                    ],
                    Stroke::new(0.5, Color32::from_white_alpha((detail_alpha * 255.0) as u8)),
                );

                // Checkmark for compressed packages
                if pkg_x >= exit_x {
                    let font_size = (radius * 0.08).clamp(6.0, 14.0);
                    painter.text(
                        Pos2::new(pkg_x, pkg_y + pkg_h * 0.5),
                        egui::Align2::CENTER_CENTER,
                        "\u{2713}",
                        egui::FontId::monospace(font_size),
                        Color32::from_white_alpha(115),
                    );
                }
            }
        } else {
            // Idle: one static package on the input belt
            let idle_x = belt_left + (input_belt_end - belt_left) * 0.4;
            painter.rect_filled(
                egui::Rect::from_min_size(
                    Pos2::new(idle_x - raw_w * 0.5, belt_y - raw_h),
                    egui::vec2(raw_w, raw_h),
                ),
                egui::Rounding::same(radius * 0.02),
                mul(accent, 0.20),
            );
        }

        // ── Labels ────────────────────────────────────────────────────────────
        let label_y = belt_y + belt_h + radius * 0.14;
        let label_size = (radius * 0.10).clamp(6.0, 11.0);
        let label_color = mul(accent, 0.30);
        painter.text(
            Pos2::new((belt_left + input_belt_end) * 0.5, label_y),
            egui::Align2::CENTER_CENTER,
            "INPUT",
            egui::FontId::monospace(label_size),
            label_color,
        );
        painter.text(
            Pos2::new(roller_cx, top_roller_y - roller_r - radius * 0.10),
            egui::Align2::CENTER_CENTER,
            "ROLLERS",
            egui::FontId::monospace(label_size),
            label_color,
        );
        painter.text(
            Pos2::new((output_belt_start + belt_right) * 0.5, label_y),
            egui::Align2::CENTER_CENTER,
            "OUTPUT",
            egui::FontId::monospace(label_size),
            label_color,
        );

        // ── Direction arrows when active ──────────────────────────────────────
        if drive > 0.01 {
            let arrow_y = label_y + radius * 0.10;
            let arrow_color = mul(accent, 0.15);
            let arrow_stroke = Stroke::new(radius * 0.010, arrow_color);

            // Input arrow →
            let ax1 = belt_left + radius * 0.10;
            let ax2 = input_belt_end - radius * 0.10;
            painter.line_segment([Pos2::new(ax1, arrow_y), Pos2::new(ax2, arrow_y)], arrow_stroke);
            painter.line_segment(
                [Pos2::new(ax2 - radius * 0.04, arrow_y - radius * 0.025), Pos2::new(ax2, arrow_y)],
                arrow_stroke,
            );
            painter.line_segment(
                [Pos2::new(ax2 - radius * 0.04, arrow_y + radius * 0.025), Pos2::new(ax2, arrow_y)],
                arrow_stroke,
            );

            // Output arrow →
            let bx1 = output_belt_start + radius * 0.10;
            let bx2 = belt_right - radius * 0.10;
            painter.line_segment([Pos2::new(bx1, arrow_y), Pos2::new(bx2, arrow_y)], arrow_stroke);
            painter.line_segment(
                [Pos2::new(bx2 - radius * 0.04, arrow_y - radius * 0.025), Pos2::new(bx2, arrow_y)],
                arrow_stroke,
            );
            painter.line_segment(
                [Pos2::new(bx2 - radius * 0.04, arrow_y + radius * 0.025), Pos2::new(bx2, arrow_y)],
                arrow_stroke,
            );
        }

        // ── Size comparison indicators (RAW vs CHD) ───────────────────────────
        if drive > 0.01 {
            let ind_y = belt_y - raw_h - radius * 0.22;
            let dash_color_raw = Color32::from_rgba_unmultiplied(152, 152, 157, (drive * 0.35 * 255.0) as u8);
            let dash_color_chd = Color32::from_rgba_unmultiplied(167, 139, 250, (drive * 0.35 * 255.0) as u8);

            // RAW size indicator (dashed outline)
            let raw_ind = egui::Rect::from_min_size(
                Pos2::new(belt_left + radius * 0.14, ind_y),
                egui::vec2(raw_w, raw_h),
            );
            painter.rect(
                raw_ind,
                egui::Rounding::same(1.0),
                Color32::TRANSPARENT,
                Stroke::new(radius * 0.010, dash_color_raw),
            );
            painter.text(
                Pos2::new(raw_ind.center().x, raw_ind.top() - radius * 0.04),
                egui::Align2::CENTER_CENTER,
                "RAW",
                egui::FontId::monospace(label_size),
                dash_color_raw,
            );

            // CHD size indicator (dashed outline)
            let chd_ind = egui::Rect::from_min_size(
                Pos2::new(belt_right - radius * 0.28, ind_y + (raw_h - chd_h)),
                egui::vec2(chd_w, chd_h),
            );
            painter.rect(
                chd_ind,
                egui::Rounding::same(1.0),
                Color32::TRANSPARENT,
                Stroke::new(radius * 0.010, dash_color_chd),
            );
            painter.text(
                Pos2::new(chd_ind.center().x, chd_ind.top() - radius * 0.04),
                egui::Align2::CENTER_CENTER,
                "CHD",
                egui::FontId::monospace(label_size),
                dash_color_chd,
            );
        }
    }

    /// Document verification scanner: a document with fake content lines is
    /// scanned by a glowing beam that sweeps up and down. Sector status dots,
    /// a progress indicator strip, and CRC readout show verification state.
    fn draw_verify_scanner(
        &self,
        painter: &egui::Painter,
        center: Pos2,
        radius: f32,
        accent: Color32,
        visuals: &Visuals,
    ) {
        let mul = |c: Color32, f: f32| c.linear_multiply(f.clamp(0.0, 1.0));
        let bg = visuals.extreme_bg_color;
        let drive = self.verify.drive;
        let phase = self.verify.phase_smooth;

        let _scan_color = Color32::from_rgb(167, 139, 250);
        let _ok_color = Color32::from_rgb(34, 197, 94);

        // ── Document body ─────────────────────────────────────────────────────
        let doc_w = radius * 1.20;
        let doc_h = radius * 1.60;
        let doc_x = center.x - doc_w * 0.5;
        let doc_y = center.y - radius * 0.52;
        let corner_fold = radius * 0.15;

        // Shadow
        painter.rect_filled(
            egui::Rect::from_min_size(
                Pos2::new(doc_x + 3.0, doc_y + 3.0),
                egui::vec2(doc_w, doc_h),
            ),
            egui::Rounding::same(radius * 0.04),
            Color32::from_black_alpha(55),
        );

        // Main document rectangle (with corner fold implied by overlay)
        let doc_rect =
            egui::Rect::from_min_size(Pos2::new(doc_x, doc_y), egui::vec2(doc_w, doc_h));
        painter.rect(
            doc_rect,
            egui::Rounding::same(radius * 0.04),
            mul(bg, 0.95),
            Stroke::new(radius * 0.012, mul(accent, 0.30)),
        );

        // Corner fold triangle
        let fold_pts = vec![
            Pos2::new(doc_x + doc_w - corner_fold, doc_y),
            Pos2::new(doc_x + doc_w, doc_y + corner_fold),
            Pos2::new(doc_x + doc_w - corner_fold, doc_y + corner_fold),
        ];
        painter.add(egui::Shape::convex_polygon(
            fold_pts,
            mul(bg, 0.85),
            Stroke::new(radius * 0.008, mul(accent, 0.18)),
        ));
        // Fold edge lines
        painter.line_segment(
            [
                Pos2::new(doc_x + doc_w - corner_fold, doc_y),
                Pos2::new(doc_x + doc_w - corner_fold, doc_y + corner_fold),
            ],
            Stroke::new(radius * 0.008, mul(accent, 0.15)),
        );

        // ── Document content (fake text lines) ───────────────────────────────
        let content_x = doc_x + radius * 0.14;
        let content_w = doc_w - radius * 0.28;
        let line_h = radius * 0.045;
        let line_gap = radius * 0.080;
        let start_y = doc_y + radius * 0.18;
        let text_color = mul(accent, 0.12);
        let text_color_dim = mul(accent, 0.07);

        // Title block
        painter.rect_filled(
            egui::Rect::from_min_size(
                Pos2::new(content_x, start_y),
                egui::vec2(content_w * 0.6, line_h * 1.3),
            ),
            egui::Rounding::same(1.0),
            text_color,
        );
        // Subtitle
        painter.rect_filled(
            egui::Rect::from_min_size(
                Pos2::new(content_x, start_y + line_h * 2.2),
                egui::vec2(content_w * 0.4, line_h * 0.8),
            ),
            egui::Rounding::same(1.0),
            text_color_dim,
        );
        // Separator
        painter.line_segment(
            [
                Pos2::new(content_x, start_y + line_h * 3.8),
                Pos2::new(content_x + content_w, start_y + line_h * 3.8),
            ],
            Stroke::new(0.5, mul(accent, 0.08)),
        );

        // Text lines with varying lengths
        let line_lengths: [f32; 11] = [1.0, 0.85, 0.92, 0.7, 1.0, 0.88, 0.6, 0.95, 0.75, 0.82, 0.5];
        let lines_start_y = start_y + line_h * 4.6;
        for (i, &len_frac) in line_lengths.iter().enumerate() {
            let ly = lines_start_y + i as f32 * line_gap;
            if ly + line_h > doc_y + doc_h - radius * 0.12 {
                break;
            }
            painter.rect_filled(
                egui::Rect::from_min_size(
                    Pos2::new(content_x, ly),
                    egui::vec2(content_w * len_frac, line_h),
                ),
                egui::Rounding::same(1.0),
                text_color_dim,
            );
        }

        // Small data table area
        let table_y = lines_start_y + 5.0 * line_gap + line_h * 1.5;
        if table_y + line_gap * 3.0 < doc_y + doc_h - radius * 0.10 {
            let col_w = content_w / 3.0 - radius * 0.02;
            for row in 0..3_usize {
                for col in 0..3_usize {
                    painter.rect_filled(
                        egui::Rect::from_min_size(
                            Pos2::new(
                                content_x + col as f32 * (content_w / 3.0 + radius * 0.01),
                                table_y + row as f32 * (line_h + radius * 0.03),
                            ),
                            egui::vec2(col_w, line_h),
                        ),
                        egui::Rounding::same(1.0),
                        mul(accent, 0.05),
                    );
                }
            }
        }

        // ── Scan line (active) ────────────────────────────────────────────────
        if drive > 0.01 {
            // Ping-pong scan position
            let scan_cycle = phase * 2.0; // full up+down per phase cycle
            let scan_dir = (scan_cycle.floor() as i32) % 2 == 0;
            let scan_frac = scan_cycle.fract();
            let scan_progress = if scan_dir { scan_frac } else { 1.0 - scan_frac };
            let scan_y = egui::lerp(
                doc_y + radius * 0.06..=doc_y + doc_h - radius * 0.06,
                ease_in_out_smooth(scan_progress),
            );

            // Wide glow band — soft lavender
            let glow_half = radius * 0.22;
            painter.rect_filled(
                egui::Rect::from_min_max(
                    Pos2::new(doc_x, scan_y - glow_half),
                    Pos2::new(doc_x + doc_w, scan_y + glow_half),
                ),
                egui::Rounding::ZERO,
                Color32::from_rgba_unmultiplied(167, 139, 250, (drive * 0.07 * 255.0) as u8),
            );

            // Bright scan line — lavender
            painter.line_segment(
                [
                    Pos2::new(doc_x + radius * 0.04, scan_y),
                    Pos2::new(doc_x + doc_w - radius * 0.04, scan_y),
                ],
                Stroke::new(
                    radius * 0.022,
                    Color32::from_rgba_unmultiplied(167, 139, 250, (drive * 0.78 * 255.0) as u8),
                ),
            );

            // Scan line end dots — pale lavender
            for sx in [doc_x + radius * 0.04, doc_x + doc_w - radius * 0.04] {
                painter.circle_filled(
                    Pos2::new(sx, scan_y),
                    radius * 0.022,
                    Color32::from_rgba_unmultiplied(220, 215, 245, (drive * 0.88 * 255.0) as u8),
                );
            }

            // Highlight content lines — soft sage tint
            for (i, &len_frac) in line_lengths.iter().enumerate() {
                let ly = lines_start_y + i as f32 * line_gap + line_h * 0.5;
                if ly + line_h > doc_y + doc_h - radius * 0.12 {
                    break;
                }
                let dist = (ly - scan_y).abs();
                if dist < radius * 0.12 {
                    let highlight = (1.0 - dist / (radius * 0.12)) * 0.25 * drive;
                    painter.rect_filled(
                        egui::Rect::from_min_size(
                            Pos2::new(content_x, lines_start_y + i as f32 * line_gap),
                            egui::vec2(content_w * len_frac, line_h),
                        ),
                        egui::Rounding::same(1.0),
                        Color32::from_rgba_unmultiplied(167, 139, 250, (highlight * 255.0) as u8),
                    );
                }
            }

            // ── Progress indicator strip (right of document) ──────────────────
            let ind_x = doc_x + doc_w + radius * 0.12;
            let ind_w = radius * 0.04;

            // Track
            painter.rect_filled(
                egui::Rect::from_min_size(
                    Pos2::new(ind_x, doc_y),
                    egui::vec2(ind_w, doc_h),
                ),
                egui::Rounding::same(radius * 0.02),
                mul(accent, 0.06),
            );

            // Scanned portion (green → cyan fill)
            let scanned_h = scan_progress * doc_h;
            painter.rect_filled(
                egui::Rect::from_min_size(
                    Pos2::new(ind_x, doc_y),
                    egui::vec2(ind_w, scanned_h),
                ),
                egui::Rounding::same(radius * 0.02),
                Color32::from_rgba_unmultiplied(147, 119, 230, (drive * 0.45 * 255.0) as u8),
            );

            // Position marker
            painter.rect_filled(
                egui::Rect::from_center_size(
                    Pos2::new(ind_x + ind_w * 0.5, doc_y + scanned_h),
                    egui::vec2(ind_w + radius * 0.03, radius * 0.035),
                ),
                egui::Rounding::same(radius * 0.01),
                Color32::from_rgba_unmultiplied(180, 160, 255, (drive * 0.80 * 255.0) as u8),
            );

            // ── Status readout (below document) ──────────────────────────────
            let status_y = doc_y + doc_h + radius * 0.14;
            let font_sm = (radius * 0.085).clamp(6.0, 11.0);
            let current_sector = (scan_progress * 16.0).floor() as usize;

            // Sector info — lavender
            painter.text(
                Pos2::new(doc_x, status_y),
                egui::Align2::LEFT_CENTER,
                format!("SECTOR {:02}/16", current_sector.min(16)),
                egui::FontId::monospace(font_sm),
                Color32::from_rgba_unmultiplied(167, 139, 250, (drive * 0.58 * 255.0) as u8),
            );

            // Pseudo CRC (deterministic from phase + sector)
            let crc_seed = (phase * 8.0).floor() as u32 ^ (current_sector as u32 * 0x1F);
            let crc_val = crc_seed & 0xFFFF;
            painter.text(
                Pos2::new(doc_x, status_y + radius * 0.12),
                egui::Align2::LEFT_CENTER,
                format!("CRC: 0x{:04X}", crc_val),
                egui::FontId::monospace(font_sm),
                mul(accent, 0.25 * drive),
            );

            // Status dots (16 sectors)
            let dot_r = radius * 0.018;
            let dot_spacing = radius * 0.058;
            let dots_start_x = doc_x + doc_w - 16.0 * dot_spacing;
            let dot_y = status_y + radius * 0.22;
            for d in 0..16_usize {
                let dx = dots_start_x + d as f32 * dot_spacing;
                let verified = d < current_sector;
                let dot_color = if verified {
                    Color32::from_rgba_unmultiplied(34, 197, 94, (drive * 0.55 * 255.0) as u8)
                } else {
                    mul(accent, 0.10)
                };
                painter.circle_filled(Pos2::new(dx, dot_y), dot_r, dot_color);
            }

            // Integrity label
            painter.text(
                Pos2::new(doc_x + doc_w, dot_y + radius * 0.10),
                egui::Align2::RIGHT_CENTER,
                "INTEGRITY OK",
                egui::FontId::monospace(font_sm),
                Color32::from_rgba_unmultiplied(34, 197, 94, (drive * 0.45 * 255.0) as u8),
            );
        } else {
            // ── Idle state label ──────────────────────────────────────────────
            let status_y = doc_y + doc_h + radius * 0.16;
            let font_sm = (radius * 0.085).clamp(6.0, 11.0);
            painter.text(
                Pos2::new(center.x, status_y),
                egui::Align2::CENTER_CENTER,
                "AWAITING VERIFICATION",
                egui::FontId::monospace(font_sm),
                mul(accent, 0.22),
            );
        }
    }

    /// Terminal-style hash computation: a terminal window shows a running
    /// sha256sum command with block-by-block hash output, a flickering active
    /// line, hex character rain on the sides, and a master hash accumulator.
    fn draw_hash_pulses(
        &self,
        painter: &egui::Painter,
        center: Pos2,
        radius: f32,
        accent: Color32,
        visuals: &Visuals,
    ) {
        let mul = |c: Color32, f: f32| c.linear_multiply(f.clamp(0.0, 1.0));
        let bg = visuals.extreme_bg_color;
        let drive = self.hash.drive;
        let phase = self.hash.phase_smooth;

        // Colour palette — soft neutral theme for hash stage
        let amber = |a: f32| Color32::from_rgba_unmultiplied(167, 139, 250, (a * 255.0) as u8);  // lavender prompt
        let cyan_c = |a: f32| Color32::from_rgba_unmultiplied(152, 152, 157, (a * 255.0) as u8); // muted gray labels
        let dim_white = |a: f32| Color32::from_rgba_unmultiplied(245, 245, 247, (a * 255.0) as u8); // near-white text
        let ok_green = |a: f32| Color32::from_rgba_unmultiplied(34, 197, 94, (a * 255.0) as u8); // green OK

        // Pseudo-random hash generator (LCG)
        let gen_hash = |seed: u32, length: usize| -> String {
            let chars = b"0123456789abcdef";
            let mut s = seed;
            let mut out = String::with_capacity(length);
            for _ in 0..length {
                s = s.wrapping_mul(1103515245).wrapping_add(12345) & 0x7FFFFFFF;
                out.push(chars[(s % 16) as usize] as char);
            }
            out
        };

        // ── Terminal frame ────────────────────────────────────────────────────
        let term_w = radius * 2.0;
        let term_h = radius * 1.70;
        let term_x = center.x - term_w * 0.5;
        let term_y = center.y - term_h * 0.5;

        // Frame background + border
        let term_rect =
            egui::Rect::from_min_size(Pos2::new(term_x, term_y), egui::vec2(term_w, term_h));
        painter.rect(
            term_rect,
            egui::Rounding::same(radius * 0.06),
            mul(bg, 0.92),
            Stroke::new(radius * 0.010, mul(accent, 0.18)),
        );

        // Title bar
        let title_h = radius * 0.16;
        let title_rect = egui::Rect::from_min_size(
            Pos2::new(term_x, term_y),
            egui::vec2(term_w, title_h),
        );
        painter.rect_filled(
            title_rect,
            egui::Rounding {
                nw: radius * 0.06,
                ne: radius * 0.06,
                sw: 0.0,
                se: 0.0,
            },
            mul(accent, 0.05),
        );
        // Title bar separator
        painter.line_segment(
            [
                Pos2::new(term_x, term_y + title_h),
                Pos2::new(term_x + term_w, term_y + title_h),
            ],
            Stroke::new(0.5, mul(accent, 0.12)),
        );

        // Traffic-light dots
        let dot_r = radius * 0.028;
        let dot_colors = [
            Color32::from_rgba_unmultiplied(255, 80, 80, 128),
            Color32::from_rgba_unmultiplied(255, 180, 40, 128),
            Color32::from_rgba_unmultiplied(80, 200, 80, 128),
        ];
        for (d, &color) in dot_colors.iter().enumerate() {
            painter.circle_filled(
                Pos2::new(
                    term_x + radius * 0.10 + d as f32 * radius * 0.09,
                    term_y + title_h * 0.5,
                ),
                dot_r,
                color,
            );
        }

        // Title text
        let title_font = (radius * 0.065).clamp(6.0, 10.0);
        painter.text(
            Pos2::new(center.x, term_y + title_h * 0.5),
            egui::Align2::CENTER_CENTER,
            "sha256sum \u{2014} hash_engine",
            egui::FontId::monospace(title_font),
            mul(accent, 0.28),
        );

        // ── Content area ──────────────────────────────────────────────────────
        let content_x = term_x + radius * 0.12;
        let content_y = term_y + title_h + radius * 0.10;
        let line_h = radius * 0.105;
        let content_w = term_w - radius * 0.24;
        let font_sm = (radius * 0.075).clamp(6.0, 11.0);
        let _font_md = (radius * 0.082).clamp(7.0, 12.0);

        if drive > 0.01 {
            let mut line = 0_usize;
            let max_line_y = term_y + term_h - radius * 0.08;

            // Helper to check if we can draw a line
            let line_y = |l: usize| content_y + l as f32 * line_h;

            // ── Static header ─────────────────────────────────────────────
            if line_y(line) < max_line_y {
                painter.text(
                    Pos2::new(content_x, line_y(line)),
                    egui::Align2::LEFT_CENTER,
                    "$ sha256sum --verify disc.iso",
                    egui::FontId::monospace(font_sm),
                    amber(0.7 * drive),
                );
            }
            line += 2; // blank line

            if line_y(line) < max_line_y {
                painter.text(
                    Pos2::new(content_x, line_y(line)),
                    egui::Align2::LEFT_CENTER,
                    "[INFO] Loading input blocks...",
                    egui::FontId::monospace(font_sm),
                    mul(accent, 0.20 * drive),
                );
            }
            line += 1;
            if line_y(line) < max_line_y {
                painter.text(
                    Pos2::new(content_x, line_y(line)),
                    egui::Align2::LEFT_CENTER,
                    "[INFO] Block size: 2048 bytes",
                    egui::FontId::monospace(font_sm),
                    mul(accent, 0.20 * drive),
                );
            }
            line += 2; // blank line

            // ── Block hash lines ──────────────────────────────────────────
            let num_hashes = 8_usize;
            // Use phase to drive which hash is "current"
            let hash_cycle = (phase * (num_hashes + 3) as f32).floor() as usize;
            let current_active = hash_cycle % (num_hashes + 3);
            let batch = hash_cycle / (num_hashes + 3);

            for i in 0..num_hashes {
                let ly = line_y(line);
                if ly > max_line_y - radius * 0.40 {
                    break;
                }

                if i <= current_active && i < num_hashes {
                    let seed = (i as u32 * 7 + (phase * 30.0).floor() as u32 * 13) ^ 0xDEAD;
                    let hash_str = gen_hash(seed, 32);
                    let blk_num = i + batch * num_hashes;

                    // Block label
                    let label = format!("BLK {:04}", blk_num);
                    painter.text(
                        Pos2::new(content_x, ly),
                        egui::Align2::LEFT_CENTER,
                        &label,
                        egui::FontId::monospace(font_sm),
                        cyan_c(0.5 * drive),
                    );

                    // Hash value
                    painter.text(
                        Pos2::new(content_x + radius * 0.42, ly),
                        egui::Align2::LEFT_CENTER,
                        &hash_str,
                        egui::FontId::monospace(font_sm),
                        dim_white(0.55 * drive),
                    );

                    // OK status
                    painter.text(
                        Pos2::new(content_x + content_w, ly),
                        egui::Align2::RIGHT_CENTER,
                        "OK",
                        egui::FontId::monospace(font_sm),
                        ok_green(0.5 * drive),
                    );

                    // Active line highlight + flickering hash
                    if i == current_active {
                        painter.rect_filled(
                            egui::Rect::from_min_size(
                                Pos2::new(content_x - radius * 0.03, ly - line_h * 0.4),
                                egui::vec2(content_w + radius * 0.06, line_h * 0.85),
                            ),
                            egui::Rounding::same(radius * 0.02),
                            amber(0.04 * drive),
                        );

                        // Flickering computation hash (overwrites the static one)
                        let flicker_seed = (phase * 200.0).floor() as u32;
                        let flicker_hash = gen_hash(flicker_seed, 32);
                        let flicker_alpha =
                            0.3 + 0.3 * (phase * TAU * 20.0).sin();
                        painter.text(
                            Pos2::new(content_x + radius * 0.42, ly),
                            egui::Align2::LEFT_CENTER,
                            &flicker_hash,
                            egui::FontId::monospace(font_sm),
                            amber(flicker_alpha * drive),
                        );
                    }
                }
                line += 1;
            }

            // ── Bottom status area ────────────────────────────────────────
            line += 1;
            let status_y = line_y(line);
            if status_y < max_line_y {
                // Separator line
                painter.line_segment(
                    [
                        Pos2::new(content_x, status_y - line_h * 0.4),
                        Pos2::new(content_x + content_w, status_y - line_h * 0.4),
                    ],
                    Stroke::new(0.5, mul(accent, 0.08)),
                );

                // Block progress
                let total_blocks = 2048_u32;
                let processed = ((phase * 15.0).rem_euclid(1.0) * total_blocks as f32) as u32;
                let throughput_val = 42.5 + (phase * TAU * 3.0).sin() * 5.0;
                painter.text(
                    Pos2::new(content_x, status_y),
                    egui::Align2::LEFT_CENTER,
                    &format!("Blocks: {}/{}", processed, total_blocks),
                    egui::FontId::monospace(font_sm),
                    mul(accent, 0.20 * drive),
                );
                painter.text(
                    Pos2::new(content_x + content_w, status_y),
                    egui::Align2::RIGHT_CENTER,
                    &format!("{:.1} MB/s", throughput_val),
                    egui::FontId::monospace(font_sm),
                    mul(accent, 0.20 * drive),
                );

                // Master hash (two lines, bold amber)
                let master_seed = (phase * 5.0).floor() as u32;
                let master_hash = gen_hash(master_seed, 64);
                let hash_font = (radius * 0.082).clamp(7.0, 12.0);
                let hash_alpha = 0.6 + 0.2 * (phase * TAU * 8.0).sin();

                let line1 = &master_hash[..32];
                let line2 = &master_hash[32..];
                painter.text(
                    Pos2::new(content_x, status_y + line_h * 1.2),
                    egui::Align2::LEFT_CENTER,
                    line1,
                    egui::FontId::monospace(hash_font),
                    amber(hash_alpha * drive),
                );
                painter.text(
                    Pos2::new(content_x, status_y + line_h * 2.2),
                    egui::Align2::LEFT_CENTER,
                    line2,
                    egui::FontId::monospace(hash_font),
                    amber(hash_alpha * drive),
                );

                // Cursor blink
                let cursor_on = (phase * 5.0).floor() as i32 % 2 == 0;
                if cursor_on {
                    painter.rect_filled(
                        egui::Rect::from_min_size(
                            Pos2::new(
                                content_x + radius * 1.30,
                                status_y + line_h * 1.9,
                            ),
                            egui::vec2(radius * 0.05, line_h * 0.65),
                        ),
                        egui::Rounding::ZERO,
                        amber(0.6 * drive),
                    );
                }
            }

            // ── Falling hex rain (sides) ──────────────────────────────────
            let hex_chars = b"0123456789abcdef";
            let rain_font = (radius * 0.055).clamp(5.0, 9.0);
            let rain_rows = 20_usize;

            // Left side rain (amber)
            for col in 0..3_usize {
                let col_x = term_x + radius * 0.02 + col as f32 * radius * 0.04;
                for row in 0..rain_rows {
                    let char_time =
                        (phase * 4.0 + col as f32 * 0.7 + row as f32 * 0.15).rem_euclid(1.0);
                    let char_y =
                        term_y + title_h + row as f32 * (term_h - title_h) / rain_rows as f32;
                    let seed = (phase * 12.0 + col as f32 * 3.0 + row as f32 * 7.0)
                        .floor() as usize
                        % 16;
                    let alpha = (1.0 - char_time) * 0.12 * drive;
                    if alpha > 0.005 {
                        painter.text(
                            Pos2::new(col_x, char_y),
                            egui::Align2::LEFT_TOP,
                            std::str::from_utf8(&[hex_chars[seed]]).unwrap_or("0"),
                            egui::FontId::monospace(rain_font),
                            amber(alpha),
                        );
                    }
                }
            }
            // Right side rain (cyan)
            for col in 0..3_usize {
                let col_x = term_x + term_w - radius * 0.04 - col as f32 * radius * 0.04;
                for row in 0..rain_rows {
                    let char_time =
                        (phase * 3.5 + col as f32 * 0.5 + row as f32 * 0.12).rem_euclid(1.0);
                    let char_y =
                        term_y + title_h + row as f32 * (term_h - title_h) / rain_rows as f32;
                    let seed = (phase * 15.0 + col as f32 * 5.0 + row as f32 * 11.0)
                        .floor() as usize
                        % 16;
                    let alpha = (1.0 - char_time) * 0.10 * drive;
                    if alpha > 0.005 {
                        painter.text(
                            Pos2::new(col_x, char_y),
                            egui::Align2::RIGHT_TOP,
                            std::str::from_utf8(&[hex_chars[seed]]).unwrap_or("0"),
                            egui::FontId::monospace(rain_font),
                            cyan_c(alpha),
                        );
                    }
                }
            }
        } else {
            // ── Idle state ────────────────────────────────────────────────
            painter.text(
                Pos2::new(content_x, content_y),
                egui::Align2::LEFT_CENTER,
                "$ _",
                egui::FontId::monospace(font_sm),
                mul(accent, 0.22),
            );
            painter.text(
                Pos2::new(content_x, content_y + line_h * 2.0),
                egui::Align2::LEFT_CENTER,
                "Ready. Awaiting input...",
                egui::FontId::monospace(font_sm),
                mul(accent, 0.12),
            );
        }
    }
}

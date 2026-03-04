use std::{
    f32::consts::TAU,
    time::{Duration, Instant},
};

use eframe::egui::{self, Color32, Pos2, Rgba, Stroke, Visuals};

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
        self.drive = lerp_f32(self.drive, target, 1.0 - (-delta * responsiveness).exp());
        self.velocity = lerp_f32(self.velocity, self.drive, 1.0 - (-delta * stiffness).exp());
        self.phase = (self.phase + delta * self.velocity).rem_euclid(1.0);

        let target_phase = (self.phase + self.offset).rem_euclid(1.0);
        let prev = self.phase_smooth;
        self.phase_smooth = lerp_phase(
            self.phase_smooth,
            target_phase,
            1.0 - (-delta * smooth_k).exp(),
        );

        self.drive > EPS
            || self.velocity.abs() > EPS
            || phase_distance(prev, self.phase_smooth) > EPS
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

        // Spinner — kept as a separate angle accumulator rather than a phase
        let spinner_drive = blend_activity(&mut self.spinner_drive, any_active, delta, 6.0);
        self.spinner_velocity = lerp_f32(
            self.spinner_velocity,
            spinner_drive,
            1.0 - (-delta * 9.0_f32).exp(),
        );
        const EPS: f32 = 0.0005;
        if spinner_drive > EPS || self.spinner_velocity.abs() > EPS {
            self.spinner_angle =
                (self.spinner_angle + delta * self.spinner_velocity).rem_euclid(TAU);
            self.spinner_angle_smooth = lerp_angle(
                self.spinner_angle_smooth,
                self.spinner_angle,
                1.0 - (-delta * 13.0_f32).exp(),
            );
            needs_repaint = true;
        }

        // Phase animations — stiffness / smooth_k / responsiveness tuned per stage
        needs_repaint |= self.compress.tick(compress_active, delta, 7.0, 10.0, 5.5);
        needs_repaint |= self.verify.tick(verify_active, delta, 6.8, 9.5, 5.0);
        needs_repaint |= self.hash.tick(hash_active, delta, 7.5, 10.5, 5.5);
        needs_repaint |= self.rip.tick(rip_active, delta, 6.2, 7.5, 4.8);

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

    fn draw_rip_reader(
        &self,
        painter: &egui::Painter,
        center: Pos2,
        radius: f32,
        accent: Color32,
        visuals: &Visuals,
    ) {
        // Soft drop shadow for depth
        let shadow = egui::Rect::from_center_size(
            center + egui::vec2(0.0, radius * 0.52),
            egui::vec2(radius * 1.45, radius * 0.28),
        );
        painter.rect_filled(
            shadow,
            egui::Rounding::same(radius * 0.12),
            Color32::from_black_alpha(26),
        );

        // Reader body
        let body = egui::Rect::from_center_size(center, egui::vec2(radius * 1.6, radius * 0.9));
        painter.rect(
            body,
            egui::Rounding::same(radius * 0.16),
            visuals.extreme_bg_color.linear_multiply(0.96),
            Stroke::new(radius * 0.028, accent.linear_multiply(0.5)),
        );
        let highlight = egui::Rect::from_min_max(
            Pos2::new(body.left() + radius * 0.12, body.top() + radius * 0.10),
            Pos2::new(body.right() - radius * 0.12, body.top() + radius * 0.28),
        );
        painter.rect_filled(
            highlight,
            egui::Rounding::same(radius * 0.12),
            accent.linear_multiply(0.08),
        );
        painter.line_segment(
            [
                Pos2::new(body.left() + radius * 0.14, body.bottom() - radius * 0.12),
                Pos2::new(body.right() - radius * 0.14, body.bottom() - radius * 0.12),
            ],
            Stroke::new(radius * 0.014, Color32::from_black_alpha(20)),
        );

        // Disc + hub
        let disc_center = center + egui::vec2(-radius * 0.26, 0.0);
        let disc_radius = radius * 0.44;
        painter.circle_filled(
            disc_center,
            disc_radius,
            visuals.extreme_bg_color.linear_multiply(0.99),
        );
        painter.circle_stroke(
            disc_center,
            disc_radius,
            Stroke::new(radius * 0.03, accent.linear_multiply(0.7)),
        );
        painter.circle_stroke(
            disc_center,
            disc_radius * 0.86,
            Stroke::new(radius * 0.006, accent.linear_multiply(0.22)),
        );
        painter.circle_stroke(
            disc_center,
            disc_radius * 0.68,
            Stroke::new(radius * 0.006, accent.linear_multiply(0.15)),
        );

        let hub_r = disc_radius * 0.18;
        painter.circle_filled(
            disc_center,
            hub_r,
            visuals.extreme_bg_color.linear_multiply(0.98),
        );
        painter.circle_stroke(
            disc_center,
            hub_r,
            Stroke::new(radius * 0.012, accent.linear_multiply(0.45)),
        );

        // Rotating spokes
        let spoke_count = 6;
        for i in 0..spoke_count {
            let ang = self.spinner_angle_smooth + (i as f32) * TAU / (spoke_count as f32);
            let dir = egui::vec2(ang.cos(), ang.sin());
            painter.line_segment(
                [
                    disc_center + dir * disc_radius * 0.24,
                    disc_center + dir * disc_radius * 0.88,
                ],
                Stroke::new(radius * 0.008, accent.linear_multiply(0.15)),
            );
        }

        // Funnel and stream
        let throat_start = disc_center + egui::vec2(disc_radius * 0.88, 0.0);
        let throat_end = center + egui::vec2(radius * 0.20, 0.0);
        let funnel_height = radius * 0.38;
        painter.add(egui::Shape::convex_polygon(
            vec![
                throat_start + egui::vec2(0.0, -funnel_height * 0.28),
                throat_end + egui::vec2(0.0, -funnel_height * 0.5),
                throat_end + egui::vec2(0.0, funnel_height * 0.5),
                throat_start + egui::vec2(0.0, funnel_height * 0.28),
            ],
            accent.linear_multiply(0.08),
            Stroke::NONE,
        ));

        let start_x = center.x + radius * 0.12;
        let end_x = center.x + radius * 0.82;
        let y = center.y;
        painter.line_segment(
            [Pos2::new(start_x, y), Pos2::new(end_x, y)],
            Stroke::new(radius * 0.025, accent.linear_multiply(0.15)),
        );

        let flow_t = self.rip.phase_smooth;
        let glow_x = egui::lerp(start_x..=end_x, flow_t);
        let glow_pos = Pos2::new(glow_x, y);
        let extinguish = if flow_t > 0.9 {
            ((1.0 - flow_t) / 0.1).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let glow_alpha = self.rip.drive * 0.95 * extinguish;
        if glow_alpha > 0.01 {
            for i in 0..3 {
                let glow_radius = radius * (0.04 + i as f32 * 0.02);
                let alpha = (0.25 - i as f32 * 0.08) * glow_alpha;
                painter.circle_filled(glow_pos, glow_radius, accent.linear_multiply(alpha));
            }
        }

        let led_center = Pos2::new(body.right() - radius * 0.16, body.top() + radius * 0.16);
        painter.circle_filled(led_center, radius * 0.06, accent.linear_multiply(0.7));
        painter.circle_stroke(
            led_center,
            radius * 0.085,
            Stroke::new(radius * 0.01, accent.linear_multiply(0.25)),
        );
    }

    /// Factory compression: packets arrive on a conveyor belt, are crushed by a
    /// hydraulic press from above, then fall into a container on the right.
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
        let cycle = self.compress.phase_smooth;

        // ── Panel ────────────────────────────────────────────────────────────
        let panel = egui::Rect::from_center_size(center, egui::vec2(radius * 1.88, radius * 1.32));
        painter.rect(
            panel,
            egui::Rounding::same(radius * 0.13),
            mul(bg, 0.94),
            Stroke::new(radius * 0.018, mul(accent, 0.32)),
        );
        let margin = radius * 0.13;
        let inner = panel.shrink(margin);

        // ── Layout constants ─────────────────────────────────────────────────
        let belt_y = center.y + radius * 0.14; // top surface of conveyor belt
        let press_cx = center.x - radius * 0.08; // x-center of the press
        let pkt_w = radius * 0.38;
        let pkt_h = radius * 0.26;
        let roller_r = radius * 0.052;

        let container_left = press_cx + pkt_w * 0.62 + radius * 0.10;
        let container_right = inner.right();
        let container_bottom = inner.bottom();

        let press_platen_w = pkt_w * 1.32;
        let press_platen_h = radius * 0.062;
        let platen_up_y = inner.top() + press_platen_h + radius * 0.02; // bottom of platen when UP
        let platen_dn_y = belt_y - pkt_h * 0.11; // bottom of platen when DOWN

        // ── press_t: 0 = fully up, 1 = fully down ────────────────────────────
        let press_t: f32 = if cycle < 0.38 {
            0.0
        } else if cycle < 0.54 {
            ease_in_out_smooth((cycle - 0.38) / 0.16)
        } else if cycle < 0.70 {
            1.0
        } else if cycle < 0.82 {
            1.0 - ease_in_out_smooth((cycle - 0.70) / 0.12)
        } else {
            0.0
        };
        let platen_bottom_y = lerp_f32(platen_up_y, platen_dn_y, press_t);

        // Packet deformation tracks the press
        let crush = ease_in_out_smooth(press_t);
        let cur_pkt_h = lerp_f32(pkt_h, pkt_h * 0.11, crush);
        let cur_pkt_w = lerp_f32(pkt_w, pkt_w * 1.22, crush);

        // ── Packet-drawing helper (closure) ───────────────────────────────────
        let draw_pkt = |rect: egui::Rect, alpha: f32| {
            painter.rect(
                rect,
                egui::Rounding::same(radius * 0.04),
                mul(accent, alpha * 0.12),
                Stroke::new(radius * 0.015, mul(accent, alpha * 0.88)),
            );
            if rect.width() > radius * 0.09 {
                for li in 0..3_usize {
                    let ly = rect.top() + rect.height() * (li as f32 + 1.0) / 4.0;
                    let lm = rect.width() * 0.14;
                    painter.line_segment(
                        [
                            Pos2::new(rect.left() + lm, ly),
                            Pos2::new(rect.right() - lm, ly),
                        ],
                        Stroke::new(radius * 0.007, mul(accent, alpha * 0.25)),
                    );
                }
            }
        };

        // ── Conveyor belt ─────────────────────────────────────────────────────
        let belt_left = inner.left();
        let belt_right = press_cx + pkt_w * 0.5 + radius * 0.06;
        for dy in [-radius * 0.011, radius * 0.011] {
            painter.line_segment(
                [
                    Pos2::new(belt_left, belt_y + dy),
                    Pos2::new(belt_right, belt_y + dy),
                ],
                Stroke::new(radius * 0.013, mul(accent, 0.52)),
            );
        }
        // Rollers
        for rx in [belt_left + roller_r, belt_right - roller_r] {
            painter.circle_filled(Pos2::new(rx, belt_y), roller_r, mul(bg, 0.85));
            painter.circle_stroke(
                Pos2::new(rx, belt_y),
                roller_r,
                Stroke::new(radius * 0.012, mul(accent, 0.68)),
            );
        }
        // Animated tick marks (belt motion)
        let tick_spacing = radius * 0.15;
        let tick_offset = (cycle * tick_spacing * 2.0).rem_euclid(tick_spacing);
        let mut tx = belt_left + tick_offset;
        while tx < belt_right {
            painter.line_segment(
                [
                    Pos2::new(tx, belt_y - roller_r * 0.75),
                    Pos2::new(tx, belt_y + roller_r * 0.75),
                ],
                Stroke::new(radius * 0.007, mul(accent, 0.20)),
            );
            tx += tick_spacing;
        }

        // ── Packet lifecycle ──────────────────────────────────────────────────
        //   0.00–0.38  slide in from left to press position
        //   0.38–0.82  at press (height = cur_pkt_h driven by press_t)
        //   0.82–0.93  crushed packet slides to container
        //   0.93–1.00  packet falls into container (fades)
        let pkt_start_cx = belt_left + pkt_w * 0.5 + radius * 0.03;

        if cycle < 0.38 {
            let x = lerp_f32(pkt_start_cx, press_cx, ease_in_out_smooth(cycle / 0.38));
            draw_pkt(
                egui::Rect::from_min_max(
                    Pos2::new(x - pkt_w * 0.5, belt_y - pkt_h),
                    Pos2::new(x + pkt_w * 0.5, belt_y),
                ),
                1.0,
            );
        } else if cycle < 0.82 {
            draw_pkt(
                egui::Rect::from_min_max(
                    Pos2::new(press_cx - cur_pkt_w * 0.5, belt_y - cur_pkt_h),
                    Pos2::new(press_cx + cur_pkt_w * 0.5, belt_y),
                ),
                1.0,
            );
        } else if cycle < 0.93 {
            let slide_t = ease_in_out_smooth((cycle - 0.82) / 0.11);
            let dest_cx = (container_left + container_right) * 0.5;
            let x = lerp_f32(press_cx, dest_cx, slide_t);
            draw_pkt(
                egui::Rect::from_min_max(
                    Pos2::new(x - cur_pkt_w * 0.5, belt_y - cur_pkt_h),
                    Pos2::new(x + cur_pkt_w * 0.5, belt_y),
                ),
                1.0,
            );
        } else {
            // Falling into container
            let fall_t = (cycle - 0.93) / 0.07;
            let dest_cx = (container_left + container_right) * 0.5;
            let y_top = lerp_f32(
                belt_y - cur_pkt_h,
                container_bottom - cur_pkt_h,
                ease_in_out_smooth(fall_t),
            );
            draw_pkt(
                egui::Rect::from_min_max(
                    Pos2::new(dest_cx - cur_pkt_w * 0.5, y_top),
                    Pos2::new(dest_cx + cur_pkt_w * 0.5, y_top + cur_pkt_h),
                ),
                1.0 - fall_t,
            );
        }

        // ── Hydraulic press ───────────────────────────────────────────────────
        // Rod
        painter.line_segment(
            [
                Pos2::new(press_cx, inner.top()),
                Pos2::new(press_cx, platen_bottom_y - press_platen_h),
            ],
            Stroke::new(radius * 0.055, mul(accent, 0.68)),
        );
        // Platen
        let platen = egui::Rect::from_min_max(
            Pos2::new(
                press_cx - press_platen_w * 0.5,
                platen_bottom_y - press_platen_h,
            ),
            Pos2::new(press_cx + press_platen_w * 0.5, platen_bottom_y),
        );
        painter.rect_filled(
            platen,
            egui::Rounding::same(radius * 0.022),
            mul(accent, 0.82),
        );
        // Ridges on press face (shows it's a pressing surface)
        for ri in 0..4_usize {
            let rx = platen.left() + (ri as f32 + 0.5) * platen.width() / 4.0;
            painter.line_segment(
                [
                    Pos2::new(rx, platen.bottom() - press_platen_h * 0.35),
                    Pos2::new(rx, platen.bottom()),
                ],
                Stroke::new(radius * 0.014, mul(accent, 0.42)),
            );
        }
        // Mount at top of rod
        painter.rect_filled(
            egui::Rect::from_center_size(
                Pos2::new(press_cx, inner.top() + radius * 0.042),
                egui::vec2(press_platen_w * 0.52, radius * 0.084),
            ),
            egui::Rounding::same(radius * 0.02),
            mul(accent, 0.58),
        );

        // ── Container (open-top box) ──────────────────────────────────────────
        let wall_s = Stroke::new(radius * 0.022, mul(accent, 0.80));
        painter.line_segment(
            [
                Pos2::new(container_left, belt_y),
                Pos2::new(container_left, container_bottom),
            ],
            wall_s,
        );
        painter.line_segment(
            [
                Pos2::new(container_right, belt_y),
                Pos2::new(container_right, container_bottom),
            ],
            wall_s,
        );
        painter.line_segment(
            [
                Pos2::new(container_left, container_bottom),
                Pos2::new(container_right, container_bottom),
            ],
            wall_s,
        );

        // Stacked compressed slices inside
        let container_cx = (container_left + container_right) * 0.5;
        let inner_w = container_right - container_left - radius * 0.055;
        let n_slices = 4_usize;
        let s_h = ((container_bottom - belt_y) * 0.13).max(2.0_f32);
        let s_gap = (s_h * 0.38).max(1.0_f32);
        for i in 0..n_slices {
            let sy = container_bottom - radius * 0.028 - i as f32 * (s_h + s_gap) - s_h * 0.5;
            if sy > belt_y {
                painter.rect_filled(
                    egui::Rect::from_center_size(
                        Pos2::new(container_cx, sy),
                        egui::vec2(inner_w * 0.84, s_h),
                    ),
                    egui::Rounding::same(1.5),
                    mul(accent, 0.58 + i as f32 * 0.08),
                );
            }
        }
    }

    fn draw_verify_scanner(
        &self,
        painter: &egui::Painter,
        center: Pos2,
        radius: f32,
        accent: Color32,
        visuals: &Visuals,
    ) {
        let doc_rect = egui::Rect::from_center_size(center, egui::vec2(radius * 1.2, radius * 1.4));
        painter.rect(
            doc_rect,
            egui::Rounding::same(radius * 0.12),
            visuals.extreme_bg_color.linear_multiply(0.97),
            Stroke::new(radius * 0.02, accent.linear_multiply(0.6)),
        );
        let line_count = 8;
        for i in 0..line_count {
            let t = i as f32 / (line_count - 1).max(1) as f32;
            let y = egui::lerp(
                doc_rect.top() + radius * 0.15..=doc_rect.bottom() - radius * 0.15,
                t,
            );
            painter.line_segment(
                [
                    Pos2::new(doc_rect.left() + radius * 0.18, y),
                    Pos2::new(doc_rect.right() - radius * 0.18, y),
                ],
                Stroke::new(
                    radius * 0.01,
                    visuals.weak_text_color().linear_multiply(0.45),
                ),
            );
        }
        let pingpong_raw = if self.verify.phase_smooth < 0.5 {
            self.verify.phase_smooth * 3.0
        } else {
            1.0 - (self.verify.phase_smooth - 0.5) * 2.0
        };
        let pingpong = ease_in_out_smooth(pingpong_raw);
        let scan_pos = egui::lerp(
            doc_rect.top() + radius * 0.18..=doc_rect.bottom() - radius * 0.18,
            pingpong,
        );
        let bar_rect = egui::Rect::from_min_max(
            Pos2::new(doc_rect.left() + radius * 0.14, scan_pos - radius * 0.08),
            Pos2::new(doc_rect.right() - radius * 0.14, scan_pos + radius * 0.08),
        );
        painter.rect_filled(
            bar_rect,
            egui::Rounding::same(radius * 0.06),
            accent.linear_multiply(0.38),
        );
        painter.line_segment(
            [
                Pos2::new(bar_rect.left() + radius * 0.06, bar_rect.center().y),
                Pos2::new(bar_rect.right() - radius * 0.06, bar_rect.center().y),
            ],
            Stroke::new(radius * 0.008, Color32::from_white_alpha(40)),
        );
    }

    fn draw_hash_pulses(
        &self,
        painter: &egui::Painter,
        center: Pos2,
        radius: f32,
        accent: Color32,
        visuals: &Visuals,
    ) {
        let mul = |c: Color32, f: f32| c.linear_multiply(f.clamp(0.0, 1.0));

        // Hex character grid — each column scrolls at its own speed, creating depth
        let charset = *b"0123456789ABCDEF";
        let rows: usize = 4;
        let cols: usize = 6;
        let cell_x = radius * 0.245;
        let cell_y = radius * 0.19;
        let font_size = (radius * 0.215).clamp(8.0, 20.0);

        let base_text = visuals
            .override_text_color
            .unwrap_or_else(|| visuals.weak_text_color());

        let sweep_col = ((self.hash.phase_smooth * cols as f32).floor() as i32)
            .rem_euclid(cols as i32) as usize;

        // Subtle highlight behind the active sweep column
        let col_x = (sweep_col as f32 - (cols as f32 - 1.0) / 2.0) * cell_x;
        let col_rect = egui::Rect::from_center_size(
            center + egui::vec2(col_x, 0.0),
            egui::vec2(cell_x * 0.85, cell_y * rows as f32 * 0.92),
        );
        painter.rect_filled(
            col_rect,
            egui::Rounding::same(radius * 0.04),
            mul(accent, 0.10),
        );

        for row in 0..rows {
            for col in 0..cols {
                // Each column has a slightly different scroll speed
                let speed_mul = 1.0 + col as f32 * 0.12;
                let phase =
                    (self.hash.phase_smooth * speed_mul + row as f32 * 0.25).rem_euclid(1.0);
                let idx = ((phase * charset.len() as f32) as usize) % charset.len();
                let ch = charset[idx] as char;

                let offset_x = (col as f32 - (cols as f32 - 1.0) / 2.0) * cell_x;
                let offset_y = (row as f32 - (rows as f32 - 1.0) / 2.0) * cell_y;

                let intensity = if col == sweep_col { 1.0 } else { 0.38 };
                let tint = egui::lerp(Rgba::from(base_text)..=Rgba::from(accent), intensity);

                painter.text(
                    center + egui::vec2(offset_x, offset_y),
                    egui::Align2::CENTER_CENTER,
                    ch,
                    egui::FontId::monospace(font_size),
                    tint.into(),
                );
            }
        }
    }
}

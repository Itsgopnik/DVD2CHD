use super::workflow::{JobStage, JobStageKind, StageState, TimelineKind};
use super::App;
use dvd2chd_core::StageEvent;

impl App {
    pub(super) fn prepare_timeline(&mut self, kind: TimelineKind, include_hash: bool) {
        let mut stages = match kind {
            TimelineKind::Device => vec![
                JobStage::new(JobStageKind::Input, "stage.ripping", StageState::Pending),
                JobStage::new(JobStageKind::Chd, "stage.chd", StageState::Pending),
                JobStage::new(JobStageKind::Verify, "stage.verify", StageState::Pending),
            ],
            TimelineKind::File => vec![
                JobStage::new(JobStageKind::Chd, "stage.chd", StageState::Pending),
                JobStage::new(JobStageKind::Verify, "stage.verify", StageState::Pending),
            ],
        };
        if include_hash {
            stages.push(JobStage::new(
                JobStageKind::Hash,
                "stage.hashes",
                StageState::Pending,
            ));
        }
        if let Some(first) = stages.first_mut() {
            first.state = StageState::Active;
        }
        self.animation.restart();
        self.timeline = stages;
        #[cfg(debug_assertions)]
        {
            self.debug_animation_override = None;
            if let Some(active) = self.active_stage_kind() {
                self.debug_update_indicator(Some(active));
            } else {
                self.debug_update_indicator(None);
            }
        }
    }

    pub(super) fn mark_stage_active(&mut self, kind: JobStageKind) {
        if self.timeline.is_empty() {
            return;
        }
        if !self.timeline.iter().any(|s| s.kind == kind) {
            return;
        }
        let mut encountered = false;
        for stage in &mut self.timeline {
            if stage.kind == kind {
                encountered = true;
                if stage.state != StageState::Done {
                    if stage.state != StageState::Active {
                        self.animation.reset_phase_for(stage.kind);
                    }
                    stage.state = StageState::Active;
                }
            } else if !encountered {
                stage.state = StageState::Done;
            } else if stage.state != StageState::Done {
                stage.state = StageState::Pending;
            }
        }
        #[cfg(debug_assertions)]
        if self.debug_animation_override.is_none() && self.active_stage_kind() == Some(kind) {
            self.debug_update_indicator(Some(kind));
        }
    }

    pub(super) fn mark_stage_done(&mut self, kind: JobStageKind) {
        if self.timeline.is_empty() {
            return;
        }
        if !self.timeline.iter().any(|s| s.kind == kind) {
            return;
        }
        let mut activate_next = false;
        for stage in &mut self.timeline {
            if activate_next && stage.state == StageState::Pending {
                stage.state = StageState::Active;
                activate_next = false;
            }
            if stage.kind == kind {
                stage.state = StageState::Done;
                activate_next = true;
            }
        }
        #[cfg(debug_assertions)]
        if self.debug_animation_override.is_none() {
            if let Some(active) = self.active_stage_kind() {
                self.debug_update_indicator(Some(active));
            } else {
                self.debug_update_indicator(None);
            }
        }
    }

    pub(super) fn mark_all_stages_done(&mut self) {
        for stage in &mut self.timeline {
            stage.state = StageState::Done;
        }
        #[cfg(debug_assertions)]
        {
            if self.debug_animation_override.is_none() {
                self.debug_update_indicator(None);
            }
        }
    }

    pub(super) fn active_stage_kind(&self) -> Option<JobStageKind> {
        #[cfg(debug_assertions)]
        if let Some(kind) = self.debug_animation_override {
            return Some(kind);
        }
        self.timeline
            .iter()
            .find(|stage| stage.state == StageState::Active)
            .map(|stage| stage.kind)
    }

    pub(super) fn reset_timeline_after_failure(&mut self) {
        self.animation.restart();
        for stage in &mut self.timeline {
            stage.state = StageState::Pending;
        }
        #[cfg(debug_assertions)]
        {
            if self.debug_animation_override.is_none() {
                self.debug_update_indicator(None);
            }
        }
    }

    pub(super) fn handle_stage_event(&mut self, event: StageEvent) {
        match event {
            StageEvent::RipStarted => self.mark_stage_active(JobStageKind::Input),
            StageEvent::RipFinished => self.mark_stage_done(JobStageKind::Input),
            StageEvent::ChdStarted => {
                self.mark_stage_done(JobStageKind::Input);
                self.mark_stage_active(JobStageKind::Chd);
            }
            StageEvent::ChdFinished => self.mark_stage_done(JobStageKind::Chd),
            StageEvent::VerifyStarted => self.mark_stage_active(JobStageKind::Verify),
            StageEvent::VerifyFinished => self.mark_stage_done(JobStageKind::Verify),
            StageEvent::HashStarted => self.mark_stage_active(JobStageKind::Hash),
            StageEvent::HashFinished => self.mark_stage_done(JobStageKind::Hash),
        }
    }

    #[cfg(debug_assertions)]
    pub(super) fn debug_stage_name(kind: JobStageKind) -> &'static str {
        match kind {
            JobStageKind::Input => "RIP",
            JobStageKind::Chd => "CHD",
            JobStageKind::Verify => "VERIFY",
            JobStageKind::Hash => "HASH",
        }
    }

    #[cfg(debug_assertions)]
    pub(super) fn debug_update_indicator(&mut self, kind: Option<JobStageKind>) {
        if let Some(k) = kind {
            let label = Self::debug_stage_name(k);
            if self.debug_animation_indicator != label {
                self.debug_animation_indicator = label.to_string();
            }
        } else if !self.debug_animation_indicator.is_empty() {
            self.debug_animation_indicator.clear();
        }
    }

    #[cfg(debug_assertions)]
    pub(super) fn debug_apply_override(&mut self) {
        if self.running {
            self.debug_update_indicator(self.active_stage_kind());
            return;
        }
        if let Some(kind) = self.debug_animation_override {
            if self.debug_timeline_backup.is_none() {
                self.debug_timeline_backup = Some(self.timeline.clone());
            }
            if self.timeline.is_empty() {
                self.timeline = vec![
                    JobStage::new(JobStageKind::Input, "stage.ripping", StageState::Active),
                    JobStage::new(JobStageKind::Chd, "stage.chd", StageState::Pending),
                    JobStage::new(JobStageKind::Verify, "stage.verify", StageState::Pending),
                    JobStage::new(JobStageKind::Hash, "stage.hashes", StageState::Pending),
                ];
            }

            let override_idx = self
                .timeline
                .iter()
                .position(|s| s.kind == kind)
                .unwrap_or(0);
            for (idx, stage) in self.timeline.iter_mut().enumerate() {
                stage.state = if idx == override_idx {
                    StageState::Active
                } else {
                    StageState::Pending
                };
            }

            self.animation.restart();
            self.debug_update_indicator(Some(kind));
        } else {
            if let Some(prev) = self.debug_timeline_backup.take() {
                self.timeline = prev;
            }
            if !self.running {
                self.animation.restart();
            }
            self.debug_update_indicator(self.active_stage_kind());
        }
    }
}

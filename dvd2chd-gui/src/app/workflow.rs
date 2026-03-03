use rust_i18n::t;

#[derive(Clone)]
pub struct JobStage {
    pub kind: JobStageKind,
    pub label_key: &'static str,
    pub state: StageState,
}

impl JobStage {
    pub fn new(kind: JobStageKind, label_key: &'static str, state: StageState) -> Self {
        Self { kind, label_key, state }
    }

    pub fn label(&self) -> String {
        t!(self.label_key).to_string()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StageState {
    Pending,
    Active,
    Done,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum JobStageKind {
    Input,
    Chd,
    Verify,
    Hash,
}

#[derive(Clone, Copy)]
pub enum TimelineKind {
    File,
    Device,
}

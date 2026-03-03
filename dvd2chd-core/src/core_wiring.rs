use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
};

use crate::{
    self as core, ArchiveOptions, CoreError, ExtractOptions, FileOptions, Profile, ProgressSink,
    StageEvent,
};

#[derive(Debug)]
pub enum UiMsg {
    Log(String),
    Progress(f32), // 0.0..=1.0
    Label(String), // Status/ETA
    Done(Result<PathBuf, CoreError>),
    Stage(StageEvent),
}

pub struct UiSink {
    pub tx: mpsc::Sender<UiMsg>,
    pub cancel: Arc<AtomicBool>,
}

impl ProgressSink for UiSink {
    fn log(&self, s: &str) {
        let _ = self.tx.send(UiMsg::Log(s.to_string()));
    }
    fn percent(&self, p: f32) {
        let _ = self.tx.send(UiMsg::Progress(p.clamp(0.0, 1.0)));
    }
    fn label(&self, t: &str) {
        let _ = self.tx.send(UiMsg::Label(t.to_string()));
    }
    fn stage(&self, event: StageEvent) {
        let _ = self.tx.send(UiMsg::Stage(event));
    }
    fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

pub enum Mode {
    File {
        in_path: PathBuf,
        out_dir: PathBuf,
        opts: FileOptions,
    },
    Device {
        dev_path: PathBuf,
        profile: Profile,
        opts: ArchiveOptions,
    },
    Extract {
        in_path: PathBuf,
        out_dir: PathBuf,
        opts: ExtractOptions,
    },
}

pub fn spawn_core_job(
    mode: Mode,
    tx: mpsc::Sender<UiMsg>,
    cancel: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let sink: Arc<dyn ProgressSink> = Arc::new(UiSink {
            tx: tx.clone(),
            cancel,
        });

        let _ = tx.send(UiMsg::Label("Starting…".into()));

        let result = match mode {
            Mode::File {
                in_path,
                out_dir,
                opts,
            } => core::convert_file(&in_path, &out_dir, &opts, sink),
            Mode::Device {
                dev_path,
                profile,
                opts,
            } => core::archive_device(&dev_path, profile, &opts, sink),
            Mode::Extract {
                in_path,
                out_dir,
                opts,
            } => core::extract_chd(&in_path, &out_dir, &opts, sink),
        };

        let _ = tx.send(UiMsg::Done(result));
    })
}

pub fn new_channel() -> (mpsc::Sender<UiMsg>, mpsc::Receiver<UiMsg>) {
    mpsc::channel()
}

pub fn request_cancel(cancel: &Arc<AtomicBool>) {
    cancel.store(true, Ordering::Relaxed);
}

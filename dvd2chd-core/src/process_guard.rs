//! Prozess-Guard: sauberes Abbrechen laufender Tools (ddrescue/cdrdao/chdman)
//! Plattformübergreifend via Child::kill()

use std::process::{Child, Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::Duration;

pub struct RunningJob {
    cancel: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
}

impl RunningJob {
    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }

    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        // Lock robust gegen Poisoning behandeln:
        let mut guard = self.child.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(mut ch) = guard.take() {
            let _ = ch.kill();
            // Ensure the process gets reaped so it does not linger as a zombie.
            let _ = ch.wait();
        }
    }
}

pub fn spawn_monitored(mut cmd: Command) -> std::io::Result<RunningJob> {
    cmd.stdin(Stdio::null());
    let child = cmd.spawn()?;

    let cancel = Arc::new(AtomicBool::new(false));
    let child_arc = Arc::new(Mutex::new(Some(child)));

    {
        let cancel_w = cancel.clone();
        let child_w = child_arc.clone();
        thread::spawn(move || {
            loop {
                // 1) Cancel gedrückt → kill
                if cancel_w.load(Ordering::Relaxed) {
                    let mut guard = child_w.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(mut ch) = guard.take() {
                        let _ = ch.kill();
                        let _ = ch.wait();
                    }
                    break;
                }

                // 2) Prozess bereits beendet?
                let done = {
                    let mut guard = child_w.lock().unwrap_or_else(|e| e.into_inner());
                    match guard.as_mut().and_then(|c| c.try_wait().ok()).flatten() {
                        Some(_status) => {
                            guard.take();
                            true
                        }
                        None => false,
                    }
                };
                if done {
                    break;
                }

                thread::sleep(Duration::from_millis(120));
            }
        });
    }

    Ok(RunningJob {
        cancel,
        child: child_arc,
    })
}

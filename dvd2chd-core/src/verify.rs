use anyhow::anyhow;
use std::{
    io::{BufRead, BufReader},
    path::Path,
    process::{Command, Stdio},
    sync::Arc,
};

use crate::{CoreError, CoreResult, ProgressSink, StageEvent, CHDMAN_PERCENT_FLOAT_RE};
use crate::util::wait_with_cancel;

pub(crate) fn run_verify(chdman: &Path, chd: &Path, sink: Arc<dyn ProgressSink>) -> CoreResult<()> {
    sink.label("Verification…");
    sink.stage(StageEvent::VerifyStarted);
    let mut cmd = Command::new(chdman);
    cmd.arg("verify")
        .arg("-i")
        .arg(chd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(CoreError::Io)?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| CoreError::Any(anyhow!("stdout not piped")))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| CoreError::Any(anyhow!("stderr not piped")))?;

    {
        let s = sink.clone();
        let chd_name = chd.to_string_lossy().to_string();
        let re = &*CHDMAN_PERCENT_FLOAT_RE;
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(c) = re.captures(&line) {
                    if let Ok(p) = c[1].parse::<f32>() {
                        s.percent(0.96 + 0.04 * (p.min(100.0) / 100.0));
                        s.label(&format!("Verification {p:.0}%"));
                    }
                }
                s.log(&format!("verify {} :: {}", chd_name, line));
            }
        });
    }
    {
        let s = sink.clone();
        let chd_name = chd.to_string_lossy().to_string();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                s.log(&format!("verify {} :: {}", chd_name, line));
            }
        });
    }

    let status = wait_with_cancel(&mut child, || sink.is_cancelled()).map_err(CoreError::Io)?;
    if sink.is_cancelled() {
        return Err(CoreError::Cancelled);
    }
    if !status.success() {
        return Err(CoreError::VerifyFailed);
    }
    sink.percent(1.0);
    sink.label("Verification complete");
    sink.stage(StageEvent::VerifyFinished);
    Ok(())
}

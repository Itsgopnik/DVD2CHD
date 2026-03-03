use anyhow::anyhow;
use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
};

use crate::{CoreError, CoreResult, ProgressSink, StageEvent, CHDMAN_PERCENT_RE};
use crate::util::wait_with_cancel;
use crate::verify::run_verify;
use super::{DEVICE_PROGRESS_CHD, DEVICE_PROGRESS_VERIFY};
use super::cd::{ensure_cue_from_toc, make_cue_paths_relative};

pub(super) fn chd_dvd_atomic(
    chdman: &Path,
    iso: &Path,
    out_chd: &Path,
    extra_args: &str,
    nice: bool,
    ionice: bool,
    sink: Arc<dyn ProgressSink>,
) -> CoreResult<PathBuf> {
    let tmp = out_chd.with_extension("chd.part");
    let _ = fs::remove_file(&tmp);
    // Parse extra arguments for chdman. If parsing fails (e.g. invalid
    // quoting), propagate an error so the caller can report it to the user.
    let extras = match shell_words::split(extra_args) {
        Ok(v) => v,
        Err(e) => {
            return Err(CoreError::Any(anyhow!(
                "Fehler beim Parsen von extra_chd_args: {e}"
            )));
        }
    };

    let mut base = Command::new(chdman);
    base.arg("createdvd")
        .arg("-i")
        .arg(iso)
        .arg("-o")
        .arg(&tmp)
        .args(extras);
    let mut cmd = wrap_priority(base, nice, ionice);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    sink.stage(StageEvent::ChdStarted);
    sink.log(&format!("createdvd {} → {}", iso.display(), tmp.display()));
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
        let re = &*CHDMAN_PERCENT_RE;
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(c) = re.captures(&line) {
                    if let Ok(p) = c[1].parse::<f32>() {
                        let stage = p / 100.0;
                        let global = DEVICE_PROGRESS_CHD.lerp(stage);
                        s.percent(global);
                        s.label(&format!("CHD: {p:.0}%"));
                    }
                }
                s.log(&line);
            }
        });
    }
    {
        let s = sink.clone();
        std::thread::spawn(move || {
            for l in BufReader::new(stderr).lines().map_while(Result::ok) {
                s.log(&l);
            }
        });
    }

    let status = wait_with_cancel(&mut child, || sink.is_cancelled()).map_err(CoreError::Io)?;
    if sink.is_cancelled() {
        let _ = fs::remove_file(&tmp);
        return Err(CoreError::Cancelled);
    }
    if !status.success() {
        let _ = fs::remove_file(&tmp);
        return Err(CoreError::Any(anyhow!("chdman createdvd: {status}")));
    }

    sink.stage(StageEvent::ChdFinished);
    run_verify(chdman, &tmp, sink.clone())?;
    sink.percent(DEVICE_PROGRESS_VERIFY.end);
    fs::rename(&tmp, out_chd).map_err(CoreError::Io)?;
    Ok(out_chd.to_path_buf())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn chd_cd_atomic(
    chdman: &Path,
    cue_or_toc: &Path,
    bin: &Path,
    out_chd: &Path,
    extra_args: &str,
    nice: bool,
    ionice: bool,
    sink: Arc<dyn ProgressSink>,
) -> CoreResult<PathBuf> {
    // Ensure CUE file exists
    let mut cue = cue_or_toc.to_path_buf();
    if cue.extension().and_then(|e| e.to_str()) == Some("toc") {
        cue = ensure_cue_from_toc(&cue, bin, sink.clone())?;
    } else {
        make_cue_paths_relative(&cue, Some(bin), sink.clone())?;
    }

    let tmp = out_chd.with_extension("chd.part");
    let _ = fs::remove_file(&tmp);
    let extras = match shell_words::split(extra_args) {
        Ok(v) => v,
        Err(e) => {
            return Err(CoreError::Any(anyhow!(
                "Fehler beim Parsen von extra_chd_args: {e}"
            )));
        }
    };

    let mut base = Command::new(chdman);
    base.arg("createcd")
        .arg("-i")
        .arg(&cue)
        .arg("-o")
        .arg(&tmp)
        .args(extras);
    let mut cmd = wrap_priority(base, nice, ionice);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    sink.stage(StageEvent::ChdStarted);
    sink.log(&format!("createcd {} → {}", cue.display(), tmp.display()));
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
        let re = &*CHDMAN_PERCENT_RE;
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(c) = re.captures(&line) {
                    if let Ok(p) = c[1].parse::<f32>() {
                        let stage = p / 100.0;
                        let global = DEVICE_PROGRESS_CHD.lerp(stage);
                        s.percent(global);
                        s.label(&format!("CHD: {p:.0}%"));
                    }
                }
                s.log(&line);
            }
        });
    }
    {
        let s = sink.clone();
        std::thread::spawn(move || {
            for l in BufReader::new(stderr).lines().map_while(Result::ok) {
                s.log(&l);
            }
        });
    }

    let status = wait_with_cancel(&mut child, || sink.is_cancelled()).map_err(CoreError::Io)?;
    if sink.is_cancelled() {
        let _ = fs::remove_file(&tmp);
        return Err(CoreError::Cancelled);
    }
    if !status.success() {
        let _ = fs::remove_file(&tmp);
        return Err(CoreError::Any(anyhow!("chdman createcd: {status}")));
    }

    sink.stage(StageEvent::ChdFinished);
    run_verify(chdman, &tmp, sink.clone())?;
    sink.percent(DEVICE_PROGRESS_VERIFY.end);
    fs::rename(&tmp, out_chd).map_err(CoreError::Io)?;
    Ok(out_chd.to_path_buf())
}

pub(crate) fn wrap_priority(base: Command, run_nice: bool, run_ionice: bool) -> Command {
    use std::os::unix::process::CommandExt;
    if run_nice || run_ionice {
        let nice = run_nice;
        let ionice = run_ionice;
        let mut cmd = base;
        unsafe {
            cmd.pre_exec(move || {
                apply_linux_priorities(nice, ionice)?;
                Ok(())
            });
        }
        return cmd;
    }
    base
}

pub(crate) fn apply_linux_priorities(run_nice: bool, run_ionice: bool) -> std::io::Result<()> {
    use std::io;

    if run_ionice {
        const IOPRIO_CLASS_BE: u32 = 2;
        const IOPRIO_CLASS_SHIFT: u32 = 13;
        const IOPRIO_WHO_PROCESS: libc::c_int = 1;
        let prio_value = ((IOPRIO_CLASS_BE << IOPRIO_CLASS_SHIFT) | 7) as libc::c_int;
        let res = unsafe {
            libc::syscall(
                libc::SYS_ioprio_set,
                IOPRIO_WHO_PROCESS as libc::c_long,
                0 as libc::c_long,
                prio_value as libc::c_long,
            )
        };
        if res == -1 {
            return Err(io::Error::last_os_error());
        }
    }
    if run_nice {
        let rc = unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, 10) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

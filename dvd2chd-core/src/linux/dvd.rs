use anyhow::anyhow;
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use super::{ProgressRange, DEVICE_PROGRESS_RIP};
use crate::util::{ensure_tool, wait_with_cancel};
use crate::{CoreError, CoreResult, ProgressSink};

pub(super) fn rip_dvd(
    ddrescue: &Path,
    dev: &Path,
    iso: &Path,
    use_ddrescue: bool,
    scrape: bool,
    sink: Arc<dyn ProgressSink>,
) -> CoreResult<()> {
    let total = disc_logical_size_bytes(dev);

    if use_ddrescue {
        ensure_tool(ddrescue, &["--version"]).map_err(|_| CoreError::MissingTool("ddrescue"))?;
        run_ddrescue(ddrescue, dev, iso, total, false, sink.clone())?;
        if scrape {
            sink.log("🩹 ddrescue: Recovery pass (-r3)…");
            run_ddrescue(ddrescue, dev, iso, total, true, sink)?;
        }
    } else {
        run_dd(dev, iso, total, sink)?;
    }
    Ok(())
}

pub(super) fn run_ddrescue(
    bin: &Path,
    dev: &Path,
    iso: &Path,
    total: Option<u64>,
    scrape: bool,
    sink: Arc<dyn ProgressSink>,
) -> CoreResult<()> {
    let map = iso.with_extension("map");
    let mut cmd = Command::new(bin);
    cmd.arg("-b").arg("2048").arg("-d").arg("-f");
    if scrape {
        cmd.arg("-r").arg("3");
    } else {
        cmd.arg("-n");
    }
    cmd.arg(dev)
        .arg(iso)
        .arg(&map)
        .stderr(Stdio::piped())
        .stdout(Stdio::null());
    sink.log(&format!(
        "ddrescue {} → {} (Map: {})",
        if scrape { "-r3" } else { "-n" },
        iso.display(),
        map.display()
    ));

    let mut child = cmd.spawn().map_err(CoreError::Io)?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| CoreError::Any(anyhow!("stderr not piped")))?;
    let start = std::time::Instant::now();
    {
        let s = sink.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                s.log(&line);
            }
        });
    }

    // Run the progress poller concurrently with the child process so that the
    // loop exits as soon as wait_with_cancel returns, even if ddrescue exits
    // before writing the expected number of bytes (which would otherwise cause
    // the old blocking poll to loop forever).
    let poll_done = Arc::new(AtomicBool::new(false));
    if total.is_some() {
        let done2 = poll_done.clone();
        let iso2 = iso.to_path_buf();
        let sink2 = sink.clone();
        thread::spawn(move || {
            poll_progress_until(&iso2, total, DEVICE_PROGRESS_RIP, sink2, done2);
        });
    }
    let status = wait_with_cancel(&mut child, || sink.is_cancelled()).map_err(CoreError::Io)?;
    poll_done.store(true, Ordering::Relaxed);
    if sink.is_cancelled() {
        return Err(CoreError::Cancelled);
    }
    if !status.success() {
        return Err(CoreError::Any(anyhow!("ddrescue: {status}")));
    }
    let elapsed = start.elapsed();
    sink.log(&format!(
        "✅ ddrescue {} complete in {:.1}s",
        if scrape { "-r3" } else { "-n" },
        elapsed.as_secs_f64()
    ));
    Ok(())
}

pub(super) fn run_dd(
    dev: &Path,
    iso: &Path,
    total: Option<u64>,
    sink: Arc<dyn ProgressSink>,
) -> CoreResult<()> {
    // 512 KiB per read — 256 DVD sectors, well-aligned and fast for optical drives
    const BUF_SIZE: usize = 256 * 2048;

    sink.log("Starting read…");
    let start = Instant::now();

    let mut src = fs::File::open(dev).map_err(CoreError::Io)?;
    let mut dst = fs::File::create(iso).map_err(CoreError::Io)?;

    let mut buf = vec![0u8; BUF_SIZE];
    let mut written: u64 = 0;
    let mut last_report = Instant::now();
    let mut last_written: u64 = 0;
    let mut avg_mbps: f64 = 0.0;

    loop {
        if sink.is_cancelled() {
            return Err(CoreError::Cancelled);
        }

        let n = match src.read(&mut buf) {
            Ok(0) => break, // EOF — disc fully read
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(CoreError::Io(e)),
        };

        dst.write_all(&buf[..n]).map_err(CoreError::Io)?;
        written += n as u64;

        let now = Instant::now();
        let dt = now.duration_since(last_report).as_secs_f64();
        if dt >= 0.5 {
            let delta = written.saturating_sub(last_written) as f64;
            let mbps = (delta / dt) / (1024.0 * 1024.0);
            if mbps.is_finite() && mbps > 0.0 {
                avg_mbps = if avg_mbps == 0.0 {
                    mbps
                } else {
                    avg_mbps * 0.7 + mbps * 0.3
                };
            }
            last_written = written;
            last_report = now;

            if let Some(total_bytes) = total.filter(|&t| t > 0) {
                let p = (written as f64 / total_bytes as f64).min(1.0) as f32;
                let global = DEVICE_PROGRESS_RIP.lerp(p);
                let remain = total_bytes.saturating_sub(written) as f64;
                let eta_s = if avg_mbps > 0.01 {
                    (remain / (avg_mbps * 1024.0 * 1024.0)) as u64
                } else {
                    0
                };
                let eta = if eta_s > 0 {
                    format!(
                        " — ETA {:02}:{:02}:{:02}",
                        eta_s / 3600,
                        (eta_s / 60) % 60,
                        eta_s % 60
                    )
                } else {
                    String::new()
                };
                sink.percent(global);
                sink.label(&format!(
                    "{:.0}% • {:.1} MB/s{eta}",
                    global * 100.0,
                    avg_mbps
                ));
            } else {
                sink.label(&format!(
                    "{:.0} MiB written • {:.1} MB/s",
                    written as f64 / (1024.0 * 1024.0),
                    avg_mbps
                ));
            }
        }
    }

    dst.flush().map_err(CoreError::Io)?;
    sink.log(&format!(
        "✅ Read complete in {:.1}s ({:.0} MiB)",
        start.elapsed().as_secs_f64(),
        written as f64 / (1024.0 * 1024.0)
    ));
    Ok(())
}

pub(super) fn disc_logical_size_bytes(dev: &Path) -> Option<u64> {
    if let Ok(out) = Command::new("isosize")
        .arg(dev)
        .stdout(Stdio::piped())
        .output()
    {
        if out.status.success() {
            if let Ok(s) = String::from_utf8(out.stdout) {
                if let Ok(n) = s.trim().parse::<u64>() {
                    return Some(n);
                }
            }
        }
    }
    if let Ok(out) = Command::new("isoinfo")
        .args(["-d", "-i"])
        .arg(dev)
        .stdout(Stdio::piped())
        .output()
    {
        if out.status.success() {
            let txt = String::from_utf8_lossy(&out.stdout);
            let mut blocks: Option<u64> = None;
            let mut block_size: u64 = 2048u64;
            for line in txt.lines() {
                if let Some(v) = line.strip_prefix("Volume size is:") {
                    blocks = v.split_whitespace().next().and_then(|n| n.parse().ok());
                } else if let Some(v) = line.strip_prefix("Logical block size is:") {
                    if let Ok(sz) = v.split_whitespace().next().unwrap_or("2048").parse::<u64>() {
                        block_size = sz.max(1);
                    }
                }
            }
            if let Some(b) = blocks {
                return Some(b.saturating_mul(block_size));
            }
        }
    }
    None
}

pub(super) fn poll_progress_until(
    iso: &Path,
    total: Option<u64>,
    range: ProgressRange,
    sink: Arc<dyn ProgressSink>,
    done: Arc<AtomicBool>,
) {
    let total = match total {
        Some(v) if v > 0 => v,
        _ => return,
    };
    let mut last = 0u64;
    let mut last_t = std::time::Instant::now();
    let mut avg_mbps = 0.0f64;
    loop {
        if sink.is_cancelled() || done.load(Ordering::Relaxed) {
            break;
        }
        if let Ok(meta) = fs::metadata(iso) {
            let done = meta.len().min(total);
            let now = std::time::Instant::now();
            let dt = now.duration_since(last_t).as_secs_f64();
            if dt >= 0.5 {
                let delta = done.saturating_sub(last) as f64;
                if dt > f64::EPSILON {
                    let mbps = (delta / dt) / (1024.0 * 1024.0);
                    if mbps.is_finite() {
                        avg_mbps = if avg_mbps == 0.0 {
                            mbps
                        } else {
                            avg_mbps * 0.7 + mbps * 0.3
                        };
                    }
                }
                last = done;
                last_t = now;
                let p = (done as f64 / total as f64) as f32;
                let global = range.lerp(p);
                let remain = (total.saturating_sub(done)) as f64;
                let eta_s = if avg_mbps > 0.01 {
                    (remain / (avg_mbps * 1024.0 * 1024.0)) as u64
                } else {
                    0
                };
                let eta = if eta_s > 0 {
                    format!(
                        " — ETA {:02}:{:02}:{:02}",
                        eta_s / 3600,
                        (eta_s / 60) % 60,
                        eta_s % 60
                    )
                } else {
                    String::new()
                };
                sink.percent(global);
                sink.label(&format!(
                    "{:.0}% • {:.1} MB/s{eta}",
                    global * 100.0,
                    avg_mbps
                ));
                if done >= total {
                    break;
                }
            }
        }
        thread::sleep(Duration::from_millis(350));
    }
}

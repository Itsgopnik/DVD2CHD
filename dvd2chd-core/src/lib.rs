//! dvd2chd-core – Core logic (platform-agnostic where possible), no GUI.
pub mod core_wiring;
pub mod process_guard;
pub mod tools;

mod hash;
mod util;
mod verify;

#[cfg(target_os = "linux")]
mod linux;

use anyhow::{anyhow, Result};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
};

use hash::log_hashes;
use util::{ensure_tool, unique_path, wait_with_cancel};
use verify::run_verify;

// ---------- Errors ----------

#[derive(thiserror::Error, Debug)]
pub enum CoreError {
    #[error("Tool missing or not executable: {0}")]
    MissingTool(&'static str),
    #[error("Verification failed")]
    VerifyFailed,
    #[error("Cancelled")]
    Cancelled,
    #[error("Not supported on this platform")]
    UnsupportedPlatform,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Any(#[from] anyhow::Error),
}

pub type CoreResult<T> = std::result::Result<T, CoreError>;

static CHDMAN_PERCENT_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(\d+)%").expect("compile chdman percentage regex"));
static CHDMAN_PERCENT_FLOAT_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(\d+(?:\.\d+)?)%").expect("compile chdman percentage (float) regex"));

// ---------- Progress/Interfaces ----------

pub trait ProgressSink: Send + Sync {
    fn log(&self, line: &str);
    fn percent(&self, p: f32);
    fn label(&self, text: &str);
    fn stage(&self, _event: StageEvent) {}
    fn is_cancelled(&self) -> bool;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Profile {
    #[default]
    Auto,
    PS1,
    PS2,
    GenericCd,
    PC,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ArchiveOptions {
    #[serde(default)]
    pub out_dir: Option<PathBuf>,
    #[serde(default)]
    pub custom_name: Option<String>,
    pub use_ddrescue: bool,
    pub ddrescue_scrape: bool,
    pub prefer_id_rename: bool,
    pub rename_by_label: bool,
    pub delete_image_after: bool,
    pub cd_speed_x: Option<u32>,
    pub cd_buffers: Option<u32>,
    pub extra_chd_args: String,
    pub run_nice: bool,
    pub run_ionice: bool,
    pub compute_md5: bool,
    pub compute_sha1: bool,
    pub compute_sha256: bool,
    pub auto_eject: bool,
    // Override tool paths (optional)
    pub chdman_path: Option<PathBuf>,
    pub ddrescue_path: Option<PathBuf>,
    pub cdrdao_path: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FileOptions {
    pub force_createdvd: Option<bool>, // None=Auto, Some(true)=DVD, Some(false)=CD
    pub extra_chd_args: String,
    pub run_nice: bool,
    pub run_ionice: bool,
    pub compute_md5: bool,
    pub compute_sha1: bool,
    pub compute_sha256: bool,
    pub delete_image_after: bool,
    pub chdman_path: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageEvent {
    RipStarted,
    RipFinished,
    ChdStarted,
    ChdFinished,
    VerifyStarted,
    VerifyFinished,
    HashStarted,
    HashFinished,
}

// ---------- Public High-Level APIs ----------

/// Archives a **device** (Linux: `/dev/srX`), creates CHD and returns final path.
pub fn archive_device(
    device: &Path,
    profile: Profile,
    opts: &ArchiveOptions,
    sink: Arc<dyn ProgressSink>,
) -> CoreResult<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        linux::archive_device_linux(device, profile, opts, sink)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(CoreError::UnsupportedPlatform)
    }
}

/// Converts a **file** (ISO/CUE) to CHD.
pub fn convert_file(
    input: &Path,
    out_dir: &Path,
    opts: &FileOptions,
    sink: Arc<dyn ProgressSink>,
) -> CoreResult<PathBuf> {
    let chdman = opts.chdman_path.clone().unwrap_or_else(|| {
        if cfg!(windows) {
            PathBuf::from("chdman.exe")
        } else {
            PathBuf::from("chdman")
        }
    });
    ensure_tool(&chdman, &["-help"]).map_err(|_| CoreError::MissingTool("chdman"))?;

    let ext = input
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let use_createdvd = opts.force_createdvd.unwrap_or(ext == "iso");

    let out_name = input
        .file_stem()
        .ok_or_else(|| CoreError::Any(anyhow!("No filename")))?;
    let chd_final = unique_path(out_dir.join(format!("{}.chd", out_name.to_string_lossy())));
    let chd_part = chd_final.with_extension("chd.part");
    let _ = fs::remove_file(&chd_part);

    // Call chdman
    let subcmd = if use_createdvd {
        "createdvd"
    } else {
        "createcd"
    };
    // Parse additional chdman arguments. Propagate parsing errors so users
    // receive feedback when supplying invalid shell syntax.
    let extras = match shell_words::split(&opts.extra_chd_args) {
        Ok(v) => v,
        Err(e) => {
            return Err(CoreError::Any(anyhow!("Error parsing extra_chd_args: {e}")));
        }
    };

    let mut base = Command::new(&chdman);
    base.arg(subcmd)
        .arg("-i")
        .arg(input)
        .arg("-o")
        .arg(&chd_part)
        .args(extras);
    let mut cmd = wrap_priority(base, opts.run_nice, opts.run_ionice);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    sink.stage(StageEvent::ChdStarted);
    sink.log(&format!(
        "Starting: {subcmd} {} → {}",
        input.display(),
        chd_part.display()
    ));

    let mut child = cmd
        .spawn()
        .map_err(|e| CoreError::Any(anyhow!("chdman not executable: {e}")))?;
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
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(c) = CHDMAN_PERCENT_RE.captures(&line) {
                    if let Ok(p) = c[1].parse::<f32>() {
                        s.percent((p / 100.0).min(1.0));
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
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                s.log(&line);
            }
        });
    }

    let status = wait_with_cancel(&mut child, || sink.is_cancelled()).map_err(CoreError::Io)?;
    if sink.is_cancelled() {
        let _ = fs::remove_file(&chd_part);
        return Err(CoreError::Cancelled);
    }
    if !status.success() {
        let _ = fs::remove_file(&chd_part);
        return Err(CoreError::Any(anyhow!("chdman exited with {}", status)));
    }

    sink.stage(StageEvent::ChdFinished);
    // Verify → Rename
    run_verify(&chdman, &chd_part, sink.clone())?;

    // Log compression ratio before renaming
    if let (Ok(src_meta), Ok(chd_meta)) = (input.metadata(), chd_part.metadata()) {
        let src_bytes = src_meta.len();
        let chd_bytes = chd_meta.len();
        if src_bytes > 0 {
            let savings = (1.0 - chd_bytes as f64 / src_bytes as f64) * 100.0;
            sink.log(&format!(
                "📦 {:.1} MB → {:.1} MB ({:.1}% smaller)",
                src_bytes as f64 / 1_048_576.0,
                chd_bytes as f64 / 1_048_576.0,
                savings,
            ));
        }
    }

    fs::rename(&chd_part, &chd_final).map_err(CoreError::Io)?;

    if opts.delete_image_after {
        if let Err(e) = fs::remove_file(input) {
            if e.kind() != std::io::ErrorKind::NotFound {
                sink.log(&format!(
                    "⚠ Could not delete source file {}: {e}",
                    input.display()
                ));
            }
        } else {
            sink.log(&format!("🗑 Deleted source: {}", input.display()));
        }
    }

    if opts.compute_md5 || opts.compute_sha1 || opts.compute_sha256 {
        sink.stage(StageEvent::HashStarted);
        // Use the helper to compute and log hashes. Errors propagate as CoreError::Any.
        log_hashes(
            &chd_final,
            opts.compute_md5,
            opts.compute_sha1,
            opts.compute_sha256,
            &sink,
        )
        .map_err(CoreError::Any)?;
        sink.stage(StageEvent::HashFinished);
        sink.percent(1.0);
    }

    Ok(chd_final)
}

// ---------- Extraction ----------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExtractMode {
    #[default]
    Auto,
    Dvd,
    Cd,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ExtractOptions {
    pub mode: ExtractMode,
    pub run_nice: bool,
    pub run_ionice: bool,
    pub chdman_path: Option<PathBuf>,
}

/// Extracts a CHD file back to ISO (DVD) or BIN+CUE (CD).
pub fn extract_chd(
    input: &Path,
    out_dir: &Path,
    opts: &ExtractOptions,
    sink: Arc<dyn ProgressSink>,
) -> CoreResult<PathBuf> {
    let chdman = opts.chdman_path.clone().unwrap_or_else(|| {
        if cfg!(windows) {
            PathBuf::from("chdman.exe")
        } else {
            PathBuf::from("chdman")
        }
    });
    ensure_tool(&chdman, &["-help"]).map_err(|_| CoreError::MissingTool("chdman"))?;

    let stem = input
        .file_stem()
        .ok_or_else(|| CoreError::Any(anyhow!("No filename")))?
        .to_string_lossy()
        .into_owned();

    match opts.mode {
        ExtractMode::Dvd => {
            let out = unique_path(out_dir.join(format!("{stem}.iso")));
            run_extract(&chdman, "extractdvd", input, &out, opts, sink)
        }
        ExtractMode::Cd => {
            let out = unique_path(out_dir.join(format!("{stem}.cue")));
            run_extract(&chdman, "extractcd", input, &out, opts, sink)
        }
        ExtractMode::Auto => {
            let iso = unique_path(out_dir.join(format!("{stem}.iso")));
            match run_extract(&chdman, "extractdvd", input, &iso, opts, sink.clone()) {
                Ok(p) => Ok(p),
                Err(dvd_err) => {
                    sink.log(&format!("extractdvd failed ({dvd_err}), retrying as CD…"));
                    let cue = unique_path(out_dir.join(format!("{stem}.cue")));
                    run_extract(&chdman, "extractcd", input, &cue, opts, sink)
                }
            }
        }
    }
}

fn run_extract(
    chdman: &Path,
    subcmd: &str,
    input: &Path,
    output: &Path,
    opts: &ExtractOptions,
    sink: Arc<dyn ProgressSink>,
) -> CoreResult<PathBuf> {
    sink.log(&format!(
        "Starting: {subcmd} {} → {}",
        input.display(),
        output.display()
    ));

    let mut base = Command::new(chdman);
    base.arg(subcmd).arg("-i").arg(input).arg("-o").arg(output);
    let mut cmd = wrap_priority(base, opts.run_nice, opts.run_ionice);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| CoreError::Any(anyhow!("chdman not executable: {e}")))?;
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
        let subcmd_owned = subcmd.to_owned();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(c) = CHDMAN_PERCENT_FLOAT_RE.captures(&line) {
                    if let Ok(p) = c[1].parse::<f32>() {
                        s.percent((p / 100.0).min(1.0));
                        s.label(&format!("{subcmd_owned}: {p:.1}%"));
                    }
                }
                s.log(&line);
            }
        });
    }
    {
        let s = sink.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                s.log(&line);
            }
        });
    }

    let status = wait_with_cancel(&mut child, || sink.is_cancelled()).map_err(CoreError::Io)?;
    if sink.is_cancelled() {
        let _ = fs::remove_file(output);
        return Err(CoreError::Cancelled);
    }
    if !status.success() {
        let _ = fs::remove_file(output);
        return Err(CoreError::Any(anyhow!(
            "chdman {subcmd} exited with {status}"
        )));
    }

    sink.percent(1.0);
    Ok(output.to_path_buf())
}

/// Compute MD5/SHA1/SHA-256 for a file.
pub fn compute_hashes(
    path: &Path,
    do_md5: bool,
    do_sha1: bool,
    do_sha256: bool,
) -> Result<(Option<String>, Option<String>, Option<String>)> {
    hash::compute_hashes(path, do_md5, do_sha1, do_sha256)
}

fn wrap_priority(base: Command, run_nice: bool, run_ionice: bool) -> Command {
    #[cfg(target_os = "linux")]
    {
        linux::chd::wrap_priority(base, run_nice, run_ionice)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (run_nice, run_ionice);
        base
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::sanitize_filename;
    use std::io::Write;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn sanitize_filename_replaces_disallowed_chars() {
        let input = "Sp!el:Name?<>|";
        let sanitized = sanitize_filename(input);
        assert_eq!(sanitized, "Sp_el_Name");
    }

    #[test]
    fn unique_path_appends_index_for_existing_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = dir.path().join("image.chd");
        std::fs::write(&base, b"dummy").expect("write base");
        let new_path = unique_path(base.clone());
        assert!(new_path != base);
        assert!(new_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("image (1)"));
    }

    #[test]
    fn compute_hashes_returns_expected_digests() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file_path = dir.path().join("data.bin");
        {
            let mut f = std::fs::File::create(&file_path).expect("create file");
            f.write_all(b"hello").expect("write data");
        }
        let (md5, sha1, sha256) = compute_hashes(&file_path, true, true, true).expect("hashes");
        assert_eq!(md5.as_deref(), Some("5d41402abc4b2a76b9719d911017c592"));
        assert_eq!(
            sha1.as_deref(),
            Some("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d")
        );
        assert_eq!(
            sha256.as_deref(),
            Some("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn wrap_priority_sets_nice_and_ionice() {
        use std::process::Command;
        use std::thread;
        use std::time::Duration;

        if which::which("sleep").is_err() {
            eprintln!("Skipping test: `sleep` command not available");
            return;
        }

        let mut cmd = Command::new("sleep");
        cmd.arg("0.25");
        let mut child = wrap_priority(cmd, true, true).spawn().expect("spawn sleep");
        let pid = child.id() as libc::c_int;
        // Allow the child process to enter sleep so the schedulers apply values.
        thread::sleep(Duration::from_millis(50));

        let nice = unsafe { libc::getpriority(libc::PRIO_PROCESS, pid as libc::id_t) };
        assert!(nice >= 10, "expected nice >= 10, got {nice} (pid {pid})");

        const IOPRIO_CLASS_SHIFT: u32 = 13;
        const IOPRIO_CLASS_BE: u32 = 2;
        const IOPRIO_WHO_PROCESS: libc::c_int = 1;

        let prio_raw = unsafe {
            libc::syscall(
                libc::SYS_ioprio_get,
                IOPRIO_WHO_PROCESS as libc::c_long,
                pid as libc::c_long,
                0 as libc::c_long,
            )
        };
        assert!(
            prio_raw != -1,
            "ioprio_get failed: {}",
            std::io::Error::last_os_error()
        );

        let prio = prio_raw as u32;
        let class = prio >> IOPRIO_CLASS_SHIFT;
        let data_mask = (1 << IOPRIO_CLASS_SHIFT) - 1;
        let data = prio & data_mask;
        assert_eq!(
            class, IOPRIO_CLASS_BE,
            "expected BE class, got {class} (pid {pid})"
        );
        assert_eq!(data, 7, "expected data value 7, got {data} (pid {pid})");

        child.wait().expect("wait child");
    }

    #[test]
    fn profile_default_is_auto() {
        assert_eq!(Profile::default(), Profile::Auto);
    }

    #[test]
    fn archive_options_serialization() {
        let opts = ArchiveOptions {
            out_dir: Some(PathBuf::from("/tmp")),
            custom_name: None,
            use_ddrescue: true,
            ddrescue_scrape: false,
            prefer_id_rename: true,
            rename_by_label: false,
            delete_image_after: true,
            cd_speed_x: Some(8),
            cd_buffers: Some(128),
            extra_chd_args: "-c zstd".to_string(),
            run_nice: true,
            run_ionice: false,
            compute_md5: true,
            compute_sha1: false,
            compute_sha256: false,
            auto_eject: false,
            chdman_path: None,
            ddrescue_path: None,
            cdrdao_path: None,
        };
        let json = serde_json::to_string(&opts).expect("serialize");
        let decoded: ArchiveOptions = serde_json::from_str(&json).expect("deserialize");
        assert!(decoded.use_ddrescue);
        assert_eq!(decoded.extra_chd_args, "-c zstd");
    }

    #[test]
    fn file_options_serialization() {
        let opts = FileOptions {
            force_createdvd: Some(true),
            extra_chd_args: "-c lzma".to_string(),
            run_nice: false,
            run_ionice: true,
            compute_md5: false,
            compute_sha1: true,
            compute_sha256: false,
            delete_image_after: false,
            chdman_path: Some(PathBuf::from("/usr/bin/chdman")),
        };
        let json = serde_json::to_string(&opts).expect("serialize");
        let decoded: FileOptions = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded.force_createdvd, Some(true));
        assert!(decoded.run_ionice);
    }

    #[test]
    fn core_error_display() {
        let err = CoreError::MissingTool("chdman");
        assert_eq!(err.to_string(), "Tool missing or not executable: chdman");

        let err = CoreError::VerifyFailed;
        assert_eq!(err.to_string(), "Verification failed");

        let err = CoreError::Cancelled;
        assert_eq!(err.to_string(), "Cancelled");
    }

    #[test]
    fn sanitize_preserves_allowed_chars() {
        assert_eq!(sanitize_filename("Game_Name-v1.0"), "Game_Name-v1.0");
        assert_eq!(
            sanitize_filename("Test (USA) [Disc 1]"),
            "Test (USA) [Disc 1]"
        );
    }

    #[test]
    fn sanitize_trims_underscores() {
        assert_eq!(sanitize_filename("___test___"), "test");
        assert_eq!(sanitize_filename("_a_b_"), "a_b");
    }

    struct TestSink {
        cancelled: AtomicBool,
    }
    impl ProgressSink for TestSink {
        fn log(&self, _line: &str) {}
        fn percent(&self, _p: f32) {}
        fn label(&self, _text: &str) {}
        fn is_cancelled(&self) -> bool {
            self.cancelled.load(std::sync::atomic::Ordering::Relaxed)
        }
    }

    #[test]
    fn test_sink_cancellation() {
        let sink = TestSink {
            cancelled: AtomicBool::new(false),
        };
        assert!(!sink.is_cancelled());
        sink.cancelled
            .store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(sink.is_cancelled());
    }
}

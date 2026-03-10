use anyhow::{Context, Result};
use std::{
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::Duration,
};

/// Hide the console window that would otherwise flash when spawning
/// a child process on Windows (e.g. chdman, powershell, wmic).
/// On non-Windows platforms this is a no-op.
#[cfg(windows)]
pub(crate) fn hide_console_window(cmd: &mut Command) -> &mut Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW)
}
#[cfg(not(windows))]
pub(crate) fn hide_console_window(cmd: &mut Command) -> &mut Command {
    cmd
}

pub(crate) fn ensure_tool(bin: &Path, args: &[&str]) -> Result<()> {
    let mut cmd = Command::new(bin);
    cmd.args(args).stdout(Stdio::null()).stderr(Stdio::null());
    hide_console_window(&mut cmd);
    cmd.status().map(|_| ()).context("Tool not executable")
}

pub(crate) fn unique_path(p: PathBuf) -> PathBuf {
    if !p.exists() {
        return p;
    }
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    let ext = p.extension().map(|e| e.to_string_lossy().to_string());
    for i in 1..10_000 {
        let name = if let Some(e) = &ext {
            format!("{stem} ({i}).{e}")
        } else {
            format!("{stem} ({i})")
        };
        let cand = p.with_file_name(name);
        if !cand.exists() {
            return cand;
        }
    }
    p
}

pub(crate) fn sanitize_filename(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        let ok = ch.is_ascii_alphanumeric() || " _-.,&+[](){}".contains(ch);
        out.push(if ok { ch } else { '_' });
    }
    out.trim_matches('_').to_string()
}

pub(crate) fn wait_with_cancel(
    child: &mut Child,
    cancelled: impl Fn() -> bool,
) -> std::io::Result<ExitStatus> {
    loop {
        if cancelled() {
            let _ = child.kill();
            return child.wait();
        }
        if let Some(st) = child.try_wait()? {
            return Ok(st);
        }
        thread::sleep(Duration::from_millis(120));
    }
}

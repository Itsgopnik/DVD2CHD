use anyhow::anyhow;
use regex::Regex;
use std::{
    fs,
    io::{BufRead, BufReader, Read as _, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
};

use crate::util::{ensure_tool, unique_path};
use crate::{CoreError, CoreResult, ProgressSink};

pub(super) fn rip_cd_raw(
    cdrdao: &Path,
    dev: &Path,
    base: &Path,
    speed: Option<u32>,
    buffers: Option<u32>,
    sink: Arc<dyn ProgressSink>,
) -> CoreResult<(PathBuf, PathBuf)> {
    ensure_tool(cdrdao, &["--version"]).map_err(|_| CoreError::MissingTool("cdrdao"))?;
    let bin = unique_path(base.with_extension("bin"));
    let toc = unique_path(base.with_extension("toc"));

    let mut cmd = Command::new(cdrdao);
    cmd.arg("read-cd")
        .arg("--read-raw")
        .arg("--datafile")
        .arg(&bin)
        .arg("--device")
        .arg(dev)
        .arg(&toc)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(x) = speed {
        cmd.arg("--speed").arg(x.to_string());
    }
    if let Some(b) = buffers {
        cmd.arg("--buffers").arg(b.to_string());
    }

    sink.log(&format!("cdrdao → {} / {}", bin.display(), toc.display()));
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
        std::thread::spawn(move || {
            for l in BufReader::new(stdout).lines().map_while(Result::ok) {
                s.log(&l);
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

    let status = child.wait().map_err(CoreError::Io)?;
    if !status.success() {
        return Err(CoreError::Any(anyhow!("cdrdao: {status}")));
    }
    Ok((bin, toc))
}

pub(super) fn ensure_cue_from_toc(
    toc: &Path,
    bin: &Path,
    sink: Arc<dyn ProgressSink>,
) -> CoreResult<PathBuf> {
    let cue = toc.with_extension("cue");
    let ok = Command::new("toc2cue")
        .arg(toc)
        .arg(&cue)
        .status()
        .ok()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        sink.log(&format!("✓ toc2cue: {}", cue.display()));
    } else {
        // Simple fallback: Single-Track DATA
        let mut txt = String::new();
        fs::File::open(toc)
            .and_then(|mut f| f.read_to_string(&mut txt))
            .map_err(CoreError::Io)?;
        let mut tracks = 0;
        let mut is_data = false;
        let mut mode = "MODE2/2352";
        for l in txt.lines().map(|l| l.trim().to_ascii_uppercase()) {
            if l.starts_with("TRACK") {
                tracks += 1;
                if l.contains("MODE1") {
                    is_data = true;
                    mode = "MODE1/2352";
                }
                if l.contains("MODE2") || l.contains("DATA") {
                    is_data = true;
                    mode = "MODE2/2352";
                }
            }
            if l.contains("AUDIO") {
                is_data = false;
            }
        }
        if tracks != 1 || !is_data {
            return Err(CoreError::Any(anyhow!(
                "toc2cue not available & fallback not possible ({} tracks).",
                tracks
            )));
        }
        let mut f = fs::File::create(&cue).map_err(CoreError::Io)?;
        writeln!(
            f,
            "FILE \"{}\" BINARY",
            bin.file_name().unwrap().to_string_lossy()
        )
        .ok();
        writeln!(f, "  TRACK 01 {mode}").ok();
        writeln!(f, "    INDEX 01 00:00:00").ok();
        sink.log(&format!("✓ Fallback CUE created: {}", cue.display()));
    }
    make_cue_paths_relative(&cue, Some(bin), sink)?;
    Ok(cue)
}

pub(super) fn make_cue_paths_relative(
    cue_path: &Path,
    preferred_bin: Option<&Path>,
    sink: Arc<dyn ProgressSink>,
) -> CoreResult<()> {
    let mut txt = String::new();
    fs::File::open(cue_path)
        .and_then(|mut f| f.read_to_string(&mut txt))
        .map_err(CoreError::Io)?;
    let re = Regex::new(r#"(?i)^\s*FILE\s+("([^"]+)"|(\S+))\s+(\S+)\s*$"#)
        .map_err(|e| CoreError::Any(anyhow!("Regex: {e}")))?;
    let mut out = String::with_capacity(txt.len() + 64);
    let mut did = false;
    let mut first = true;

    for line in txt.lines() {
        if let Some(c) = re.captures(line) {
            let ftype = c.get(4).map(|m| m.as_str()).unwrap_or("BINARY");
            let new_name = if first {
                first = false;
                preferred_bin
                    .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
                    .unwrap_or_else(|| {
                        let old = c.get(2).or_else(|| c.get(3)).unwrap().as_str();
                        Path::new(old)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| old.to_string())
                    })
            } else {
                let old = c.get(2).or_else(|| c.get(3)).unwrap().as_str();
                Path::new(old)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| old.to_string())
            };
            out.push_str(&format!(r#"FILE "{}" {}"#, new_name, ftype.to_uppercase()));
            out.push('\n');
            did = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }

    if did {
        fs::write(cue_path, out).map_err(CoreError::Io)?;
        sink.log(&format!("✎ CUE normalized: {}", cue_path.display()));
    }
    Ok(())
}

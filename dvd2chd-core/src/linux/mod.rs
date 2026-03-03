pub(super) mod cd;
pub(super) mod chd;
pub(super) mod dvd;

use anyhow::anyhow;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    hash::log_hashes,
    util::{ensure_tool, sanitize_filename, unique_path},
    ArchiveOptions, CoreError, CoreResult, Profile, ProgressSink, StageEvent,
};

use self::cd::rip_cd_raw;
use self::chd::{chd_cd_atomic, chd_dvd_atomic};
use self::dvd::rip_dvd;

#[derive(Clone, Copy)]
pub(crate) struct ProgressRange {
    pub(crate) start: f32,
    pub(crate) end: f32,
}

impl ProgressRange {
    pub(crate) const fn new(start: f32, end: f32) -> Self {
        Self { start, end }
    }

    pub(crate) fn lerp(self, t: f32) -> f32 {
        let clamped = t.clamp(0.0, 1.0);
        self.start + (self.end - self.start) * clamped
    }
}

pub(crate) const DEVICE_PROGRESS_RIP: ProgressRange = ProgressRange::new(0.0, 0.65);
pub(crate) const DEVICE_PROGRESS_CHD: ProgressRange = ProgressRange::new(0.65, 0.92);
pub(crate) const DEVICE_PROGRESS_VERIFY: ProgressRange = ProgressRange::new(0.92, 0.98);
pub(crate) const DEVICE_PROGRESS_HASH: ProgressRange = ProgressRange::new(0.98, 1.0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MediaKind {
    Unknown,
    Cd,
    Dvd,
}

pub fn archive_device_linux(
    dev: &Path,
    profile: Profile,
    opts: &ArchiveOptions,
    sink: Arc<dyn ProgressSink>,
) -> CoreResult<PathBuf> {
    if !dev.exists() {
        return Err(CoreError::Any(anyhow!(
            "Device {} not found or not mounted",
            dev.display()
        )));
    }

    // Check tools
    let chdman = opts
        .chdman_path
        .clone()
        .unwrap_or_else(|| PathBuf::from("chdman"));
    ensure_tool(&chdman, &["-help"]).map_err(|_| CoreError::MissingTool("chdman"))?;

    let ddrescue = opts
        .ddrescue_path
        .clone()
        .unwrap_or_else(|| PathBuf::from("ddrescue"));
    let cdrdao = opts
        .cdrdao_path
        .clone()
        .unwrap_or_else(|| PathBuf::from("cdrdao"));

    let media = query_media_kind(dev);
    sink.log(&format!("📀 Media: {:?} ({})", media, dev.display()));

    // Name
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut basename = format!("disc_{ts}");

    // Optional ID/Label directly from device (heuristic only)
    if opts.prefer_id_rename {
        if let Some(id) = ps_id_from_source(dev) {
            let safe = sanitize_filename(&id);
            if !safe.is_empty() {
                basename = safe;
            }
        }
    }
    if basename.starts_with("disc_") && opts.rename_by_label {
        if let Some(lbl) = read_volume_label_from_source(dev) {
            let safe = sanitize_filename(&lbl);
            if !safe.is_empty() {
                basename = safe;
            }
        }
    }
    if let Some(name) = &opts.custom_name {
        let safe = sanitize_filename(name);
        if !safe.is_empty() {
            basename = safe;
        }
    }

    // Paths
    let out_dir = opts
        .out_dir
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    fs::create_dir_all(&out_dir).map_err(CoreError::Io)?;
    let out_base = out_dir.join(&basename);

    // Automatically select profile if needed
    let chosen = if let Profile::Auto = profile {
        match media {
            MediaKind::Dvd => Profile::PS2, // pragmatic
            MediaKind::Cd => Profile::GenericCd,
            MediaKind::Unknown => Profile::PC,
        }
    } else {
        profile
    };
    sink.log(&format!("🔎 Profile selected: {:?}", chosen));

    let run_dvd_flow = |sink: Arc<dyn ProgressSink>| -> CoreResult<PathBuf> {
        // DVD → ISO → CHD
        let iso = unique_path(out_base.with_extension("iso"));
        let map = iso.with_extension("map");
        sink.stage(StageEvent::RipStarted);
        rip_dvd(
            &ddrescue,
            dev,
            &iso,
            opts.use_ddrescue,
            opts.ddrescue_scrape,
            sink.clone(),
        )?;
        sink.stage(StageEvent::RipFinished);
        sink.percent(DEVICE_PROGRESS_RIP.end);
        sink.label(&format!(
            "{:.0}% • Creating CHD…",
            DEVICE_PROGRESS_RIP.end * 100.0
        ));
        let chd = chd_dvd_atomic(
            &chdman,
            &iso,
            &out_base.with_extension("chd"),
            &opts.extra_chd_args,
            opts.run_nice,
            opts.run_ionice,
            sink.clone(),
        )?;
        log_compression_ratio(&iso, &chd, &sink);
        if opts.delete_image_after {
            remove_temp_file(&iso, &sink);
            if opts.use_ddrescue {
                remove_temp_file(&map, &sink);
            }
        }
        if opts.compute_md5 || opts.compute_sha1 || opts.compute_sha256 {
            sink.stage(StageEvent::HashStarted);
            // Log hashes via helper. Any error bubbles up as CoreError::Any.
            log_hashes(&chd, opts.compute_md5, opts.compute_sha1, opts.compute_sha256, &sink)
                .map_err(CoreError::Any)?;
            sink.stage(StageEvent::HashFinished);
            sink.percent(DEVICE_PROGRESS_HASH.end);
        }
        if !(opts.compute_md5 || opts.compute_sha1 || opts.compute_sha256) {
            sink.percent(1.0);
        }
        if opts.auto_eject {
            eject_drive(dev, &sink);
        }
        Ok(chd)
    };

    match media {
        MediaKind::Dvd => match chosen {
            Profile::PS2 | Profile::PC => run_dvd_flow(sink),
            other => Err(CoreError::Any(anyhow!(
                "Profile/media incompatible: {:?} & {:?}",
                other,
                MediaKind::Dvd
            ))),
        },
        MediaKind::Unknown => match chosen {
            Profile::PS2 | Profile::PC => {
                sink.log("⚠ Media could not be detected – treating as DVD.");
                run_dvd_flow(sink)
            }
            other => Err(CoreError::Any(anyhow!(
                "Profile/media incompatible: {:?} & {:?}",
                other,
                MediaKind::Unknown
            ))),
        },
        // CDs (PS1 / Generic / PC-CD)
        MediaKind::Cd => {
            ensure_tool(&cdrdao, &["--version"])
                .map_err(|_| CoreError::MissingTool("cdrdao"))?;
            sink.stage(StageEvent::RipStarted);
            let (bin, toc) = rip_cd_raw(
                &cdrdao,
                dev,
                &out_base,
                opts.cd_speed_x,
                opts.cd_buffers,
                sink.clone(),
            )?;
            sink.stage(StageEvent::RipFinished);
            sink.percent(DEVICE_PROGRESS_RIP.end);
            sink.label(&format!(
                "{:.0}% • Creating CHD…",
                DEVICE_PROGRESS_RIP.end * 100.0
            ));
            let chd = chd_cd_atomic(
                &chdman,
                &toc,
                &bin,
                &out_base.with_extension("chd"),
                &opts.extra_chd_args,
                opts.run_nice,
                opts.run_ionice,
                sink.clone(),
            )?;
            log_compression_ratio(&bin, &chd, &sink);
            if opts.delete_image_after {
                remove_temp_file(&bin, &sink);
                remove_temp_file(&toc, &sink);
                let cue = toc.with_extension("cue");
                remove_temp_file(&cue, &sink);
            }
            if opts.compute_md5 || opts.compute_sha1 || opts.compute_sha256 {
                sink.stage(StageEvent::HashStarted);
                log_hashes(&chd, opts.compute_md5, opts.compute_sha1, opts.compute_sha256, &sink)
                    .map_err(CoreError::Any)?;
                sink.stage(StageEvent::HashFinished);
                sink.percent(DEVICE_PROGRESS_HASH.end);
            }
            if !(opts.compute_md5 || opts.compute_sha1 || opts.compute_sha256) {
                sink.percent(1.0);
            }
            if opts.auto_eject {
                eject_drive(dev, &sink);
            }
            Ok(chd)
        }
    }
}

pub(crate) fn query_media_kind(dev: &Path) -> MediaKind {
    let out = Command::new("udevadm")
        .args(["info", "--query=property", "--name"])
        .arg(dev)
        .output();
    if let Ok(out) = out {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            let has = |k: &str| s.lines().any(|l| l.trim() == format!("{k}=1"));
            if has("ID_CDROM_MEDIA_DVD") {
                return MediaKind::Dvd;
            }
            if has("ID_CDROM_MEDIA_CD") {
                return MediaKind::Cd;
            }
        }
    }
    MediaKind::Unknown
}

pub(crate) fn read_volume_label_from_source(src: &Path) -> Option<String> {
    let out = Command::new("isoinfo")
        .args(["-d", "-i"])
        .arg(src)
        .stdout(Stdio::piped())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let txt = String::from_utf8_lossy(&out.stdout);
    for line in txt.lines() {
        if let Some(v) = line.strip_prefix("Volume id:") {
            let t = v.trim().to_string();
            if !t.is_empty() {
                return Some(t);
            }
        }
    }
    None
}

pub(crate) fn ps_id_from_source(src: &Path) -> Option<String> {
    let out = Command::new("isoinfo")
        .args(["-i"])
        .arg(src)
        .args(["-x", "/SYSTEM.CNF;1"])
        .stdout(Stdio::piped())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let txt = String::from_utf8_lossy(&out.stdout);
    let re = regex::Regex::new(r"BOOT\d?\s*=\s*cdrom0?:\\([^;]+);1").ok()?;
    let id = re.captures(&txt)?.get(1)?.as_str().to_string();
    Some(id.replace('\\', "/"))
}

/// Tries to eject an optical drive robustly:
/// 1. `udisksctl eject -b <dev>` — handles mounted discs on desktop Linux
/// 2. Fallback: `eject <dev>` — works on minimal systems without udisks2
fn eject_drive(dev: &Path, sink: &Arc<dyn ProgressSink>) {
    // Try udisksctl first (unmounts + ejects, works even if disc is auto-mounted)
    let udisks_ok = Command::new("udisksctl")
        .args(["eject", "-b"])
        .arg(dev)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if udisks_ok {
        sink.log(&format!("💿 Laufwerk ausgeworfen (udisksctl): {}", dev.display()));
        return;
    }

    // Fallback: plain eject
    match Command::new("eject").arg(dev).status() {
        Ok(s) if s.success() => {
            sink.log(&format!("💿 Laufwerk ausgeworfen: {}", dev.display()));
        }
        Ok(s) => {
            sink.log(&format!(
                "⚠ Auswerfen fehlgeschlagen (eject, Exit {}): {}",
                s.code().unwrap_or(-1),
                dev.display()
            ));
        }
        Err(e) => {
            sink.log(&format!(
                "⚠ Auswerfen fehlgeschlagen ({}): {}",
                e, dev.display()
            ));
        }
    }
}

fn log_compression_ratio(src: &Path, chd: &Path, sink: &Arc<dyn ProgressSink>) {
    if let (Ok(src_meta), Ok(chd_meta)) = (src.metadata(), chd.metadata()) {
        let src_bytes = src_meta.len();
        let chd_bytes = chd_meta.len();
        if src_bytes > 0 {
            let savings = (1.0 - chd_bytes as f64 / src_bytes as f64) * 100.0;
            sink.log(&format!(
                "📦 {:.1} MB → {:.1} MB ({:.1}% kleiner)",
                src_bytes as f64 / 1_048_576.0,
                chd_bytes as f64 / 1_048_576.0,
                savings,
            ));
        }
    }
}

pub(crate) fn remove_temp_file(path: &Path, sink: &Arc<dyn ProgressSink>) {
    if let Err(err) = fs::remove_file(path) {
        if err.kind() != std::io::ErrorKind::NotFound {
            sink.log(&format!(
                "⚠ Could not delete temporary file {}: {err}",
                path.display()
            ));
        }
    }
}

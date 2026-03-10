//! Windows-native optical disc ripper.
//!
//! Replaces cdrdao / ddrescue for `aarch64-pc-windows-msvc` and other
//! Windows targets. Uses Win32 IOCTLs directly — no extra dependencies.
//!
//! Flow:
//!   1. Enumerate optical drives  (`list_optical_drives`)
//!   2. Open drive handle
//!   3. Read TOC → parse tracks
//!   4. Probe: try raw-sector read → CD path; on failure → DVD path
//!   5. CD  : read 2352-byte raw sectors → BIN + generated CUE
//!      DVD : read 2048-byte data sectors → ISO
//!   6. chdman createcd / createdvd → verify → optional hash

use crate::{
    hash::log_hashes,
    util::{ensure_tool, sanitize_filename, unique_path, wait_with_cancel},
    verify::run_verify,
    ArchiveOptions, CoreError, CoreResult, ProgressSink, StageEvent, CHDMAN_PERCENT_RE,
};
use std::{
    ffi::OsString,
    fs::{self, File},
    io::{BufRead, BufReader, Write},
    os::windows::ffi::OsStringExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
};

// ── Progress ranges (mirror the Linux implementation) ────────────────────────
const PROG_RIP_END: f32 = 0.65;
const PROG_CHD_START: f32 = 0.65;
const PROG_CHD_END: f32 = 0.92;
const PROG_VERIFY_END: f32 = 0.98;

fn lerp(start: f32, end: f32, t: f32) -> f32 {
    start + (end - start) * t.clamp(0.0, 1.0)
}

/// Format a speed suffix like " — 12.3 MB/s" from bytes processed and elapsed seconds.
/// Returns empty string if elapsed < 0.5 s (to avoid noisy initial values).
fn speed_suffix(bytes_done: f64, elapsed_secs: f64) -> String {
    if elapsed_secs > 0.5 && bytes_done > 0.0 {
        format!(" — {:.1} MB/s", bytes_done / elapsed_secs / 1_048_576.0)
    } else {
        String::new()
    }
}

// ── Win32 types & constants ───────────────────────────────────────────────────
type Handle = isize;
const INVALID_HANDLE_VALUE: Handle = -1;

const GENERIC_READ: u32 = 0x80000000;
const FILE_SHARE_READ: u32 = 0x00000001;
const FILE_SHARE_WRITE: u32 = 0x00000002;
const OPEN_EXISTING: u32 = 3;
const FILE_BEGIN: u32 = 0;
const DRIVE_CDROM: u32 = 5;

// IOCTL codes
const IOCTL_CDROM_READ_TOC: u32 = 0x00024000;
const IOCTL_CDROM_RAW_READ: u32 = 0x0002403E;
const IOCTL_STORAGE_EJECT_MEDIA: u32 = 0x002D4808;

// Sector sizes
const SECTOR_DATA: usize = 2048;
const SECTOR_RAW: usize = 2352;

// TRACK_MODE_TYPE enum values (ntddcdrm.h)
const TRACK_MODE_YELLOW: u32 = 0; // YellowMode2  – Mode 1 and Mode 2 data
const TRACK_MODE_XA: u32 = 1; // XAForm2       – Mode 2 XA Form 2 (PS1)
const TRACK_MODE_CDDA: u32 = 2; // CDDA          – audio

// Read chunk size (sectors per ReadFile call for DVD)
const CHUNK_SECTORS: u32 = 64;

// Bad-sector retry limit
const DEFAULT_RETRIES: u32 = 3;

// ── Win32 FFI ─────────────────────────────────────────────────────────────────
extern "system" {
    fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        sa: usize,
        disp: u32,
        flags: u32,
        tmpl: Handle,
    ) -> Handle;
    fn ReadFile(h: Handle, buf: *mut u8, to_read: u32, read: *mut u32, ov: usize) -> i32;
    fn SetFilePointerEx(h: Handle, dist: i64, new_pos: *mut i64, method: u32) -> i32;
    fn DeviceIoControl(
        h: Handle,
        code: u32,
        in_buf: usize,
        in_size: u32,
        out_buf: usize,
        out_size: u32,
        returned: *mut u32,
        ov: usize,
    ) -> i32;
    fn CloseHandle(h: Handle) -> i32;
    fn GetLastError() -> u32;
    fn GetLogicalDrives() -> u32;
    fn GetDriveTypeW(root: *const u16) -> u32;
    fn GetVolumeInformationW(
        root: *const u16,
        vol_name: *mut u16,
        vol_name_len: u32,
        serial: *mut u32,
        max_comp: *mut u32,
        fs_flags: *mut u32,
        fs_name: *mut u16,
        fs_name_len: u32,
    ) -> i32;
}

// ── Handle RAII guard ─────────────────────────────────────────────────────────
struct DriveHandle(Handle);

impl Drop for DriveHandle {
    fn drop(&mut self) {
        if self.0 != INVALID_HANDLE_VALUE {
            unsafe { CloseHandle(self.0) };
        }
    }
}

// ── TOC structures (matches WDK CDROM_TOC / TRACK_DATA) ──────────────────────
const MAX_TOC_TRACKS: usize = 100;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RawTrackData {
    reserved: u8,
    // Windows TRACK_DATA bitfield layout (MSVC packs Control first = low nibble):
    //   bits 3-0 = Control  (data flag in bit 2: 0=audio, 1=data)
    //   bits 7-4 = Adr      (Q-channel mode, typically 1)
    control_adr: u8,
    track_number: u8,
    reserved1: u8,
    address: [u8; 4], // [0]=rsvd, [1]=min, [2]=sec, [3]=frame (MSF absolute)
}

impl RawTrackData {
    fn is_audio(&self) -> bool {
        // Control is in the low nibble (bits 3-0); bit 2 = data flag.
        // 0 → audio track, 1 → data track.
        (self.control_adr & 0x04) == 0
    }
    /// Logical (0-based) LBA, accounting for the 2-second (150-frame) pre-gap.
    fn logical_lba(&self) -> u32 {
        let m = self.address[1] as u32;
        let s = self.address[2] as u32;
        let f = self.address[3] as u32;
        ((m * 60 + s) * 75 + f).saturating_sub(150)
    }
}

#[repr(C)]
struct CdromToc {
    length: [u8; 2],
    first_track: u8,
    last_track: u8,
    track_data: [RawTrackData; MAX_TOC_TRACKS],
}

// ── IOCTL_CDROM_RAW_READ input structure ─────────────────────────────────────
#[repr(C)]
struct RawReadInfo {
    disk_offset: u64, // byte offset = logical_lba × 2048
    sector_count: u32,
    track_mode: u32,
}

// ── Parsed track ──────────────────────────────────────────────────────────────
#[derive(Debug, Clone)]
struct Track {
    number: u8,
    is_audio: bool,
    lba_start: u32, // inclusive, logical (0-based)
    lba_end: u32,   // exclusive
}

impl Track {
    fn sector_count(&self) -> u32 {
        self.lba_end.saturating_sub(self.lba_start)
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Returns device paths for every optical drive on the system,
/// e.g. `["\\\\.\\D:", "\\\\.\\E:"]`.
pub fn list_optical_drives() -> Vec<String> {
    let mut result = Vec::new();
    let mask = unsafe { GetLogicalDrives() };
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let letter = (b'A' + i as u8) as char;
        let root: Vec<u16> = format!("{}:\\", letter)
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        if unsafe { GetDriveTypeW(root.as_ptr()) } == DRIVE_CDROM {
            result.push(format!("\\\\.\\{}:", letter));
        }
    }
    result
}

/// Rip an optical drive to CHD.  Entry point called instead of `archive_device`
/// on Windows.
/// Eject the optical disc from the given drive path (e.g. "D:", "\\.\D:").
/// Returns Ok(()) on success, Err on failure.
pub fn eject_drive_windows(dev: &Path) -> CoreResult<()> {
    let dev_str = dev.to_string_lossy().to_string();
    let drive_letter = extract_drive_letter(&dev_str).ok_or_else(|| {
        CoreError::Any(anyhow::anyhow!(
            "Cannot parse drive letter from: {}",
            dev_str
        ))
    })?;
    let win32_path = format!("\\\\.\\{}:", drive_letter);
    let drive = open_drive(&win32_path)?;
    let mut returned: u32 = 0;
    let ok = unsafe {
        DeviceIoControl(
            drive.0,
            IOCTL_STORAGE_EJECT_MEDIA,
            0,
            0,
            0,
            0,
            &mut returned,
            0,
        )
    };
    if ok == 0 {
        let err = unsafe { GetLastError() };
        return Err(CoreError::Any(anyhow::anyhow!(
            "IOCTL_STORAGE_EJECT_MEDIA failed (Win32 err={})",
            err
        )));
    }
    Ok(())
}

pub fn archive_device_windows(
    dev: &Path,
    opts: &ArchiveOptions,
    sink: Arc<dyn ProgressSink>,
) -> CoreResult<PathBuf> {
    let dev_str = dev.to_string_lossy().to_string();

    // Resolve chdman
    let chdman = opts
        .chdman_path
        .clone()
        .unwrap_or_else(|| PathBuf::from("chdman.exe"));
    ensure_tool(&chdman, &["-help"]).map_err(|_| CoreError::MissingTool("chdman"))?;

    let out_dir = opts
        .out_dir
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    fs::create_dir_all(&out_dir).map_err(CoreError::Io)?;

    // Extract drive letter (e.g. 'D' from "\\.\D:" or "D:\")
    let drive_letter = extract_drive_letter(&dev_str).ok_or_else(|| {
        CoreError::Any(anyhow::anyhow!(
            "Cannot parse drive letter from: {}",
            dev_str
        ))
    })?;

    // Volume label → disc name
    let label = get_volume_label(drive_letter).unwrap_or_else(|| "disc".to_string());
    let raw_name = opts.custom_name.as_deref().unwrap_or(&label);
    let basename = {
        let s = sanitize_filename(raw_name);
        if s.is_empty() {
            "disc".to_string()
        } else {
            s
        }
    };

    // Always use the Win32 device namespace path (\\.\X:) for CreateFileW.
    // The incoming path may be "E:", "E:\", "\\.\E:", etc. — normalise it.
    let win32_path = format!("\\\\.\\{}:", drive_letter);

    sink.log(&format!("💿 Drive: {}  Label: {}", win32_path, label));

    // Open drive
    let drive = open_drive(&win32_path)?;

    // Read TOC
    let tracks = read_toc(drive.0)?;
    if tracks.is_empty() {
        return Err(CoreError::Any(anyhow::anyhow!("No tracks found on disc")));
    }
    let total_sectors: u32 = tracks.iter().map(|t| t.sector_count()).sum();
    let has_audio = tracks.iter().any(|t| t.is_audio);

    sink.log(&format!(
        "📋 Tracks: {}  Sectors: {}  Audio: {}",
        tracks.len(),
        total_sectors,
        has_audio
    ));

    // Detect disc type: probe raw-sector read to distinguish CD from DVD
    let is_cd = has_audio || probe_is_cd(drive.0);
    sink.log(&format!(
        "🔍 Disc type: {}",
        if is_cd { "CD (BIN+CUE)" } else { "DVD (ISO)" }
    ));

    let chd_out = unique_path(out_dir.join(format!("{}.chd", basename)));

    if is_cd {
        // ── CD path ──────────────────────────────────────────────────────────
        let bin_path = unique_path(out_dir.join(format!("{}.bin", basename)));
        let cue_path = bin_path.with_extension("cue");

        sink.stage(StageEvent::RipStarted);
        let sector_modes = rip_cd_to_bin(
            drive.0,
            &tracks,
            &bin_path,
            DEFAULT_RETRIES,
            total_sectors,
            &sink,
        )?;
        sink.stage(StageEvent::RipFinished);
        sink.percent(PROG_RIP_END);

        // Generate CUE
        let bin_name = bin_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        write_cue(&bin_name, &tracks, &sector_modes, &cue_path).map_err(CoreError::Io)?;
        sink.log(&format!("📄 CUE written: {}", cue_path.display()));

        // CHD
        let chd = run_chdman(
            &chdman,
            "createcd",
            &cue_path,
            &chd_out,
            &opts.extra_chd_args,
            &sink,
        )?;

        if opts.delete_image_after {
            let _ = fs::remove_file(&bin_path);
            let _ = fs::remove_file(&cue_path);
        }

        finish_job(&chd, &chdman, opts, &sink)?;
        if opts.auto_eject {
            eject_after_rip(dev, &sink);
        }
        Ok(chd)
    } else {
        // ── DVD path ──────────────────────────────────────────────────────────
        let iso_path = unique_path(out_dir.join(format!("{}.iso", basename)));

        sink.stage(StageEvent::RipStarted);
        rip_dvd_to_iso(drive.0, total_sectors, &iso_path, DEFAULT_RETRIES, &sink)?;
        sink.stage(StageEvent::RipFinished);
        sink.percent(PROG_RIP_END);

        // CHD
        let chd = run_chdman(
            &chdman,
            "createdvd",
            &iso_path,
            &chd_out,
            &opts.extra_chd_args,
            &sink,
        )?;

        if opts.delete_image_after {
            let _ = fs::remove_file(&iso_path);
        }

        finish_job(&chd, &chdman, opts, &sink)?;
        if opts.auto_eject {
            eject_after_rip(dev, &sink);
        }
        Ok(chd)
    }
}

fn eject_after_rip(dev: &Path, sink: &Arc<dyn ProgressSink>) {
    match eject_drive_windows(dev) {
        Ok(()) => sink.log(&format!("💿 Disc ausgeworfen: {}", dev.display())),
        Err(e) => sink.log(&format!("⚠ Auswerfen fehlgeschlagen: {e}")),
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

fn extract_drive_letter(dev: &str) -> Option<char> {
    // Accepts: "D:", "D:\", "\\.\D:", "\\\\.\\D:", "//.//D:", etc.
    for ch in dev.chars() {
        if ch.is_ascii_alphabetic() {
            return Some(ch.to_ascii_uppercase());
        }
    }
    None
}

fn str_to_wide_nul(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn get_volume_label(letter: char) -> Option<String> {
    let root = str_to_wide_nul(&format!("{}:\\", letter));
    let mut buf = vec![0u16; 256];
    let ok = unsafe {
        GetVolumeInformationW(
            root.as_ptr(),
            buf.as_mut_ptr(),
            buf.len() as u32,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        )
    };
    if ok == 0 {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let label = OsString::from_wide(&buf[..len])
        .to_string_lossy()
        .to_string();
    if label.is_empty() {
        None
    } else {
        Some(label)
    }
}

fn open_drive(path: &str) -> CoreResult<DriveHandle> {
    let wide = str_to_wide_nul(path);
    let h = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            0,
            OPEN_EXISTING,
            0, // no special flags – lets ReadFile and DeviceIoControl both work
            INVALID_HANDLE_VALUE,
        )
    };
    if h == INVALID_HANDLE_VALUE {
        let err = unsafe { GetLastError() };
        const ERROR_ACCESS_DENIED: u32 = 5;
        if err == ERROR_ACCESS_DENIED {
            return Err(CoreError::Any(anyhow::anyhow!(
                "Zugriff verweigert auf {}. \
                 Bitte die App als Administrator starten: \
                 Rechtsklick auf dvd2chd-gui.exe → \"Als Administrator ausführen\".",
                path
            )));
        }
        return Err(CoreError::Any(anyhow::anyhow!(
            "Cannot open drive {}: Win32 error {}",
            path,
            err
        )));
    }
    Ok(DriveHandle(h))
}

fn read_toc(h: Handle) -> CoreResult<Vec<Track>> {
    let mut toc: CdromToc = unsafe { std::mem::zeroed() };
    let mut returned = 0u32;
    let ok = unsafe {
        DeviceIoControl(
            h,
            IOCTL_CDROM_READ_TOC,
            0,
            0,
            &mut toc as *mut _ as usize,
            std::mem::size_of::<CdromToc>() as u32,
            &mut returned,
            0,
        )
    };
    if ok == 0 {
        let err = unsafe { GetLastError() };
        return Err(CoreError::Any(anyhow::anyhow!(
            "IOCTL_CDROM_READ_TOC failed: Win32 error {}",
            err
        )));
    }

    let first = toc.first_track as usize;
    let last = toc.last_track as usize;
    if first == 0 || last < first || last > MAX_TOC_TRACKS - 1 {
        return Err(CoreError::Any(anyhow::anyhow!("Invalid TOC")));
    }

    // track_data[0] = first_track, track_data[last-first+1] = lead-out (0xAA)
    let mut tracks = Vec::new();
    for i in 0..(last - first + 1) {
        let td = &toc.track_data[i];
        let next_td = &toc.track_data[i + 1]; // next track or lead-out
        tracks.push(Track {
            number: td.track_number,
            is_audio: td.is_audio(),
            lba_start: td.logical_lba(),
            lba_end: next_td.logical_lba(),
        });
    }
    Ok(tracks)
}

/// Returns `true` if the disc supports raw (2352-byte) sector reads → CD.
/// DVDs do not support IOCTL_CDROM_RAW_READ and will return an error.
fn probe_is_cd(h: Handle) -> bool {
    let info = RawReadInfo {
        disk_offset: 0,
        sector_count: 1,
        track_mode: TRACK_MODE_YELLOW,
    };
    let mut buf = [0u8; SECTOR_RAW];
    let mut returned = 0u32;
    let ok = unsafe {
        DeviceIoControl(
            h,
            IOCTL_CDROM_RAW_READ,
            &info as *const _ as usize,
            std::mem::size_of::<RawReadInfo>() as u32,
            buf.as_mut_ptr() as usize,
            SECTOR_RAW as u32,
            &mut returned,
            0,
        )
    };
    ok != 0
}

/// Read one raw sector (2352 bytes).  Returns `None` on unrecoverable error.
fn read_raw_sector(h: Handle, lba: u32, track_mode: u32) -> Option<[u8; SECTOR_RAW]> {
    let info = RawReadInfo {
        disk_offset: lba as u64 * SECTOR_DATA as u64,
        sector_count: 1,
        track_mode,
    };
    let mut buf = [0u8; SECTOR_RAW];
    let mut returned = 0u32;
    let ok = unsafe {
        DeviceIoControl(
            h,
            IOCTL_CDROM_RAW_READ,
            &info as *const _ as usize,
            std::mem::size_of::<RawReadInfo>() as u32,
            buf.as_mut_ptr() as usize,
            SECTOR_RAW as u32,
            &mut returned,
            0,
        )
    };
    if ok != 0 {
        Some(buf)
    } else {
        None
    }
}

/// Extract the mode byte (0x01 = Mode1, 0x02 = Mode2) from a raw sector.
fn sector_mode_byte(sector: &[u8; SECTOR_RAW]) -> u8 {
    // Raw sector layout: 12 sync + 3 address + 1 mode + ...
    // Sync: 00 FF FF FF FF FF FF FF FF FF FF 00
    if sector[0] == 0x00 && sector[11] == 0x00 {
        sector[15]
    } else {
        1 // default to Mode 1
    }
}

/// Rip all tracks to a single BIN file using raw (2352-byte) sector reads.
/// Returns a `Vec<u8>` where each entry is the mode byte of that track's first
/// data sector (1 = MODE1, 2 = MODE2; audio tracks get 0).
fn rip_cd_to_bin(
    h: Handle,
    tracks: &[Track],
    bin_path: &Path,
    retries: u32,
    total_sectors: u32,
    sink: &Arc<dyn ProgressSink>,
) -> CoreResult<Vec<u8>> {
    if total_sectors == 0 {
        return Err(CoreError::Any(anyhow::anyhow!(
            "Disc has 0 sectors — nothing to rip"
        )));
    }
    let mut file = File::create(bin_path).map_err(CoreError::Io)?;
    let mut sector_modes: Vec<u8> = Vec::with_capacity(tracks.len());
    let mut done_sectors = 0u32;
    let rip_start = std::time::Instant::now();

    sink.log(&format!(
        "Ripping {} sectors (raw 2352 B) → {}",
        total_sectors,
        bin_path.display()
    ));

    for track in tracks {
        let track_mode = if track.is_audio {
            TRACK_MODE_CDDA
        } else {
            TRACK_MODE_YELLOW
        };

        let mut first_mode_byte = if track.is_audio { 0u8 } else { 1u8 };
        let mut first_sector = true;

        for lba in track.lba_start..track.lba_end {
            if sink.is_cancelled() {
                return Err(CoreError::Cancelled);
            }

            // Retry loop for bad sectors
            let mut sector_buf = None;
            for attempt in 0..=retries {
                if let Some(buf) = read_raw_sector(h, lba, track_mode) {
                    sector_buf = Some(buf);
                    break;
                }
                let err = unsafe { GetLastError() };
                // Last chance: try XA mode for data sectors
                if !track.is_audio && attempt == retries - 1 {
                    if let Some(buf) = read_raw_sector(h, lba, TRACK_MODE_XA) {
                        sector_buf = Some(buf);
                        break;
                    }
                }
                if attempt < retries {
                    sink.log(&format!(
                        "⚠ Retry sector {} (attempt {}, err={})",
                        lba,
                        attempt + 1,
                        err
                    ));
                }
            }

            let buf = match sector_buf {
                Some(b) => b,
                None => {
                    // Pad with zeros so the BIN file stays aligned
                    sink.log(&format!("✖ Bad sector {} – padding with zeros", lba));
                    [0u8; SECTOR_RAW]
                }
            };

            if first_sector && !track.is_audio {
                first_mode_byte = sector_mode_byte(&buf);
                first_sector = false;
            }

            file.write_all(&buf).map_err(CoreError::Io)?;
            done_sectors += 1;

            // Progress every 32 sectors
            if done_sectors.is_multiple_of(32) {
                let t = done_sectors as f32 / total_sectors as f32;
                sink.percent(lerp(0.0, PROG_RIP_END, t));
                let spd = speed_suffix(
                    done_sectors as f64 * SECTOR_RAW as f64,
                    rip_start.elapsed().as_secs_f64(),
                );
                sink.label(&format!("Rip: {:.0}%{spd}", t * 100.0));
            }
        }

        sector_modes.push(first_mode_byte);
    }

    sink.log(&format!("✔ BIN written: {} sectors", done_sectors));
    Ok(sector_modes)
}

/// Rip a data DVD (or data-only CD) to an ISO file using ReadFile.
fn rip_dvd_to_iso(
    h: Handle,
    total_sectors: u32,
    iso_path: &Path,
    retries: u32,
    sink: &Arc<dyn ProgressSink>,
) -> CoreResult<()> {
    if total_sectors == 0 {
        return Err(CoreError::Any(anyhow::anyhow!(
            "Disc has 0 sectors — nothing to rip"
        )));
    }
    // Seek to disc start
    let ok = unsafe { SetFilePointerEx(h, 0, std::ptr::null_mut(), FILE_BEGIN) };
    if ok == 0 {
        return Err(CoreError::Any(anyhow::anyhow!("SetFilePointerEx failed")));
    }

    let mut file = File::create(iso_path).map_err(CoreError::Io)?;
    let chunk = CHUNK_SECTORS as usize * SECTOR_DATA;
    let mut buf = vec![0u8; chunk];
    let mut done_sectors = 0u32;
    let rip_start = std::time::Instant::now();

    sink.log(&format!(
        "Ripping {} sectors (ISO 2048 B) → {}",
        total_sectors,
        iso_path.display()
    ));

    while done_sectors < total_sectors {
        if sink.is_cancelled() {
            return Err(CoreError::Cancelled);
        }

        let remaining = total_sectors - done_sectors;
        let to_read = (remaining.min(CHUNK_SECTORS) as usize) * SECTOR_DATA;
        let buf_slice = &mut buf[..to_read];

        let mut read_bytes = 0u32;
        let mut ok = 0i32;

        for attempt in 0..=retries {
            read_bytes = 0;
            ok = unsafe {
                ReadFile(
                    h,
                    buf_slice.as_mut_ptr(),
                    to_read as u32,
                    &mut read_bytes,
                    0,
                )
            };
            if ok != 0 && read_bytes > 0 {
                break;
            }
            if attempt < retries {
                sink.log(&format!(
                    "⚠ Read error at sector {} (attempt {})",
                    done_sectors,
                    attempt + 1
                ));
            }
        }

        if ok == 0 || read_bytes == 0 {
            // Pad the remaining sectors with zeros and advance
            let pad_sectors = (to_read / SECTOR_DATA) as u32;
            sink.log(&format!(
                "✖ Unrecoverable read error at sector {} – padding {} sectors",
                done_sectors, pad_sectors
            ));
            let zeros = vec![0u8; to_read];
            file.write_all(&zeros).map_err(CoreError::Io)?;
            done_sectors += pad_sectors;
        } else {
            file.write_all(&buf_slice[..read_bytes as usize])
                .map_err(CoreError::Io)?;
            done_sectors += (read_bytes as usize / SECTOR_DATA) as u32;
        }
        let t = done_sectors as f32 / total_sectors as f32;
        sink.percent(lerp(0.0, PROG_RIP_END, t));
        let spd = speed_suffix(
            done_sectors as f64 * SECTOR_DATA as f64,
            rip_start.elapsed().as_secs_f64(),
        );
        sink.label(&format!("Rip: {:.0}%{spd}", t * 100.0));
    }

    sink.log(&format!("✔ ISO written: {} sectors", done_sectors));
    Ok(())
}

/// Write a CUE sheet for a BIN file.
/// `sector_modes[i]` is the mode byte for track `i` (0=audio, 1=Mode1, 2=Mode2).
fn write_cue(
    bin_name: &str,
    tracks: &[Track],
    sector_modes: &[u8],
    cue_path: &Path,
) -> std::io::Result<()> {
    let mut f = File::create(cue_path)?;
    writeln!(f, "FILE \"{}\" BINARY", bin_name)?;

    let mut bin_lba: u32 = 0; // running offset in the BIN file (in sectors)
    for (i, track) in tracks.iter().enumerate() {
        let track_type = if track.is_audio {
            "AUDIO"
        } else {
            match sector_modes.get(i).copied().unwrap_or(1) {
                2 => "MODE2/2352",
                _ => "MODE1/2352",
            }
        };

        writeln!(f, "  TRACK {:02} {}", track.number, track_type)?;

        // Index position = offset within BIN file expressed as MM:SS:FF
        let ff = bin_lba % 75;
        let ss = (bin_lba / 75) % 60;
        let mm = bin_lba / (75 * 60);
        writeln!(f, "    INDEX 01 {:02}:{:02}:{:02}", mm, ss, ff)?;

        bin_lba += track.sector_count();
    }
    Ok(())
}

/// Run `chdman createcd` or `chdman createdvd`, parse progress, return CHD path.
fn run_chdman(
    chdman: &Path,
    subcmd: &str, // "createcd" or "createdvd"
    input: &Path,
    out_chd: &Path,
    extra_args: &str,
    sink: &Arc<dyn ProgressSink>,
) -> CoreResult<PathBuf> {
    let tmp = out_chd.with_extension("chd.part");
    let _ = fs::remove_file(&tmp);

    let extras = match shell_words::split(extra_args) {
        Ok(v) => v,
        Err(e) => {
            return Err(CoreError::Any(anyhow::anyhow!(
                "Invalid extra_chd_args: {}",
                e
            )));
        }
    };

    let mut cmd = Command::new(chdman);
    cmd.arg(subcmd)
        .arg("-i")
        .arg(input)
        .arg("-o")
        .arg(&tmp)
        .args(&extras)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::util::hide_console_window(&mut cmd);

    sink.stage(StageEvent::ChdStarted);
    sink.log(&format!(
        "{} {} → {}",
        subcmd,
        input.display(),
        tmp.display()
    ));

    // Get input file size for speed calculation
    let input_bytes = input.metadata().map(|m| m.len()).unwrap_or(0);

    let mut child = cmd
        .spawn()
        .map_err(|e| CoreError::Any(anyhow::anyhow!("chdman spawn failed: {}", e)))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| CoreError::Any(anyhow::anyhow!("stdout not piped")))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| CoreError::Any(anyhow::anyhow!("stderr not piped")))?;

    {
        let s = sink.clone();
        let re = &*CHDMAN_PERCENT_RE;
        let chd_start = std::time::Instant::now();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(c) = re.captures(&line) {
                    if let Ok(p) = c[1].parse::<f32>() {
                        s.percent(lerp(PROG_CHD_START, PROG_CHD_END, p / 100.0));
                        let spd = speed_suffix(
                            input_bytes as f64 * (p as f64 / 100.0),
                            chd_start.elapsed().as_secs_f64(),
                        );
                        s.label(&format!("CHD: {:.0}%{spd}", p));
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
        let _ = fs::remove_file(&tmp);
        return Err(CoreError::Cancelled);
    }
    if !status.success() {
        let _ = fs::remove_file(&tmp);
        return Err(CoreError::Any(anyhow::anyhow!(
            "chdman {} failed: {}",
            subcmd,
            status
        )));
    }

    sink.stage(StageEvent::ChdFinished);
    run_verify(chdman, &tmp, sink.clone())?;
    sink.percent(PROG_VERIFY_END);

    fs::rename(&tmp, out_chd).map_err(CoreError::Io)?;
    Ok(out_chd.to_path_buf())
}

/// After CHD is created: optional hashing → final progress.
fn finish_job(
    chd: &Path,
    _chdman: &Path,
    opts: &ArchiveOptions,
    sink: &Arc<dyn ProgressSink>,
) -> CoreResult<()> {
    if opts.compute_md5 || opts.compute_sha1 || opts.compute_sha256 {
        sink.stage(StageEvent::HashStarted);
        log_hashes(
            chd,
            opts.compute_md5,
            opts.compute_sha1,
            opts.compute_sha256,
            sink,
        )
        .map_err(CoreError::Any)?;
        sink.stage(StageEvent::HashFinished);
    }
    sink.percent(1.0);
    Ok(())
}

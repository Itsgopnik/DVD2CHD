# DVD2CHD

> **This is a fork** of the original [DVD2CHD](https://github.com/itsgopnik/dvd2chd) project.
> The main additions are a **Windows-native optical disc ripper** and a redesigned GUI with a
> clean Notion/Arc-inspired design and smooth continuous animations.

A Rust tool for archiving optical media (DVDs and CDs) to compressed CHD (Compressed Hunks of Data)
format, with a modern graphical interface.

---

## What's new in this fork

| Change | Details |
|--------|---------|
| **Windows device ripping** | Native Win32 ripper replaces cdrdao/ddrescue on Windows. CD and DVD ripping work out of the box — no third-party ripping tools needed. Disc eject via `IOCTL_STORAGE_EJECT_MEDIA` (manual button + auto-eject after rip). |
| **GUI redesign** | "Soft & Neutral" design inspired by Notion/Arc Browser: warm neutral tones, soft lavender accents, 12 px window / 8 px widget rounding, neutral drop shadows. System Auto theme (follows OS dark/light) + High-Contrast option. No console window in release builds (debug builds keep the console for diagnostics). |
| **Smooth animations** | Continuous per-stage animations (Rip, Compress, Verify, Hash) with seamless phase wrap-around, edge-fading packages, idle breathing effects, and slow CPU-friendly repaint when idle. |
| **Taskbar progress** | Windows taskbar shows real-time job progress (green bar on the taskbar icon) via raw COM `ITaskbarList3` — no extra crate dependencies. |
| **Desktop notifications** | Cross-platform "job finished" notifications via `notify-rust` (Windows toast, Linux D-Bus, macOS alerts). Replaces the previous Linux-only `notify-send` approach. |
| **Live speed display** | Progress bar shows real-time read/write speed (MB/s) during ripping and CHD compression — both device ripping and file conversion. |
| **Smooth progress bar** | Progress bar interpolates smoothly toward the target value instead of jumping. Resets snap instantly when a new job starts. |
| **Theme crossfade** | Switching between System Auto and High-Contrast fades smoothly over 300 ms instead of a hard cut. |
| **Tooltips** | All buttons, checkboxes, and interactive elements now show descriptive hover text (localized DE/EN). |
| **App icon** | Custom `.ico` embedded in the EXE via `winres` — shows up in Explorer, taskbar, and the title bar. |
| **Silent subprocesses** | All child processes (chdman, wmic) launch with `CREATE_NO_WINDOW` — no console flashes. Drive detection uses native `GetDriveTypeW` instead of PowerShell. |
| **ARM64 Windows support** | Tested and builds cleanly on `aarch64-pc-windows-msvc`. |

---

## Features

- **Multi-profile support** — PS1, PS2, generic CD, and PC disc profiles
- **Device archiving** — Direct ripping from optical drives (Linux via cdrdao/ddrescue; Windows via native Win32 IOCTLs)
- **File conversion** — Convert existing ISO/CUE files to CHD (all platforms)
- **Auto-detection** — Automatically detects media type (CD vs DVD) and selects optimal settings
- **Verification** — Built-in CHD integrity check after creation
- **Hashing** — Optional MD5/SHA1/SHA256 hash computation
- **Process priority** — `nice` and `ionice` support on Linux to keep the system responsive
- **Auto-install** — Missing tools can be installed via system package manager (Linux)
- **Modern GUI** — egui-based interface with dark/light/high-contrast themes, animations, batch mode

---

## System Requirements

### Runtime Dependencies

#### Windows

| Tool | Required | Where to get it |
|------|----------|-----------------|
| `chdman.exe` | **Yes** | [MAME Tools](https://www.mamedev.org/release.html) — download the MAME package and extract `chdman.exe` |

> No ripping tools (cdrdao, ddrescue, dd) are required on Windows.
> The built-in Windows ripper uses `DeviceIoControl` IOCTLs directly.
>
> Set the `chdman.exe` path in **Options → Tools** if it is not on your `PATH`.

#### Linux

| Tool | Required | Notes |
|------|----------|-------|
| `chdman` | **Yes** | From MAME tools — for CHD creation |
| `cdrdao` | For CD ripping | Raw CD audio/data capture |
| `ddrescue` | Optional | Better error recovery for damaged discs |
| `isoinfo` / `isosize` | Optional | More accurate disc size detection |

> `dd` has been replaced by a native Rust implementation — no external `dd` binary required.

---

## Building

### Windows — x64 (most common)

**Step 1 — Install Rust**

```powershell
winget install Rustlang.Rustup
# Then open a new terminal so rustup is on PATH
rustup toolchain install stable-x86_64-pc-windows-msvc
rustup default stable-x86_64-pc-windows-msvc
```

**Step 2 — Install Visual Studio Build Tools**

Download and run the [VS Build Tools installer](https://visualstudio.microsoft.com/visual-cpp-build-tools/).
Select the **"Desktop development with C++"** workload. The MSVC compiler and Windows SDK are
included automatically.

Alternatively, install the full Visual Studio 2022 Community edition.

**Step 3 — Install LLVM**

The `ring` crate (used by hashing) requires `clang` on Windows.

```powershell
winget install LLVM.LLVM
# Restart your terminal after this
```

Verify: `clang --version` should print something like `clang version 19.x.x`.

**Step 4 — Build**

```powershell
cargo build --release
# Binary: target\release\dvd2chd-gui.exe
```

That's it for x64. If `cargo build` fails on the `ring` crate with a message about `clang` not
found, set the environment variable explicitly:

```powershell
$env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"
cargo build --release
```

---

### Windows — ARM64 (`aarch64-pc-windows-msvc`)

ARM64 requires a few extra steps because the MSVC ARM64 toolchain and LLVM ARM64 binaries need to
be explicitly pointed at.

**Step 1 — Install Rust with the ARM64 target**

```powershell
winget install Rustlang.Rustup
rustup toolchain install stable-aarch64-pc-windows-msvc
rustup default stable-aarch64-pc-windows-msvc
```

**Step 2 — Install Visual Studio with the ARM64 build tools**

In the VS Installer, under **"Desktop development with C++"**, enable the optional component:
- **MSVC v143 – VS 2022 C++ ARM64/ARM64EC build tools (latest)**

And also install the **Windows 11 SDK**.

**Step 3 — Install LLVM for ARM64**

```powershell
winget install LLVM.LLVM
```

This installs the native ARM64 build of LLVM on ARM64 Windows.

**Step 4 — Set environment variables permanently (run once)**

The ARM64 MSVC and LLVM paths are not on `PATH` by default. A setup script is included that
writes `LIB`, `INCLUDE`, `CC`, `AR` and the required `PATH` entries permanently to your
**user** environment — no administrator rights needed, and you never have to repeat this.

**From PowerShell** (if `Set-ExecutionPolicy` is available):
```powershell
Set-ExecutionPolicy -Scope CurrentUser -ExecutionPolicy RemoteSigned
.\setup-env-permanent.ps1
```

**From cmd.exe** (if you get "command not found" for `Set-ExecutionPolicy`):
```cmd
powershell -ExecutionPolicy Bypass -File .\setup-env-permanent.ps1
```

Edit the two path variables at the top of [`setup-env-permanent.ps1`](setup-env-permanent.ps1)
if your VS version number differs from `14.50.35717`. You can find the correct version under
`C:\Program Files\Microsoft Visual Studio\18\Insiders\VC\Tools\MSVC\`.

After the script finishes:

1. **Close** the current terminal (and VS Code if it is open)
2. **Reopen** VS Code / a new terminal
3. `cargo run -p dvd2chd-gui` — works from now on without any extra steps

**Per-session alternative:** If you prefer not to set permanent variables, dot-source
[`dev.ps1`](dev.ps1) at the start of each terminal session instead:

```powershell
. .\dev.ps1
cargo run -p dvd2chd-gui
```

**Resulting binary:** `target\release\dvd2chd-gui.exe`

---

### Linux

```bash
# Debian / Ubuntu
sudo apt install build-essential pkg-config libgtk-3-dev

# Fedora
sudo dnf install gcc pkg-config gtk3-devel

cargo build --release
# Binary: target/release/dvd2chd-gui
```

### Install Required Tools (Linux)

**Debian / Ubuntu:**
```bash
sudo apt install mame-tools cdrdao gddrescue
```

**Arch Linux:**
```bash
sudo pacman -S cdrdao ddrescue
# chdman: AUR → yay -S chdman  — or set the path in Options → Tools
```

**Fedora:**
```bash
sudo dnf install mame cdrdao ddrescue
```

> The GUI can also install missing tools automatically via the **"⬇ Installieren"** button in the
> toolbar (uses `pkexec` for privilege escalation).

---

## Usage

**During development** (compiles and runs in one step):

```powershell
# Windows (ARM64: run . .\dev.ps1 first if env vars are not set)
cargo run -p dvd2chd-gui

# Linux
cargo run -p dvd2chd-gui
```

**Run the already-built release binary:**

```powershell
# Windows
.\target\release\dvd2chd-gui.exe

# Linux
./target/release/dvd2chd-gui
```

### Quick Start

> **Windows:** The app requests Administrator rights automatically via UAC on every launch
> (required for raw optical drive access). Click **Yes** in the UAC prompt to continue.

1. Select a **source** — ISO/CUE file *or* an optical drive (e.g. `D:\` on Windows, `/dev/sr0` on Linux)
2. Choose an **output folder**
3. Select a **profile** (Auto works for most discs)
4. Click **▶ Start**

### Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Ctrl+S` | Start job |
| `Ctrl+O` | Open file |
| `Ctrl+G` | Set device path |

---

## Architecture

Cargo workspace with two crates:

```
dvd2chd/
├── dvd2chd-core/          # Core logic (platform-aware)
│   └── src/
│       ├── lib.rs             # Public API, types, re-exports
│       ├── util.rs            # sanitize_filename, unique_path, …
│       ├── hash.rs            # MD5/SHA1/SHA256 computation
│       ├── verify.rs          # CHD verification (chdman verify)
│       ├── windows_rip.rs     # ★ Windows-native CD/DVD ripper (Win32 IOCTLs)
│       └── linux/
│           ├── mod.rs         # archive_device_linux, media detection
│           ├── dvd.rs         # DVD ripping (native Rust reader + ddrescue)
│           ├── cd.rs          # CD ripping via cdrdao, CUE handling
│           └── chd.rs         # chdman invocation, priority wrapping
└── dvd2chd-gui/           # egui/eframe GUI
    └── src/
        ├── main.rs
        ├── drive.rs           # Drive enumeration (Linux/Windows/macOS)
        ├── pkg_install.rs     # System package manager auto-install
        ├── tool_fetch.rs      # Binary download via manifest URL
        └── app/               # GUI application (split into submodules)
            ├── mod.rs         # App struct, update loop
            ├── animation.rs   # Stage animations (Rip, CHD, Verify, Hash)
            ├── draw_layout.rs / draw_toolbar.rs / draw_dialogs.rs
            ├── job.rs / timeline.rs / workflow.rs
            ├── tools_check.rs / source.rs / presets.rs / log.rs
            ├── taskbar.rs     # Windows taskbar progress (raw COM ITaskbarList3)
            └── state.rs
```

### Windows Ripper (`windows_rip.rs`)

The Windows ripper replaces cdrdao and ddrescue using only Win32 API calls — no extra
runtime dependencies or third-party ripping tools are needed.

**Pipeline:**

```
Open drive (CreateFileW on \\.\D:)
    │
    ├─ Read TOC (IOCTL_CDROM_READ_TOC)
    │       MSF addresses → LBA offsets, track type flags
    │
    ├─ Probe disc type
    │       Try IOCTL_CDROM_RAW_READ on sector 0
    │       Succeeds → CD path    Fails (DVDs reject raw reads) → DVD path
    │
    ├─ CD path ──────────────────────────────────────────────────────────────
    │   Read 2352-byte raw sectors per track (IOCTL_CDROM_RAW_READ)
    │   Detect sector mode from header byte 15 → MODE1/2352, MODE2/2352, AUDIO
    │   Bad-sector retry (3×, fallback mode XAForm2, then zero-pad)
    │   Write .bin file + generated .cue sheet
    │   chdman createcd -i disc.cue -o disc.chd.part
    │
    └─ DVD path ─────────────────────────────────────────────────────────────
        Read 2048-byte data sectors in 64-sector chunks (ReadFile + SetFilePointerEx)
        Write .iso file
        chdman createdvd -i disc.iso -o disc.chd.part
            │
            └─ run_verify → optional hash → rename .chd.part → .chd
                    │
                    └─ optional: IOCTL_STORAGE_EJECT_MEDIA (auto-eject)
```

**Progress ranges (mirrors Linux):**

| Stage | Range |
|-------|-------|
| Rip   | 0 % → 65 % |
| CHD   | 65 % → 92 % |
| Verify | 92 % → 98 % |
| Hash  | 98 % → 100 % |

### Key Design Patterns

- **`ProgressSink` trait** — unified progress reporting from core to GUI; same interface on Linux and Windows
- **Cooperative cancellation** — `is_cancelled()` polling, no forceful kills mid-write
- **Atomic writes** — `.chd.part` files ensure crash-safe output; renamed to `.chd` only on success
- **`PhaseAnim` struct** — exponential-decay animation smoothing with seamless wrap-around (integer-factor phase multipliers ensure visual continuity at cycle boundaries)
- **Idle breathing** — time-based sine-wave pulse when no job is running (100 ms repaint interval to save CPU)
- **Smooth progress** — `progress_display` field smoothly interpolates toward the true `progress` value using exponential decay, snapping on reset
- **Taskbar progress** — raw COM `ITaskbarList3` FFI on Windows (no crate dependency), lazy-initialized on first use
- **Theme crossfade** — `lerp_palette()` blends old/new `ThemePalette` over 300 ms when the effective theme changes
- **Silent subprocesses** — `CREATE_NO_WINDOW` flag on all Windows `Command` spawns via `hide_console_window()` helper; drive detection uses native `GetLogicalDriveStringsW` + `GetDriveTypeW` instead of PowerShell

---

## Profiles

| Profile | Media | Linux tools | Windows tools |
|---------|-------|-------------|---------------|
| **Auto** | Any | Detected via `udevadm` | Detected via IOCTL probe |
| **PS1** | CD | `cdrdao` (raw) → `chdman createcd` | Win32 raw read → `chdman createcd` |
| **PS2** | DVD | native reader / `ddrescue` → `chdman createdvd` | Win32 read → `chdman createdvd` |
| **Generic CD** | Data/Audio CD | `cdrdao` → `chdman createcd` | Win32 raw read → `chdman createcd` |
| **PC** | DVD/CD | native reader / `ddrescue` → `chdman` | Win32 read → `chdman` |

---

## Troubleshooting

### Windows

**UAC prompt on launch**

The app embeds a Windows manifest requesting Administrator rights (`requireAdministrator`),
so Windows will automatically show a UAC prompt ("Do you want to allow this app to make
changes to your device?") every time it starts. This is required for raw optical drive access
(`IOCTL_CDROM_RAW_READ`) and is a Windows security boundary that cannot be bypassed.

Simply click **Yes** in the UAC dialog and the app starts normally.

> **Note:** File conversion (ISO/CUE → CHD) also works fine with elevated rights —
> the UAC prompt is a one-click confirmation, not a restriction.

**"Zugriff verweigert" / Win32 error 5 — still happening after UAC**

Another process has the drive locked (e.g. Windows Explorer auto-playing the disc, or a
virtual drive / daemon tool). Eject and reinsert the disc, or close any application that
accessed it, then try again.

**"chdman fehlt" / chdman not found**

Download `chdman.exe` from the [MAME release page](https://www.mamedev.org/release.html).
Extract it and either:
- place it on your `PATH` (e.g. copy to `C:\Windows\System32\`), or
- set the path directly in the app: left panel → **"chdman…"** button → browse to `chdman.exe`

**Drive not listed in the GUI**

Only drives reported as `DRIVE_CDROM` by `GetDriveTypeW` appear. Check Device Manager to confirm
the drive is recognized by Windows. USB adapters for slot-loading drives sometimes need a driver.

**Build fails: `clang` not found (ring crate)**

```powershell
winget install LLVM.LLVM
# Then in the same terminal:
$env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"
cargo build --release
```

**ARM64: LNK1181 or "cannot open input file" errors**

The `LIB` environment variable is missing or points to the wrong architecture folder.
Make sure you are using `\lib\arm64\` (not `\lib\x64\`) paths — see the ARM64 build section above.

### Linux

**"Permission denied on /dev/sr0"**

```bash
sudo usermod -a -G cdrom $USER
# Log out and back in
```

**"chdman fehlt"**

```bash
# Debian/Ubuntu
sudo apt install mame-tools
# Arch
yay -S chdman
```

**CHD verification failed**

Source media may be damaged. Enable ddrescue + scrape pass in **Options → Ripping** for better
recovery on Linux. On Windows, the built-in ripper retries each bad sector 3 times and
zero-pads unrecoverable sectors (matching ddrescue's default behaviour for scratched discs).

**Many consecutive bad sectors starting at a specific track boundary (e.g. sector 1134)**

This typically means a disc with mixed content — for example a CD-Extra / Enhanced CD (audio
tracks first, then a data track at the end). The ripper correctly identifies each track type
from the TOC and selects the matching raw-read mode (`CDDA` for audio, `YellowMode2` for data).
The retry log now also prints the Win32 error code (`err=N`) to help diagnose the root cause:

- `err=1` — `ERROR_INVALID_FUNCTION`: the drive rejected the requested track mode for that sector.
  This often means the sector type does not match the mode used; check whether the disc is a
  mixed-mode, CD-Extra, or XA (PS1) disc.
- `err=23` — `ERROR_CRC`: data integrity error (physically damaged sector).
- `err=27` — `ERROR_SECTOR_NOT_FOUND`: sector not found on disc (physical defect or wrong offset).

---

## Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/my-feature`)
3. Run `cargo fmt && cargo clippy --workspace` before committing
4. Open a Pull Request

---

## Legal Notice

This tool is intended for **personal archival of media you own**. Creating backup copies of optical
discs for personal use is permitted in many jurisdictions (e.g. §53 UrhG in Germany), but laws
vary by country — please verify the regulations applicable to you.

**Important:** This tool does not circumvent copy protection. Discs with active DRM (e.g.
CSS-encrypted DVDs) will produce encrypted output that cannot be used as a functional backup.
Circumventing copy protection mechanisms may be illegal regardless of media ownership.

The authors assume no liability for misuse of this software.

---

## License

MIT — see [LICENSE](LICENSE).

## Acknowledgments

- [itsgopnik/DVD2CHD](https://github.com/Itsgopnik/DVD2CHD) — original project by [@Itsgopnik](https://github.com/Itsgopnik), this fork adds Windows-native support
- [MAME Project](https://www.mamedev.org) — `chdman`
- [cdrdao](https://cdrdao.sourceforge.net) — CD ripping (Linux)
- [GNU ddrescue](https://www.gnu.org/software/ddrescue/) — data recovery (Linux)
- [egui](https://github.com/emilk/egui) — immediate mode GUI framework
- Microsoft Win32 `IOCTL_CDROM_RAW_READ` / `IOCTL_CDROM_READ_TOC` — Windows raw disc access

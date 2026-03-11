# DVD2CHD

[![CI](https://github.com/Itsgopnik/dvd2chd/actions/workflows/ci.yml/badge.svg)](https://github.com/Itsgopnik/dvd2chd/actions/workflows/ci.yml)

A Rust tool for archiving optical media (DVDs and CDs) to compressed CHD (Compressed Hunks of Data) format, with a modern graphical interface.

## Features

- **Multi-profile support** — PS1, PS2, generic CD, and PC disc profiles with auto-detection
- **Device archiving** — Direct ripping from optical drives (Linux & Windows) with auto-eject
- **File conversion** — Convert existing ISO/CUE files to CHD
- **CHD extraction** — Extract CHD back to ISO or BIN+CUE (auto-detects DVD vs. CD)
- **Batch mode** — Queue multiple files for sequential processing
- **Drag & drop** — Drop ISO/CUE files directly onto the window
- **Verification** — Built-in CHD integrity check after creation
- **Hashing** — Optional MD5, SHA1, and SHA256 computation
- **Process priority** — `nice` and `ionice` support to keep the system responsive
- **Auto-install** — Missing tools can be installed via system package manager or downloaded automatically
- **Desktop notifications** — Get notified when a job finishes
- **Internationalization** — English and German UI
- **Modern GUI** — egui-based interface with dark/light/high-contrast/auto themes, smooth animations (with reduce-motion toggle), and responsive layout

## System Requirements

### Runtime Dependencies

**Linux (primary platform):**

| Tool | Required | Notes |
|------|----------|-------|
| `chdman` | Yes | From MAME tools — for CHD creation |
| `cdrdao` | For CD ripping | Raw CD audio/data capture |
| `ddrescue` | Optional | Better error recovery than the built-in reader |
| `isoinfo` / `isosize` | Optional | More accurate disc size detection |

> **Note:** `dd` has been replaced by a native Rust implementation — no external `dd` binary required.

**Windows:**
- `chdman` — required for CHD creation (place in the same folder as `dvd2chd-gui.exe` or add to PATH)
- Native Win32 disc ripper built-in — no external ripping tools needed

**macOS:**
- `chdman` — required for CHD creation
- Device ripping is not supported; file conversion works

### Build Dependencies

- Rust 1.70+ (2021 edition)
- Standard C build tools (`gcc` / `clang`, `pkg-config`) for native dependencies

## Installation

### Pre-built Packages

Download the latest release from the [Releases page](https://github.com/Itsgopnik/DVD2CHD/releases):

| Format | Distro | Install command |
|--------|--------|-----------------|
| `.deb` | Debian / Ubuntu | `sudo dpkg -i dvd2chd_*.deb` |
| `.rpm` | Fedora / openSUSE / RHEL | `sudo rpm -i dvd2chd-*.rpm` |
| `.AppImage` | Any Linux | `chmod +x dvd2chd-*.AppImage && ./dvd2chd-*.AppImage` |
| `PKGBUILD` | Arch / CachyOS / Manjaro | `makepkg -si` |
| `.tar.gz` | Any Linux | Extract and run `dvd2chd-gui` |
| `.zip` | Windows x86_64 | Extract and run `dvd2chd-gui.exe` |

> The `.deb`, `.rpm`, `.tar.gz`, and Windows `.zip` packages bundle `chdman`.
> The Linux packages additionally include `cdrdao` and `ddrescue`.
> The AppImage requires tools to be installed system-wide.

### From Source

```bash
git clone https://github.com/Itsgopnik/DVD2CHD.git
cd DVD2CHD

cargo build --release

# Binary: target/release/dvd2chd-gui
```

### Install Required Tools (Linux)

**Debian / Ubuntu:**
```bash
sudo apt install mame-tools cdrdao gddrescue
```

**Arch Linux / CachyOS:**
```bash
sudo pacman -S cdrdao ddrescue
# chdman: available via AUR (yay -S chdman) or set path manually in Options → Tools
```

**Fedora:**
```bash
sudo dnf install mame cdrdao ddrescue
```

**openSUSE:**
```bash
sudo zypper install mame cdrdao ddrescue
```

> The GUI can also install missing tools automatically via the toolbar button (uses `pkexec` for privilege escalation).

## Usage

```bash
./target/release/dvd2chd-gui
```

### Quick Start

1. Select a **source** (ISO/CUE file or optical drive)
2. Choose an **output folder**
3. Select a **profile** (Auto works for most discs)
4. Click **▶ Start**

### Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Ctrl+S` | Start job |
| `Ctrl+O` | Open file |
| `Ctrl+G` | Set device path |

## Building

```bash
# Development build
cargo build

# Release build (optimized)
cargo build --release

# Tests
cargo test --workspace

# Linting
cargo clippy --workspace
```

## Architecture

Cargo workspace with two crates:

```
dvd2chd/
├── dvd2chd-core/          # Core logic
│   └── src/
│       ├── lib.rs         # Public API, types, re-exports
│       ├── util.rs        # sanitize_filename, unique_path, …
│       ├── hash.rs        # MD5/SHA1 computation
│       ├── verify.rs      # CHD verification
│       ├── windows_rip.rs # Native Win32 DVD/CD ripper
│       └── linux/
│           ├── mod.rs     # archive_device_linux, media detection
│           ├── dvd.rs     # DVD ripping (native Rust reader + ddrescue)
│           ├── cd.rs      # CD ripping via cdrdao, CUE handling
│           └── chd.rs     # chdman invocation, priority wrapping
└── dvd2chd-gui/           # egui/eframe GUI
    └── src/
        ├── main.rs
        ├── drive.rs       # Drive enumeration (Linux/Windows/macOS)
        ├── pkg_install.rs # System package manager auto-install
        ├── tool_fetch.rs  # Binary download via manifest URL
        └── app/           # GUI application (split into submodules)
            ├── mod.rs     # App struct, update loop
            ├── animation.rs
            ├── draw_layout.rs / draw_toolbar.rs / draw_dialogs.rs
            ├── job.rs / timeline.rs / workflow.rs
            ├── tools_check.rs / source.rs / presets.rs / log.rs
            └── state.rs
```

### Key Design Patterns

- **`ProgressSink` trait** — unified progress reporting from core to GUI
- **Cooperative cancellation** — `is_cancelled()` polling, no forceful kills
- **Atomic writes** — `.part` files ensure crash-safe output
- **`PhaseAnim` struct** — exponential-decay animation smoothing in the GUI

## Profiles

| Profile | Media | Tools |
|---------|-------|-------|
| **Auto** | Any | Detected via `udevadm` |
| **PS1** | CD | `cdrdao` (raw) → `chdman createcd` |
| **PS2** | DVD | native reader / `ddrescue` → `chdman createdvd` |
| **Generic CD** | Data/Audio CD | `cdrdao` → `chdman createcd` |
| **PC** | DVD/CD | native reader / `ddrescue` → `chdman` |

## Troubleshooting

**"chdman fehlt"** — Install `mame-tools` (apt) or set the binary path in Options → Tools.

**"Permission denied on /dev/sr0"** — Add your user to the `cdrom` group:
```bash
sudo usermod -a -G cdrom $USER
# Log out and back in
```

**CHD verification failed** — Source media may be damaged. Enable ddrescue + scrape pass in Options → Ripping for better recovery.

## Contributing

Contributions are welcome! This project was built with AI assistance, so there's likely room for improvement — feel free to open issues or submit PRs.

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/my-feature`)
3. Run `cargo fmt && cargo clippy --workspace` before committing
4. Open a Pull Request

## Legal Notice

This tool is intended for **personal archival of media you own**. Creating backup copies of optical discs for personal use is permitted in many jurisdictions (e.g. §53 UrhG in Germany), but laws vary by country — please verify the regulations applicable to you.

**Important:** This tool does not circumvent copy protection. Discs with active DRM (e.g. CSS-encrypted DVDs) will produce encrypted output that cannot be used as a functional backup. Circumventing copy protection mechanisms may be illegal regardless of media ownership.

The authors assume no liability for misuse of this software.

## License

MIT — see [LICENSE](LICENSE).

## Acknowledgments

- [MAME Project](https://www.mamedev.org) — `chdman`
- [cdrdao](https://cdrdao.sourceforge.net) — CD ripping
- [GNU ddrescue](https://www.gnu.org/software/ddrescue/) — data recovery
- [egui](https://github.com/emilk/egui) — immediate mode GUI framework
- [@Seiroh0](https://github.com/Seiroh0) — Windows native ripper

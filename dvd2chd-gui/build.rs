use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    // Install icon + .desktop file into ~/.local so KDE/Wayland can find the
    // app icon when running via `cargo run` (no system-wide install needed).
    install_dev_desktop_files(&manifest_dir);

    let tools_dir = manifest_dir.join("../tools/linux");

    if !tools_dir.exists() {
        return;
    }

    println!("cargo:rerun-if-changed={}", tools_dir.display());
    if let Some(target_dir) = find_target_dir() {
        let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
        let dest_dir = target_dir.join(&profile).join("tools");
        let _ = fs::create_dir_all(&dest_dir);

        if let Ok(entries) = fs::read_dir(&tools_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                println!("cargo:rerun-if-changed={}", path.display());
                if path.is_file() {
                    if let Some(name) = path.file_name() {
                        let _ = fs::copy(&path, dest_dir.join(name));
                    }
                }
            }
        }
    }
}

/// Copies `assets/icon.png` → `~/.local/share/icons/hicolor/256x256/apps/dvd2chd.png`
/// and writes a `.desktop` file with the correct `Exec=` path so that KDE/Wayland
/// can resolve the app icon from the `app_id` ("dvd2chd-gui") during development.
fn install_dev_desktop_files(manifest_dir: &Path) {
    let home = match env::var("HOME") {
        Ok(h) => PathBuf::from(h),
        Err(_) => return,
    };

    // ── Icon ──────────────────────────────────────────────────────────────────
    let src_icon = manifest_dir.join("assets/icon.png");
    println!("cargo:rerun-if-changed={}", src_icon.display());
    let icon_dest_dir = home.join(".local/share/icons/hicolor/256x256/apps");
    if src_icon.exists() {
        if fs::create_dir_all(&icon_dest_dir).is_ok() {
            let _ = fs::copy(&src_icon, icon_dest_dir.join("dvd2chd.png"));
        }
    }

    // ── .desktop file ─────────────────────────────────────────────────────────
    // Compute the path to the binary that `cargo run` will produce.
    let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    let bin_path = find_target_dir()
        .map(|t| t.join(&profile).join("dvd2chd-gui"))
        .unwrap_or_else(|| PathBuf::from("dvd2chd-gui"));

    let desktop_content = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=DVD2CHD\n\
         GenericName=Optical Disc Dumper\n\
         Comment=Dump and convert optical media to CHD archives\n\
         Exec={exec}\n\
         TryExec={exec}\n\
         Icon=dvd2chd\n\
         Categories=AudioVideo;Utility;\n\
         StartupNotify=true\n\
         StartupWMClass=DVD2CHD (GUI)\n\
         Keywords=DVD;CD;CHD;Archive;Backup;\n\
         Terminal=false\n",
        exec = bin_path.display(),
    );

    let apps_dir = home.join(".local/share/applications");
    if fs::create_dir_all(&apps_dir).is_ok() {
        let _ = fs::write(apps_dir.join("dvd2chd-gui.desktop"), desktop_content);
    }

    // Ask KDE to refresh its icon cache so the new icon is picked up immediately.
    let _ = std::process::Command::new("kbuildsycoca6").arg("--noincremental").output();
}

fn find_target_dir() -> Option<PathBuf> {
    if let Ok(dir) = env::var("CARGO_TARGET_DIR") {
        return Some(PathBuf::from(dir));
    }
    let out_dir = PathBuf::from(env::var("OUT_DIR").ok()?);
    find_ancestor_named(&out_dir, "target")
}

fn find_ancestor_named(path: &Path, name: &str) -> Option<PathBuf> {
    let mut current = Some(path);
    while let Some(dir) = current {
        if dir.file_name().map(|n| n == name).unwrap_or(false) {
            return Some(dir.to_path_buf());
        }
        current = dir.parent();
    }
    None
}

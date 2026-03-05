use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    // On Windows: embed a UAC manifest so Windows automatically shows an
    // "Run as Administrator?" prompt on launch. Raw optical-drive access
    // (IOCTL_CDROM_RAW_READ) requires elevated privileges.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_manifest::embed_manifest(
            embed_manifest::new_manifest("dvd2chd-gui")
                .requested_execution_level(
                    embed_manifest::manifest::ExecutionLevel::RequireAdministrator,
                ),
        )
        .expect("failed to embed UAC manifest");

        // Embed the application icon into the EXE.
        // The .ico file lives in assets/ next to this build script.
        let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
        let ico_path = PathBuf::from(&manifest_dir).join("assets/dvd2chd.ico");
        if ico_path.exists() {
            let mut res = winres::WindowsResource::new();
            res.set_icon(ico_path.to_str().expect("ico path not valid UTF-8"));
            res.compile().expect("failed to embed application icon");
        }
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
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

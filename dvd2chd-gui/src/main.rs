// Hide console window in release builds; keep it for debug/dev builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

rust_i18n::i18n!("locales");

mod app;
pub mod drive;
pub mod pkg_install;
pub mod tool_fetch;

fn main() {
    app::run();
}



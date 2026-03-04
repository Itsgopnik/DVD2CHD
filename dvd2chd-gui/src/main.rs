rust_i18n::i18n!("locales");

mod app;
pub mod drive;
pub mod pkg_install;
pub mod tool_fetch;

fn main() {
    app::run();
}

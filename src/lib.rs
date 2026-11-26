//! rawv: a fast, minimal photo and RAW viewer.

/// The version from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The commit the executables are built from, `+` for uncommitted changes (`build.rs`).
pub const COMMIT: &str = env!("RAWV_COMMIT");

/// Names this build, as the settings panel and `rawv-cli --version` show it.
pub fn build_name() -> String {
    format!("rawv {VERSION} \u{b7} {COMMIT}")
}

pub mod app;
pub mod bench;
pub mod command;
pub mod config;
pub mod default_app;
pub mod draw;
pub mod filework;
pub mod gallery;
pub mod library;
pub mod media;
pub mod orientations;
pub mod ratings;
pub mod repaint;
pub mod script;
pub mod texture;
pub mod theme;
pub mod thumbs;
pub mod viewer;
pub mod workers;

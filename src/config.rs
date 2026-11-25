//! Settings kept between sessions, and where the app stores its data.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::library::listing::SortOrder;
use crate::media::load::RawMode;

/// Overrides the data folder (settings and thumbnail cache), for tests and isolated runs.
const DATA_DIR_VARIABLE: &str = "RAWV_DATA_DIR";

pub const MIN_THUMB_SCALE: f32 = 0.6;
pub const MAX_THUMB_SCALE: f32 = 3.0;
/// Sizes the developed RAW cache can be limited to, in gigabytes; 0 turns it off.
pub const RAW_CACHE_SIZES_GB: [u32; 4] = [0, 5, 20, 50];

/// The size after `gigabytes` in [`RAW_CACHE_SIZES_GB`]; the largest is followed by off.
pub fn next_raw_cache_size(gigabytes: u32) -> u32 {
    let position = RAW_CACHE_SIZES_GB
        .iter()
        .position(|&size| size >= gigabytes)
        .unwrap_or(0);
    RAW_CACHE_SIZES_GB[(position + 1) % RAW_CACHE_SIZES_GB.len()]
}

pub fn raw_cache_label(gigabytes: u32) -> String {
    match gigabytes {
        0 => "Off".to_owned(),
        size => format!("Up to {size} GB"),
    }
}

pub fn raw_cache_bytes(gigabytes: u32) -> u64 {
    u64::from(gigabytes) * 1024 * 1024 * 1024
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct WindowGeometry {
    pub size: [f32; 2],
    pub position: Option<[f32; 2]>,
    pub maximized: bool,
}

/// What the mouse wheel does in the viewer. Ctrl+wheel does the other.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum WheelAction {
    #[default]
    Zoom,
    Step,
}

impl WheelAction {
    pub fn toggled(self) -> Self {
        match self {
            Self::Zoom => Self::Step,
            Self::Step => Self::Zoom,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Zoom => "Zoom",
            Self::Step => "Next / previous",
        }
    }
}

/// What the arrow keys do in the viewer.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum ArrowAction {
    /// Move a zoomed-in image; where it fits, Left and Right step.
    #[default]
    Move,
    /// Always go to the previous (Left, Up) or next (Right, Down) image.
    Step,
}

impl ArrowAction {
    pub fn toggled(self) -> Self {
        match self {
            Self::Move => Self::Step,
            Self::Step => Self::Move,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Move => "Move zoomed image",
            Self::Step => "Next / previous",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub last_folder: Option<PathBuf>,
    pub thumb_scale: f32,
    pub sort: SortOrder,
    pub show_info: bool,
    /// Show every nested subfolder as a roll, not only the direct ones.
    pub nested: bool,
    pub wheel: WheelAction,
    pub arrows: ArrowAction,
    pub raw: RawMode,
    /// Size limit of the developed RAW cache in gigabytes; 0 turns it off.
    pub raw_cache_gb: u32,
    pub window: Option<WindowGeometry>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            last_folder: None,
            thumb_scale: 1.0,
            sort: SortOrder::default(),
            show_info: true,
            nested: false,
            wheel: WheelAction::default(),
            arrows: ArrowAction::default(),
            raw: RawMode::default(),
            raw_cache_gb: 20,
            window: None,
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let mut config: Self = std::fs::read(config_file())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        config.thumb_scale = config.thumb_scale.clamp(MIN_THUMB_SCALE, MAX_THUMB_SCALE);
        config
    }

    pub fn save(&self) {
        let path = config_file();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }
}

fn data_dir_override() -> Option<PathBuf> {
    std::env::var_os(DATA_DIR_VARIABLE).map(PathBuf::from)
}

fn config_file() -> PathBuf {
    data_dir_override()
        .or_else(|| dirs::config_dir().map(|dir| dir.join("rawv")))
        .unwrap_or_default()
        .join("config.json")
}

pub fn thumb_store_file() -> PathBuf {
    data_dir_override()
        .or_else(|| dirs::cache_dir().map(|dir| dir.join("rawv")))
        .unwrap_or_default()
        .join("thumbs.redb")
}

pub fn developed_dir() -> PathBuf {
    data_dir_override()
        .or_else(|| dirs::cache_dir().map(|dir| dir.join("rawv")))
        .unwrap_or_default()
        .join("developed")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_cache_sizes_cycle_through_off() {
        assert_eq!(next_raw_cache_size(20), 50);
        assert_eq!(next_raw_cache_size(50), 0);
        assert_eq!(next_raw_cache_size(0), 5);
        assert_eq!(next_raw_cache_size(7), 50);
        assert_eq!(raw_cache_label(0), "Off");
        assert_eq!(raw_cache_label(20), "Up to 20 GB");
    }
}

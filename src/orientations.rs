//! Rotations applied in this session. They win over what a cached thumbnail or image recorded.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::media::orientation::Orientation;

#[derive(Default)]
pub struct Orientations(HashMap<PathBuf, Orientation>);

impl Orientations {
    pub fn effective(&self, path: &Path, from_file: Orientation) -> Orientation {
        self.0.get(path).copied().unwrap_or(from_file)
    }

    pub fn set(&mut self, path: PathBuf, orientation: Orientation) {
        self.0.insert(path, orientation);
    }
}

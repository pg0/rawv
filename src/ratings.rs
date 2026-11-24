//! Ratings given in this session. They win over what a cached thumbnail or image recorded.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::media::xmp::Rating;

#[derive(Default)]
pub struct Ratings(HashMap<PathBuf, Rating>);

impl Ratings {
    pub fn effective(&self, path: &Path, from_file: Rating) -> Rating {
        self.0.get(path).copied().unwrap_or(from_file)
    }

    pub fn set(&mut self, path: PathBuf, rating: Rating) {
        self.0.insert(path, rating);
    }

    /// Drops a rating that could not be written, so the file's own shows again.
    pub fn forget(&mut self, path: &Path) {
        self.0.remove(path);
    }
}

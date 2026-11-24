//! Reading image files: metadata probing, decoding, thumbnails and orientation.

pub mod developed;
pub mod formats;
pub mod load;
pub mod orientation;
pub mod pixels;
pub mod probe;
pub mod raw;
pub mod remove;
pub mod render;
pub mod rotate;
pub mod thumb;
pub mod tiff;

use std::fmt;

#[derive(Debug, Clone)]
pub struct MediaError(String);

impl MediaError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    pub fn from_display(error: impl fmt::Display) -> Self {
        Self(error.to_string())
    }
}

impl fmt::Display for MediaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for MediaError {}

impl From<std::io::Error> for MediaError {
    fn from(error: std::io::Error) -> Self {
        Self(error.to_string())
    }
}

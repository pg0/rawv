//! Full-resolution image loading for the viewer.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::MediaError;
use super::animation;
use super::developed::DevelopedCache;
use super::formats::{self, Kind};
use super::orientation::Orientation;
use super::pixels::{self, Channels, Pixels};
use super::probe::{self, MediaFile, Probe, ShotInfo};
use super::raw;
use super::xmp::Rating;

/// Camera previews with a long edge below this many pixels count as small.
const SMALL_PREVIEW_LONG_EDGE: u32 = 3000;

pub fn is_small_preview(size: [u32; 2]) -> bool {
    size[0].max(size[1]) < SMALL_PREVIEW_LONG_EDGE
}

/// How the user wants RAW files shown.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default, Serialize, Deserialize)]
pub enum RawMode {
    /// Show the JPEG the camera embedded in the RAW file. Fast.
    Embedded,
    /// The embedded JPEG where it is large, the developed sensor data where it is small.
    #[default]
    Auto,
    /// Demosaic the sensor data. Slow, but full size.
    Developed,
}

impl RawMode {
    pub fn next(self) -> Self {
        match self {
            Self::Embedded => Self::Auto,
            Self::Auto => Self::Developed,
            Self::Developed => Self::Embedded,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Embedded => "Camera preview",
            Self::Auto => "Full size if preview is small",
            Self::Developed => "Always full size",
        }
    }

    /// Whether to develop the sensor data, given the size of the embedded preview.
    pub fn develops(self, preview: Option<[u32; 2]>) -> bool {
        match self {
            Self::Embedded => false,
            Self::Auto => preview.is_none_or(is_small_preview),
            Self::Developed => true,
        }
    }
}

/// The two pictures a RAW file can give. Other files have only their one picture.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Rendition {
    /// The JPEG the camera embedded.
    Preview,
    /// The demosaiced sensor data.
    Developed,
}

/// Where developed images are kept between runs, and the identity of the file version.
pub struct DevelopSlot<'a> {
    pub cache: &'a DevelopedCache,
    pub key: &'a str,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    Jpeg,
    Bitmap,
    RawPreview,
    RawDeveloped,
}

pub struct FullImage {
    /// RGBA pixels in stored orientation, at most `max_side` on each edge.
    pub pixels: Pixels,
    /// Pixel size before any downscaling.
    pub original_size: [u32; 2],
    pub orientation: Orientation,
    pub shot: ShotInfo,
    pub source: Source,
    /// The developed image came from the local cache.
    pub cached: bool,
    /// The rating stored in the file.
    pub rating: Rating,
    /// How long each frame of an animation is shown, `pixels` first; empty for a still picture.
    pub delays: Vec<Duration>,
    /// The frames of an animation after `pixels`, the same size.
    pub later_frames: Vec<Pixels>,
}

pub fn load_full(
    path: &Path,
    kind: Kind,
    rendition: Rendition,
    max_side: u32,
    slot: Option<DevelopSlot<'_>>,
) -> Result<FullImage, MediaError> {
    let mut cached = false;
    if kind == Kind::Bitmap
        && let Some(animation) = animation::decode(path, max_side)?
    {
        return animated(animation);
    }
    let (pixels, probe, source, raw_orientation) = match kind {
        Kind::Jpeg => {
            let data = std::fs::read(path)?;
            if formats::is_jpeg(&data) {
                let probe = probe::probe_jpeg(&data);
                let pixels = pixels::decode_jpeg(&data, Channels::Rgba)?;
                (pixels, probe, Source::Jpeg, None)
            } else {
                let pixels = pixels::decode_bitmap(&data)?;
                (pixels, Probe::default(), Source::Bitmap, None)
            }
        }
        Kind::Bitmap => {
            let image = image::ImageReader::open(path)?
                .with_guessed_format()?
                .decode()
                .map_err(MediaError::from_display)?;
            (
                pixels::from_dynamic(image),
                Probe::default(),
                Source::Bitmap,
                None,
            )
        }
        Kind::Raw => {
            let mut source = MediaFile::open(path)?;
            let probe = source.probe(kind);
            let stored = match rendition {
                Rendition::Developed => slot.as_ref().and_then(|slot| slot.cache.get(slot.key)),
                Rendition::Preview => None,
            };
            cached = stored.is_some();
            // Also needed when developing: the camera's JPEG sets brightness and contrast.
            let embedded = match stored {
                Some(_) => None,
                None => largest_embedded(&mut source, &probe),
            };
            drop(source);
            let (raw, source) = match (rendition, stored, embedded) {
                (_, Some(raw), _) => (raw, Source::RawDeveloped),
                (Rendition::Preview, None, Some(pixels)) => {
                    let preview = raw::RawPixels {
                        pixels,
                        orientation: None,
                    };
                    (preview, Source::RawPreview)
                }
                (Rendition::Preview, None, None) => match raw::preview(path)? {
                    Some(preview) => (preview, Source::RawPreview),
                    None => (develop(path, None, slot.as_ref())?, Source::RawDeveloped),
                },
                (Rendition::Developed, None, embedded) => (
                    develop(path, embedded.as_ref(), slot.as_ref())?,
                    Source::RawDeveloped,
                ),
            };
            (raw.pixels, probe, source, raw.orientation)
        }
    };

    let orientation = if probe.slot.is_some() {
        probe.orientation
    } else {
        raw_orientation.unwrap_or(probe.orientation)
    };
    let original_size = pixels.size;
    Ok(FullImage {
        pixels: pixels.fit_within(max_side)?,
        original_size,
        orientation,
        rating: probe.rating,
        shot: probe.shot,
        source,
        cached,
        delays: Vec::new(),
        later_frames: Vec::new(),
    })
}

/// A GIF or WebP file; with one frame it is a still picture.
fn animated(animation: animation::Animation) -> Result<FullImage, MediaError> {
    let mut frames = animation.frames.into_iter();
    let first = frames
        .next()
        .ok_or_else(|| MediaError::new("the file holds no picture"))?;
    let (later_frames, later_delays): (Vec<Pixels>, Vec<Duration>) =
        frames.map(|frame| (frame.pixels, frame.delay)).unzip();
    let delays = if later_frames.is_empty() {
        Vec::new()
    } else {
        std::iter::once(first.delay).chain(later_delays).collect()
    };
    Ok(FullImage {
        pixels: first.pixels,
        original_size: animation.original_size,
        orientation: Orientation::default(),
        shot: ShotInfo::default(),
        source: Source::Bitmap,
        cached: false,
        rating: 0,
        delays,
        later_frames,
    })
}

/// Develops the sensor data and hands the result to the cache.
fn develop(
    path: &Path,
    reference: Option<&Pixels>,
    slot: Option<&DevelopSlot<'_>>,
) -> Result<raw::RawPixels, MediaError> {
    let developed = raw::develop(path, reference)?;
    if let Some(slot) = slot {
        slot.cache.put(slot.key, &developed);
    }
    Ok(developed)
}

fn largest_embedded(source: &mut MediaFile, probe: &Probe) -> Option<Pixels> {
    probe.embedded.iter().rev().find_map(|&blob| {
        if !source.holds_preview(blob) {
            return None;
        }
        let data = source.read_embedded(blob).ok()?;
        pixels::decode_jpeg(&data, Channels::Rgba).ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_mode_develops_only_small_or_missing_previews() {
        assert!(!RawMode::Auto.develops(Some([4240, 2384])));
        assert!(RawMode::Auto.develops(Some([1920, 1080])));
        assert!(RawMode::Auto.develops(Some([1080, 1616])));
        assert!(RawMode::Auto.develops(None));
        assert!(!RawMode::Embedded.develops(Some([1920, 1080])));
        assert!(!RawMode::Embedded.develops(None));
        assert!(RawMode::Developed.develops(Some([4240, 2384])));
    }

    #[test]
    fn raw_modes_cycle_back_to_start() {
        let start = RawMode::default();
        assert_eq!(start.next().next().next(), start);
    }
}

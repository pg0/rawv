//! Thumbnail creation.
//!
//! Two qualities: `Embedded` uses the tiny thumbnail cameras store in the file header
//! (a few kilobytes to read), `Full` is scaled down from real image data.

use std::path::Path;

use super::MediaError;
use super::formats::Kind;
use super::orientation::Orientation;
use super::pixels::{self, Channels, Pixels};
use super::probe::{Embedded, MediaFile, Probe};
use super::raw;

pub const THUMB_LONG_EDGE: u32 = 480;
const MIN_EMBEDDED_LONG_EDGE: u32 = 96;
const THUMB_JPEG_QUALITY: u8 = 85;
/// Brightest channel value still counted as part of a black letterbox bar.
const BAR_MAX_LEVEL: u8 = 24;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub enum ThumbQuality {
    Embedded = 1,
    Full = 2,
}

impl ThumbQuality {
    pub fn from_byte(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Embedded),
            2 => Some(Self::Full),
            _ => None,
        }
    }
}

/// The part of a thumbnail that holds picture content, in normalized stored coordinates.
/// Camera thumbnails are often letterboxed with black bars.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ContentRect {
    pub min: [f32; 2],
    pub max: [f32; 2],
}

impl ContentRect {
    pub const FULL: Self = Self {
        min: [0.0, 0.0],
        max: [1.0, 1.0],
    };

    /// Finds symmetric black bars along the top/bottom or left/right edges of RGBA pixels.
    pub fn without_bars(pixels: &Pixels) -> Self {
        let [width, height] = pixels.size;
        if width < 16 || height < 16 || pixels.channels != Channels::Rgba {
            return Self::FULL;
        }
        let is_dark = |x: u32, y: u32| {
            let index = (y as usize * width as usize + x as usize) * 4;
            pixels.data[index..index + 3]
                .iter()
                .all(|&channel| channel <= BAR_MAX_LEVEL)
        };
        let dark_row = |y: u32| (0..width).all(|x| is_dark(x, y));
        let dark_column = |x: u32| (0..height).all(|y| is_dark(x, y));

        if let Some(bar) = symmetric_bar(height, dark_row) {
            let inset = (bar + 1) as f32 / height as f32;
            return Self {
                min: [0.0, inset],
                max: [1.0, 1.0 - inset],
            };
        }
        if let Some(bar) = symmetric_bar(width, dark_column) {
            let inset = (bar + 1) as f32 / width as f32;
            return Self {
                min: [inset, 0.0],
                max: [1.0 - inset, 1.0],
            };
        }
        Self::FULL
    }

    pub fn size(self) -> [f32; 2] {
        [self.max[0] - self.min[0], self.max[1] - self.min[1]]
    }
}

/// Bar thickness when lines `0..bar` and `len-bar..len` are dark and of matching size.
fn symmetric_bar(len: u32, is_dark: impl Fn(u32) -> bool) -> Option<u32> {
    let limit = len * 3 / 10;
    let leading = (0..limit).take_while(|&i| is_dark(i)).count() as u32;
    let trailing = (0..limit).take_while(|&i| is_dark(len - 1 - i)).count() as u32;
    (leading >= 2 && trailing >= 2 && leading.abs_diff(trailing) <= 2)
        .then(|| leading.max(trailing))
}

pub struct Thumb {
    /// JPEG bytes as kept in the thumbnail store.
    pub jpeg: Vec<u8>,
    /// Decoded RGBA pixels of `jpeg`.
    pub pixels: Pixels,
    pub orientation: Orientation,
    pub quality: ThumbQuality,
    pub content: ContentRect,
}

/// Creates a thumbnail of at least quality `want` where the file allows it.
pub fn make_thumb(path: &Path, kind: Kind, want: ThumbQuality) -> Result<Thumb, MediaError> {
    match kind {
        Kind::Jpeg => jpeg_thumb(path, want),
        Kind::Raw => raw_thumb(path, want),
        Kind::Bitmap => {
            let image = image::ImageReader::open(path)?
                .with_guessed_format()?
                .decode()
                .map_err(MediaError::from_display)?;
            scaled_thumb(pixels::from_dynamic(image), Orientation::NORMAL)
        }
    }
}

fn jpeg_thumb(path: &Path, want: ThumbQuality) -> Result<Thumb, MediaError> {
    let mut source = MediaFile::open(path)?;
    let probe = source.probe(Kind::Jpeg);
    if want == ThumbQuality::Embedded
        && !source.is_complete()
        && let Some(thumb) = embedded_jpeg_thumb(&mut source, &probe)
    {
        return Ok(thumb);
    }
    let data = source.read_all()?;
    let pixels = pixels::decode_jpeg(&data, Channels::Rgb)?;
    scaled_thumb(pixels, probe.orientation)
}

/// The EXIF thumbnail a camera stored in the file's head.
fn embedded_jpeg_thumb(source: &mut MediaFile, probe: &Probe) -> Option<Thumb> {
    let bytes = source.read_embedded(*probe.embedded.first()?).ok()?;
    let pixels = pixels::decode_jpeg(&bytes, Channels::Rgba).ok()?;
    if pixels.long_edge() < MIN_EMBEDDED_LONG_EDGE {
        return None;
    }
    Some(Thumb {
        jpeg: bytes,
        content: ContentRect::without_bars(&pixels),
        pixels,
        orientation: probe.orientation,
        quality: ThumbQuality::Embedded,
    })
}

fn raw_thumb(path: &Path, want: ThumbQuality) -> Result<Thumb, MediaError> {
    let mut source = MediaFile::open(path)?;
    let probe = source.probe(Kind::Raw);
    let wanted_edge = match want {
        ThumbQuality::Embedded => MIN_EMBEDDED_LONG_EDGE,
        ThumbQuality::Full => THUMB_LONG_EDGE,
    };

    if let Some((bytes, size)) = pick_embedded(&mut source, &probe.embedded, wanted_edge)
        && let Ok(pixels) = pixels::decode_jpeg(&bytes, Channels::Rgba)
    {
        if size[0].max(size[1]) > THUMB_LONG_EDGE {
            return scaled_thumb(pixels, probe.orientation);
        }
        return Ok(Thumb {
            jpeg: bytes,
            content: ContentRect::without_bars(&pixels),
            pixels,
            orientation: probe.orientation,
            quality: ThumbQuality::Embedded,
        });
    }
    drop(source);

    let raw = match raw::preview(path)? {
        Some(raw) => raw,
        None => raw::develop(path, None)?,
    };
    let orientation = if probe.slot.is_some() {
        probe.orientation
    } else {
        raw.orientation.unwrap_or(probe.orientation)
    };
    scaled_thumb(raw.pixels, orientation)
}

/// Picks the smallest embedded JPEG whose long edge reaches `wanted_edge`.
/// For the `Full` request a smaller blob is not accepted; the caller then falls back to `rawler`.
fn pick_embedded(
    source: &mut MediaFile,
    candidates: &[Embedded],
    wanted_edge: u32,
) -> Option<(Vec<u8>, [u32; 2])> {
    candidates.iter().find_map(|&blob| {
        if !source.holds_preview(blob) {
            return None;
        }
        let bytes = source.read_embedded(blob).ok()?;
        let size = pixels::jpeg_size(&bytes)?;
        (size[0].max(size[1]) >= wanted_edge).then_some((bytes, size))
    })
}

fn scaled_thumb(pixels: Pixels, orientation: Orientation) -> Result<Thumb, MediaError> {
    let pixels = pixels.fit_within(THUMB_LONG_EDGE)?;
    let jpeg = pixels::encode_jpeg(&pixels, THUMB_JPEG_QUALITY)?;
    Ok(Thumb {
        jpeg,
        pixels: pixels.into_rgba(),
        orientation,
        quality: ThumbQuality::Full,
        content: ContentRect::FULL,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image_with_bars(size: [u32; 2], top_bottom: u32, left_right: u32) -> Pixels {
        let mut data = Vec::new();
        for y in 0..size[1] {
            for x in 0..size[0] {
                let in_bar = y < top_bottom
                    || y >= size[1] - top_bottom
                    || x < left_right
                    || x >= size[0] - left_right;
                let level = if in_bar { 3 } else { 140 };
                data.extend_from_slice(&[level, level, level, 255]);
            }
        }
        Pixels {
            size,
            channels: Channels::Rgba,
            data,
        }
    }

    #[test]
    fn crops_top_and_bottom_bars() {
        let content = ContentRect::without_bars(&image_with_bars([160, 120], 15, 0));
        assert_eq!(content.min, [0.0, 16.0 / 120.0]);
        assert_eq!(content.max, [1.0, 1.0 - 16.0 / 120.0]);
    }

    #[test]
    fn crops_left_and_right_bars() {
        let content = ContentRect::without_bars(&image_with_bars([160, 120], 0, 35));
        assert_eq!(content.min, [36.0 / 160.0, 0.0]);
        assert_eq!(content.max[1], 1.0);
    }

    #[test]
    fn keeps_whole_image_without_bars_or_with_one_sided_darkness() {
        assert_eq!(
            ContentRect::without_bars(&image_with_bars([160, 120], 0, 0)),
            ContentRect::FULL
        );
        let mut one_sided = image_with_bars([160, 120], 0, 0);
        for value in &mut one_sided.data[..160 * 20 * 4] {
            *value = 0;
        }
        assert_eq!(ContentRect::without_bars(&one_sided), ContentRect::FULL);
    }
}

//! HEIC and HEIF pictures, as iPhones take them, decoded by `zune-heic` in pure Rust.
//!
//! The decoder applies the file's crop, rotation and mirroring, so the pixels come out
//! upright and need no further orientation.

use image::{DynamicImage, GrayAlphaImage, GrayImage, RgbImage, RgbaImage};
use zune_heic::HeifDecoder;
use zune_heic::zune_core::bytestream::ZCursor;
use zune_heic::zune_core::colorspace::ColorSpace;

use super::MediaError;
use super::pixels::{self, Channels, Pixels};

/// The picture of a HEIC file's bytes, in premultiplied RGBA or plain RGB.
pub fn decode(data: &[u8]) -> Result<Pixels, MediaError> {
    let mut decoder = HeifDecoder::new(ZCursor::new(data));
    let samples = decoder.decode().map_err(heic_error)?;
    let (width, height) = match (decoder.width(), decoder.height()) {
        (Some(width), Some(height)) => (width as u32, height as u32),
        _ => return Err(MediaError::new("HEIC without a picture size")),
    };
    let premultiplied = decoder.is_alpha_premultiplied();
    let layout_error = || MediaError::new("HEIC decoded to an unexpected pixel layout");
    let image = match decoder.colorspace() {
        Some(ColorSpace::RGB) => DynamicImage::ImageRgb8(
            RgbImage::from_raw(width, height, samples).ok_or_else(layout_error)?,
        ),
        Some(ColorSpace::RGBA) if premultiplied => {
            return Ok(Pixels {
                size: [width, height],
                channels: Channels::Rgba,
                data: RgbaImage::from_raw(width, height, samples)
                    .ok_or_else(layout_error)?
                    .into_raw(),
            });
        }
        Some(ColorSpace::RGBA) => DynamicImage::ImageRgba8(
            RgbaImage::from_raw(width, height, samples).ok_or_else(layout_error)?,
        ),
        Some(ColorSpace::Luma) => DynamicImage::ImageLuma8(
            GrayImage::from_raw(width, height, samples).ok_or_else(layout_error)?,
        ),
        Some(ColorSpace::LumaA) => DynamicImage::ImageLumaA8(
            GrayAlphaImage::from_raw(width, height, samples).ok_or_else(layout_error)?,
        ),
        other => {
            return Err(MediaError::new(format!(
                "HEIC colour layout {other:?} is not shown"
            )));
        }
    };
    Ok(pixels::from_dynamic(image))
}

fn heic_error(error: zune_heic::HeicErrors) -> MediaError {
    MediaError::new(format!("HEIC: {error:?}"))
}

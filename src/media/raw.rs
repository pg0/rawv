//! RAW decoding through `rawler`: full development and previews for containers
//! the lightweight probe cannot read.

use std::path::Path;

use rawler::RawImage;
use rawler::decoders::RawDecodeParams;
use rawler::imgop::develop::{Intermediate, ProcessingStep, RawDevelop};
use rawler::imgop::xyz::Illuminant;
use rawler::rawsource::RawSource;

use super::MediaError;
use super::orientation::Orientation;
use super::pixels::{self, Pixels};
use super::render::{self, CameraColor};

pub struct RawPixels {
    pub pixels: Pixels,
    /// Orientation stated by the RAW container, when `rawler` could read it.
    pub orientation: Option<Orientation>,
}

/// Demosaics the sensor data into sRGB. `reference`, the JPEG the camera embedded, sets
/// brightness and contrast.
pub fn develop(path: &Path, reference: Option<&Pixels>) -> Result<RawPixels, MediaError> {
    let source = RawSource::new(path)?;
    let decoder = rawler::get_decoder(&source).map_err(MediaError::from_display)?;
    let params = RawDecodeParams::default();
    let raw = decoder
        .raw_image(&source, &params, false)
        .map_err(MediaError::from_display)?;
    let orientation = Some(Orientation::from_exif(raw.orientation.to_u16()));
    let pixels = match render_three_colour(&raw, reference)? {
        Some(pixels) => pixels,
        None => render_with_rawler(&raw)?,
    };
    Ok(RawPixels {
        pixels,
        orientation,
    })
}

/// Renders a three-colour sensor with a daylight colour matrix. Other sensors give `None`.
fn render_three_colour(
    raw: &RawImage,
    reference: Option<&Pixels>,
) -> Result<Option<Pixels>, MediaError> {
    let Some((_, matrix)) = raw.color_matrix_find_first([Illuminant::D65]) else {
        return Ok(None);
    };
    let Ok(matrix) = <[f32; 9]>::try_from(matrix.as_slice()) else {
        return Ok(None);
    };
    let sensor_steps = [
        ProcessingStep::Rescale,
        ProcessingStep::Demosaic,
        ProcessingStep::FujiRotate,
        ProcessingStep::CropActiveArea,
        ProcessingStep::CropDefault,
    ];
    let demosaiced = RawDevelop::new_with(&sensor_steps)
        .develop_intermediate(raw)
        .map_err(MediaError::from_display)?;
    let Intermediate::ThreeColor(camera) = demosaiced else {
        return Ok(None);
    };
    let [red, green, blue, _] = raw.wb_coeffs;
    let color = CameraColor {
        white_balance: if red.is_nan() {
            [1.0; 3]
        } else {
            [red, green, blue]
        },
        xyz_to_camera: [
            [matrix[0], matrix[1], matrix[2]],
            [matrix[3], matrix[4], matrix[5]],
            [matrix[6], matrix[7], matrix[8]],
        ],
    };
    Ok(render::render(
        &camera.data,
        camera.width,
        camera.height,
        &color,
        reference,
    ))
}

/// The whole development as `rawler` does it, for sensors the own renderer does not cover.
fn render_with_rawler(raw: &RawImage) -> Result<Pixels, MediaError> {
    let image = RawDevelop::default()
        .develop_intermediate(raw)
        .map_err(MediaError::from_display)?
        .to_dynamic_image()
        .ok_or_else(|| MediaError::new("RAW development produced no image"))?;
    Ok(pixels::from_dynamic(image))
}

/// The largest preview `rawler` can extract, for containers without a TIFF directory.
pub fn preview(path: &Path) -> Result<Option<RawPixels>, MediaError> {
    let source = RawSource::new(path)?;
    let decoder = rawler::get_decoder(&source).map_err(MediaError::from_display)?;
    let params = RawDecodeParams::default();
    let orientation = decoder
        .raw_metadata(&source, &params)
        .ok()
        .and_then(|metadata| metadata.exif.orientation)
        .map(Orientation::from_exif);
    let image = match decoder.full_image(&source, &params) {
        Ok(Some(image)) => Some(image),
        _ => decoder
            .preview_image(&source, &params)
            .map_err(MediaError::from_display)?,
    };
    Ok(image.map(|image| RawPixels {
        pixels: pixels::from_dynamic(image),
        orientation,
    }))
}

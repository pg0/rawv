//! Pixel buffers, JPEG coding and resizing.

use fast_image_resize::images::Image;
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};
use zune_core::bytestream::ZCursor;
use zune_core::colorspace::ColorSpace;
use zune_core::options::DecoderOptions;
use zune_jpeg::JpegDecoder;

use super::MediaError;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Channels {
    Rgb,
    Rgba,
}

impl Channels {
    pub fn count(self) -> usize {
        match self {
            Self::Rgb => 3,
            Self::Rgba => 4,
        }
    }
}

/// 8-bit pixels in stored (not display) orientation.
#[derive(Clone, Debug)]
pub struct Pixels {
    pub size: [u32; 2],
    pub channels: Channels,
    pub data: Vec<u8>,
}

impl Pixels {
    pub fn long_edge(&self) -> u32 {
        self.size[0].max(self.size[1])
    }

    /// Returns RGBA pixels, converting from RGB when needed.
    pub fn into_rgba(self) -> Self {
        match self.channels {
            Channels::Rgba => self,
            Channels::Rgb => {
                let mut data = Vec::with_capacity(self.data.len() / 3 * 4);
                for rgb in self.data.as_chunks::<3>().0 {
                    data.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
                }
                Self {
                    size: self.size,
                    channels: Channels::Rgba,
                    data,
                }
            }
        }
    }

    /// Scales down so the long edge is at most `max_long_edge`. Never scales up.
    pub fn fit_within(self, max_long_edge: u32) -> Result<Self, MediaError> {
        let long_edge = self.long_edge();
        if long_edge <= max_long_edge || long_edge == 0 {
            return Ok(self);
        }
        let scale = f64::from(max_long_edge) / f64::from(long_edge);
        let width = ((f64::from(self.size[0]) * scale).round() as u32).max(1);
        let height = ((f64::from(self.size[1]) * scale).round() as u32).max(1);
        let pixel_type = match self.channels {
            Channels::Rgb => PixelType::U8x3,
            Channels::Rgba => PixelType::U8x4,
        };
        let source = Image::from_vec_u8(self.size[0], self.size[1], self.data, pixel_type)
            .map_err(MediaError::from_display)?;
        let mut target = Image::new(width, height, pixel_type);
        let options = ResizeOptions::new()
            .resize_alg(ResizeAlg::Convolution(FilterType::Hamming))
            .use_alpha(false);
        Resizer::new()
            .resize(&source, &mut target, &options)
            .map_err(MediaError::from_display)?;
        Ok(Self {
            size: [width, height],
            channels: self.channels,
            data: target.into_vec(),
        })
    }
}

/// Reads only the frame header and returns the pixel size.
pub fn jpeg_size(data: &[u8]) -> Option<[u32; 2]> {
    let mut decoder = JpegDecoder::new(ZCursor::new(data));
    decoder.decode_headers().ok()?;
    let info = decoder.info()?;
    Some([u32::from(info.width), u32::from(info.height)])
}

pub fn decode_jpeg(data: &[u8], channels: Channels) -> Result<Pixels, MediaError> {
    let colorspace = match channels {
        Channels::Rgb => ColorSpace::RGB,
        Channels::Rgba => ColorSpace::RGBA,
    };
    let options = DecoderOptions::new_fast().jpeg_set_out_colorspace(colorspace);
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(data), options);
    let decoded = decoder.decode().map_err(MediaError::from_display)?;
    let info = decoder
        .info()
        .ok_or_else(|| MediaError::new("JPEG has no frame header"))?;
    let size = [u32::from(info.width), u32::from(info.height)];
    if decoded.len() != size[0] as usize * size[1] as usize * channels.count() {
        return Err(MediaError::new(
            "JPEG decoded to an unexpected pixel layout",
        ));
    }
    Ok(Pixels {
        size,
        channels,
        data: decoded,
    })
}

pub fn encode_jpeg(pixels: &Pixels, quality: u8) -> Result<Vec<u8>, MediaError> {
    use image::ImageEncoder;
    let rgb;
    let data: &[u8] = match pixels.channels {
        Channels::Rgb => &pixels.data,
        Channels::Rgba => {
            rgb = pixels
                .data
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[1], p[2]])
                .collect::<Vec<u8>>();
            &rgb
        }
    };
    let mut out = Vec::with_capacity(data.len() / 8);
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
        .write_image(
            data,
            pixels.size[0],
            pixels.size[1],
            image::ExtendedColorType::Rgb8,
        )
        .map_err(MediaError::from_display)?;
    Ok(out)
}

/// Decodes a PNG, WebP, GIF, BMP or TIFF file held in memory.
pub fn decode_bitmap(data: &[u8]) -> Result<Pixels, MediaError> {
    image::load_from_memory(data)
        .map(from_dynamic)
        .map_err(MediaError::from_display)
}

/// Converts any image the `image` crate produced into premultiplied RGBA.
pub fn from_dynamic(image: image::DynamicImage) -> Pixels {
    let has_alpha = image.color().has_alpha();
    let rgba = image.into_rgba8();
    let size = [rgba.width(), rgba.height()];
    let mut data = rgba.into_raw();
    if has_alpha {
        for pixel in data.as_chunks_mut::<4>().0 {
            let alpha = u16::from(pixel[3]);
            if alpha < 255 {
                for channel in &mut pixel[..3] {
                    *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
                }
            }
        }
    }
    Pixels {
        size,
        channels: Channels::Rgba,
        data,
    }
}

#[cfg(test)]
pub mod test_support {
    use super::*;

    /// A decodable JPEG with a horizontal gradient.
    pub fn gradient_jpeg(width: u32, height: u32) -> Vec<u8> {
        let mut data = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            for x in 0..width {
                data.extend_from_slice(&[(x * 255 / width) as u8, (y * 255 / height) as u8, 128]);
            }
        }
        let pixels = Pixels {
            size: [width, height],
            channels: Channels::Rgb,
            data,
        };
        encode_jpeg(&pixels, 90).expect("encode")
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::gradient_jpeg;
    use super::*;

    #[test]
    fn jpeg_round_trip_keeps_size_and_layout() {
        let jpeg = gradient_jpeg(64, 40);
        assert_eq!(jpeg_size(&jpeg), Some([64, 40]));
        let rgb = decode_jpeg(&jpeg, Channels::Rgb).expect("rgb");
        assert_eq!(rgb.data.len(), 64 * 40 * 3);
        let rgba = decode_jpeg(&jpeg, Channels::Rgba).expect("rgba");
        assert_eq!(rgba.data.len(), 64 * 40 * 4);
        assert_eq!(rgb.into_rgba().data.len(), 64 * 40 * 4);
    }

    #[test]
    fn fit_within_scales_down_only() {
        let jpeg = gradient_jpeg(200, 100);
        let pixels = decode_jpeg(&jpeg, Channels::Rgb).expect("decode");
        let small = pixels.clone().fit_within(50).expect("resize");
        assert_eq!(small.size, [50, 25]);
        assert_eq!(small.data.len(), 50 * 25 * 3);
        let same = pixels.fit_within(400).expect("resize");
        assert_eq!(same.size, [200, 100]);
    }
}

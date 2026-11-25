//! Animated GIF and WebP files: every frame, with how long it is shown.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::time::Duration;

use image::codecs::gif::GifDecoder;
use image::codecs::webp::WebPDecoder;
use image::{AnimationDecoder, DynamicImage, ImageFormat, ImageReader};

use super::MediaError;
use super::pixels::{self, Pixels};

/// Decoded frames kept for one file at most; a longer animation shows its first frame only.
const MAX_BYTES: usize = 256 << 20;
/// Browsers show frames with a shorter delay for this long, and so does rawv.
const SHORTEST_DELAY: Duration = Duration::from_millis(20);
const DEFAULT_DELAY: Duration = Duration::from_millis(100);

pub struct Frame {
    /// Premultiplied RGBA, the whole canvas, at most `max_side` on its long edge.
    pub pixels: Pixels,
    pub delay: Duration,
}

pub struct Animation {
    /// Canvas size before any downscaling.
    pub original_size: [u32; 2],
    /// One frame for a still picture or an animation too large to keep.
    pub frames: Vec<Frame>,
}

/// The frames of a GIF or WebP file; `None` for other formats.
pub fn decode(path: &Path, max_side: u32) -> Result<Option<Animation>, MediaError> {
    let format = ImageReader::open(path)?.with_guessed_format()?.format();
    let reader =
        || -> Result<BufReader<File>, MediaError> { Ok(BufReader::new(File::open(path)?)) };
    let frames = match format {
        Some(ImageFormat::Gif) => GifDecoder::new(reader()?)
            .map_err(MediaError::from_display)?
            .into_frames(),
        Some(ImageFormat::WebP) => WebPDecoder::new(reader()?)
            .map_err(MediaError::from_display)?
            .into_frames(),
        _ => return Ok(None),
    };
    let mut animation = Animation {
        original_size: [0, 0],
        frames: Vec::new(),
    };
    let mut bytes = 0;
    for frame in frames {
        let frame = frame.map_err(MediaError::from_display)?;
        let delay = shown_for(frame.delay().into());
        let canvas = frame.into_buffer();
        if animation.frames.is_empty() {
            animation.original_size = [canvas.width(), canvas.height()];
        }
        let pixels = pixels::from_dynamic(DynamicImage::ImageRgba8(canvas)).fit_within(max_side)?;
        bytes += pixels.data.len();
        if bytes > MAX_BYTES && !animation.frames.is_empty() {
            animation.frames.truncate(1);
            break;
        }
        animation.frames.push(Frame { pixels, delay });
    }
    Ok(Some(animation))
}

fn shown_for(delay: Duration) -> Duration {
    if delay < SHORTEST_DELAY {
        DEFAULT_DELAY
    } else {
        delay
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::gif::{GifEncoder, Repeat};
    use image::{Delay, Rgba, RgbaImage};

    fn gif_file(name: &str, colors: &[[u8; 4]], delay_ms: u32) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("rawv-test-{}-{name}.gif", std::process::id()));
        let file = File::create(&path).expect("create");
        let mut encoder = GifEncoder::new(file);
        encoder.set_repeat(Repeat::Infinite).expect("repeat");
        let frames = colors.iter().map(|&color| {
            image::Frame::from_parts(
                RgbaImage::from_pixel(8, 6, Rgba(color)),
                0,
                0,
                Delay::from_numer_denom_ms(delay_ms, 1),
            )
        });
        encoder.encode_frames(frames).expect("encode");
        path
    }

    #[test]
    fn decodes_every_frame_with_its_delay() {
        let path = gif_file(
            "three",
            &[[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]],
            70,
        );
        let animation = decode(&path, 4096).expect("decode").expect("a GIF");
        assert_eq!(animation.original_size, [8, 6]);
        assert_eq!(animation.frames.len(), 3);
        assert_eq!(animation.frames[1].pixels.data[..3], [0, 255, 0]);
        assert_eq!(animation.frames[2].delay, Duration::from_millis(70));
        std::fs::remove_file(&path).expect("cleanup");
    }

    #[test]
    fn a_zero_delay_is_shown_like_browsers_do() {
        let path = gif_file("fast", &[[1, 2, 3, 255], [4, 5, 6, 255]], 0);
        let animation = decode(&path, 4096).expect("decode").expect("a GIF");
        assert!(
            animation
                .frames
                .iter()
                .all(|frame| frame.delay == DEFAULT_DELAY)
        );
        std::fs::remove_file(&path).expect("cleanup");
    }

    #[test]
    fn other_formats_are_not_animations() {
        let path = std::env::temp_dir().join(format!("rawv-test-{}-still.png", std::process::id()));
        RgbaImage::from_pixel(4, 4, Rgba([9, 9, 9, 255]))
            .save(&path)
            .expect("save");
        assert!(decode(&path, 4096).expect("decode").is_none());
        std::fs::remove_file(&path).expect("cleanup");
    }
}

//! Turning decoded pixels into GPU textures.

use egui::{ColorImage, Context, TextureFilter, TextureHandle, TextureOptions, TextureWrapMode};

use crate::media::pixels::Pixels;

/// Linear filtering with mipmaps, so large photos stay clean when shown small.
const OPTIONS: TextureOptions = TextureOptions {
    magnification: TextureFilter::Linear,
    minification: TextureFilter::Linear,
    wrap_mode: TextureWrapMode::ClampToEdge,
    mipmap_mode: Some(TextureFilter::Linear),
};

pub fn upload(ctx: &Context, name: &str, pixels: Pixels) -> TextureHandle {
    let pixels = pixels.into_rgba();
    let size = [pixels.size[0] as usize, pixels.size[1] as usize];
    let image = match bytemuck::allocation::try_cast_vec(pixels.data) {
        Ok(colors) => ColorImage::new(size, colors),
        Err((_, bytes)) => ColorImage::from_rgba_premultiplied(size, &bytes),
    };
    ctx.load_texture(name, image, OPTIONS)
}

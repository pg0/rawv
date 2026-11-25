//! Colours, fonts and sizes. Every drawn element takes its look from here.

use std::sync::Arc;

use egui::{Color32, FontData, FontDefinitions, FontFamily, FontId};

pub const BACKGROUND: Color32 = Color32::from_rgb(0x11, 0x11, 0x11);
pub const HEADER: Color32 = Color32::from_rgb(0x0e, 0x0e, 0x0e);
pub const BORDER: Color32 = Color32::from_rgb(0x22, 0x22, 0x22);
pub const ROLL_BORDER: Color32 = Color32::from_rgb(0x1c, 0x1c, 0x1c);
pub const INK: Color32 = Color32::from_rgb(0xe8, 0xe8, 0xe8);
pub const INK_BRIGHT: Color32 = Color32::WHITE;
pub const MUTED: Color32 = Color32::from_rgb(0x9a, 0x9a, 0x9a);
pub const FAINT: Color32 = Color32::from_rgb(0x55, 0x55, 0x55);
pub const ACCENT: Color32 = Color32::from_rgb(0xe2, 0xb7, 0x3a);
pub const REJECT: Color32 = Color32::from_rgb(0xe0, 0x4f, 0x3f);
pub const ON_ACCENT: Color32 = Color32::from_rgb(0x11, 0x11, 0x11);
pub const STRIP: Color32 = Color32::from_rgb(0x0c, 0x0c, 0x0c);
pub const SPROCKET: Color32 = Color32::BLACK;
pub const FRAME: Color32 = Color32::BLACK;
pub const FRAME_EMPTY: Color32 = Color32::from_rgb(0x1a, 0x1a, 0x1a);
pub const FRAME_HOVER: Color32 = Color32::from_rgb(0x44, 0x44, 0x44);
pub const BUTTON: Color32 = Color32::from_rgb(0x1a, 0x1a, 0x1a);
pub const BUTTON_HOVER: Color32 = Color32::from_rgb(0x22, 0x22, 0x22);
pub const BUTTON_BORDER: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x2a);
pub const VIEWER_BACKGROUND: Color32 = Color32::from_rgb(0x05, 0x05, 0x05);
pub const OVERLAY: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 170);
pub const OVERLAY_BORDER: Color32 = Color32::from_rgb(0x33, 0x33, 0x33);

const STRONG_FAMILY: &str = "strong";

pub fn body(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub fn strong(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(STRONG_FAMILY.into()))
}

pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

pub fn apply(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    ctx.set_theme(egui::Theme::Dark);
    // Panels and tooltips appear at once instead of fading in.
    ctx.all_styles_mut(|style| style.animation_time = 0.0);
    ctx.set_visuals_of(egui::Theme::Dark, visuals());
    ctx.all_styles_mut(|style| {
        style.spacing.scroll = egui::style::ScrollStyle::floating();
        style.interaction.tooltip_delay = 0.4;
    });
}

fn visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BACKGROUND;
    visuals.window_fill = HEADER;
    visuals.extreme_bg_color = STRIP;
    visuals.override_text_color = Some(INK);
    visuals.selection.bg_fill = ACCENT.gamma_multiply(0.35);
    visuals.widgets.noninteractive.bg_stroke.color = BORDER;
    visuals
}

/// Uses the system's Segoe UI and Consolas where present, keeping egui's fonts as fallback.
fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let font_dir = std::env::var_os("WINDIR")
        .map(|windir| std::path::PathBuf::from(windir).join("Fonts"))
        .unwrap_or_default();
    let mut load = |name: &str, file: &str| -> bool {
        match std::fs::read(font_dir.join(file)) {
            Ok(bytes) => {
                fonts
                    .font_data
                    .insert(name.to_owned(), Arc::new(FontData::from_owned(bytes)));
                true
            }
            Err(_) => false,
        }
    };
    let has_regular = load("system-regular", "segoeui.ttf");
    let has_strong = load("system-strong", "seguisb.ttf");
    let has_mono = load("system-mono", "consola.ttf");

    let proportional = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mut strong_family = proportional.clone();
    if has_strong {
        strong_family.insert(0, "system-strong".to_owned());
    } else if has_regular {
        strong_family.insert(0, "system-regular".to_owned());
    }
    fonts
        .families
        .insert(FontFamily::Name(STRONG_FAMILY.into()), strong_family);
    if has_regular {
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "system-regular".to_owned());
    }
    if has_mono {
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, "system-mono".to_owned());
    }
    fonts
}

/// The icon file `build.rs` also puts into the executables; made by `tools/make_icon.py`.
const ICON: &[u8] = include_bytes!("../assets/rawv.ico");

/// The window icon, the largest picture in the icon file.
pub fn window_icon() -> Option<egui::IconData> {
    let picture = image::load_from_memory_with_format(ICON, image::ImageFormat::Ico)
        .ok()?
        .into_rgba8();
    Some(egui::IconData {
        width: picture.width(),
        height: picture.height(),
        rgba: picture.into_raw(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_icon_decodes_from_the_icon_file() {
        let icon = window_icon().expect("icon decodes");
        assert_eq!((icon.width, icon.height), (256, 256));
        assert_eq!(icon.rgba.len(), 256 * 256 * 4);
    }
}

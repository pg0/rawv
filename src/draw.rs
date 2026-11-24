//! Painting helpers shared by the gallery and the viewer.

use egui::{Color32, Mesh, Painter, Pos2, Rect, Shape, TextureId, pos2};

use crate::media::orientation::Orientation;
use crate::media::thumb::ContentRect;

/// Paints a texture into `target`, applying `orientation` at draw time.
///
/// `content` is the part of the stored texture that holds the picture and `crop` the
/// part of the oriented picture to show (normalized, `Rect::from_min_max((0,0),(1,1))` for all).
pub fn image(
    painter: &Painter,
    texture: TextureId,
    target: Rect,
    orientation: Orientation,
    content: ContentRect,
    crop: Rect,
) {
    let content_size = content.size();
    let corners = [
        (target.left_top(), crop.left_top()),
        (target.right_top(), crop.right_top()),
        (target.right_bottom(), crop.right_bottom()),
        (target.left_bottom(), crop.left_bottom()),
    ];
    let mut mesh = Mesh::with_texture(texture);
    for (position, shown) in corners {
        let stored = orientation.stored_uv([shown.x, shown.y]);
        mesh.vertices.push(egui::epaint::Vertex {
            pos: position,
            uv: pos2(
                content.min[0] + stored[0] * content_size[0],
                content.min[1] + stored[1] * content_size[1],
            ),
            color: Color32::WHITE,
        });
    }
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    painter.add(Shape::mesh(mesh));
}

/// Size of the oriented picture content, in the texture's pixels.
pub fn shown_size(size: [u32; 2], orientation: Orientation, content: ContentRect) -> [f32; 2] {
    let content_size = content.size();
    orientation.display_size([
        size[0] as f32 * content_size[0],
        size[1] as f32 * content_size[1],
    ])
}

/// The centred part of a picture with aspect ratio `image_aspect` that fills `frame_aspect`.
pub fn cover_crop(image_aspect: f32, frame_aspect: f32) -> Rect {
    if image_aspect > frame_aspect {
        let visible = frame_aspect / image_aspect;
        Rect::from_min_max(
            pos2((1.0 - visible) / 2.0, 0.0),
            pos2((1.0 + visible) / 2.0, 1.0),
        )
    } else {
        let visible = image_aspect / frame_aspect;
        Rect::from_min_max(
            pos2(0.0, (1.0 - visible) / 2.0),
            pos2(1.0, (1.0 + visible) / 2.0),
        )
    }
}

pub const WHOLE: Rect = Rect {
    min: Pos2::ZERO,
    max: pos2(1.0, 1.0),
};

/// A small triangle pointing right (collapsed) or down (expanded).
pub fn chevron(painter: &Painter, center: Pos2, expanded: bool, color: Color32) {
    let r = 3.5;
    let points = if expanded {
        vec![
            pos2(center.x - r, center.y - r * 0.6),
            pos2(center.x + r, center.y - r * 0.6),
            pos2(center.x, center.y + r * 0.8),
        ]
    } else {
        vec![
            pos2(center.x - r * 0.6, center.y - r),
            pos2(center.x + r * 0.8, center.y),
            pos2(center.x - r * 0.6, center.y + r),
        ]
    };
    painter.add(Shape::convex_polygon(points, color, egui::Stroke::NONE));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_crop_keeps_centre_and_fills_frame() {
        let wide = cover_crop(3.0, 1.5);
        assert!((wide.width() - 0.5).abs() < 1e-6);
        assert!((wide.center().x - 0.5).abs() < 1e-6);
        assert_eq!(wide.height(), 1.0);

        let tall = cover_crop(0.75, 1.5);
        assert!((tall.height() - 0.5).abs() < 1e-6);
        assert_eq!(tall.width(), 1.0);

        let same = cover_crop(1.5, 1.5);
        assert!((same.width() - 1.0).abs() < 1e-6 && (same.height() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn shown_size_applies_content_then_orientation() {
        let content = ContentRect {
            min: [0.0, 0.125],
            max: [1.0, 0.875],
        };
        let upright = shown_size([160, 120], Orientation::NORMAL, content);
        assert_eq!(upright, [160.0, 90.0]);
        let rotated = shown_size([160, 120], Orientation::from_exif(6), content);
        assert_eq!(rotated, [90.0, 160.0]);
    }
}

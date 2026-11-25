//! Painting helpers shared by the gallery and the viewer.

use egui::{Color32, Mesh, Painter, Pos2, Rect, Shape, Stroke, TextureId, Vec2, pos2, vec2};

use crate::media::orientation::Orientation;
use crate::media::thumb::ContentRect;
use crate::media::xmp::{REJECTED, Rating};
use crate::theme;

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

/// Width and height of the mark [`rating`] draws with a star of radius `star`.
pub fn rating_size(rating: Rating, star: f32) -> Vec2 {
    let width = if rating == REJECTED {
        FLAG_WIDTH
    } else {
        2.0 + DIGIT_WIDTH
    };
    vec2(width * star, star * 2.0)
}

/// Width of the flag, in star radiuses.
const FLAG_WIDTH: f32 = 1.7;
/// Width of the number beside the star, in star radiuses, with the gap before it.
const DIGIT_WIDTH: f32 = 1.6;

/// Draws a rating ending at `right`, vertically centred on it: a flag for a rejected
/// picture, else a star and the number of stars. An unrated picture gets no mark.
pub fn rating(painter: &Painter, right: Pos2, rating: Rating, star: f32) {
    if rating == REJECTED {
        let pole = right - vec2(FLAG_WIDTH * star, 0.0);
        let stroke = Stroke::new((star * 0.28).max(1.0), theme::REJECT);
        painter.line_segment([pole - vec2(0.0, star), pole + vec2(0.0, star)], stroke);
        painter.add(Shape::convex_polygon(
            vec![
                pole - vec2(0.0, star),
                pole + vec2(FLAG_WIDTH * star, -0.45 * star),
                pole + vec2(0.0, 0.1 * star),
            ],
            theme::REJECT,
            Stroke::NONE,
        ));
        return;
    }
    if rating <= 0 {
        return;
    }
    painter.text(
        right,
        egui::Align2::RIGHT_CENTER,
        rating.to_string(),
        theme::mono(star * 2.3),
        theme::ACCENT,
    );
    let center = right - vec2((DIGIT_WIDTH + 1.0) * star, 0.0);
    let mut mesh = Mesh::default();
    mesh.colored_vertex(center, theme::ACCENT);
    for point in 0..10u8 {
        let angle = (f32::from(point) * 36.0 - 90.0).to_radians();
        let reach = if point % 2 == 0 { star } else { star * 0.42 };
        mesh.colored_vertex(
            center + vec2(angle.cos(), angle.sin()) * reach,
            theme::ACCENT,
        );
    }
    for point in 0..10u32 {
        mesh.add_triangle(0, 1 + point, 1 + (point + 1) % 10);
    }
    painter.add(Shape::mesh(mesh));
}

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

/// A folder with two subfolders hanging off it, filling `square`.
pub fn subfolders_icon(painter: &Painter, square: Rect, color: Color32) {
    let at = |x: f32, y: f32| square.min + vec2(x, y) * (square.width() / 14.0);
    let line = Stroke::new(1.3, color);
    folder(
        painter,
        Rect::from_min_max(at(0.0, 0.0), at(8.0, 5.0)),
        color,
    );
    painter.line_segment([at(2.5, 5.0), at(2.5, 12.2)], line);
    for y in [7.5, 12.2] {
        painter.line_segment([at(2.5, y), at(6.0, y)], line);
        folder(
            painter,
            Rect::from_min_max(at(6.5, y - 1.7), at(14.0, y + 1.7)),
            color,
        );
    }
}

/// Two chevrons pointing apart: open every roll.
pub fn expand_all_icon(painter: &Painter, square: Rect, color: Color32) {
    let at = |x: f32, y: f32| square.min + vec2(x, y) * (square.width() / 14.0);
    let line = Stroke::new(1.6, color);
    painter.line(vec![at(2.0, 5.5), at(7.0, 1.5), at(12.0, 5.5)], line);
    painter.line(vec![at(2.0, 8.5), at(7.0, 12.5), at(12.0, 8.5)], line);
}

/// A grid of `per_row` × `per_row` tiles: many small thumbnails or a few large ones.
pub fn thumb_size_icon(painter: &Painter, square: Rect, color: Color32, per_row: usize) {
    let per_row = per_row.max(1);
    let gap = 1.5 * square.width() / 14.0;
    let tile = (square.width() - gap * (per_row - 1) as f32) / per_row as f32;
    for row in 0..per_row {
        for column in 0..per_row {
            let min = square.min + vec2(column as f32, row as f32) * (tile + gap);
            painter.rect_filled(Rect::from_min_size(min, vec2(tile, tile)), 1.0, color);
        }
    }
}

/// An arrow down for ascending or up for descending order, beside an "A" for sorting by
/// name or a clock for sorting by date.
pub fn sort_icon(painter: &Painter, square: Rect, color: Color32, by_date: bool, descending: bool) {
    let at = |x: f32, y: f32| square.min + vec2(x, y) * (square.width() / 14.0);
    let line = Stroke::new(1.5, color);
    let (tail, head, wing) = if descending {
        (13.0, 1.0, 4.0)
    } else {
        (1.0, 13.0, 10.0)
    };
    painter.line_segment([at(3.0, tail), at(3.0, head)], line);
    painter.line(vec![at(0.5, wing), at(3.0, head), at(5.5, wing)], line);
    if by_date {
        painter.circle_stroke(at(10.0, 7.0), 3.6 * square.width() / 14.0, line);
        painter.line(
            vec![at(10.0, 5.0), at(10.0, 7.0), at(11.6, 7.8)],
            Stroke::new(1.2, color),
        );
    } else {
        painter.text(
            at(10.0, 7.5),
            egui::Align2::CENTER_CENTER,
            "A",
            theme::strong(11.0),
            color,
        );
    }
}

/// A filled folder: a body with a tab on its upper left.
fn folder(painter: &Painter, rect: Rect, color: Color32) {
    let tab = Rect::from_min_size(rect.min, vec2(rect.width() * 0.45, rect.height() * 0.5));
    painter.rect_filled(tab, 1.0, color);
    let body = Rect::from_min_max(
        pos2(rect.left(), rect.top() + rect.height() * 0.25),
        rect.max,
    );
    painter.rect_filled(body, 1.0, color);
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

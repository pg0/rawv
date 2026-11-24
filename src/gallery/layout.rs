//! Geometry of a roll: frames on a film strip, wrapped into rows when expanded.

use egui::{Vec2, vec2};

pub const META_WIDTH: f32 = 150.0;
pub const PAGE_MARGIN: f32 = 16.0;
pub const META_GAP: f32 = 12.0;
pub const ROLL_PADDING: f32 = 10.0;
pub const STRIP_PAD_X: f32 = 8.0;
pub const STRIP_PAD_TOP: f32 = 14.0;
pub const STRIP_PAD_BOTTOM: f32 = 18.0;
pub const FRAME_GAP: f32 = 6.0;
pub const ROW_GAP: f32 = 18.0;
pub const META_MIN_HEIGHT: f32 = 70.0;
/// Height of the caption of a roll without images.
pub const EMPTY_ROLL_HEIGHT: f32 = 44.0;
const BASE_FRAME: Vec2 = vec2(120.0, 78.0);
/// Rows the selection moves with Page Up and Page Down.
pub const PAGE_ROWS: usize = 4;
/// Thumbnail size presets: label and scale.
pub const THUMB_SIZES: [(&str, f32); 3] = [("S", 1.0), ("M", 1.6), ("L", 2.4)];

fn nearest_thumb_size(scale: f32) -> usize {
    let distance = |index: &usize| (THUMB_SIZES[*index].1 - scale).abs();
    (0..THUMB_SIZES.len())
        .min_by(|a, b| distance(a).total_cmp(&distance(b)))
        .unwrap_or(0)
}

/// Label of the preset nearest to `scale`.
pub fn thumb_size_label(scale: f32) -> &'static str {
    THUMB_SIZES[nearest_thumb_size(scale)].0
}

/// Scale of the preset after the one nearest to `scale`; the largest is followed by the smallest.
pub fn next_thumb_size(scale: f32) -> f32 {
    THUMB_SIZES[(nearest_thumb_size(scale) + 1) % THUMB_SIZES.len()].1
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub frame: Vec2,
}

impl Metrics {
    pub fn new(thumb_scale: f32) -> Self {
        Self {
            frame: (BASE_FRAME * thumb_scale).round(),
        }
    }

    fn pitch_x(self) -> f32 {
        self.frame.x + FRAME_GAP
    }

    fn pitch_y(self) -> f32 {
        self.frame.y + ROW_GAP
    }

    /// Frames per row that fit into the strip's inner width.
    pub fn columns(self, inner_width: f32) -> usize {
        (((inner_width + FRAME_GAP) / self.pitch_x()).floor() as usize).max(1)
    }

    pub fn rows(count: usize, columns: usize) -> usize {
        count.div_ceil(columns.max(1)).max(1)
    }

    pub fn strip_height(self, rows: usize) -> f32 {
        let rows = rows.max(1) as f32;
        STRIP_PAD_TOP + rows * self.frame.y + (rows - 1.0) * ROW_GAP + STRIP_PAD_BOTTOM
    }

    pub fn roll_height(self, rows: usize) -> f32 {
        self.strip_height(rows).max(META_MIN_HEIGHT) + 2.0 * ROLL_PADDING + 1.0
    }

    /// Offset of frame `index` from the top-left of the frame area.
    pub fn frame_offset(self, index: usize, columns: usize) -> Vec2 {
        let columns = columns.max(1);
        vec2(
            (index % columns) as f32 * self.pitch_x(),
            (index / columns) as f32 * self.pitch_y(),
        )
    }

    /// Total width of `count` frames laid out in a single row.
    pub fn row_width(self, count: usize) -> f32 {
        (count as f32 * self.pitch_x() - FRAME_GAP).max(0.0)
    }

    /// Index range of the frames in one row that intersect `min_x..max_x`.
    pub fn visible_in_row(self, min_x: f32, max_x: f32, count: usize) -> std::ops::Range<usize> {
        let first = (min_x / self.pitch_x()).floor().max(0.0) as usize;
        let last = (max_x / self.pitch_x()).ceil().max(0.0) as usize;
        first.min(count)..last.min(count)
    }

    /// Row range of a wrapped strip that intersects `min_y..max_y`.
    pub fn visible_rows(self, min_y: f32, max_y: f32, rows: usize) -> std::ops::Range<usize> {
        let first = (min_y / self.pitch_y()).floor().max(0.0) as usize;
        let last = (max_y / self.pitch_y()).ceil().max(0.0) as usize;
        first.min(rows)..last.min(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnail_sizes_cycle_from_the_nearest_preset() {
        assert_eq!(thumb_size_label(1.0), "S");
        assert_eq!(next_thumb_size(1.0), 1.6);
        assert_eq!(next_thumb_size(1.6), 2.4);
        assert_eq!(next_thumb_size(2.4), 1.0);
        assert_eq!(thumb_size_label(1.15), "S");
        assert_eq!(next_thumb_size(1.15), 1.6);
        assert_eq!(thumb_size_label(3.0), "L");
    }

    #[test]
    fn columns_and_rows_wrap_like_a_flex_row() {
        let metrics = Metrics::new(1.0);
        assert_eq!(metrics.columns(120.0), 1);
        assert_eq!(metrics.columns(245.9), 1);
        assert_eq!(metrics.columns(246.0), 2);
        assert_eq!(metrics.columns(10.0), 1);
        assert_eq!(Metrics::rows(17, 8), 3);
        assert_eq!(Metrics::rows(16, 8), 2);
        assert_eq!(Metrics::rows(0, 8), 1);
    }

    #[test]
    fn frame_offsets_follow_rows_and_columns() {
        let metrics = Metrics::new(1.0);
        assert_eq!(metrics.frame_offset(0, 8), vec2(0.0, 0.0));
        assert_eq!(metrics.frame_offset(9, 8), vec2(126.0, 96.0));
        assert_eq!(metrics.row_width(3), 372.0);
        assert_eq!(metrics.row_width(0), 0.0);
    }

    #[test]
    fn visible_ranges_cover_partially_visible_frames() {
        let metrics = Metrics::new(1.0);
        assert_eq!(metrics.visible_in_row(0.0, 126.0, 10), 0..1);
        assert_eq!(metrics.visible_in_row(125.0, 127.0, 10), 0..2);
        assert_eq!(metrics.visible_in_row(5000.0, 6000.0, 10), 10..10);
        assert_eq!(metrics.visible_rows(-50.0, 100.0, 5), 0..2);
    }

    #[test]
    fn strip_height_grows_by_one_pitch_per_row() {
        let metrics = Metrics::new(1.0);
        assert_eq!(metrics.strip_height(1), 110.0);
        assert_eq!(metrics.strip_height(3) - metrics.strip_height(2), 96.0);
    }
}

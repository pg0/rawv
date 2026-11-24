//! Image orientation as stored in the EXIF `Orientation` tag.
//!
//! Pixels are always kept in stored order; the orientation is applied when drawing.

/// Display transform: mirror horizontally (optional), then rotate clockwise by quarter turns.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct Orientation {
    quarter_turns: u8,
    mirrored: bool,
}

impl Orientation {
    pub const NORMAL: Self = Self {
        quarter_turns: 0,
        mirrored: false,
    };

    /// Maps an EXIF orientation value (1..=8). Anything else is treated as upright.
    pub fn from_exif(value: u16) -> Self {
        let (quarter_turns, mirrored) = match value {
            2 => (0, true),
            3 => (2, false),
            4 => (2, true),
            5 => (3, true),
            6 => (1, false),
            7 => (1, true),
            8 => (3, false),
            _ => (0, false),
        };
        Self {
            quarter_turns,
            mirrored,
        }
    }

    pub fn to_exif(self) -> u16 {
        match (self.quarter_turns, self.mirrored) {
            (0, true) => 2,
            (2, false) => 3,
            (2, true) => 4,
            (3, true) => 5,
            (1, false) => 6,
            (1, true) => 7,
            (3, false) => 8,
            _ => 1,
        }
    }

    pub fn is_valid_exif(value: u16) -> bool {
        (1..=8).contains(&value)
    }

    /// Adds a clockwise rotation of `quarter_turns` on top of the current orientation.
    pub fn rotated_cw(self, quarter_turns: u8) -> Self {
        Self {
            quarter_turns: (self.quarter_turns + quarter_turns) % 4,
            mirrored: self.mirrored,
        }
    }

    pub fn swaps_axes(self) -> bool {
        self.quarter_turns % 2 == 1
    }

    /// Size of the image as displayed, given its stored size.
    pub fn display_size(self, stored: [f32; 2]) -> [f32; 2] {
        if self.swaps_axes() {
            [stored[1], stored[0]]
        } else {
            stored
        }
    }

    /// Maps a normalized display coordinate to the normalized stored coordinate it shows.
    pub fn stored_uv(self, display: [f32; 2]) -> [f32; 2] {
        let [u, v] = display;
        let [su, sv] = match self.quarter_turns {
            1 => [v, 1.0 - u],
            2 => [1.0 - u, 1.0 - v],
            3 => [1.0 - v, u],
            _ => [u, v],
        };
        if self.mirrored {
            [1.0 - su, sv]
        } else {
            [su, sv]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exif_values_round_trip() {
        for value in 1..=8 {
            assert_eq!(Orientation::from_exif(value).to_exif(), value);
        }
        assert_eq!(Orientation::from_exif(0), Orientation::NORMAL);
        assert_eq!(Orientation::from_exif(9), Orientation::NORMAL);
    }

    #[test]
    fn four_quarter_turns_return_to_start() {
        for value in 1..=8 {
            let start = Orientation::from_exif(value);
            assert_eq!(start.rotated_cw(1).rotated_cw(3), start);
            assert_eq!(start.rotated_cw(2).rotated_cw(2), start);
        }
    }

    #[test]
    fn clockwise_rotation_follows_exif_sequence() {
        let upright = Orientation::NORMAL;
        assert_eq!(upright.rotated_cw(1).to_exif(), 6);
        assert_eq!(upright.rotated_cw(2).to_exif(), 3);
        assert_eq!(upright.rotated_cw(3).to_exif(), 8);
    }

    #[test]
    fn rotate_90_shows_stored_bottom_left_at_display_top_left() {
        let o = Orientation::from_exif(6);
        assert_eq!(o.stored_uv([0.0, 0.0]), [0.0, 1.0]);
        assert_eq!(o.stored_uv([1.0, 0.0]), [0.0, 0.0]);
        assert_eq!(o.display_size([300.0, 200.0]), [200.0, 300.0]);
    }

    #[test]
    fn transpose_swaps_coordinates() {
        let o = Orientation::from_exif(5);
        assert_eq!(o.stored_uv([0.25, 0.75]), [0.75, 0.25]);
    }

    #[test]
    fn mirror_flips_only_horizontally() {
        let o = Orientation::from_exif(2);
        assert_eq!(o.stored_uv([0.25, 0.75]), [0.75, 0.75]);
    }
}

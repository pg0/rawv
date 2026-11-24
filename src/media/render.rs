//! Turns demosaiced camera colours into display pixels.
//!
//! Sensor channels are white-balanced, clipped where the sensor was saturated and converted
//! to sRGB. Brightness and contrast then follow the JPEG the camera embedded in the file,
//! when there is one: each channel's tone curve is chosen so that the developed image has
//! the same distribution of levels as that JPEG.

use super::pixels::{Channels, Pixels};
use super::thumb::ContentRect;

/// Entries of a tone curve, indexed by linear light from black to white.
const CURVE_SIZE: usize = 4096;
/// A reference with fewer pixels gives too rough a histogram to match tones to.
const MIN_REFERENCE_PIXELS: u64 = 100_000;
/// Every n-th pixel in both directions is measured for the developed image's histogram.
const SAMPLE_STEP: usize = 3;
/// Aspect ratios closer than this count as equal.
const ASPECT_TOLERANCE: f32 = 1.01;

const SRGB_TO_XYZ: Matrix = [
    [0.412_456_4, 0.357_576_1, 0.180_437_5],
    [0.212_672_9, 0.715_152_2, 0.072_175_0],
    [0.019_333_9, 0.119_192, 0.950_304_1],
];

type Matrix = [[f32; 3]; 3];
/// Display level for each step of linear light.
type Curve = Vec<u8>;

/// How a camera's colour channels relate to standard colours.
pub struct CameraColor {
    /// Multipliers that make neutral grey equal in all channels.
    pub white_balance: [f32; 3],
    /// From CIE XYZ under daylight (D65) to the camera's channels.
    pub xyz_to_camera: Matrix,
}

/// Camera channels to linear sRGB.
struct Transform {
    gain: [f32; 3],
    camera_to_srgb: Matrix,
}

impl Transform {
    fn new(color: &CameraColor) -> Option<Self> {
        let balance = color.white_balance;
        if balance.iter().any(|gain| !gain.is_finite() || *gain <= 0.0) {
            return None;
        }
        // A saturated sensor reads 1.0 in every channel. Scaling the smallest multiplier
        // to 1 and clipping there turns that into white instead of a colour cast.
        let smallest = balance.iter().copied().fold(f32::INFINITY, f32::min);
        let mut srgb_to_camera = multiply(&color.xyz_to_camera, &SRGB_TO_XYZ);
        for row in &mut srgb_to_camera {
            let sum: f32 = row.iter().sum();
            if sum.abs() < f32::EPSILON {
                return None;
            }
            row.iter_mut().for_each(|value| *value /= sum);
        }
        Some(Self {
            gain: balance.map(|gain| gain / smallest),
            camera_to_srgb: invert(&srgb_to_camera)?,
        })
    }

    fn apply(&self, camera: [f32; 3]) -> [f32; 3] {
        let balanced =
            [0, 1, 2].map(|channel| (camera[channel] * self.gain[channel]).clamp(0.0, 1.0));
        self.camera_to_srgb.map(|row| {
            (row[0] * balanced[0] + row[1] * balanced[1] + row[2] * balanced[2]).clamp(0.0, 1.0)
        })
    }
}

fn multiply(a: &Matrix, b: &Matrix) -> Matrix {
    let mut product = [[0.0; 3]; 3];
    for (row, out) in product.iter_mut().enumerate() {
        for (column, value) in out.iter_mut().enumerate() {
            *value = (0..3).map(|k| a[row][k] * b[k][column]).sum();
        }
    }
    product
}

fn invert(m: &Matrix) -> Option<Matrix> {
    let cofactor = |row: usize, column: usize| {
        let (r1, r2) = ((row + 1) % 3, (row + 2) % 3);
        let (c1, c2) = ((column + 1) % 3, (column + 2) % 3);
        m[r1][c1] * m[r2][c2] - m[r1][c2] * m[r2][c1]
    };
    let determinant: f32 = (0..3)
        .map(|column| m[0][column] * cofactor(0, column))
        .sum();
    if determinant.abs() < 1e-9 {
        return None;
    }
    let mut inverse = [[0.0; 3]; 3];
    for (row, out) in inverse.iter_mut().enumerate() {
        for (column, value) in out.iter_mut().enumerate() {
            *value = cofactor(column, row) / determinant;
        }
    }
    Some(inverse)
}

fn curve_index(linear: f32) -> usize {
    (linear * (CURVE_SIZE - 1) as f32 + 0.5) as usize
}

/// The standard sRGB encoding of linear light.
fn srgb_curve() -> Curve {
    (0..CURVE_SIZE)
        .map(|index| {
            let linear = index as f32 / (CURVE_SIZE - 1) as f32;
            let encoded = if linear <= 0.003_130_8 {
                12.92 * linear
            } else {
                1.055 * linear.powf(1.0 / 2.4) - 0.055
            };
            (encoded * 255.0).round() as u8
        })
        .collect()
}

/// The curve that gives pixels counted in `developed` (one count per curve entry) the
/// distribution of display levels counted in `reference`.
fn matching_curve(developed: &[u32], reference: &[u32; 256]) -> Curve {
    let developed_total: u64 = developed.iter().map(|&count| u64::from(count)).sum();
    let reference_total: u64 = reference.iter().map(|&count| u64::from(count)).sum();
    // Share of reference pixels below each level.
    let mut below_level = [0.0f64; 257];
    for (level, &count) in reference.iter().enumerate() {
        below_level[level + 1] = below_level[level] + f64::from(count) / reference_total as f64;
    }
    let mut level = 0;
    let mut below = 0u64;
    developed
        .iter()
        .map(|&count| {
            // The middle of this entry's pixels, as a share of all pixels.
            let share = (below as f64 + f64::from(count) / 2.0) / developed_total as f64;
            below += u64::from(count);
            while level < 255 && below_level[level + 1] < share {
                level += 1;
            }
            let span = below_level[level + 1] - below_level[level];
            let within = if span > 0.0 {
                ((share - below_level[level]) / span).clamp(0.0, 1.0)
            } else {
                0.5
            };
            (level as f64 + within - 0.5).round().clamp(0.0, 255.0) as u8
        })
        .collect()
}

/// Counts of each display level per channel in the picture area of `reference`.
fn reference_histograms(reference: &Pixels) -> [[u32; 256]; 3] {
    let content = ContentRect::without_bars(reference);
    let [width, height] = reference.size.map(|side| side as usize);
    let span = |min: f32, max: f32, side: usize| {
        let first = (min * side as f32).round() as usize;
        let last = (max * side as f32).round() as usize;
        first.min(side)..last.min(side)
    };
    let columns = span(content.min[0], content.max[0], width);
    let channels = reference.channels.count();
    let mut histograms = [[0u32; 256]; 3];
    for y in span(content.min[1], content.max[1], height) {
        let row = &reference.data[y * width * channels..(y + 1) * width * channels];
        for pixel in row[columns.start * channels..columns.end * channels].chunks_exact(channels) {
            for (histogram, &level) in histograms.iter_mut().zip(pixel) {
                histogram[usize::from(level)] += 1;
            }
        }
    }
    histograms
}

/// The centred part of a `width` x `height` image that has the aspect ratio `aspect`,
/// as column and row ranges.
fn centred_region(
    width: usize,
    height: usize,
    aspect: f32,
) -> (std::ops::Range<usize>, std::ops::Range<usize>) {
    let own = width as f32 / height as f32;
    if aspect > own * ASPECT_TOLERANCE {
        let rows = ((width as f32 / aspect).round() as usize).clamp(1, height);
        let top = (height - rows) / 2;
        (0..width, top..top + rows)
    } else if aspect < own / ASPECT_TOLERANCE {
        let columns = ((height as f32 * aspect).round() as usize).clamp(1, width);
        let left = (width - columns) / 2;
        (left..left + columns, 0..height)
    } else {
        (0..width, 0..height)
    }
}

/// Tone curves that make the developed image look like `reference`, or `None` when the
/// reference is too small to tell.
fn matched_curves(
    camera: &[[f32; 3]],
    width: usize,
    height: usize,
    transform: &Transform,
    reference: &Pixels,
) -> Option<[Curve; 3]> {
    let wanted = reference_histograms(reference);
    let measured: u64 = wanted[0].iter().map(|&count| u64::from(count)).sum();
    if measured < MIN_REFERENCE_PIXELS {
        return None;
    }
    let content = ContentRect::without_bars(reference).size();
    let aspect = content[0] * reference.size[0] as f32 / (content[1] * reference.size[1] as f32);
    let (columns, rows) = centred_region(width, height, aspect);
    let mut developed = [
        vec![0u32; CURVE_SIZE],
        vec![0u32; CURVE_SIZE],
        vec![0u32; CURVE_SIZE],
    ];
    for y in rows.step_by(SAMPLE_STEP) {
        for x in columns.clone().step_by(SAMPLE_STEP) {
            let linear = transform.apply(camera[y * width + x]);
            for (histogram, value) in developed.iter_mut().zip(linear) {
                histogram[curve_index(value)] += 1;
            }
        }
    }
    Some([0, 1, 2].map(|channel| matching_curve(&developed[channel], &wanted[channel])))
}

/// Renders `camera`, the demosaiced sensor channels of a `width` x `height` image scaled to
/// 0..=1, as RGBA display pixels. `reference` is the camera's own JPEG of the picture.
/// Returns `None` when the colour description is unusable.
pub fn render(
    camera: &[[f32; 3]],
    width: usize,
    height: usize,
    color: &CameraColor,
    reference: Option<&Pixels>,
) -> Option<Pixels> {
    if width == 0 || height == 0 || camera.len() != width * height {
        return None;
    }
    let transform = Transform::new(color)?;
    let curves = reference
        .and_then(|reference| matched_curves(camera, width, height, &transform, reference))
        .unwrap_or_else(|| {
            let curve = srgb_curve();
            [curve.clone(), curve.clone(), curve]
        });

    let mut data = vec![0u8; width * height * 4];
    let threads = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
    let rows_per_thread = height.div_ceil(threads);
    std::thread::scope(|scope| {
        let bands = data
            .chunks_mut(rows_per_thread * width * 4)
            .zip(camera.chunks(rows_per_thread * width));
        for (out, band) in bands {
            let (transform, curves) = (&transform, &curves);
            scope.spawn(move || {
                for (pixel, &sensor) in out.as_chunks_mut::<4>().0.iter_mut().zip(band) {
                    let linear = transform.apply(sensor);
                    *pixel = [
                        curves[0][curve_index(linear[0])],
                        curves[1][curve_index(linear[1])],
                        curves[2][curve_index(linear[2])],
                        255,
                    ];
                }
            });
        }
    });
    Some(Pixels {
        size: [width as u32, height as u32],
        channels: Channels::Rgba,
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A camera whose channels are linear sRGB.
    const XYZ_TO_SRGB: Matrix = [
        [3.240_454_2, -1.537_138_5, -0.498_531_4],
        [-0.969_266, 1.876_010_8, 0.041_556_0],
        [0.055_643_4, -0.204_025_9, 1.057_225_2],
    ];

    fn srgb_camera(white_balance: [f32; 3]) -> CameraColor {
        CameraColor {
            white_balance,
            xyz_to_camera: XYZ_TO_SRGB,
        }
    }

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3)
    }

    #[test]
    fn a_saturated_sensor_becomes_white_not_coloured() {
        let transform = Transform::new(&srgb_camera([2.0, 1.0, 1.5])).expect("transform");
        assert!(close(transform.apply([1.0, 1.0, 1.0]), [1.0, 1.0, 1.0]));
        assert!(close(transform.apply([0.9, 1.0, 0.8]), [1.0, 1.0, 1.0]));
    }

    #[test]
    fn white_balance_makes_grey_neutral() {
        let transform = Transform::new(&srgb_camera([2.0, 1.0, 1.5])).expect("transform");
        assert!(close(
            transform.apply([0.1, 0.2, 0.2 / 1.5]),
            [0.2, 0.2, 0.2]
        ));
        assert!(close(transform.apply([0.0, 0.0, 0.0]), [0.0, 0.0, 0.0]));
    }

    #[test]
    fn unusable_colour_descriptions_are_refused() {
        assert!(Transform::new(&srgb_camera([f32::NAN, 1.0, 1.0])).is_none());
        assert!(Transform::new(&srgb_camera([0.0, 1.0, 1.0])).is_none());
        let flat = CameraColor {
            white_balance: [1.0; 3],
            xyz_to_camera: [[1.0, 1.0, 1.0]; 3],
        };
        assert!(Transform::new(&flat).is_none());
    }

    #[test]
    fn inverse_times_matrix_is_identity() {
        let inverse = invert(&SRGB_TO_XYZ).expect("inverse");
        let identity = multiply(&inverse, &SRGB_TO_XYZ);
        for (row, values) in identity.iter().enumerate() {
            for (column, value) in values.iter().enumerate() {
                let expected = if row == column { 1.0 } else { 0.0 };
                assert!((value - expected).abs() < 1e-4);
            }
        }
    }

    #[test]
    fn without_a_reference_the_srgb_encoding_is_used() {
        let camera = [[0.0; 3], [0.5; 3], [1.0; 3], [0.214; 3]];
        let pixels = render(&camera, 2, 2, &srgb_camera([1.0; 3]), None).expect("render");
        assert_eq!(pixels.size, [2, 2]);
        let greys: Vec<u8> = pixels
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .map(|pixel| pixel[1])
            .collect();
        assert_eq!(greys, [0, 188, 255, 127]);
        assert!(
            pixels
                .data
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[3] == 255)
        );
    }

    #[test]
    fn matching_curve_reproduces_the_reference_distribution() {
        // Developed pixels spread evenly over the lower half of linear light.
        let mut developed = vec![0u32; CURVE_SIZE];
        developed[..CURVE_SIZE / 2].fill(10);
        // Reference pixels spread evenly over levels 100..200.
        let mut reference = [0u32; 256];
        reference[100..200].fill(7);
        let curve = matching_curve(&developed, &reference);
        assert_eq!(curve[0], 100);
        assert_eq!(curve[CURVE_SIZE / 4], 150);
        assert_eq!(curve[CURVE_SIZE / 2 - 1], 199);
        assert!(curve.windows(2).all(|pair| pair[0] <= pair[1]));
    }

    fn grey_reference(width: u32, height: u32, level: impl Fn(u32, u32) -> u8) -> Pixels {
        let mut data = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let grey = level(x, y);
                data.extend_from_slice(&[grey, grey, grey, 255]);
            }
        }
        Pixels {
            size: [width, height],
            channels: Channels::Rgba,
            data,
        }
    }

    #[test]
    fn tones_follow_the_camera_jpeg() {
        // A dark linear ramp, and a camera JPEG that shows the same ramp from 60 to 220.
        let (width, height) = (600usize, 400usize);
        let camera: Vec<[f32; 3]> = (0..height)
            .flat_map(|_| (0..width).map(|x| [0.25 * x as f32 / (width - 1) as f32; 3]))
            .collect();
        let reference = grey_reference(600, 400, |x, _| (60 + x * 160 / 599) as u8);
        let pixels = render(
            &camera,
            width,
            height,
            &srgb_camera([1.0; 3]),
            Some(&reference),
        )
        .expect("render");
        let level = |x: usize| i32::from(pixels.data[x * 4]);
        assert!((level(0) - 60).abs() <= 2, "left edge {}", level(0));
        assert!(
            (level(width / 2) - 140).abs() <= 2,
            "middle {}",
            level(width / 2)
        );
        assert!(
            (level(width - 1) - 220).abs() <= 2,
            "right edge {}",
            level(width - 1)
        );
    }

    #[test]
    fn a_tiny_reference_is_ignored() {
        let camera = vec![[0.5; 3]; 16];
        let reference = grey_reference(160, 120, |_, _| 10);
        let pixels =
            render(&camera, 4, 4, &srgb_camera([1.0; 3]), Some(&reference)).expect("render");
        assert_eq!(pixels.data[0], 188);
    }

    #[test]
    fn letterbox_bars_and_cropped_previews_are_left_out_of_the_measurement() {
        let reference = grey_reference(
            1600,
            1080,
            |_, y| if (90..990).contains(&y) { 128 } else { 0 },
        );
        let histograms = reference_histograms(&reference);
        assert_eq!(histograms[0][0], 0);
        assert!(histograms[0][128] > 1_000_000);

        // A 16:9 preview of a 3:2 image covers the middle rows.
        assert_eq!(centred_region(4240, 2832, 16.0 / 9.0), (0..4240, 223..2608));
        assert_eq!(centred_region(4240, 2832, 1.0), (704..3536, 0..2832));
        assert_eq!(
            centred_region(4240, 2832, 4240.0 / 2832.0),
            (0..4240, 0..2832)
        );
    }
}

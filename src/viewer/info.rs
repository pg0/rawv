//! Text for the viewer's info line.

use crate::media::load::{self, Source};
use crate::media::probe::ShotInfo;

pub fn file_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let bytes = bytes as f64;
    if bytes >= KB * KB * KB {
        format!("{:.2} GB", bytes / (KB * KB * KB))
    } else if bytes >= KB * KB {
        format!("{:.1} MB", bytes / (KB * KB))
    } else {
        format!("{:.0} KB", (bytes / KB).max(1.0))
    }
}

pub fn exposure_time(seconds: f64) -> String {
    if seconds >= 1.0 {
        format!("{}s", trim_number(seconds))
    } else if seconds > 0.0 {
        format!("1/{}s", (1.0 / seconds).round())
    } else {
        String::new()
    }
}

/// Describes where a RAW file's picture came from. `size` is the picture's pixel size.
pub fn source_label(source: Source, size: [u32; 2]) -> Option<&'static str> {
    match source {
        Source::RawPreview if load::is_small_preview(size) => {
            Some("RAW small camera preview, P for full size")
        }
        Source::RawPreview => Some("RAW camera preview"),
        Source::RawDeveloped => Some("RAW developed"),
        Source::Jpeg | Source::Bitmap | Source::Heic => None,
    }
}

/// Camera, lens and exposure pieces, in reading order. Missing data is left out.
pub fn shot_pieces(shot: &ShotInfo) -> Vec<String> {
    let mut pieces = Vec::new();
    let camera = match (&shot.make, &shot.model) {
        (Some(make), Some(model)) if model.to_lowercase().starts_with(&make.to_lowercase()) => {
            Some(model.clone())
        }
        (Some(make), Some(model)) => Some(format!("{make} {model}")),
        (None, Some(model)) => Some(model.clone()),
        (Some(make), None) => Some(make.clone()),
        (None, None) => None,
    };
    pieces.extend(camera);
    pieces.extend(shot.lens.clone());
    pieces.extend(shot.f_number.map(|f| format!("f/{}", trim_number(f))));
    pieces.extend(
        shot.exposure_time
            .map(exposure_time)
            .filter(|text| !text.is_empty()),
    );
    pieces.extend(shot.iso.map(|iso| format!("ISO {iso}")));
    pieces.extend(shot.focal_length.map(|mm| format!("{}mm", trim_number(mm))));
    pieces.extend(shot.date_time.as_deref().map(date_time));
    pieces
}

/// `2025:08:15 11:33:07` becomes `2025-08-15 11:33`. Other text is passed through.
fn date_time(exif: &str) -> String {
    let bytes = exif.as_bytes();
    if exif.is_ascii() && bytes.len() >= 16 && bytes[4] == b':' && bytes[7] == b':' {
        format!(
            "{}-{}-{} {}",
            &exif[0..4],
            &exif[5..7],
            &exif[8..10],
            &exif[11..16]
        )
    } else {
        exif.to_owned()
    }
}

fn trim_number(value: f64) -> String {
    let text = format!("{value:.1}");
    text.strip_suffix(".0").map_or(text.clone(), str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_sizes_and_exposures() {
        assert_eq!(file_size(500), "1 KB");
        assert_eq!(file_size(452_192), "442 KB");
        assert_eq!(file_size(15_507_456), "14.8 MB");
        assert_eq!(exposure_time(0.008), "1/125s");
        assert_eq!(exposure_time(2.0), "2s");
        assert_eq!(exposure_time(2.5), "2.5s");
        assert_eq!(exposure_time(0.0), "");
    }

    #[test]
    fn small_raw_previews_point_to_the_full_decode() {
        let small = source_label(Source::RawPreview, [1920, 1080]).expect("label");
        assert!(small.contains("P for full size"));
        assert_eq!(
            source_label(Source::RawPreview, [4240, 2384]),
            Some("RAW camera preview")
        );
        assert_eq!(
            source_label(Source::RawDeveloped, [1920, 1080]),
            Some("RAW developed")
        );
        assert_eq!(source_label(Source::Jpeg, [100, 100]), None);
    }

    #[test]
    fn shot_pieces_skip_missing_data_and_duplicate_make() {
        let shot = ShotInfo {
            make: Some("SONY".into()),
            model: Some("ZV-E1".into()),
            lens: Some("24-70mm F2.8".into()),
            exposure_time: Some(0.008),
            f_number: Some(2.8),
            focal_length: Some(24.0),
            iso: Some(100),
            date_time: Some("2025:08:15 11:33:07".into()),
        };
        assert_eq!(
            shot_pieces(&shot),
            [
                "SONY ZV-E1",
                "24-70mm F2.8",
                "f/2.8",
                "1/125s",
                "ISO 100",
                "24mm",
                "2025-08-15 11:33"
            ]
        );
        let canon = ShotInfo {
            make: Some("Canon".into()),
            model: Some("Canon EOS R5".into()),
            ..ShotInfo::default()
        };
        assert_eq!(shot_pieces(&canon), ["Canon EOS R5"]);
        assert!(shot_pieces(&ShotInfo::default()).is_empty());
    }
}

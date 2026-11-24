//! Lossless rotation: rewrites the two-byte EXIF orientation value in place.
//! Pixel data is never touched and the file's modification time is kept.

use std::fs::OpenOptions;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

use super::formats::Kind;
use super::orientation::Orientation;
use super::probe::MediaFile;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RotateOutcome {
    /// The new orientation was written to the file.
    Saved,
    /// The file has no EXIF orientation value to rewrite; the rotation applies to this session only.
    ViewOnly,
}

pub fn persist(path: &Path, kind: Kind, orientation: Orientation) -> io::Result<RotateOutcome> {
    let Some(slot) = MediaFile::open(path)?.probe(kind).slot else {
        return Ok(RotateOutcome::ViewOnly);
    };
    let modified = std::fs::metadata(path)?.modified()?;
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;

    let mut current = [0u8; 2];
    file.seek(SeekFrom::Start(slot.pos))?;
    file.read_exact(&mut current)?;
    let current_value = if slot.big_endian {
        u16::from_be_bytes(current)
    } else {
        u16::from_le_bytes(current)
    };
    if !Orientation::is_valid_exif(current_value) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "orientation value changed on disk",
        ));
    }

    let value = orientation.to_exif();
    let bytes = if slot.big_endian {
        value.to_be_bytes()
    } else {
        value.to_le_bytes()
    };
    file.seek(SeekFrom::Start(slot.pos))?;
    file.write_all(&bytes)?;
    file.flush()?;
    file.set_modified(modified)?;
    Ok(RotateOutcome::Saved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::probe::test_support::jpeg_with_exif;

    fn temp_file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("rawv-test-{}-{name}", std::process::id()));
        std::fs::write(&path, bytes).expect("write temp file");
        path
    }

    #[test]
    fn rewrites_only_the_orientation_bytes_and_keeps_mtime() {
        for big_endian in [false, true] {
            let original = jpeg_with_exif(1, big_endian, &[0x55u8; 600]);
            let path = temp_file(&format!("rotate-{big_endian}.jpg"), &original);
            let mtime_before = std::fs::metadata(&path)
                .expect("meta")
                .modified()
                .expect("mtime");

            let target = Orientation::NORMAL.rotated_cw(1);
            let outcome = persist(&path, Kind::Jpeg, target).expect("persist");
            assert_eq!(outcome, RotateOutcome::Saved);

            let written = std::fs::read(&path).expect("read back");
            let probe = crate::media::probe::probe_jpeg(&written);
            assert_eq!(probe.orientation, target);
            let differing = original
                .iter()
                .zip(&written)
                .filter(|(a, b)| a != b)
                .count();
            assert_eq!(written.len(), original.len());
            assert_eq!(differing, 1);
            let mtime_after = std::fs::metadata(&path)
                .expect("meta")
                .modified()
                .expect("mtime");
            assert_eq!(mtime_before, mtime_after);
            std::fs::remove_file(&path).expect("cleanup");
        }
    }

    #[test]
    fn file_without_orientation_is_view_only_and_untouched() {
        let original = vec![0xFF, 0xD8, 0xFF, 0xD9];
        let path = temp_file("rotate-none.jpg", &original);
        let outcome =
            persist(&path, Kind::Jpeg, Orientation::NORMAL.rotated_cw(2)).expect("persist");
        assert_eq!(outcome, RotateOutcome::ViewOnly);
        assert_eq!(std::fs::read(&path).expect("read back"), original);
        std::fs::remove_file(&path).expect("cleanup");
    }
}

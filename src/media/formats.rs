//! File type classification by extension, with a look at the content where names lie.

use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Kind {
    Jpeg,
    Raw,
    /// Formats decoded by the `image` crate.
    Bitmap,
}

const RAW_EXTENSIONS: &[&str] = &[
    "arw", "cr2", "cr3", "nef", "nrw", "raf", "rw2", "orf", "dng", "pef", "srw", "sr2", "srf",
    "x3f", "3fr", "kdc", "dcr", "mrw", "erf", "mef", "mos", "iiq", "crw",
];

const BITMAP_EXTENSIONS: &[&str] = &["png", "webp", "gif", "bmp", "tif", "tiff"];

/// Whether `head`, the first bytes of a file, begins a JPEG. Files named `.jpg` sometimes
/// hold a PNG or WebP; those are decoded like the other bitmap formats.
pub fn is_jpeg(head: &[u8]) -> bool {
    head.starts_with(&[0xFF, 0xD8])
}

pub fn kind_of(path: &Path) -> Option<Kind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" | "jpe" | "jfif" => Some(Kind::Jpeg),
        e if RAW_EXTENSIONS.contains(&e) => Some(Kind::Raw),
        e if BITMAP_EXTENSIONS.contains(&e) => Some(Kind::Bitmap),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_case_insensitively() {
        assert_eq!(kind_of(Path::new("a/DSC0001.ARW")), Some(Kind::Raw));
        assert_eq!(kind_of(Path::new("a/photo.JpEg")), Some(Kind::Jpeg));
        assert_eq!(kind_of(Path::new("a/scan.tiff")), Some(Kind::Bitmap));
        assert_eq!(kind_of(Path::new("a/notes.txt")), None);
        assert_eq!(kind_of(Path::new("a/noext")), None);
    }
}

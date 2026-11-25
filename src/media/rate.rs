//! Writing a rating into a photo file, as XMP metadata other programs read too.
//!
//! Picture data is never touched and the file's times are kept. Three cases, tried in
//! this order:
//!
//! 1. The file has an XMP packet with room: the packet is changed in place.
//! 2. A JPEG without such a packet gets one as a new segment. The file is written anew
//!    next to the original and then takes its place.
//! 3. A TIFF-based RAW file without such a packet gets one appended, together with a copy
//!    of its first directory that points to it. Only the four bytes in the header that
//!    say where the first directory starts change in the existing data. This is done for
//!    Sony RAW and DNG files only.

use std::fs::{File, FileTimes, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

use super::formats::{self, Kind};
use super::xmp::{self, Rating};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RateOutcome {
    /// The rating is in the file.
    Saved,
    /// This kind of file cannot hold a rating. The file is untouched.
    Unsupported,
}

const XMP_SEGMENT_HEADER: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
const EXIF_SEGMENT_HEADER: &[u8] = b"Exif\0\0";
const MAX_SEGMENT_BODY: usize = 0xFFFF - 2;
const TAG_MAKE: u16 = 0x010F;
const TAG_XMP: u16 = 0x02BC;
const TAG_DNG_VERSION: u16 = 0xC612;
const TYPE_BYTE: u16 = 1;
const TIFF_MAGIC: u16 = 42;
const MAX_IFD_ENTRIES: usize = 1024;
/// XMP packets larger than this are not expected in photo files.
const MAX_PACKET: u32 = 4 * 1024 * 1024;

pub fn persist(path: &Path, kind: Kind, rating: Rating) -> io::Result<RateOutcome> {
    match kind {
        Kind::Jpeg => rate_jpeg(path, rating),
        Kind::Raw => rate_tiff(path, rating),
        Kind::Bitmap => Ok(RateOutcome::Unsupported),
    }
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Overwrites `bytes` at `offset` and puts the modification time back.
fn write_in_place(path: &Path, offset: u64, bytes: &[u8]) -> io::Result<()> {
    let modified = std::fs::metadata(path)?.modified()?;
    let mut file = OpenOptions::new().write(true).open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    file.write_all(bytes)?;
    file.sync_all()?;
    file.set_modified(modified)
}

// ---------------------------------------------------------------------------------- JPEG

struct JpegLayout {
    /// The XMP packet's position and length, when the file has one.
    packet: Option<(usize, usize)>,
    /// Where a new XMP segment goes: behind the JFIF and EXIF segments.
    insert_at: usize,
}

fn jpeg_layout(data: &[u8]) -> io::Result<JpegLayout> {
    if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
        return Err(invalid("not a JPEG file"));
    }
    let mut layout = JpegLayout {
        packet: None,
        insert_at: 2,
    };
    let mut pos = 2;
    while pos + 4 <= data.len() && data[pos] == 0xFF {
        let marker = data[pos + 1];
        if !(0xE0..=0xEF).contains(&marker) {
            break;
        }
        let length = usize::from(u16::from_be_bytes([data[pos + 2], data[pos + 3]]));
        let (body, end) = (pos + 4, pos + 2 + length);
        if length < 2 || end > data.len() {
            return Err(invalid("JPEG segment runs past the end of the file"));
        }
        let content = &data[body..end];
        if marker == 0xE1 && content.starts_with(XMP_SEGMENT_HEADER) && layout.packet.is_none() {
            let start = body + XMP_SEGMENT_HEADER.len();
            layout.packet = Some((start, end - start));
        }
        if marker == 0xE0 || (marker == 0xE1 && content.starts_with(EXIF_SEGMENT_HEADER)) {
            layout.insert_at = end;
        }
        pos = end;
    }
    Ok(layout)
}

fn xmp_segment(packet: &[u8]) -> io::Result<Vec<u8>> {
    let body = XMP_SEGMENT_HEADER.len() + packet.len();
    if body > MAX_SEGMENT_BODY {
        return Err(invalid("XMP packet too large for a JPEG segment"));
    }
    let mut segment = vec![0xFF, 0xE1];
    segment.extend_from_slice(&((body + 2) as u16).to_be_bytes());
    segment.extend_from_slice(XMP_SEGMENT_HEADER);
    segment.extend_from_slice(packet);
    Ok(segment)
}

fn rate_jpeg(path: &Path, rating: Rating) -> io::Result<RateOutcome> {
    let data = std::fs::read(path)?;
    if !formats::is_jpeg(&data) {
        return Ok(RateOutcome::Unsupported);
    }
    let layout = jpeg_layout(&data)?;
    let rebuilt = match layout.packet {
        Some((start, length)) => {
            let packet = &data[start..start + length];
            let Some(changed) = xmp::with_rating(packet, rating) else {
                return Ok(RateOutcome::Unsupported);
            };
            if let Some(same_length) = xmp::fitted(changed.clone(), length) {
                write_in_place(path, start as u64, &same_length)?;
                return Ok(RateOutcome::Saved);
            }
            // No blanks left to even out the change: the segment is written with its new length.
            let segment_start = start - XMP_SEGMENT_HEADER.len() - 4;
            let mut rebuilt = data[..segment_start].to_vec();
            rebuilt.extend_from_slice(&xmp_segment(&changed)?);
            rebuilt.extend_from_slice(&data[start + length..]);
            rebuilt
        }
        None => {
            let mut rebuilt = data[..layout.insert_at].to_vec();
            rebuilt.extend_from_slice(&xmp_segment(&xmp::new_packet(rating))?);
            rebuilt.extend_from_slice(&data[layout.insert_at..]);
            rebuilt
        }
    };
    replace_file(path, &rebuilt)?;
    Ok(RateOutcome::Saved)
}

/// Puts `bytes` in the place of the file at `path`, which keeps its times. The new file is
/// complete on disk before the old one goes.
fn replace_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let metadata = std::fs::metadata(path)?;
    let mut times = FileTimes::new().set_modified(metadata.modified()?);
    if let Ok(accessed) = metadata.accessed() {
        times = times.set_accessed(accessed);
    }
    #[cfg(windows)]
    if let Ok(created) = metadata.created() {
        use std::os::windows::fs::FileTimesExt;
        times = times.set_created(created);
    }
    let mut name = path
        .file_name()
        .ok_or_else(|| invalid("path without a file name"))?
        .to_os_string();
    name.push(".rawv-part");
    let part = path.with_file_name(name);
    let written = (|| {
        let mut file = File::create(&part)?;
        file.write_all(bytes)?;
        file.set_times(times)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&part, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    written
}

// ---------------------------------------------------------------------------------- TIFF

struct Tiff {
    file: File,
    big_endian: bool,
    magic: u16,
}

/// The first directory of a TIFF file, entries kept as their twelve raw bytes each.
struct Directory {
    entries: Vec<[u8; 12]>,
    next: [u8; 4],
}

impl Tiff {
    fn open(path: &Path) -> io::Result<(Self, u32)> {
        let mut file = OpenOptions::new().read(true).write(true).open(path)?;
        let mut header = [0u8; 8];
        file.read_exact(&mut header)?;
        let big_endian = match &header[0..2] {
            b"II" => false,
            b"MM" => true,
            _ => return Err(invalid("not a TIFF-based file")),
        };
        let mut tiff = Self {
            file,
            big_endian,
            magic: 0,
        };
        tiff.magic = tiff.u16_from([header[2], header[3]]);
        let first = tiff.u32_from([header[4], header[5], header[6], header[7]]);
        Ok((tiff, first))
    }

    fn u16_from(&self, bytes: [u8; 2]) -> u16 {
        if self.big_endian {
            u16::from_be_bytes(bytes)
        } else {
            u16::from_le_bytes(bytes)
        }
    }

    fn u32_from(&self, bytes: [u8; 4]) -> u32 {
        if self.big_endian {
            u32::from_be_bytes(bytes)
        } else {
            u32::from_le_bytes(bytes)
        }
    }

    fn u16_bytes(&self, value: u16) -> [u8; 2] {
        if self.big_endian {
            value.to_be_bytes()
        } else {
            value.to_le_bytes()
        }
    }

    fn u32_bytes(&self, value: u32) -> [u8; 4] {
        if self.big_endian {
            value.to_be_bytes()
        } else {
            value.to_le_bytes()
        }
    }

    fn read_at(&mut self, offset: u64, length: usize) -> io::Result<Vec<u8>> {
        self.file.seek(SeekFrom::Start(offset))?;
        let mut bytes = vec![0u8; length];
        self.file.read_exact(&mut bytes)?;
        Ok(bytes)
    }

    fn directory(&mut self, offset: u32) -> io::Result<Directory> {
        let count = self.read_at(u64::from(offset), 2)?;
        let count = usize::from(self.u16_from([count[0], count[1]]));
        if count == 0 || count > MAX_IFD_ENTRIES {
            return Err(invalid("implausible TIFF directory"));
        }
        let raw = self.read_at(u64::from(offset) + 2, count * 12 + 4)?;
        let (entries, next) = raw.split_at(count * 12);
        Ok(Directory {
            entries: entries.as_chunks::<12>().0.to_vec(),
            next: [next[0], next[1], next[2], next[3]],
        })
    }

    fn tag(&self, entry: &[u8; 12]) -> u16 {
        self.u16_from([entry[0], entry[1]])
    }

    fn count(&self, entry: &[u8; 12]) -> u32 {
        self.u32_from([entry[4], entry[5], entry[6], entry[7]])
    }

    fn value(&self, entry: &[u8; 12]) -> u32 {
        self.u32_from([entry[8], entry[9], entry[10], entry[11]])
    }

    /// Whether this is a kind of file known to stay readable with its first directory
    /// moved to the end: DNG, and Sony's RAW files.
    fn may_move_directory(&mut self, directory: &Directory) -> bool {
        if self.magic != TIFF_MAGIC {
            return false;
        }
        if directory
            .entries
            .iter()
            .any(|entry| self.tag(entry) == TAG_DNG_VERSION)
        {
            return true;
        }
        let Some(make) = directory
            .entries
            .iter()
            .find(|entry| self.tag(entry) == TAG_MAKE)
            .copied()
        else {
            return false;
        };
        let length = self.count(&make).min(32) as usize;
        let text = if length <= 4 {
            Ok(make[8..8 + length].to_vec())
        } else {
            self.read_at(u64::from(self.value(&make)), length)
        };
        text.is_ok_and(|text| text.to_ascii_uppercase().starts_with(b"SONY"))
    }
}

fn rate_tiff(path: &Path, rating: Rating) -> io::Result<RateOutcome> {
    let modified = std::fs::metadata(path)?.modified()?;
    let (mut tiff, first) = match Tiff::open(path) {
        Ok(opened) => opened,
        Err(error) if error.kind() == io::ErrorKind::InvalidData => {
            return Ok(RateOutcome::Unsupported);
        }
        Err(error) => return Err(error),
    };
    let directory = tiff.directory(first)?;
    let existing = directory
        .entries
        .iter()
        .position(|entry| tiff.tag(entry) == TAG_XMP);

    let packet = match existing {
        Some(index) => {
            let entry = directory.entries[index];
            let (length, offset) = (tiff.count(&entry), tiff.value(&entry));
            if length <= 4 || length > MAX_PACKET {
                return Ok(RateOutcome::Unsupported);
            }
            let packet = tiff.read_at(u64::from(offset), length as usize)?;
            let Some(changed) = xmp::with_rating(&packet, rating) else {
                return Ok(RateOutcome::Unsupported);
            };
            if let Some(same_length) = xmp::fitted(changed.clone(), packet.len()) {
                drop(tiff);
                write_in_place(path, u64::from(offset), &same_length)?;
                return Ok(RateOutcome::Saved);
            }
            // A packet without spare blanks: the changed one goes to the end of the file,
            // given room for the next change.
            xmp::fitted(changed.clone(), changed.len() + 1024).unwrap_or(changed)
        }
        None => xmp::new_packet(rating),
    };
    if !tiff.may_move_directory(&directory) {
        return Ok(RateOutcome::Unsupported);
    }

    // Appended: the packet, then the directory with an entry for it. The header is
    // pointed to the new directory only after both are safely on disk.
    let end = tiff.file.seek(SeekFrom::End(0))?;
    let packet_at = end + end % 2;
    let directory_at = packet_at + packet.len() as u64 + packet.len() as u64 % 2;
    let directory_end = directory_at + 2 + (directory.entries.len() as u64 + 1) * 12 + 4;
    let (Ok(packet_offset), Ok(directory_offset), Ok(_)) = (
        u32::try_from(packet_at),
        u32::try_from(directory_at),
        u32::try_from(directory_end),
    ) else {
        return Ok(RateOutcome::Unsupported);
    };

    let mut xmp_entry = [0u8; 12];
    xmp_entry[0..2].copy_from_slice(&tiff.u16_bytes(TAG_XMP));
    xmp_entry[2..4].copy_from_slice(&tiff.u16_bytes(TYPE_BYTE));
    xmp_entry[4..8].copy_from_slice(&tiff.u32_bytes(packet.len() as u32));
    xmp_entry[8..12].copy_from_slice(&tiff.u32_bytes(packet_offset));
    let mut entries: Vec<[u8; 12]> = directory
        .entries
        .iter()
        .copied()
        .filter(|entry| tiff.tag(entry) != TAG_XMP)
        .collect();
    let position = entries.partition_point(|entry| tiff.tag(entry) < TAG_XMP);
    entries.insert(position, xmp_entry);

    let mut appended = vec![0u8; (packet_at - end) as usize];
    appended.extend_from_slice(&packet);
    appended.resize((directory_at - end) as usize, 0);
    appended.extend_from_slice(&tiff.u16_bytes(entries.len() as u16));
    for entry in &entries {
        appended.extend_from_slice(entry);
    }
    appended.extend_from_slice(&directory.next);

    tiff.file.seek(SeekFrom::Start(end))?;
    tiff.file.write_all(&appended)?;
    tiff.file.sync_all()?;
    tiff.file.seek(SeekFrom::Start(4))?;
    tiff.file.write_all(&tiff.u32_bytes(directory_offset))?;
    tiff.file.sync_all()?;
    tiff.file.set_modified(modified)?;
    Ok(RateOutcome::Saved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::orientation::Orientation;
    use crate::media::probe::test_support::{jpeg_with_exif, tiff_with_entries};
    use crate::media::probe::{probe_jpeg, probe_tiff};
    use crate::media::xmp::REJECTED;
    use std::io::Cursor;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    const SHORT: u16 = 3;

    fn temp_file(name: &str, bytes: &[u8]) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("rawv-test-rate-{}-{name}", std::process::id()));
        std::fs::write(&path, bytes).expect("write temp file");
        let old = SystemTime::now() - Duration::from_secs(86_400);
        let file = OpenOptions::new().write(true).open(&path).expect("open");
        file.set_modified(old).expect("set time");
        path
    }

    fn modified(path: &Path) -> SystemTime {
        std::fs::metadata(path)
            .expect("metadata")
            .modified()
            .expect("mtime")
    }

    fn differing(a: &[u8], b: &[u8]) -> usize {
        a.iter().zip(b).filter(|(x, y)| x != y).count()
    }

    #[test]
    fn a_jpeg_without_xmp_gets_a_new_segment_and_nothing_else_changes() {
        let original = jpeg_with_exif(6, false, &[0x55u8; 600]);
        let path = temp_file("new.jpg", &original);
        let before = modified(&path);

        assert_eq!(
            persist(&path, Kind::Jpeg, 3).expect("persist"),
            RateOutcome::Saved
        );
        let rated = std::fs::read(&path).expect("read back");
        let layout = jpeg_layout(&original).expect("layout");
        let added = rated.len() - original.len();
        assert_eq!(&rated[..layout.insert_at], &original[..layout.insert_at]);
        assert_eq!(
            &rated[layout.insert_at + added..],
            &original[layout.insert_at..]
        );
        assert!(
            layout.insert_at > 2,
            "the segment goes behind the EXIF block"
        );
        let probe = probe_jpeg(&rated);
        assert_eq!(probe.rating, 3);
        assert_eq!(probe.orientation, Orientation::from_exif(6));
        assert_eq!(modified(&path), before);
        assert!(
            !path
                .with_file_name(format!(
                    "{}.rawv-part",
                    path.file_name().expect("name").to_string_lossy()
                ))
                .exists()
        );

        // Later changes stay inside the packet: same length, few bytes.
        assert_eq!(
            persist(&path, Kind::Jpeg, 5).expect("persist"),
            RateOutcome::Saved
        );
        let again = std::fs::read(&path).expect("read back");
        assert_eq!(again.len(), rated.len());
        assert_eq!(differing(&rated, &again), 1);
        assert_eq!(
            persist(&path, Kind::Jpeg, REJECTED).expect("persist"),
            RateOutcome::Saved
        );
        let rejected = std::fs::read(&path).expect("read back");
        assert_eq!(rejected.len(), rated.len());
        assert_eq!(probe_jpeg(&rejected).rating, REJECTED);
        assert_eq!(modified(&path), before);
        std::fs::remove_file(&path).expect("cleanup");
    }

    #[test]
    fn a_jpeg_packet_without_blanks_grows_with_its_segment() {
        let mut jpeg = vec![0xFF, 0xD8];
        jpeg.extend_from_slice(
            &xmp_segment(b"<rdf:RDF><xmp:Rating>0</xmp:Rating></rdf:RDF>").expect("segment"),
        );
        jpeg.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x04, 1, 2, 0xFF, 0xD9]);
        let path = temp_file("tight.jpg", &jpeg);
        assert_eq!(
            persist(&path, Kind::Jpeg, REJECTED).expect("persist"),
            RateOutcome::Saved
        );
        let rated = std::fs::read(&path).expect("read back");
        assert_eq!(rated.len(), jpeg.len() + 1);
        assert_eq!(probe_jpeg(&rated).rating, REJECTED);
        assert!(rated.ends_with(&[0xFF, 0xDB, 0x00, 0x04, 1, 2, 0xFF, 0xD9]));
        std::fs::remove_file(&path).expect("cleanup");
    }

    #[test]
    fn a_dng_without_xmp_gets_packet_and_directory_appended() {
        let original = tiff_with_entries(
            &[
                (0x0112, SHORT, 6),
                (0x0201, 4, 1000),
                (TAG_DNG_VERSION, 1, 1),
            ],
            6000,
        );
        let path = temp_file("new.dng", &original);
        let before = modified(&path);

        assert_eq!(
            persist(&path, Kind::Raw, 4).expect("persist"),
            RateOutcome::Saved
        );
        let rated = std::fs::read(&path).expect("read back");
        assert!(rated.len() > original.len());
        assert_eq!(&rated[..4], &original[..4]);
        assert_eq!(&rated[8..original.len()], &original[8..]);
        assert_ne!(&rated[4..8], &original[4..8]);
        let probe = probe_tiff(Cursor::new(&rated));
        assert_eq!(probe.rating, 4);
        assert_eq!(probe.orientation, Orientation::from_exif(6));
        assert_eq!(modified(&path), before);

        // Tags stay in ascending order, as TIFF asks for.
        let (mut tiff, first) = Tiff::open(&path).expect("open");
        let directory = tiff.directory(first).expect("directory");
        let tags: Vec<u16> = directory
            .entries
            .iter()
            .map(|entry| tiff.tag(entry))
            .collect();
        assert_eq!(tags, [0x0112, 0x0201, TAG_XMP, TAG_DNG_VERSION]);
        drop(tiff);

        // The next change stays inside the appended packet.
        assert_eq!(
            persist(&path, Kind::Raw, REJECTED).expect("persist"),
            RateOutcome::Saved
        );
        let rejected = std::fs::read(&path).expect("read back");
        assert_eq!(rejected.len(), rated.len());
        assert_eq!(&rejected[..original.len()], &rated[..original.len()]);
        assert_eq!(probe_tiff(Cursor::new(&rejected)).rating, REJECTED);
        assert_eq!(modified(&path), before);
        std::fs::remove_file(&path).expect("cleanup");
    }

    #[test]
    fn other_raw_files_without_xmp_are_left_alone() {
        let original = tiff_with_entries(&[(0x0112, SHORT, 1)], 3000);
        let path = temp_file("other.nef", &original);
        assert_eq!(
            persist(&path, Kind::Raw, 4).expect("persist"),
            RateOutcome::Unsupported
        );
        assert_eq!(std::fs::read(&path).expect("read back"), original);

        let not_tiff = temp_file("other.raf", b"FUJIFILMCCD-RAW 0201");
        assert_eq!(
            persist(&not_tiff, Kind::Raw, 4).expect("persist"),
            RateOutcome::Unsupported
        );
        assert_eq!(
            persist(&not_tiff, Kind::Bitmap, 4).expect("persist"),
            RateOutcome::Unsupported
        );
        std::fs::remove_file(&path).expect("cleanup");
        std::fs::remove_file(&not_tiff).expect("cleanup");
    }
}

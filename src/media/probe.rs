//! Cheap metadata probe: orientation, embedded JPEG previews, pixel size and shot data.
//!
//! A file is opened once and its head read in a single request; cameras keep their
//! directories and thumbnail there. That keeps the cost on a network share at one open
//! and one read per file.

use std::fs::File;
use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::path::Path;

use super::formats::Kind;
use super::orientation::Orientation;
use super::tiff::{self, Entry, TiffReader};

/// How much of a file is read up front.
const HEAD_BYTES: u64 = 128 * 1024;
const MAX_IFDS: usize = 12;
const MIN_EMBEDDED_BYTES: u64 = 512;
const MAX_EMBEDDED_BYTES: u64 = 128 * 1024 * 1024;
/// `PhotometricInterpretation` values of sensor data, which is not a viewable JPEG even
/// when stored with JPEG compression: colour filter array and linear raw.
const SENSOR_PHOTOMETRIC: [u32; 2] = [32803, 34892];

/// An open file whose head is already in memory.
pub struct MediaFile {
    file: File,
    head: Vec<u8>,
}

impl MediaFile {
    pub fn open(path: &Path) -> io::Result<Self> {
        let mut file = File::open(path)?;
        let mut head = Vec::with_capacity(HEAD_BYTES as usize);
        file.by_ref().take(HEAD_BYTES).read_to_end(&mut head)?;
        Ok(Self { file, head })
    }

    pub fn head(&self) -> &[u8] {
        &self.head
    }

    /// True when the head is the entire file.
    pub fn is_complete(&self) -> bool {
        (self.head.len() as u64) < HEAD_BYTES
    }

    /// Reads `len` bytes at `offset`, from memory when the head covers them.
    pub fn read_range(&mut self, offset: u64, len: u64) -> io::Result<Vec<u8>> {
        let end = offset.saturating_add(len);
        if end <= self.head.len() as u64 {
            return Ok(self.head[offset as usize..end as usize].to_vec());
        }
        self.file.seek(SeekFrom::Start(offset))?;
        let mut data = Vec::new();
        self.file.by_ref().take(len).read_to_end(&mut data)?;
        if data.len() as u64 != len {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "embedded data ends beyond the file",
            ));
        }
        Ok(data)
    }

    /// Checks the first bytes of `blob` for a viewable JPEG, without reading all of it.
    /// Lossless-JPEG sensor data starts with a Huffman table instead of an application
    /// or quantization segment.
    pub fn holds_preview(&mut self, blob: Embedded) -> bool {
        matches!(
            self.read_range(blob.offset, 4).as_deref(),
            Ok([0xFF, 0xD8, 0xFF, 0xE0..=0xEF | 0xDB | 0xFE])
        )
    }

    /// Reads the embedded blob `blob`.
    pub fn read_embedded(&mut self, blob: Embedded) -> io::Result<Vec<u8>> {
        self.read_range(blob.offset, blob.len)
    }

    /// The whole file: the head plus everything after it.
    pub fn read_all(mut self) -> io::Result<Vec<u8>> {
        if !self.is_complete() {
            self.file.seek(SeekFrom::Start(self.head.len() as u64))?;
            self.file.read_to_end(&mut self.head)?;
        }
        Ok(self.head)
    }

    /// Probes the file's metadata.
    pub fn probe(&mut self, kind: Kind) -> Probe {
        match kind {
            Kind::Jpeg => probe_jpeg(&self.head),
            Kind::Raw => probe_tiff(Positioned {
                source: self,
                position: 0,
            }),
            Kind::Bitmap => Probe::default(),
        }
    }
}

/// Sequential access to a [`MediaFile`], served from its head where possible.
struct Positioned<'a> {
    source: &'a mut MediaFile,
    position: u64,
}

impl Read for Positioned<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let head = &self.source.head;
        let count = if self.position < head.len() as u64 {
            let start = self.position as usize;
            let count = buf.len().min(head.len() - start);
            buf[..count].copy_from_slice(&head[start..start + count]);
            count
        } else {
            self.source.file.seek(SeekFrom::Start(self.position))?;
            self.source.file.read(buf)?
        };
        self.position += count as u64;
        Ok(count)
    }
}

impl Seek for Positioned<'_> {
    fn seek(&mut self, target: SeekFrom) -> io::Result<u64> {
        self.position = match target {
            SeekFrom::Start(position) => position,
            SeekFrom::Current(delta) => self.position.saturating_add_signed(delta),
            SeekFrom::End(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "seeking from the end is not supported",
                ));
            }
        };
        Ok(self.position)
    }
}

/// Where the orientation value lives in the file, so it can be rewritten in place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrientationSlot {
    pub pos: u64,
    pub big_endian: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Embedded {
    pub offset: u64,
    pub len: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShotInfo {
    pub make: Option<String>,
    pub model: Option<String>,
    pub lens: Option<String>,
    pub exposure_time: Option<f64>,
    pub f_number: Option<f64>,
    pub focal_length: Option<f64>,
    pub iso: Option<u32>,
    pub date_time: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Probe {
    pub orientation: Orientation,
    pub slot: Option<OrientationSlot>,
    /// Embedded JPEG blobs, smallest first.
    pub embedded: Vec<Embedded>,
    /// Stored pixel size of the main image, when the container states it.
    pub pixel_size: Option<[u32; 2]>,
    pub shot: ShotInfo,
}

/// Probes JPEG bytes. `data` may be only the head of a file.
pub fn probe_jpeg(data: &[u8]) -> Probe {
    let mut probe = Probe::default();
    if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
        return probe;
    }
    let mut pos = 2usize;
    while pos + 4 <= data.len() {
        if data[pos] != 0xFF {
            break;
        }
        let marker = data[pos + 1];
        if marker == 0xFF {
            pos += 1;
            continue;
        }
        let segment_len = usize::from(u16::from_be_bytes([data[pos + 2], data[pos + 3]]));
        let body = pos + 4;
        match marker {
            0xE1 if data.get(body..body + 6) == Some(b"Exif\0\0") => {
                let base = (body + 6) as u64;
                if let Ok((mut reader, first)) = TiffReader::open(Cursor::new(data), base) {
                    walk(&mut reader, first, &mut probe);
                }
            }
            0xC0..=0xCF if !matches!(marker, 0xC4 | 0xC8 | 0xCC) => {
                if let Some(frame) = data.get(body + 1..body + 5) {
                    let height = u32::from(u16::from_be_bytes([frame[0], frame[1]]));
                    let width = u32::from(u16::from_be_bytes([frame[2], frame[3]]));
                    if width > 0 && height > 0 {
                        probe.pixel_size = Some([width, height]);
                    }
                }
                break;
            }
            0xDA => break,
            _ => {}
        }
        pos += 2 + segment_len.max(2);
    }
    probe
}

/// Probes a TIFF-based file (most RAW formats). Unknown containers yield an empty probe.
pub fn probe_tiff<R: Read + Seek>(reader: R) -> Probe {
    let mut probe = Probe::default();
    if let Ok((mut reader, first)) = TiffReader::open(reader, 0) {
        walk(&mut reader, first, &mut probe);
    }
    probe
}

fn walk<R: Read + Seek>(reader: &mut TiffReader<R>, first: u32, probe: &mut Probe) {
    let mut pending = vec![first];
    let mut visited: Vec<u32> = Vec::new();
    let mut largest_area = 0u64;
    let stated_size = probe.pixel_size;

    while let Some(offset) = pending.pop() {
        if offset == 0 || visited.contains(&offset) || visited.len() >= MAX_IFDS {
            continue;
        }
        visited.push(offset);
        let Ok((entries, next)) = reader.read_ifd(offset) else {
            continue;
        };
        let is_first = visited.len() == 1;
        if is_first {
            read_main_ifd(reader, &entries, probe);
        }
        collect_embedded(reader, &entries, &mut probe.embedded);

        let width = tiff::find(&entries, tiff::TAG_IMAGE_WIDTH).and_then(|e| reader.scalar(e));
        let height = tiff::find(&entries, tiff::TAG_IMAGE_LENGTH).and_then(|e| reader.scalar(e));
        if let (Some(w), Some(h), None) = (width, height, stated_size) {
            let area = u64::from(w) * u64::from(h);
            if area > largest_area {
                largest_area = area;
                probe.pixel_size = Some([w, h]);
            }
        }

        if let Some(entry) = tiff::find(&entries, tiff::TAG_EXIF_IFD)
            && let Some(exif_offset) = reader.scalar(entry)
            && let Ok((exif_entries, _)) = reader.read_ifd(exif_offset)
        {
            read_exif_ifd(reader, &exif_entries, probe);
        }
        if let Some(entry) = tiff::find(&entries, tiff::TAG_SUB_IFDS)
            && let Ok(children) = reader.long_array(entry)
        {
            pending.extend(children);
        }
        pending.push(next);
    }

    probe.embedded.sort_by_key(|blob| blob.len);
    probe.embedded.dedup();
}

fn read_main_ifd<R: Read + Seek>(reader: &mut TiffReader<R>, entries: &[Entry], probe: &mut Probe) {
    if let Some(entry) = tiff::find(entries, tiff::TAG_ORIENTATION)
        && let Some(value) = reader.scalar(entry)
        && let Ok(value) = u16::try_from(value)
        && Orientation::is_valid_exif(value)
    {
        probe.orientation = Orientation::from_exif(value);
        probe.slot = Some(OrientationSlot {
            pos: entry.value_pos,
            big_endian: reader.big_endian(),
        });
    }
    probe.shot.make = ascii(reader, entries, tiff::TAG_MAKE);
    probe.shot.model = ascii(reader, entries, tiff::TAG_MODEL);
}

fn read_exif_ifd<R: Read + Seek>(reader: &mut TiffReader<R>, entries: &[Entry], probe: &mut Probe) {
    let shot = &mut probe.shot;
    shot.exposure_time = rational(reader, entries, tiff::TAG_EXPOSURE_TIME);
    shot.f_number = rational(reader, entries, tiff::TAG_F_NUMBER);
    shot.focal_length = rational(reader, entries, tiff::TAG_FOCAL_LENGTH);
    shot.iso = tiff::find(entries, tiff::TAG_ISO).and_then(|e| reader.scalar(e));
    shot.date_time = ascii(reader, entries, tiff::TAG_DATE_TIME_ORIGINAL);
    shot.lens = ascii(reader, entries, tiff::TAG_LENS_MODEL);

    let width = tiff::find(entries, tiff::TAG_PIXEL_X_DIMENSION).and_then(|e| reader.scalar(e));
    let height = tiff::find(entries, tiff::TAG_PIXEL_Y_DIMENSION).and_then(|e| reader.scalar(e));
    if let (Some(w), Some(h), None) = (width, height, probe.pixel_size)
        && w > 0
        && h > 0
    {
        probe.pixel_size = Some([w, h]);
    }
}

fn collect_embedded<R: Read + Seek>(
    reader: &mut TiffReader<R>,
    entries: &[Entry],
    out: &mut Vec<Embedded>,
) {
    let base = reader.base();
    let scalar = |tag| tiff::find(entries, tag).and_then(|e| reader.scalar(e));
    let mut candidates: Vec<(u32, u32)> = Vec::new();

    if let (Some(offset), Some(len)) =
        (scalar(tiff::TAG_JPEG_OFFSET), scalar(tiff::TAG_JPEG_LENGTH))
    {
        candidates.push((offset, len));
    }
    let sensor_data =
        scalar(tiff::TAG_PHOTOMETRIC).is_some_and(|p| SENSOR_PHOTOMETRIC.contains(&p));
    if matches!(scalar(tiff::TAG_COMPRESSION), Some(6 | 7))
        && !sensor_data
        && let (Some(offset), Some(len)) = (
            scalar(tiff::TAG_STRIP_OFFSETS),
            scalar(tiff::TAG_STRIP_BYTE_COUNTS),
        )
    {
        candidates.push((offset, len));
    }
    if let Some(entry) = tiff::find(entries, tiff::TAG_PANASONIC_JPG_FROM_RAW)
        && TiffReader::<R>::is_blob(entry)
    {
        candidates.push((reader.value_offset(entry), entry.count));
    }

    for (offset, len) in candidates {
        let blob = Embedded {
            offset: base + u64::from(offset),
            len: u64::from(len),
        };
        if (MIN_EMBEDDED_BYTES..=MAX_EMBEDDED_BYTES).contains(&blob.len) {
            out.push(blob);
        }
    }
}

fn ascii<R: Read + Seek>(
    reader: &mut TiffReader<R>,
    entries: &[Entry],
    tag: u16,
) -> Option<String> {
    let entry = tiff::find(entries, tag)?;
    reader.ascii(entry).ok().flatten()
}

fn rational<R: Read + Seek>(
    reader: &mut TiffReader<R>,
    entries: &[Entry],
    tag: u16,
) -> Option<f64> {
    let entry = tiff::find(entries, tag)?;
    reader.rational(entry).ok().flatten()
}

#[cfg(test)]
pub mod test_support {
    //! Builds small JPEG files with an EXIF block for tests.

    /// A little-endian TIFF with a single IFD of inline SHORT/LONG entries `(tag, type, value)`,
    /// padded to `len` bytes.
    pub fn tiff_with_entries(entries: &[(u16, u16, u32)], len: usize) -> Vec<u8> {
        let mut tiff = b"II\x2a\x00\x08\x00\x00\x00".to_vec();
        tiff.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for &(tag, typ, value) in entries {
            tiff.extend_from_slice(&tag.to_le_bytes());
            tiff.extend_from_slice(&typ.to_le_bytes());
            tiff.extend_from_slice(&1u32.to_le_bytes());
            tiff.extend_from_slice(&value.to_le_bytes());
        }
        tiff.extend_from_slice(&0u32.to_le_bytes());
        tiff.resize(len, 0);
        tiff
    }

    /// A JPEG wrapper around `scan` (any bytes) with an EXIF block holding the given
    /// orientation and a fake embedded thumbnail blob. Not decodable, but structurally valid.
    pub fn jpeg_with_exif(orientation: u16, big_endian: bool, thumbnail: &[u8]) -> Vec<u8> {
        let u16b = |v: u16| {
            if big_endian {
                v.to_be_bytes()
            } else {
                v.to_le_bytes()
            }
        };
        let u32b = |v: u32| {
            if big_endian {
                v.to_be_bytes()
            } else {
                v.to_le_bytes()
            }
        };
        let mut tiff = Vec::new();
        tiff.extend_from_slice(if big_endian { b"MM" } else { b"II" });
        tiff.extend_from_slice(&u16b(42));
        tiff.extend_from_slice(&u32b(8));
        // IFD0: Make (ASCII, out of line), Orientation.
        let ifd0_entries = 2u16;
        let ifd0_len = 2 + u32::from(ifd0_entries) * 12 + 4;
        let make = b"TestCam\0";
        let make_offset = 8 + ifd0_len;
        let ifd1_offset = make_offset + make.len() as u32;
        tiff.extend_from_slice(&u16b(ifd0_entries));
        tiff.extend_from_slice(&u16b(0x010F));
        tiff.extend_from_slice(&u16b(2));
        tiff.extend_from_slice(&u32b(make.len() as u32));
        tiff.extend_from_slice(&u32b(make_offset));
        tiff.extend_from_slice(&u16b(0x0112));
        tiff.extend_from_slice(&u16b(3));
        tiff.extend_from_slice(&u32b(1));
        tiff.extend_from_slice(&u16b(orientation));
        tiff.extend_from_slice(&[0, 0]);
        tiff.extend_from_slice(&u32b(ifd1_offset));
        tiff.extend_from_slice(make);
        // IFD1: thumbnail offset and length.
        let ifd1_len = 2 + 2 * 12 + 4;
        let thumb_offset = ifd1_offset + ifd1_len;
        tiff.extend_from_slice(&u16b(2));
        tiff.extend_from_slice(&u16b(0x0201));
        tiff.extend_from_slice(&u16b(4));
        tiff.extend_from_slice(&u32b(1));
        tiff.extend_from_slice(&u32b(thumb_offset));
        tiff.extend_from_slice(&u16b(0x0202));
        tiff.extend_from_slice(&u16b(4));
        tiff.extend_from_slice(&u32b(1));
        tiff.extend_from_slice(&u32b(thumbnail.len() as u32));
        tiff.extend_from_slice(&u32b(0));
        tiff.extend_from_slice(thumbnail);

        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE1];
        jpeg.extend_from_slice(&((tiff.len() + 8) as u16).to_be_bytes());
        jpeg.extend_from_slice(b"Exif\0\0");
        jpeg.extend_from_slice(&tiff);
        // SOF0: 8 bit, 400 high, 600 wide, 3 components.
        jpeg.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 8, 0x01, 0x90, 0x02, 0x58, 3]);
        jpeg.extend_from_slice(&[1, 0x11, 0, 2, 0x11, 0, 3, 0x11, 0]);
        jpeg.extend_from_slice(&[0xFF, 0xD9]);
        jpeg
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{jpeg_with_exif, tiff_with_entries};
    use super::*;

    #[test]
    fn finds_orientation_thumbnail_and_size_in_both_byte_orders() {
        for big_endian in [false, true] {
            let thumbnail = vec![0xABu8; 600];
            let jpeg = jpeg_with_exif(6, big_endian, &thumbnail);
            let probe = probe_jpeg(&jpeg);

            assert_eq!(probe.orientation.to_exif(), 6);
            assert_eq!(probe.pixel_size, Some([600, 400]));
            assert_eq!(probe.shot.make.as_deref(), Some("TestCam"));

            let slot = probe.slot.expect("orientation slot");
            assert_eq!(slot.big_endian, big_endian);
            let stored = [jpeg[slot.pos as usize], jpeg[slot.pos as usize + 1]];
            let value = if big_endian {
                u16::from_be_bytes(stored)
            } else {
                u16::from_le_bytes(stored)
            };
            assert_eq!(value, 6);

            assert_eq!(probe.embedded.len(), 1);
            let blob = probe.embedded[0];
            let range = blob.offset as usize..(blob.offset + blob.len) as usize;
            assert_eq!(&jpeg[range], thumbnail.as_slice());
        }
    }

    #[test]
    fn source_serves_the_head_from_memory_and_the_rest_from_the_file() {
        let mut bytes = jpeg_with_exif(8, false, &[0x77u8; 600]);
        bytes.resize(HEAD_BYTES as usize + 5000, 0x33);
        let path =
            std::env::temp_dir().join(format!("rawv-test-source-{}.jpg", std::process::id()));
        std::fs::write(&path, &bytes).expect("write");

        let mut source = MediaFile::open(&path).expect("open");
        assert_eq!(source.head().len() as u64, HEAD_BYTES);
        assert!(!source.is_complete());
        let probe = source.probe(Kind::Jpeg);
        assert_eq!(probe.orientation.to_exif(), 8);
        assert_eq!(
            source.read_embedded(probe.embedded[0]).expect("blob"),
            vec![0x77u8; 600]
        );
        let tail = source
            .read_range(HEAD_BYTES - 2, 6)
            .expect("range across the head end");
        assert_eq!(
            tail,
            bytes[HEAD_BYTES as usize - 2..HEAD_BYTES as usize + 4]
        );
        assert!(source.read_range(HEAD_BYTES + 4000, 2000).is_err());
        assert_eq!(source.read_all().expect("all"), bytes);
        std::fs::remove_file(&path).expect("cleanup");
    }

    #[test]
    fn raw_container_is_probed_through_the_source() {
        let jpeg = jpeg_with_exif(3, true, &[0x11u8; 700]);
        // The EXIF block of the test JPEG is a complete TIFF file: bytes after `Exif\0\0`.
        let tiff = &jpeg[12..];
        let path =
            std::env::temp_dir().join(format!("rawv-test-source-{}.dng", std::process::id()));
        std::fs::write(&path, tiff).expect("write");
        let mut source = MediaFile::open(&path).expect("open");
        assert!(source.is_complete());
        let probe = source.probe(Kind::Raw);
        assert_eq!(probe.orientation.to_exif(), 3);
        assert_eq!(probe.shot.make.as_deref(), Some("TestCam"));
        assert_eq!(
            source.read_embedded(probe.embedded[0]).expect("blob"),
            vec![0x11u8; 700]
        );
        std::fs::remove_file(&path).expect("cleanup");
    }

    #[test]
    fn jpeg_compressed_strips_count_as_preview_unless_they_hold_sensor_data() {
        const SHORT: u16 = 3;
        const LONG: u16 = 4;
        let strip = |photometric: u32| {
            tiff_with_entries(
                &[
                    (tiff::TAG_COMPRESSION, SHORT, 7),
                    (tiff::TAG_PHOTOMETRIC, SHORT, photometric),
                    (tiff::TAG_STRIP_OFFSETS, LONG, 1000),
                    (tiff::TAG_STRIP_BYTE_COUNTS, LONG, 4000),
                ],
                6000,
            )
        };
        let preview = probe_tiff(Cursor::new(strip(6)));
        assert_eq!(
            preview.embedded,
            [Embedded {
                offset: 1000,
                len: 4000
            }]
        );
        for sensor in SENSOR_PHOTOMETRIC {
            assert!(probe_tiff(Cursor::new(strip(sensor))).embedded.is_empty());
        }
    }

    #[test]
    fn preview_check_reads_only_the_first_bytes() {
        let path =
            std::env::temp_dir().join(format!("rawv-test-preview-{}.bin", std::process::id()));
        let mut bytes = vec![0u8; 3000];
        bytes[100..104].copy_from_slice(&[0xFF, 0xD8, 0xFF, 0xE1]);
        bytes[1000..1004].copy_from_slice(&[0xFF, 0xD8, 0xFF, 0xDB]);
        bytes[2000..2004].copy_from_slice(&[0xFF, 0xD8, 0xFF, 0xC4]);
        std::fs::write(&path, &bytes).expect("write");
        let mut file = MediaFile::open(&path).expect("open");
        let blob = |offset| Embedded { offset, len: 600 };
        assert!(file.holds_preview(blob(100)));
        assert!(file.holds_preview(blob(1000)));
        assert!(!file.holds_preview(blob(2000)));
        assert!(!file.holds_preview(blob(500)));
        assert!(!file.holds_preview(blob(2999)));
        std::fs::remove_file(&path).expect("cleanup");
    }

    #[test]
    fn garbage_yields_empty_probe() {
        let probe = probe_jpeg(&[0xFF, 0xD8, 0x12, 0x34, 0x56, 0x78]);
        assert!(probe.slot.is_none());
        assert!(probe.embedded.is_empty());
        let probe = probe_tiff(Cursor::new(vec![0u8; 64]));
        assert!(probe.slot.is_none());
    }
}

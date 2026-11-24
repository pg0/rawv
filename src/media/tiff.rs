//! Minimal TIFF directory reader: enough to find orientation, embedded JPEGs and shot data
//! in EXIF blocks and TIFF-based RAW files without reading the whole file.

use std::io::{self, Read, Seek, SeekFrom};

pub const TAG_IMAGE_WIDTH: u16 = 0x0100;
pub const TAG_IMAGE_LENGTH: u16 = 0x0101;
pub const TAG_COMPRESSION: u16 = 0x0103;
pub const TAG_PHOTOMETRIC: u16 = 0x0106;
pub const TAG_MAKE: u16 = 0x010F;
pub const TAG_MODEL: u16 = 0x0110;
pub const TAG_STRIP_OFFSETS: u16 = 0x0111;
pub const TAG_ORIENTATION: u16 = 0x0112;
pub const TAG_STRIP_BYTE_COUNTS: u16 = 0x0117;
pub const TAG_SUB_IFDS: u16 = 0x014A;
pub const TAG_XMP: u16 = 0x02BC;
pub const TAG_JPEG_OFFSET: u16 = 0x0201;
pub const TAG_JPEG_LENGTH: u16 = 0x0202;
pub const TAG_PANASONIC_JPG_FROM_RAW: u16 = 0x002E;
pub const TAG_EXIF_IFD: u16 = 0x8769;
pub const TAG_EXPOSURE_TIME: u16 = 0x829A;
pub const TAG_F_NUMBER: u16 = 0x829D;
pub const TAG_ISO: u16 = 0x8827;
pub const TAG_DATE_TIME_ORIGINAL: u16 = 0x9003;
pub const TAG_FOCAL_LENGTH: u16 = 0x920A;
pub const TAG_PIXEL_X_DIMENSION: u16 = 0xA002;
pub const TAG_PIXEL_Y_DIMENSION: u16 = 0xA003;
pub const TAG_LENS_MODEL: u16 = 0xA434;

const TYPE_BYTE: u16 = 1;
const TYPE_ASCII: u16 = 2;
const TYPE_SHORT: u16 = 3;
const TYPE_LONG: u16 = 4;
const TYPE_RATIONAL: u16 = 5;
const TYPE_UNDEFINED: u16 = 7;
const TYPE_IFD: u16 = 13;

const MAX_ENTRIES: u16 = 1024;
const MAX_ASCII: u32 = 256;
const MAX_ARRAY: u32 = 16;

#[derive(Clone, Copy, Debug)]
pub struct Entry {
    pub tag: u16,
    pub typ: u16,
    pub count: u32,
    /// Position of the 4-byte value field, relative to the start of the underlying reader.
    pub value_pos: u64,
    value: [u8; 4],
}

pub struct TiffReader<R> {
    reader: R,
    /// Position of the TIFF header in the underlying reader; all TIFF offsets are relative to it.
    base: u64,
    big_endian: bool,
}

impl<R: Read + Seek> TiffReader<R> {
    /// Reads the TIFF header at `base` and returns the reader plus the offset of the first IFD.
    pub fn open(mut reader: R, base: u64) -> io::Result<(Self, u32)> {
        reader.seek(SeekFrom::Start(base))?;
        let mut header = [0u8; 8];
        reader.read_exact(&mut header)?;
        let big_endian = match &header[0..2] {
            b"II" => false,
            b"MM" => true,
            _ => return Err(invalid("not a TIFF header")),
        };
        let this = Self {
            reader,
            base,
            big_endian,
        };
        let magic = this.u16_from([header[2], header[3]]);
        // 42 = TIFF, 0x55 = Panasonic RW2, 0x4F52/0x5352 = Olympus ORF.
        if !matches!(magic, 42 | 0x55 | 0x4F52 | 0x5352) {
            return Err(invalid("unknown TIFF magic"));
        }
        let first = this.u32_from([header[4], header[5], header[6], header[7]]);
        Ok((this, first))
    }

    pub fn big_endian(&self) -> bool {
        self.big_endian
    }

    pub fn base(&self) -> u64 {
        self.base
    }

    /// Reads the directory at `offset` and returns its entries plus the offset of the next one.
    pub fn read_ifd(&mut self, offset: u32) -> io::Result<(Vec<Entry>, u32)> {
        let start = self.base + u64::from(offset);
        self.reader.seek(SeekFrom::Start(start))?;
        let mut count_bytes = [0u8; 2];
        self.reader.read_exact(&mut count_bytes)?;
        let count = self.u16_from(count_bytes);
        if count > MAX_ENTRIES {
            return Err(invalid("implausible IFD entry count"));
        }
        let mut raw = vec![0u8; usize::from(count) * 12 + 4];
        self.reader.read_exact(&mut raw)?;
        let mut entries = Vec::with_capacity(usize::from(count));
        for (index, chunk) in raw
            .as_chunks::<12>()
            .0
            .iter()
            .take(usize::from(count))
            .enumerate()
        {
            entries.push(Entry {
                tag: self.u16_from([chunk[0], chunk[1]]),
                typ: self.u16_from([chunk[2], chunk[3]]),
                count: self.u32_from([chunk[4], chunk[5], chunk[6], chunk[7]]),
                value_pos: start + 2 + index as u64 * 12 + 8,
                value: [chunk[8], chunk[9], chunk[10], chunk[11]],
            });
        }
        let tail = raw.len() - 4;
        let next = self.u32_from([raw[tail], raw[tail + 1], raw[tail + 2], raw[tail + 3]]);
        Ok((entries, next))
    }

    /// A single SHORT, LONG or IFD value stored inline.
    pub fn scalar(&self, entry: &Entry) -> Option<u32> {
        if entry.count != 1 {
            return None;
        }
        match entry.typ {
            TYPE_SHORT => Some(u32::from(self.u16_from([entry.value[0], entry.value[1]]))),
            TYPE_LONG | TYPE_IFD => Some(self.u32_from(entry.value)),
            _ => None,
        }
    }

    /// Offset stored in the value field, for values that do not fit inline.
    pub fn value_offset(&self, entry: &Entry) -> u32 {
        self.u32_from(entry.value)
    }

    /// Up to `MAX_ARRAY` LONG/IFD values (for example a SubIFD offset list).
    pub fn long_array(&mut self, entry: &Entry) -> io::Result<Vec<u32>> {
        if !matches!(entry.typ, TYPE_LONG | TYPE_IFD) || entry.count == 0 {
            return Ok(Vec::new());
        }
        if entry.count == 1 {
            return Ok(vec![self.u32_from(entry.value)]);
        }
        let count = entry.count.min(MAX_ARRAY) as usize;
        let mut raw = vec![0u8; count * 4];
        self.read_at(self.u32_from(entry.value), &mut raw)?;
        Ok(raw
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| self.u32_from([c[0], c[1], c[2], c[3]]))
            .collect())
    }

    pub fn rational(&mut self, entry: &Entry) -> io::Result<Option<f64>> {
        if entry.typ != TYPE_RATIONAL || entry.count == 0 {
            return Ok(None);
        }
        let mut raw = [0u8; 8];
        self.read_at(self.u32_from(entry.value), &mut raw)?;
        let numerator = self.u32_from([raw[0], raw[1], raw[2], raw[3]]);
        let denominator = self.u32_from([raw[4], raw[5], raw[6], raw[7]]);
        if denominator == 0 {
            return Ok(None);
        }
        Ok(Some(f64::from(numerator) / f64::from(denominator)))
    }

    pub fn ascii(&mut self, entry: &Entry) -> io::Result<Option<String>> {
        if entry.typ != TYPE_ASCII || entry.count == 0 {
            return Ok(None);
        }
        let len = entry.count.min(MAX_ASCII) as usize;
        let mut raw = vec![0u8; len];
        if entry.count <= 4 {
            raw.copy_from_slice(&entry.value[..len]);
        } else {
            self.read_at(self.u32_from(entry.value), &mut raw)?;
        }
        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        let text = String::from_utf8_lossy(&raw[..end]).trim().to_owned();
        Ok((!text.is_empty()).then_some(text))
    }

    /// The first bytes, at most `limit`, of a byte blob stored out of line.
    pub fn blob(&mut self, entry: &Entry, limit: u32) -> io::Result<Vec<u8>> {
        if !Self::is_blob(entry) {
            return Ok(Vec::new());
        }
        let mut raw = vec![0u8; entry.count.min(limit) as usize];
        self.read_at(self.u32_from(entry.value), &mut raw)?;
        Ok(raw)
    }

    /// True for a byte blob (BYTE/UNDEFINED) stored out of line.
    pub fn is_blob(entry: &Entry) -> bool {
        matches!(entry.typ, TYPE_BYTE | TYPE_UNDEFINED) && entry.count > 4
    }

    fn read_at(&mut self, offset: u32, buf: &mut [u8]) -> io::Result<()> {
        self.reader
            .seek(SeekFrom::Start(self.base + u64::from(offset)))?;
        self.reader.read_exact(buf)
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
}

pub fn find(entries: &[Entry], tag: u16) -> Option<&Entry> {
    entries.iter().find(|e| e.tag == tag)
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

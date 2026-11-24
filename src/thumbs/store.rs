//! Persistent thumbnail cache in a single local database file.

use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{self, Sender};

use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};

use crate::media::orientation::Orientation;
use crate::media::thumb::{ContentRect, Thumb, ThumbQuality};

const TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("thumbs_v1");
const HEADER_LEN: usize = 10;
const MAX_BATCH: usize = 256;

#[derive(Clone, Debug, PartialEq)]
pub struct StoredThumb {
    pub quality: ThumbQuality,
    pub orientation: Orientation,
    pub content: ContentRect,
    pub jpeg: Vec<u8>,
}

impl StoredThumb {
    fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(HEADER_LEN + self.jpeg.len());
        bytes.push(self.quality as u8);
        bytes.push(self.orientation.to_exif() as u8);
        for value in [
            self.content.min[0],
            self.content.min[1],
            self.content.max[0],
            self.content.max[1],
        ] {
            let scaled = (value.clamp(0.0, 1.0) * f32::from(u16::MAX)).round() as u16;
            bytes.extend_from_slice(&scaled.to_le_bytes());
        }
        bytes.extend_from_slice(&self.jpeg);
        bytes
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        let (header, jpeg) = bytes.split_at_checked(HEADER_LEN)?;
        let unit = |index: usize| {
            f32::from(u16::from_le_bytes([header[index], header[index + 1]])) / f32::from(u16::MAX)
        };
        Some(Self {
            quality: ThumbQuality::from_byte(header[0])?,
            orientation: Orientation::from_exif(u16::from(header[1])),
            content: ContentRect {
                min: [unit(2), unit(4)],
                max: [unit(6), unit(8)],
            },
            jpeg: jpeg.to_vec(),
        })
    }
}

enum Write {
    Put {
        key: String,
        value: Vec<u8>,
    },
    SetOrientation {
        key: String,
        orientation: Orientation,
    },
}

/// Reads happen on the calling thread; writes are batched on a background thread.
#[derive(Clone)]
pub struct ThumbStore {
    database: Option<Arc<Database>>,
    writes: Option<Sender<Write>>,
}

impl ThumbStore {
    /// Opens the store. If the file cannot be opened (for example because another
    /// instance holds it), thumbnails are simply not cached in this session.
    pub fn open(path: &Path) -> Self {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let Ok(database) = Database::create(path) else {
            return Self::disabled();
        };
        let database = Arc::new(database);
        let (tx, rx) = mpsc::channel::<Write>();
        let writer_db = Arc::clone(&database);
        std::thread::spawn(move || {
            while let Ok(first) = rx.recv() {
                let mut batch = vec![first];
                batch.extend(rx.try_iter().take(MAX_BATCH - 1));
                let _ = write_batch(&writer_db, batch);
            }
        });
        Self {
            database: Some(database),
            writes: Some(tx),
        }
    }

    pub fn disabled() -> Self {
        Self {
            database: None,
            writes: None,
        }
    }

    pub fn get(&self, key: &str) -> Option<StoredThumb> {
        let database = self.database.as_ref()?;
        let table = database.begin_read().ok()?.open_table(TABLE).ok()?;
        let value = table.get(key).ok()??;
        StoredThumb::decode(value.value())
    }

    pub fn put(&self, key: String, thumb: &Thumb) {
        let stored = StoredThumb {
            quality: thumb.quality,
            orientation: thumb.orientation,
            content: thumb.content,
            jpeg: thumb.jpeg.clone(),
        };
        self.send(Write::Put {
            key,
            value: stored.encode(),
        });
    }

    pub fn set_orientation(&self, key: String, orientation: Orientation) {
        self.send(Write::SetOrientation { key, orientation });
    }

    fn send(&self, write: Write) {
        if let Some(writes) = &self.writes {
            let _ = writes.send(write);
        }
    }
}

fn write_batch(database: &Database, batch: Vec<Write>) -> Result<(), redb::Error> {
    let transaction = database.begin_write()?;
    {
        let mut table = transaction.open_table(TABLE)?;
        for write in batch {
            match write {
                Write::Put { key, value } => {
                    table.insert(key.as_str(), value.as_slice())?;
                }
                Write::SetOrientation { key, orientation } => {
                    let existing = table.get(key.as_str())?.map(|v| v.value().to_vec());
                    if let Some(mut value) = existing
                        && value.len() >= HEADER_LEN
                    {
                        value[1] = orientation.to_exif() as u8;
                        table.insert(key.as_str(), value.as_slice())?;
                    }
                }
            }
        }
    }
    transaction.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::pixels::{Channels, Pixels};
    use std::time::{Duration, Instant};

    fn sample_thumb() -> Thumb {
        Thumb {
            jpeg: vec![0xFF, 0xD8, 1, 2, 3],
            pixels: Pixels {
                size: [1, 1],
                channels: Channels::Rgba,
                data: vec![0; 4],
            },
            orientation: Orientation::from_exif(6),
            quality: ThumbQuality::Embedded,
            content: ContentRect {
                min: [0.0, 0.125],
                max: [1.0, 0.875],
            },
        }
    }

    fn wait_until(
        store: &ThumbStore,
        key: &str,
        check: impl Fn(&StoredThumb) -> bool,
    ) -> StoredThumb {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(stored) = store.get(key)
                && check(&stored)
            {
                return stored;
            }
            assert!(Instant::now() < deadline, "store write did not arrive");
            std::thread::yield_now();
        }
    }

    #[test]
    fn stored_thumb_round_trips_and_orientation_can_be_updated() {
        let path =
            std::env::temp_dir().join(format!("rawv-test-store-{}.redb", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let store = ThumbStore::open(&path);
        assert!(store.get("missing").is_none());

        store.put("a|1|2".to_owned(), &sample_thumb());
        let stored = wait_until(&store, "a|1|2", |_| true);
        assert_eq!(stored.quality, ThumbQuality::Embedded);
        assert_eq!(stored.orientation.to_exif(), 6);
        assert_eq!(stored.jpeg, vec![0xFF, 0xD8, 1, 2, 3]);
        assert!((stored.content.min[1] - 0.125).abs() < 1e-4);
        assert!((stored.content.max[1] - 0.875).abs() < 1e-4);

        store.set_orientation("a|1|2".to_owned(), Orientation::from_exif(3));
        let stored = wait_until(&store, "a|1|2", |s| s.orientation.to_exif() == 3);
        assert_eq!(stored.jpeg, vec![0xFF, 0xD8, 1, 2, 3]);

        drop(store);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn disabled_store_ignores_everything() {
        let store = ThumbStore::disabled();
        store.put("k".to_owned(), &sample_thumb());
        assert!(store.get("k").is_none());
    }

    #[test]
    fn truncated_value_is_not_decoded() {
        assert!(StoredThumb::decode(&[1, 1, 0]).is_none());
        assert!(StoredThumb::decode(&[9; HEADER_LEN]).is_none());
    }
}

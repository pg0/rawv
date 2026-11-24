//! Local cache of developed RAW images, so a file's sensor data is demosaiced only once.
//!
//! Each image is one file: a byte for the orientation the RAW container stated, then a JPEG.
//! Files are written by a background thread. When their total size passes the budget, the
//! ones that were used longest ago are deleted.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::time::SystemTime;

use super::orientation::Orientation;
use super::pixels::{self, Channels};
use super::raw::RawPixels;

const JPEG_QUALITY: u8 = 92;
/// Raised when developing gives different pixels, so older files are no longer used.
const DEVELOP_VERSION: u32 = 1;
const EXTENSION: &str = "developed";
const PART_EXTENSION: &str = "part";
/// Images waiting to be written. Further ones are skipped and developed again next time.
const PENDING_WRITES: usize = 2;

enum Message {
    Store {
        file: PathBuf,
        image: RawPixels,
    },
    Prune,
    #[cfg(test)]
    Flush(mpsc::Sender<()>),
}

struct Inner {
    dir: PathBuf,
    budget: AtomicU64,
    messages: SyncSender<Message>,
    /// Images handed over but not yet written.
    unwritten: Arc<AtomicUsize>,
}

#[derive(Clone)]
pub struct DevelopedCache {
    inner: Option<Arc<Inner>>,
}

impl DevelopedCache {
    /// Opens the cache in `dir`, keeping at most `budget` bytes. Without a usable folder
    /// the cache stays empty.
    pub fn open(dir: &Path, budget: u64) -> Self {
        if std::fs::create_dir_all(dir).is_err() {
            return Self::disabled();
        }
        let (messages, inbox) = mpsc::sync_channel(PENDING_WRITES);
        let unwritten = Arc::new(AtomicUsize::new(0));
        let inner = Arc::new(Inner {
            dir: dir.to_path_buf(),
            budget: AtomicU64::new(budget),
            messages,
            unwritten: Arc::clone(&unwritten),
        });
        let writer = Arc::downgrade(&inner);
        let dir = dir.to_path_buf();
        std::thread::spawn(move || {
            let budget = || {
                writer
                    .upgrade()
                    .map(|inner| inner.budget.load(Ordering::Relaxed))
            };
            write_loop(&dir, &inbox, &unwritten, budget);
        });
        Self { inner: Some(inner) }
    }

    pub fn disabled() -> Self {
        Self { inner: None }
    }

    /// Changes the size limit and deletes what no longer fits.
    pub fn set_budget(&self, budget: u64) {
        if let Some(inner) = &self.inner {
            inner.budget.store(budget, Ordering::Relaxed);
            // A full inbox means writes are pending; each of them prunes as well.
            let _ = inner.messages.try_send(Message::Prune);
        }
    }

    /// The developed image stored for `key`, the identity of one file version.
    pub fn get(&self, key: &str) -> Option<RawPixels> {
        let inner = self.inner.as_ref()?;
        if inner.budget.load(Ordering::Relaxed) == 0 {
            return None;
        }
        let file = inner.dir.join(file_name(key));
        let bytes = std::fs::read(&file).ok()?;
        let (&orientation, jpeg) = bytes.split_first()?;
        let pixels = pixels::decode_jpeg(jpeg, Channels::Rgba).ok()?;
        mark_used(&file);
        Some(RawPixels {
            pixels,
            orientation: (orientation != 0).then(|| Orientation::from_exif(u16::from(orientation))),
        })
    }

    /// Stores `image` for `key` in the background.
    pub fn put(&self, key: &str, image: &RawPixels) {
        let Some(inner) = &self.inner else { return };
        if inner.budget.load(Ordering::Relaxed) == 0 {
            return;
        }
        let message = Message::Store {
            file: inner.dir.join(file_name(key)),
            image: RawPixels {
                pixels: image.pixels.clone(),
                orientation: image.orientation,
            },
        };
        inner.unwritten.fetch_add(1, Ordering::Relaxed);
        if let Err(error) = inner.messages.try_send(message) {
            inner.unwritten.fetch_sub(1, Ordering::Relaxed);
            if matches!(error, TrySendError::Disconnected(_)) {
                eprintln!("developed cache: writer stopped");
            }
        }
    }

    /// True when every image handed over has been written.
    pub fn is_idle(&self) -> bool {
        self.inner
            .as_ref()
            .is_none_or(|inner| inner.unwritten.load(Ordering::Relaxed) == 0)
    }

    #[cfg(test)]
    fn wait_until_written(&self) {
        let Some(inner) = &self.inner else { return };
        let (done, wait) = mpsc::channel();
        inner
            .messages
            .send(Message::Flush(done))
            .expect("writer runs");
        wait.recv().expect("writer answers");
    }
}

fn write_loop(
    dir: &Path,
    inbox: &Receiver<Message>,
    unwritten: &AtomicUsize,
    budget: impl Fn() -> Option<u64>,
) {
    let Some(limit) = budget() else { return };
    let mut total = prune(dir, limit);
    while let Ok(message) = inbox.recv() {
        let Some(limit) = budget() else { return };
        match message {
            Message::Store { file, image } => {
                match store(&file, &image) {
                    Ok(written) => total += written,
                    Err(error) => eprintln!("developed cache: {}: {error}", file.display()),
                }
                unwritten.fetch_sub(1, Ordering::Relaxed);
            }
            Message::Prune => total = prune(dir, limit),
            #[cfg(test)]
            Message::Flush(done) => {
                let _ = done.send(());
            }
        }
        if total > limit {
            total = prune(dir, limit);
        }
    }
}

/// Writes one image and returns the file's size. The file appears under its name only
/// once it is complete.
fn store(file: &Path, image: &RawPixels) -> std::io::Result<u64> {
    let jpeg = pixels::encode_jpeg(&image.pixels, JPEG_QUALITY).map_err(std::io::Error::other)?;
    let orientation = image
        .orientation
        .map_or(0, |orientation| orientation.to_exif() as u8);
    let mut bytes = Vec::with_capacity(jpeg.len() + 1);
    bytes.push(orientation);
    bytes.extend_from_slice(&jpeg);
    let part = file.with_extension(PART_EXTENSION);
    std::fs::write(&part, &bytes)?;
    std::fs::rename(&part, file)?;
    Ok(bytes.len() as u64)
}

/// Records that `file` was just used, so pruning keeps it longer.
fn mark_used(file: &Path) {
    if let Ok(handle) = std::fs::OpenOptions::new().write(true).open(file) {
        let _ = handle.set_modified(SystemTime::now());
    }
}

/// Deletes the files used longest ago until at most `budget` bytes remain, plus leftovers
/// of interrupted writes. Returns the bytes that remain.
fn prune(dir: &Path, budget: u64) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut files: Vec<(SystemTime, u64, PathBuf)> = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let extension = path.extension().and_then(|extension| extension.to_str());
        if extension == Some(PART_EXTENSION) {
            let _ = std::fs::remove_file(&path);
        } else if extension == Some(EXTENSION)
            && let Ok(metadata) = entry.metadata()
        {
            let used = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            files.push((used, metadata.len(), path));
        }
    }
    let mut total: u64 = files.iter().map(|(_, size, _)| size).sum();
    if total <= budget {
        return total;
    }
    // Going a little below the budget keeps the next few writes from pruning again.
    let target = budget - budget / 10;
    files.sort();
    for (_, size, path) in files {
        if total <= target {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= size;
        }
    }
    total
}

/// A file name derived from `key` and the develop version.
fn file_name(key: &str) -> String {
    let versioned = format!("{key}|develop-{DEVELOP_VERSION}");
    let low = fnv1a(versioned.as_bytes(), 0xcbf2_9ce4_8422_2325);
    let high = fnv1a(versioned.as_bytes(), 0x8422_2325_cbf2_9ce4);
    format!("{high:016x}{low:016x}.{EXTENSION}")
}

/// FNV-1a: a hash that gives the same value in every build, unlike the standard hasher.
fn fnv1a(bytes: &[u8], seed: u64) -> u64 {
    bytes.iter().fold(seed, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::pixels::Pixels;
    use std::time::Duration;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rawv-test-developed-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn image(width: u32, height: u32) -> RawPixels {
        let data = (0..width * height)
            .flat_map(|i| [(i % 251) as u8, 90, 200, 255])
            .collect();
        RawPixels {
            pixels: Pixels {
                size: [width, height],
                channels: Channels::Rgba,
                data,
            },
            orientation: Some(Orientation::from_exif(6)),
        }
    }

    #[test]
    fn stores_and_returns_an_image_with_its_orientation() {
        let dir = temp_dir("roundtrip");
        let cache = DevelopedCache::open(&dir, 1 << 30);
        assert!(cache.get("a.arw|1|2").is_none());
        cache.put("a.arw|1|2", &image(64, 48));
        cache.wait_until_written();

        let stored = cache.get("a.arw|1|2").expect("stored image");
        assert_eq!(stored.pixels.size, [64, 48]);
        assert_eq!(stored.pixels.channels, Channels::Rgba);
        assert_eq!(stored.orientation, Some(Orientation::from_exif(6)));
        assert!(cache.get("a.arw|1|3").is_none());
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn a_budget_of_zero_stores_nothing_and_clears_the_folder() {
        let dir = temp_dir("off");
        let cache = DevelopedCache::open(&dir, 1 << 30);
        cache.put("a", &image(32, 32));
        cache.wait_until_written();
        assert!(cache.get("a").is_some());

        cache.set_budget(0);
        cache.wait_until_written();
        assert!(cache.get("a").is_none());
        cache.put("b", &image(32, 32));
        cache.wait_until_written();
        assert_eq!(std::fs::read_dir(&dir).expect("dir").count(), 0);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn pruning_deletes_the_files_used_longest_ago() {
        let dir = temp_dir("prune");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let now = SystemTime::now();
        for (name, age) in [("old", 300), ("middle", 200), ("new", 100)] {
            let path = dir.join(name).with_extension(EXTENSION);
            std::fs::write(&path, vec![0u8; 1000]).expect("write");
            let file = std::fs::OpenOptions::new()
                .write(true)
                .open(&path)
                .expect("open");
            file.set_modified(now - Duration::from_secs(age))
                .expect("set time");
        }
        std::fs::write(dir.join("broken.part"), b"x").expect("write");

        assert_eq!(prune(&dir, 5000), 3000);
        assert_eq!(prune(&dir, 2500), 2000);
        let mut left: Vec<String> = std::fs::read_dir(&dir)
            .expect("dir")
            .map(|entry| {
                entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        left.sort();
        assert_eq!(left, ["middle.developed", "new.developed"]);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn file_names_depend_on_the_key_and_stay_the_same() {
        assert_eq!(file_name("a"), file_name("a"));
        assert_ne!(file_name("a"), file_name("b"));
        assert_eq!(file_name("a").len(), 32 + 1 + EXTENSION.len());
    }
}

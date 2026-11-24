//! Folder contents: the images in a folder and its direct subfolders.

use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use super::natural;
use crate::media::formats::{self, Kind};

/// Identity of one file version. A changed file gets a new identity, so caches never go stale.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct FileId {
    pub path: PathBuf,
    pub modified_nanos: u128,
    pub size: u64,
}

impl FileId {
    pub fn cache_key(&self) -> String {
        format!(
            "{}|{}|{}",
            self.path.to_string_lossy(),
            self.modified_nanos,
            self.size
        )
    }
}

#[derive(Clone, Debug)]
pub struct ImageEntry {
    pub id: FileId,
    pub name: String,
    pub kind: Kind,
    pub modified: SystemTime,
}

impl ImageEntry {
    pub fn from_path(path: &Path) -> io::Result<Self> {
        let kind = formats::kind_of(path)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unsupported file type"))?;
        let metadata = std::fs::metadata(path)?;
        Ok(Self::new(path.to_path_buf(), kind, &metadata))
    }

    fn new(path: PathBuf, kind: Kind, metadata: &std::fs::Metadata) -> Self {
        let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let modified_nanos = modified
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self {
            id: FileId {
                path,
                modified_nanos,
                size: metadata.len(),
            },
            name,
            kind,
            modified,
        }
    }
}

#[derive(Clone, Debug)]
pub struct FolderEntry {
    pub path: PathBuf,
    pub name: String,
}

#[derive(Clone, Debug, Default)]
pub struct Listing {
    pub images: Vec<ImageEntry>,
    pub folders: Vec<FolderEntry>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum SortOrder {
    #[default]
    NameAscending,
    NameDescending,
    DateAscending,
    DateDescending,
}

impl SortOrder {
    pub fn next(self) -> Self {
        match self {
            Self::NameAscending => Self::NameDescending,
            Self::NameDescending => Self::DateAscending,
            Self::DateAscending => Self::DateDescending,
            Self::DateDescending => Self::NameAscending,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::NameAscending => "Name A-Z",
            Self::NameDescending => "Name Z-A",
            Self::DateAscending => "Oldest first",
            Self::DateDescending => "Newest first",
        }
    }
}

pub fn sort_images(images: &mut [ImageEntry], order: SortOrder) {
    match order {
        SortOrder::NameAscending => images.sort_by(|a, b| natural::compare(&a.name, &b.name)),
        SortOrder::NameDescending => images.sort_by(|a, b| natural::compare(&b.name, &a.name)),
        SortOrder::DateAscending => images.sort_by(|a, b| {
            a.modified
                .cmp(&b.modified)
                .then_with(|| natural::compare(&a.name, &b.name))
        }),
        SortOrder::DateDescending => images.sort_by(|a, b| {
            b.modified
                .cmp(&a.modified)
                .then_with(|| natural::compare(&a.name, &b.name))
        }),
    }
}

/// Lists one folder. Images come back in name order, folders in name order.
pub fn list_folder(path: &Path) -> io::Result<Listing> {
    let mut listing = Listing::default();
    for entry in std::fs::read_dir(path)? {
        let Ok(entry) = entry else { continue };
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let entry_path = entry.path();
        if metadata.is_dir() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !is_hidden_folder(&name) {
                listing.folders.push(FolderEntry {
                    path: entry_path,
                    name,
                });
            }
        } else if let Some(kind) = formats::kind_of(&entry_path) {
            listing
                .images
                .push(ImageEntry::new(entry_path, kind, &metadata));
        }
    }
    sort_images(&mut listing.images, SortOrder::NameAscending);
    listing
        .folders
        .sort_by(|a, b| natural::compare(&a.name, &b.name));
    Ok(listing)
}

/// System and NAS bookkeeping folders that never hold the user's photos.
fn is_hidden_folder(name: &str) -> bool {
    name.starts_with('.')
        || name.starts_with('$')
        || name.starts_with('@')
        || name.starts_with('#')
        || name.eq_ignore_ascii_case("System Volume Information")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_images_and_folders_and_skips_the_rest() {
        let root = std::env::temp_dir().join(format!("rawv-test-listing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("roll 2")).expect("mkdir");
        std::fs::create_dir_all(root.join("@eaDir")).expect("mkdir");
        std::fs::create_dir_all(root.join(".hidden")).expect("mkdir");
        for name in ["b10.jpg", "b2.JPG", "a.ARW", "notes.txt"] {
            std::fs::write(root.join(name), b"x").expect("write");
        }

        let listing = list_folder(&root).expect("list");
        let images: Vec<&str> = listing.images.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(images, ["a.ARW", "b2.JPG", "b10.jpg"]);
        let folders: Vec<&str> = listing.folders.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(folders, ["roll 2"]);
        assert_eq!(listing.images[0].kind, Kind::Raw);
        assert_eq!(listing.images[0].id.size, 1);

        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn sort_orders_cycle_back_to_start() {
        let mut order = SortOrder::default();
        for _ in 0..4 {
            order = order.next();
        }
        assert_eq!(order, SortOrder::default());
    }
}

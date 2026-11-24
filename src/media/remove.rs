//! Removing a photo: into the recycle bin where the volume has one.

use std::io;
use std::path::{Component, Path, Prefix};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Removal {
    /// The file is in the recycle bin and can be restored from there.
    Recycled,
    /// The file is gone for good.
    Deleted,
    /// The volume has no recycle bin (network shares, memory cards) and deleting for good
    /// was not allowed. The file is untouched.
    NeedsConsent,
}

/// Moves `path` to the recycle bin. Where the volume has none, the file is deleted only
/// when `permanently` allows it.
pub fn remove(path: &Path, permanently: bool) -> io::Result<Removal> {
    if has_recycle_bin(path)? {
        trash::delete(path).map_err(io::Error::other)?;
        return Ok(Removal::Recycled);
    }
    if !permanently {
        return Ok(Removal::NeedsConsent);
    }
    std::fs::remove_file(path)?;
    Ok(Removal::Deleted)
}

/// Whether the volume holding `path` keeps a recycle bin: a local drive with the system's
/// recycle folder at its root. Asked up front, because the system deletes for good without
/// saying so when told to recycle a file on a network share.
fn has_recycle_bin(path: &Path) -> io::Result<bool> {
    let resolved = std::fs::canonicalize(path)?;
    let Some(Component::Prefix(prefix)) = resolved.components().next() else {
        return Ok(false);
    };
    let (Prefix::VerbatimDisk(letter) | Prefix::Disk(letter)) = prefix.kind() else {
        return Ok(false);
    };
    let root = format!("{}:\\", char::from(letter));
    Ok(Path::new(&root).join("$RECYCLE.BIN").is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_is_an_error_not_a_removal() {
        let path = std::env::temp_dir().join(format!(
            "rawv-test-remove-{}-missing.jpg",
            std::process::id()
        ));
        assert!(remove(&path, false).is_err());
        assert!(remove(&path, true).is_err());
    }
}

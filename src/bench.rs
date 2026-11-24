//! `rawv probe` and `rawv bench`: command-line checks of the media pipeline.

use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::media::formats::{self, Kind};
use crate::media::load::{self, Rendition};
use crate::media::probe::MediaFile;
use crate::media::thumb::{self, ThumbQuality};

/// Prints what the metadata probe finds in one file.
pub fn probe_file(path: &Path) -> Result<(), String> {
    let kind = formats::kind_of(path).ok_or("unsupported file type")?;
    let started = Instant::now();
    let mut source = MediaFile::open(path).map_err(|e| e.to_string())?;
    let probe = source.probe(kind);
    println!("probe            {:>8.2} ms", ms(started));
    println!("kind             {kind:?}");
    println!("orientation      {} (exif)", probe.orientation.to_exif());
    println!("orientation slot {:?}", probe.slot);
    println!("pixel size       {:?}", probe.pixel_size);
    println!("shot             {:?}", probe.shot);
    for blob in &probe.embedded {
        let size = source
            .read_embedded(*blob)
            .ok()
            .and_then(|bytes| crate::media::pixels::jpeg_size(&bytes));
        println!(
            "embedded jpeg    offset {:>10} len {:>9} size {:?}",
            blob.offset, blob.len, size
        );
    }
    Ok(())
}

/// Times each pipeline stage for a file, or thumbnail creation for every image in a folder.
pub fn bench(path: &Path) -> Result<(), String> {
    if path.is_dir() {
        bench_folder(path)
    } else {
        bench_file(path)
    }
}

fn bench_file(path: &Path) -> Result<(), String> {
    let kind = formats::kind_of(path).ok_or("unsupported file type")?;
    println!("{}", path.display());

    for want in [ThumbQuality::Embedded, ThumbQuality::Full] {
        let started = Instant::now();
        let thumb = thumb::make_thumb(path, kind, want).map_err(|e| e.to_string())?;
        println!(
            "thumb {:<9} {:>8.2} ms  -> {:?} {}x{} ({} bytes)",
            format!("{want:?}"),
            ms(started),
            thumb.quality,
            thumb.pixels.size[0],
            thumb.pixels.size[1],
            thumb.jpeg.len()
        );
    }

    let renditions: &[Rendition] = match kind {
        Kind::Raw => &[Rendition::Preview, Rendition::Developed],
        _ => &[Rendition::Preview],
    };
    for &mode in renditions {
        let started = Instant::now();
        let image = load::load_full(path, kind, mode, 16384, None).map_err(|e| e.to_string())?;
        println!(
            "full  {:<9} {:>8.2} ms  -> {:?} {}x{} orientation {}",
            format!("{mode:?}"),
            ms(started),
            image.source,
            image.pixels.size[0],
            image.pixels.size[1],
            image.orientation.to_exif()
        );
    }
    Ok(())
}

fn bench_folder(path: &Path) -> Result<(), String> {
    let files: Vec<(PathBuf, Kind)> = std::fs::read_dir(path)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter_map(|p| formats::kind_of(&p).map(|kind| (p, kind)))
        .collect();
    println!("{} images in {}", files.len(), path.display());
    if files.is_empty() {
        return Ok(());
    }
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());

    for want in [ThumbQuality::Embedded, ThumbQuality::Full] {
        let started = Instant::now();
        let next = std::sync::atomic::AtomicUsize::new(0);
        let failed = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..threads {
                scope.spawn(|| {
                    loop {
                        let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some((file, kind)) = files.get(index) else {
                            break;
                        };
                        if thumb::make_thumb(file, *kind, want).is_err() {
                            failed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                });
            }
        });
        let total = ms(started);
        println!(
            "thumbs {:<9} {:>9.1} ms total  {:>7.2} ms/image  {} threads  {} failed",
            format!("{want:?}"),
            total,
            total / files.len() as f64,
            threads,
            failed.into_inner()
        );
    }
    Ok(())
}

fn ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

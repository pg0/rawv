//! Full-resolution images for the viewer: decodes the current image and its neighbours
//! in the background and keeps their textures ready.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use egui::TextureHandle;

use crate::library::listing::{FileId, ImageEntry};
use crate::media::MediaError;
use crate::media::developed::DevelopedCache;
use crate::media::formats::Kind;
use crate::media::load::{self, DevelopSlot, FullImage, Rendition, Source};
use crate::media::orientation::Orientation;
use crate::media::probe::ShotInfo;
use crate::media::xmp::Rating;
use crate::repaint::Repaint;
use crate::texture;
use crate::workers::{self, WantedPool};

const WORKERS: usize = 3;
/// Texture side limit assumed until the graphics driver reports the real one.
const DEFAULT_MAX_SIDE: u32 = 16384;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct LoadKey {
    pub id: FileId,
    pub kind: Kind,
    pub rendition: Rendition,
}

impl LoadKey {
    pub fn new(entry: &ImageEntry, rendition: Rendition) -> Self {
        Self {
            id: entry.id.clone(),
            kind: entry.kind,
            // Only RAW files have two renditions; other files share one slot.
            rendition: match entry.kind {
                Kind::Raw => rendition,
                _ => Rendition::Preview,
            },
        }
    }
}

pub struct Loaded {
    pub texture: TextureHandle,
    pub original_size: [u32; 2],
    /// Orientation as stored in the file when it was loaded.
    pub orientation: Orientation,
    pub shot: ShotInfo,
    pub source: Source,
    pub cached: bool,
    /// The rating stored in the file when it was loaded.
    pub rating: Rating,
    pub load_ms: f32,
    /// Every frame of an animation with how long it shows, `texture` first; empty for a
    /// still picture.
    pub frames: Vec<(TextureHandle, Duration)>,
}

impl Loaded {
    /// The frame shown `elapsed` after the animation started, its number, and how long
    /// it stays. A still picture is frame 0 for ever.
    pub fn frame_at(&self, elapsed: Duration) -> (&TextureHandle, usize, Option<Duration>) {
        let total: Duration = self.frames.iter().map(|(_, delay)| *delay).sum();
        if total.is_zero() {
            return (&self.texture, 0, None);
        }
        let mut left = Duration::from_nanos((elapsed.as_nanos() % total.as_nanos()) as u64);
        for (index, (texture, delay)) in self.frames.iter().enumerate() {
            if left < *delay {
                return (texture, index, Some(*delay - left));
            }
            left -= *delay;
        }
        (&self.texture, 0, None)
    }
}

enum Slot {
    Ready(Loaded),
    Failed(String),
}

pub enum LoadState<'a> {
    Ready(&'a Loaded),
    Loading,
    Failed(&'a str),
}

type Decoded = Result<(FullImage, f32), String>;

pub struct FullLoader {
    pool: WantedPool<LoadKey, Decoded>,
    slots: HashMap<LoadKey, Slot>,
    max_side: Arc<AtomicU32>,
}

impl FullLoader {
    pub fn new(repaint: Repaint, developed: DevelopedCache) -> Self {
        let max_side = Arc::new(AtomicU32::new(DEFAULT_MAX_SIDE));
        let worker_max_side = Arc::clone(&max_side);
        let pool = WantedPool::new(WORKERS, repaint, move |key: &LoadKey| {
            let started = Instant::now();
            let max_side = worker_max_side.load(Ordering::Relaxed);
            let cache_key = key.id.cache_key();
            let slot = DevelopSlot {
                cache: &developed,
                key: &cache_key,
            };
            workers::guarded(|| {
                load::load_full(&key.id.path, key.kind, key.rendition, max_side, Some(slot))
                    .map_err(|error| error.to_string())
            })
            .map(|image| (image, started.elapsed().as_secs_f32() * 1000.0))
        });
        Self {
            pool,
            slots: HashMap::new(),
            max_side,
        }
    }

    /// Declares which images are needed, most urgent first. Everything else is dropped.
    pub fn want(&mut self, keys: &[LoadKey]) {
        self.slots.retain(|key, _| keys.contains(key));
        let missing = keys
            .iter()
            .filter(|key| !self.slots.contains_key(*key))
            .cloned()
            .collect();
        self.pool.set_wanted(missing);
    }

    /// Uploads finished images. Call once per frame, before [`Self::want`].
    pub fn poll(&mut self, ctx: &egui::Context) {
        let max_side = ctx.input(|input| input.max_texture_side) as u32;
        self.max_side.store(max_side, Ordering::Relaxed);
        for (key, result) in self.pool.poll() {
            let slot = match result {
                Ok((image, load_ms)) => match upload(ctx, image, max_side, load_ms) {
                    Ok(loaded) => Slot::Ready(loaded),
                    Err(error) => Slot::Failed(error.to_string()),
                },
                Err(message) => Slot::Failed(message),
            };
            self.slots.insert(key, slot);
        }
    }

    pub fn state(&self, key: &LoadKey) -> LoadState<'_> {
        match self.slots.get(key) {
            Some(Slot::Ready(loaded)) => LoadState::Ready(loaded),
            Some(Slot::Failed(message)) => LoadState::Failed(message),
            None => LoadState::Loading,
        }
    }

    pub fn is_idle(&self) -> bool {
        self.pool.is_idle()
    }
}

/// Puts a decoded image and the further frames of an animation on the graphics card.
fn upload(
    ctx: &egui::Context,
    image: FullImage,
    max_side: u32,
    load_ms: f32,
) -> Result<Loaded, MediaError> {
    let texture = texture::upload(ctx, "image", image.pixels.fit_within(max_side)?);
    let mut textures = vec![texture.clone()];
    for frame in image.later_frames {
        textures.push(texture::upload(ctx, "frame", frame.fit_within(max_side)?));
    }
    let frames = if image.delays.len() == textures.len() && textures.len() > 1 {
        textures.into_iter().zip(image.delays).collect()
    } else {
        Vec::new()
    };
    Ok(Loaded {
        texture,
        original_size: image.original_size,
        orientation: image.orientation,
        shot: image.shot,
        source: image.source,
        cached: image.cached,
        rating: image.rating,
        load_ms,
        frames,
    })
}

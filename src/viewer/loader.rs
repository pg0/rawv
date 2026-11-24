//! Full-resolution images for the viewer: decodes the current image and its neighbours
//! in the background and keeps their textures ready.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

use egui::TextureHandle;

use crate::library::listing::{FileId, ImageEntry};
use crate::media::developed::DevelopedCache;
use crate::media::formats::Kind;
use crate::media::load::{self, DevelopSlot, FullImage, Rendition, Source};
use crate::media::orientation::Orientation;
use crate::media::probe::ShotInfo;
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
    pub load_ms: f32,
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
                Ok((image, load_ms)) => match image.pixels.fit_within(max_side) {
                    Ok(pixels) => Slot::Ready(Loaded {
                        texture: texture::upload(ctx, "image", pixels),
                        original_size: image.original_size,
                        orientation: image.orientation,
                        shot: image.shot,
                        source: image.source,
                        cached: image.cached,
                        load_ms,
                    }),
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

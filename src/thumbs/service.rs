//! Thumbnails for the interface: asks workers for what is on screen and keeps the textures.

use std::collections::HashMap;

use egui::TextureHandle;

use super::store::ThumbStore;
use crate::library::listing::{FileId, ImageEntry};
use crate::media::formats::Kind;
use crate::media::orientation::Orientation;
use crate::media::pixels::{self, Channels, Pixels};
use crate::media::thumb::{self, ContentRect, ThumbQuality};
use crate::repaint::Repaint;
use crate::texture;
use crate::workers::{self, WantedPool};

/// Textures kept on the GPU before the least recently shown ones are dropped.
const MAX_TEXTURES: usize = 4000;
/// A camera thumbnail is replaced by a sharper one once it would be drawn larger than this
/// multiple of its own size.
const UPSCALE_TOLERANCE: f32 = 1.0;

#[derive(Clone, PartialEq, Eq, Hash)]
struct Request {
    id: FileId,
    kind: Kind,
    want: ThumbQuality,
}

struct Payload {
    pixels: Pixels,
    orientation: Orientation,
    quality: ThumbQuality,
    content: ContentRect,
}

pub struct ThumbTexture {
    pub texture: TextureHandle,
    pub size: [u32; 2],
    /// Orientation as stored in the file when the thumbnail was made.
    pub orientation: Orientation,
    pub quality: ThumbQuality,
    pub content: ContentRect,
    /// Set when a sharper version was requested and could not be made.
    sharper_unavailable: bool,
    last_used: u64,
}

enum Slot {
    Ready(ThumbTexture),
    Failed,
}

pub struct ThumbService {
    pool: WantedPool<Request, Result<Payload, String>>,
    slots: HashMap<FileId, Slot>,
    frame: u64,
    wanted_first: Vec<Request>,
    wanted_sharper: Vec<Request>,
}

impl ThumbService {
    pub fn new(store: ThumbStore, repaint: Repaint) -> Self {
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get().clamp(2, 12));
        let pool = WantedPool::new(threads, repaint, move |request: &Request| {
            workers::guarded(|| produce(&store, request))
        });
        Self {
            pool,
            slots: HashMap::new(),
            frame: 0,
            wanted_first: Vec::new(),
            wanted_sharper: Vec::new(),
        }
    }

    /// Takes finished thumbnails from the workers. Call once at the start of a frame.
    pub fn begin_frame(&mut self, ctx: &egui::Context) {
        self.frame += 1;
        self.wanted_first.clear();
        self.wanted_sharper.clear();
        for (request, result) in self.pool.poll() {
            let slot = match result {
                Ok(payload) => {
                    let superseded = matches!(
                        self.slots.get(&request.id),
                        Some(Slot::Ready(existing)) if existing.quality > payload.quality
                    );
                    if superseded {
                        continue;
                    }
                    Slot::Ready(ThumbTexture {
                        size: payload.pixels.size,
                        texture: texture::upload(ctx, "thumb", payload.pixels),
                        orientation: payload.orientation,
                        quality: payload.quality,
                        content: payload.content,
                        sharper_unavailable: false,
                        last_used: self.frame,
                    })
                }
                Err(_) => {
                    if let Some(Slot::Ready(existing)) = self.slots.get_mut(&request.id) {
                        existing.sharper_unavailable = true;
                        continue;
                    }
                    Slot::Failed
                }
            };
            self.slots.insert(request.id, slot);
        }
    }

    /// The thumbnail for an image about to be drawn `shown_long_edge` physical pixels large.
    /// Requests it (or a sharper version) from the workers when needed.
    pub fn lookup(&mut self, entry: &ImageEntry, shown_long_edge: f32) -> ThumbState<'_> {
        let frame = self.frame;
        match self.slots.get_mut(&entry.id) {
            Some(Slot::Ready(thumb)) => {
                thumb.last_used = frame;
                let long_edge = thumb.size[0].max(thumb.size[1]) as f32;
                if thumb.quality == ThumbQuality::Embedded
                    && !thumb.sharper_unavailable
                    && shown_long_edge > long_edge * UPSCALE_TOLERANCE
                {
                    self.wanted_sharper.push(Request {
                        id: entry.id.clone(),
                        kind: entry.kind,
                        want: ThumbQuality::Full,
                    });
                }
                ThumbState::Ready(thumb)
            }
            Some(Slot::Failed) => ThumbState::Failed,
            None => {
                self.wanted_first.push(Request {
                    id: entry.id.clone(),
                    kind: entry.kind,
                    want: ThumbQuality::Embedded,
                });
                ThumbState::Pending
            }
        }
    }

    /// An already loaded thumbnail, without requesting anything.
    pub fn peek(&self, id: &FileId) -> Option<&ThumbTexture> {
        match self.slots.get(id) {
            Some(Slot::Ready(thumb)) => Some(thumb),
            _ => None,
        }
    }

    /// Publishes this frame's requests to the workers. Call once at the end of a frame.
    pub fn end_frame(&mut self) {
        let mut wanted = std::mem::take(&mut self.wanted_first);
        wanted.append(&mut self.wanted_sharper);
        self.pool.set_wanted(wanted);

        if self.slots.len() > MAX_TEXTURES {
            let mut ages: Vec<u64> = self
                .slots
                .values()
                .filter_map(|slot| match slot {
                    Slot::Ready(thumb) => Some(thumb.last_used),
                    Slot::Failed => None,
                })
                .collect();
            ages.sort_unstable();
            let excess = self.slots.len() - MAX_TEXTURES * 3 / 4;
            if let Some(&cutoff) = ages.get(excess.min(ages.len().saturating_sub(1))) {
                let frame = self.frame;
                self.slots.retain(|_, slot| match slot {
                    Slot::Ready(thumb) => thumb.last_used >= cutoff || thumb.last_used == frame,
                    Slot::Failed => true,
                });
            }
        }
    }

    pub fn is_idle(&self) -> bool {
        self.pool.is_idle()
    }
}

pub enum ThumbState<'a> {
    Ready(&'a ThumbTexture),
    Pending,
    Failed,
}

fn produce(store: &ThumbStore, request: &Request) -> Result<Payload, String> {
    let key = request.id.cache_key();
    if let Some(stored) = store.get(&key)
        && stored.quality >= request.want
        && let Ok(pixels) = pixels::decode_jpeg(&stored.jpeg, Channels::Rgba)
    {
        return Ok(Payload {
            pixels,
            orientation: stored.orientation,
            quality: stored.quality,
            content: stored.content,
        });
    }
    let thumb = thumb::make_thumb(&request.id.path, request.kind, request.want)
        .map_err(|error| error.to_string())?;
    store.put(key, &thumb);
    Ok(Payload {
        pixels: thumb.pixels,
        orientation: thumb.orientation,
        quality: thumb.quality,
        content: thumb.content,
    })
}

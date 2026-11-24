//! Writes rotations to files in the background, one at a time and in order.

use std::sync::mpsc::{self, Receiver, Sender};

use crate::library::listing::ImageEntry;
use crate::media::orientation::Orientation;
use crate::media::rotate::{self, RotateOutcome};
use crate::repaint::Repaint;
use crate::thumbs::store::ThumbStore;

pub struct RotateDone {
    pub result: Result<RotateOutcome, String>,
}

pub struct Rotator {
    jobs: Sender<(ImageEntry, Orientation)>,
    done: Receiver<RotateDone>,
    pending: usize,
}

impl Rotator {
    pub fn new(store: ThumbStore, repaint: Repaint) -> Self {
        let (job_tx, job_rx) = mpsc::channel::<(ImageEntry, Orientation)>();
        let (done_tx, done_rx) = mpsc::channel();
        std::thread::spawn(move || {
            for (entry, orientation) in job_rx {
                let result = rotate::persist(&entry.id.path, entry.kind, orientation)
                    .map_err(|error| error.to_string());
                if result == Ok(RotateOutcome::Saved) {
                    store.set_orientation(entry.id.cache_key(), orientation);
                }
                if done_tx.send(RotateDone { result }).is_err() {
                    return;
                }
                repaint.request();
            }
        });
        Self {
            jobs: job_tx,
            done: done_rx,
            pending: 0,
        }
    }

    pub fn submit(&mut self, entry: ImageEntry, orientation: Orientation) {
        if self.jobs.send((entry, orientation)).is_ok() {
            self.pending += 1;
        }
    }

    pub fn poll(&mut self) -> Vec<RotateDone> {
        let done: Vec<RotateDone> = self.done.try_iter().collect();
        self.pending = self.pending.saturating_sub(done.len());
        done
    }

    pub fn is_idle(&self) -> bool {
        self.pending == 0
    }
}

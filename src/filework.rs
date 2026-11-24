//! Changes to photo files, done in the background one at a time and in order:
//! rotating and removing.

use std::sync::mpsc::{self, Receiver, Sender};

use crate::library::listing::ImageEntry;
use crate::media::orientation::Orientation;
use crate::media::remove::{self, Removal};
use crate::media::rotate::{self, RotateOutcome};
use crate::repaint::Repaint;
use crate::thumbs::store::ThumbStore;

#[derive(Clone, Copy)]
pub enum Job {
    Rotate(Orientation),
    /// Removes the file. `permanently` is the user's consent to delete it for good where
    /// there is no recycle bin.
    Remove {
        permanently: bool,
    },
}

pub enum Outcome {
    Rotated(RotateOutcome),
    Removed(Removal),
}

pub struct Done {
    pub entry: ImageEntry,
    pub job: Job,
    pub result: Result<Outcome, String>,
}

pub struct FileWorker {
    jobs: Sender<(ImageEntry, Job)>,
    done: Receiver<Done>,
    pending: usize,
}

impl FileWorker {
    pub fn new(store: ThumbStore, repaint: Repaint) -> Self {
        let (job_tx, job_rx) = mpsc::channel::<(ImageEntry, Job)>();
        let (done_tx, done_rx) = mpsc::channel();
        std::thread::spawn(move || {
            for (entry, job) in job_rx {
                let result = run(&entry, job, &store).map_err(|error| error.to_string());
                if done_tx.send(Done { entry, job, result }).is_err() {
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

    pub fn submit(&mut self, entry: ImageEntry, job: Job) {
        if self.jobs.send((entry, job)).is_ok() {
            self.pending += 1;
        }
    }

    pub fn poll(&mut self) -> Vec<Done> {
        let done: Vec<Done> = self.done.try_iter().collect();
        self.pending = self.pending.saturating_sub(done.len());
        done
    }

    pub fn is_idle(&self) -> bool {
        self.pending == 0
    }
}

fn run(entry: &ImageEntry, job: Job, store: &ThumbStore) -> std::io::Result<Outcome> {
    let path = &entry.id.path;
    match job {
        Job::Rotate(orientation) => {
            let outcome = rotate::persist(path, entry.kind, orientation)?;
            if outcome == RotateOutcome::Saved {
                store.set_orientation(entry.id.cache_key(), orientation);
            }
            Ok(Outcome::Rotated(outcome))
        }
        Job::Remove { permanently } => remove::remove(path, permanently).map(Outcome::Removed),
    }
}

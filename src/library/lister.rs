//! Background folder listing, so slow network shares never block the interface.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

use super::listing::{self, Listing};
use crate::repaint::Repaint;

const WORKERS: usize = 3;

pub struct ListResult {
    /// The ticket passed to [`Lister::request`].
    pub ticket: u64,
    pub path: PathBuf,
    pub listing: Result<Listing, String>,
}

pub struct Lister {
    requests: Sender<(u64, PathBuf)>,
    /// Highest ticket requested so far.
    newest: Arc<AtomicU64>,
    results: Receiver<ListResult>,
    pending: usize,
}

impl Lister {
    pub fn new(repaint: Repaint) -> Self {
        let (request_tx, request_rx) = mpsc::channel::<(u64, PathBuf)>();
        let (result_tx, result_rx) = mpsc::channel();
        let request_rx = Arc::new(Mutex::new(request_rx));
        let newest = Arc::new(AtomicU64::new(0));
        for _ in 0..WORKERS {
            let request_rx = Arc::clone(&request_rx);
            let result_tx = result_tx.clone();
            let repaint = repaint.clone();
            let newest = Arc::clone(&newest);
            std::thread::spawn(move || {
                loop {
                    let next = match request_rx.lock() {
                        Ok(receiver) => receiver.recv(),
                        Err(_) => return,
                    };
                    let Ok((ticket, path)) = next else { return };
                    let listing = if ticket < newest.load(Ordering::Relaxed) {
                        Err("superseded".to_owned())
                    } else {
                        listing::list_folder(&path).map_err(|e| e.to_string())
                    };
                    let sent = result_tx.send(ListResult {
                        ticket,
                        path,
                        listing,
                    });
                    if sent.is_err() {
                        return;
                    }
                    repaint.request();
                }
            });
        }
        Self {
            requests: request_tx,
            newest,
            results: result_rx,
            pending: 0,
        }
    }

    /// Asks for the listing of `path`. Requests still waiting under an older ticket are
    /// answered with an error instead of being read from disk.
    pub fn request(&mut self, ticket: u64, path: PathBuf) {
        self.newest.fetch_max(ticket, Ordering::Relaxed);
        if self.requests.send((ticket, path)).is_ok() {
            self.pending += 1;
        }
    }

    pub fn poll(&mut self) -> Vec<ListResult> {
        let results: Vec<ListResult> = self.results.try_iter().collect();
        self.pending = self.pending.saturating_sub(results.len());
        results
    }

    pub fn is_idle(&self) -> bool {
        self.pending == 0
    }
}

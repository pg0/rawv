//! A worker pool driven by a "wanted" list instead of a queue.
//!
//! The interface publishes what it currently needs, most urgent first. Workers always take
//! the first item nobody is working on, so work that is no longer needed is never started.

use std::collections::HashSet;
use std::hash::Hash;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};

use parking_lot::{Condvar, Mutex};

use crate::repaint::Repaint;

struct State<K> {
    wanted: Vec<K>,
    in_flight: HashSet<K>,
    closed: bool,
}

struct Shared<K> {
    state: Mutex<State<K>>,
    wake: Condvar,
}

pub struct WantedPool<K, R> {
    shared: Arc<Shared<K>>,
    results: Receiver<(K, R)>,
}

impl<K, R> WantedPool<K, R>
where
    K: Clone + Eq + Hash + Send + 'static,
    R: Send + 'static,
{
    pub fn new(
        threads: usize,
        repaint: Repaint,
        work: impl Fn(&K) -> R + Send + Sync + 'static,
    ) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                wanted: Vec::new(),
                in_flight: HashSet::new(),
                closed: false,
            }),
            wake: Condvar::new(),
        });
        let (result_tx, result_rx) = mpsc::channel();
        let work = Arc::new(work);
        for _ in 0..threads.max(1) {
            let shared = Arc::clone(&shared);
            let result_tx = result_tx.clone();
            let repaint = repaint.clone();
            let work = Arc::clone(&work);
            std::thread::spawn(move || {
                while let Some(key) = next_key(&shared) {
                    let result = work(&key);
                    if result_tx.send((key, result)).is_err() {
                        return;
                    }
                    repaint.request();
                }
            });
        }
        Self {
            shared,
            results: result_rx,
        }
    }

    /// Replaces the wanted list. Items already being worked on are not started twice.
    pub fn set_wanted(&self, wanted: Vec<K>) {
        let mut state = self.shared.state.lock();
        state.wanted = wanted;
        drop(state);
        self.shared.wake.notify_all();
    }

    /// Returns finished work. A finished item leaves the wanted list.
    pub fn poll(&mut self) -> Vec<(K, R)> {
        let finished: Vec<(K, R)> = self.results.try_iter().collect();
        if !finished.is_empty() {
            let mut state = self.shared.state.lock();
            for (key, _) in &finished {
                state.in_flight.remove(key);
                state.wanted.retain(|wanted| wanted != key);
            }
        }
        finished
    }

    pub fn is_idle(&self) -> bool {
        let state = self.shared.state.lock();
        state.wanted.is_empty() && state.in_flight.is_empty()
    }
}

impl<K, R> Drop for WantedPool<K, R> {
    fn drop(&mut self) {
        self.shared.state.lock().closed = true;
        self.shared.wake.notify_all();
    }
}

fn next_key<K: Clone + Eq + Hash>(shared: &Shared<K>) -> Option<K> {
    let mut state = shared.state.lock();
    loop {
        if state.closed {
            return None;
        }
        let next = state
            .wanted
            .iter()
            .find(|key| !state.in_flight.contains(*key))
            .cloned();
        if let Some(key) = next {
            state.in_flight.insert(key.clone());
            return Some(key);
        }
        shared.wake.wait(&mut state);
    }
}

/// Runs `work`, turning a panic (for example from a decoder hitting a corrupt file) into an error.
pub fn guarded<T>(work: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    catch_unwind(AssertUnwindSafe(work))
        .unwrap_or_else(|_| Err("decoder failed on this file".to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn wait_for<K, R>(pool: &mut WantedPool<K, R>, count: usize) -> Vec<(K, R)>
    where
        K: Clone + Eq + Hash + Send + 'static,
        R: Send + 'static,
    {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut all = Vec::new();
        while all.len() < count && Instant::now() < deadline {
            all.extend(pool.poll());
            std::thread::yield_now();
        }
        all
    }

    #[test]
    fn each_wanted_item_is_worked_exactly_once() {
        let mut pool = WantedPool::new(4, Repaint::default(), |key: &u32| key * 2);
        pool.set_wanted((0..50).collect());
        let mut results = wait_for(&mut pool, 50);
        results.sort_unstable();
        let expected: Vec<(u32, u32)> = (0..50).map(|k| (k, k * 2)).collect();
        assert_eq!(results, expected);
        assert!(pool.is_idle());
    }

    #[test]
    fn replaced_wanted_list_drops_unstarted_work() {
        let mut pool = WantedPool::new(1, Repaint::default(), |key: &u32| *key);
        pool.set_wanted(Vec::new());
        assert!(pool.is_idle());
        pool.set_wanted(vec![7]);
        assert_eq!(wait_for(&mut pool, 1), vec![(7, 7)]);
    }

    #[test]
    fn guarded_reports_panics_as_errors() {
        let result: Result<(), String> = guarded(|| panic!("boom"));
        assert!(result.is_err());
        assert_eq!(guarded(|| Ok(3)), Ok(3));
    }
}

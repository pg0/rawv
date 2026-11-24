//! Lets background threads wake the interface, including ones started before the window exists.

use std::sync::{Arc, OnceLock};

#[derive(Clone, Default)]
pub struct Repaint(Arc<OnceLock<egui::Context>>);

impl Repaint {
    pub fn attach(&self, ctx: &egui::Context) {
        if self.0.set(ctx.clone()).is_ok() {
            ctx.request_repaint();
        }
    }

    pub fn request(&self) {
        if let Some(ctx) = self.0.get() {
            ctx.request_repaint();
        }
    }
}

//! Single-image view: fit, zoom, pan and stepping through a folder.

pub mod info;
pub mod loader;

use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{Color32, Event, MouseWheelUnit, Pos2, Rect, Sense, Stroke, TextureId, Ui, Vec2, vec2};

use crate::command::Command;
use crate::config::WheelAction;
use crate::draw;
use crate::library::listing::ImageEntry;
use crate::media::formats::Kind;
use crate::media::load::{RawMode, Rendition, Source};
use crate::media::orientation::Orientation;
use crate::media::thumb::ContentRect;
use crate::orientations::Orientations;
use crate::theme;
use crate::thumbs::service::ThumbService;
use loader::{FullLoader, LoadKey, LoadState, Loaded};

const MAX_ZOOM: f32 = 16.0;
const MIN_ZOOM: f32 = 0.02;
const ZOOM_STEP: f32 = 1.25;
const EDGE_ZONE: f32 = 96.0;
const INFO_HEIGHT: f32 = 56.0;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Zoom {
    Fit,
    /// Screen pixels per image pixel.
    Factor(f32),
}

pub struct Viewer {
    pub images: Arc<[ImageEntry]>,
    pub index: usize,
    /// Gallery roll the image list came from.
    pub roll: Option<usize>,
    pub raw_mode: RawMode,
    /// Set while a single file is shown and its folder listing has not arrived yet.
    pub awaits_folder: bool,
    zoom: Zoom,
    pan: Vec2,
    direction: isize,
    wheel: f32,
    /// Zoom factor used in the last frame, the base for relative zoom commands.
    shown_factor: f32,
    pointer_anchor: Vec2,
}

pub struct ViewerContext<'a> {
    pub loader: &'a FullLoader,
    pub thumbs: &'a ThumbService,
    pub orientations: &'a Orientations,
    pub show_info: bool,
    pub wheel: WheelAction,
}

struct Picture {
    texture: TextureId,
    /// Oriented size in image pixels.
    size: Vec2,
    orientation: Orientation,
    content: ContentRect,
    /// False while a thumbnail or camera preview stands in for the image.
    full: bool,
}

impl Viewer {
    pub fn new(
        images: Arc<[ImageEntry]>,
        index: usize,
        roll: Option<usize>,
        raw_mode: RawMode,
    ) -> Self {
        Self {
            index: index.min(images.len().saturating_sub(1)),
            images,
            roll,
            raw_mode,
            awaits_folder: false,
            zoom: Zoom::Fit,
            pan: Vec2::ZERO,
            direction: 1,
            wheel: 0.0,
            shown_factor: 1.0,
            pointer_anchor: Vec2::ZERO,
        }
    }

    pub fn current(&self) -> Option<&ImageEntry> {
        self.images.get(self.index)
    }

    /// What to load for `entry`, in loading order. The last one is the picture to end up
    /// with; one before it stands in until that is ready. `preview_size` tells the size of
    /// a loaded camera preview.
    fn renditions(
        &self,
        entry: &ImageEntry,
        is_current: bool,
        preview_size: &dyn Fn(&LoadKey) -> Option<[u32; 2]>,
    ) -> Vec<LoadKey> {
        let preview = LoadKey::new(entry, Rendition::Preview);
        if entry.kind != Kind::Raw {
            return vec![preview];
        }
        let developed = LoadKey::new(entry, Rendition::Developed);
        match self.raw_mode {
            RawMode::Embedded => vec![preview],
            RawMode::Developed if is_current => vec![preview, developed],
            RawMode::Developed => vec![developed],
            // Whether to develop is known once the preview is there to be measured.
            RawMode::Auto => match preview_size(&preview) {
                Some(size) if self.raw_mode.develops(Some(size)) => vec![preview, developed],
                _ => vec![preview],
            },
        }
    }

    /// The loaded picture of the current image, and whether it is the one to end up with.
    pub fn shown<'a>(&self, loader: &'a FullLoader) -> Option<(&'a Loaded, bool)> {
        let keys = self.renditions(self.current()?, true, &preview_size_in(loader));
        let wanted_failed = keys
            .last()
            .is_some_and(|key| matches!(loader.state(key), LoadState::Failed(_)));
        let last = keys.len().checked_sub(1)?;
        keys.iter()
            .enumerate()
            .rev()
            .find_map(|(index, key)| match loader.state(key) {
                LoadState::Ready(loaded) => Some((loaded, index == last || wanted_failed)),
                _ => None,
            })
    }

    /// Load state of the picture the current image ends up with.
    pub fn final_state<'a>(&self, loader: &'a FullLoader) -> LoadState<'a> {
        if let Some((loaded, true)) = self.shown(loader) {
            return LoadState::Ready(loaded);
        }
        self.current()
            .and_then(|entry| self.renditions(entry, true, &preview_size_in(loader)).pop())
            .map_or(LoadState::Loading, |key| match loader.state(&key) {
                LoadState::Failed(message) => LoadState::Failed(message),
                _ => LoadState::Loading,
            })
    }

    /// Replaces the image list, keeping the current image selected.
    pub fn adopt(&mut self, images: Arc<[ImageEntry]>, roll: usize) {
        let current = self.current().map(|entry| entry.id.path.clone());
        if let Some(index) = images
            .iter()
            .position(|e| Some(&e.id.path) == current.as_ref())
        {
            self.images = images;
            self.index = index;
            self.roll = Some(roll);
        }
        self.awaits_folder = false;
    }

    /// Continues in another roll: at its first image when moving forward, else at its last.
    pub fn enter_roll(&mut self, images: Arc<[ImageEntry]>, roll: usize, direction: isize) {
        self.direction = if direction < 0 { -1 } else { 1 };
        self.index = if direction < 0 {
            images.len().saturating_sub(1)
        } else {
            0
        };
        self.images = images;
        self.roll = Some(roll);
        self.zoom_fit();
    }

    /// Takes over the list after the image at `removed` was taken out of it. The image
    /// that followed the removed one is shown next.
    pub fn after_removal(&mut self, images: Arc<[ImageEntry]>, removed: usize) {
        if removed < self.index {
            self.index -= 1;
        }
        self.index = self.index.min(images.len().saturating_sub(1));
        self.images = images;
        self.zoom_fit();
    }

    /// Whether `step(delta)` would leave the image list.
    pub fn at_edge(&self, delta: isize) -> bool {
        if delta < 0 {
            self.index == 0
        } else {
            self.index + 1 >= self.images.len()
        }
    }

    pub fn step(&mut self, delta: isize) {
        let last = self.images.len().saturating_sub(1) as isize;
        let target = (self.index as isize + delta).clamp(0, last) as usize;
        self.direction = if delta < 0 { -1 } else { 1 };
        self.go_to(target);
    }

    pub fn go_to(&mut self, index: usize) {
        let index = index.min(self.images.len().saturating_sub(1));
        if index != self.index {
            self.index = index;
            self.zoom = Zoom::Fit;
            self.pan = Vec2::ZERO;
        }
    }

    /// Zoom shown in the last frame, in percent of the image's pixel size.
    pub fn zoom_percent(&self) -> f32 {
        self.shown_factor * 100.0
    }

    pub fn pan_offset(&self) -> [f32; 2] {
        [self.pan.x, self.pan.y]
    }

    pub fn zoom_step(&mut self, zoom_in: bool) {
        let factor = if zoom_in { ZOOM_STEP } else { 1.0 / ZOOM_STEP };
        self.zoom_to(self.shown_factor * factor, Vec2::ZERO);
    }

    pub fn zoom_fit(&mut self) {
        self.zoom = Zoom::Fit;
        self.pan = Vec2::ZERO;
    }

    /// Shows the image at one screen pixel per image pixel.
    pub fn zoom_actual(&mut self) {
        self.zoom_to(1.0, self.pointer_anchor);
    }

    /// Switches between 100 % and fit.
    fn toggle_actual(&mut self) {
        if self.zoom == Zoom::Factor(1.0) {
            self.zoom_fit();
        } else {
            self.zoom_actual();
        }
    }

    /// Zooms while keeping the image point under `anchor` (relative to the view centre) in place.
    fn zoom_to(&mut self, factor: f32, anchor: Vec2) {
        let factor = factor.clamp(MIN_ZOOM, MAX_ZOOM);
        let ratio = factor / self.shown_factor.max(f32::EPSILON);
        self.pan = anchor - (anchor - self.pan) * ratio;
        self.zoom = Zoom::Factor(factor);
        self.shown_factor = factor;
    }

    /// The images to keep loaded: the current one, then its neighbours in travel direction.
    pub fn wanted(&self, loader: &FullLoader) -> Vec<LoadKey> {
        self.wanted_given(&preview_size_in(loader))
    }

    fn wanted_given(&self, preview_size: &dyn Fn(&LoadKey) -> Option<[u32; 2]>) -> Vec<LoadKey> {
        let offsets = [0, self.direction, -self.direction, 2 * self.direction];
        let mut keys: Vec<LoadKey> = Vec::new();
        for offset in offsets {
            let index = self.index as isize + offset;
            let Some(entry) = usize::try_from(index).ok().and_then(|i| self.images.get(i)) else {
                continue;
            };
            for key in self.renditions(entry, offset == 0, preview_size) {
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
        }
        keys
    }

    pub fn ui(&mut self, ui: &mut Ui, cx: &ViewerContext<'_>) -> Vec<Command> {
        let mut commands = Vec::new();
        let rect = ui.max_rect();
        let response = ui.interact(rect, ui.scope_id().with("viewer"), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, theme::VIEWER_BACKGROUND);

        let Some(entry) = self.current().cloned() else {
            return commands;
        };
        let state = self.final_state(cx.loader);
        let picture = self.picture(&entry, cx);

        let pixels_per_point = ui.ctx().pixels_per_point();
        if let Some(picture) = &picture {
            let fit = (rect.width() * pixels_per_point / picture.size.x)
                .min(rect.height() * pixels_per_point / picture.size.y);
            let factor = match (picture.full, self.zoom) {
                (true, Zoom::Factor(factor)) => factor,
                (true, Zoom::Fit) => fit.min(1.0),
                (false, _) => fit,
            };
            if picture.full {
                self.shown_factor = factor;
            }
            let size = picture.size * factor / pixels_per_point;
            if response.dragged() {
                self.pan += response.drag_delta();
            }
            let slack = ((size - rect.size()) / 2.0).max(Vec2::ZERO);
            self.pan = self.pan.clamp(-slack, slack);
            let target = Rect::from_center_size(rect.center() + self.pan, size);
            draw::image(
                &painter,
                picture.texture,
                target,
                picture.orientation,
                picture.content,
                draw::WHOLE,
            );
        } else if let LoadState::Failed(message) = &state {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("Cannot open {}\n{message}", entry.name),
                theme::body(14.0),
                theme::MUTED,
            );
        }

        let pointer = response.hover_pos();
        if let Some(pointer) = pointer {
            self.pointer_anchor = pointer - rect.center();
        }
        if response.double_clicked() {
            commands.push(Command::ToggleFullscreen);
        }
        if response.middle_clicked() {
            self.toggle_actual();
        }
        if response.hovered() {
            self.handle_wheel(ui, cx.wheel, &mut commands);
        }

        if cx.show_info {
            self.paint_info(ui, rect, &entry, &state);
        }
        commands.extend(self.edge_buttons(ui, rect, pointer));
        commands
    }

    fn picture(&self, entry: &ImageEntry, cx: &ViewerContext<'_>) -> Option<Picture> {
        if let Some((loaded, wanted)) = self.shown(cx.loader) {
            let orientation = cx
                .orientations
                .effective(&entry.id.path, loaded.orientation);
            let size = draw::shown_size(loaded.original_size, orientation, ContentRect::FULL);
            return Some(Picture {
                texture: loaded.texture.id(),
                size: Vec2::from(size),
                orientation,
                content: ContentRect::FULL,
                full: wanted,
            });
        }
        let thumb = cx.thumbs.peek(&entry.id)?;
        let orientation = cx.orientations.effective(&entry.id.path, thumb.orientation);
        let size = draw::shown_size(thumb.size, orientation, thumb.content);
        Some(Picture {
            texture: thumb.texture.id(),
            size: Vec2::from(size),
            orientation,
            content: thumb.content,
            full: false,
        })
    }

    /// The wheel zooms around the pointer or steps through images, as `action` says;
    /// Ctrl+wheel does the other. A pinch always zooms.
    fn handle_wheel(&mut self, ui: &Ui, action: WheelAction, commands: &mut Vec<Command>) {
        let (plain, with_ctrl, pinch) = ui.input(|input| {
            let (mut plain, mut with_ctrl, mut pinch) = (0.0, 0.0, 1.0);
            for event in &input.events {
                match event {
                    Event::MouseWheel {
                        unit,
                        delta,
                        modifiers,
                        ..
                    } => {
                        let lines = match unit {
                            MouseWheelUnit::Line => delta.y,
                            MouseWheelUnit::Point => delta.y / 50.0,
                            MouseWheelUnit::Page => delta.y * 10.0,
                        };
                        if modifiers.command {
                            with_ctrl += lines;
                        } else {
                            plain += lines;
                        }
                    }
                    Event::Zoom(factor) => pinch *= factor,
                    _ => {}
                }
            }
            (plain, with_ctrl, pinch)
        });
        let (zoom_lines, step_lines) = match action {
            WheelAction::Zoom => (plain, with_ctrl),
            WheelAction::Step => (with_ctrl, plain),
        };
        let zoom = pinch * ZOOM_STEP.powf(zoom_lines);
        if zoom != 1.0 {
            self.zoom_to(self.shown_factor * zoom, self.pointer_anchor);
        }
        self.wheel += step_lines;
        while self.wheel >= 1.0 {
            commands.push(Command::Prev);
            self.wheel -= 1.0;
        }
        while self.wheel <= -1.0 {
            commands.push(Command::Next);
            self.wheel += 1.0;
        }
    }

    fn paint_info(&self, ui: &Ui, rect: Rect, entry: &ImageEntry, state: &LoadState<'_>) {
        let painter = ui.painter_at(rect);
        let bar = Rect::from_min_max(
            egui::pos2(rect.left(), rect.bottom() - INFO_HEIGHT),
            rect.right_bottom(),
        );
        let mut gradient = egui::Mesh::default();
        let clear = Color32::TRANSPARENT;
        let dark = Color32::from_black_alpha(215);
        gradient.colored_vertex(bar.left_top(), clear);
        gradient.colored_vertex(bar.right_top(), clear);
        gradient.colored_vertex(bar.right_bottom(), dark);
        gradient.colored_vertex(bar.left_bottom(), dark);
        gradient.add_triangle(0, 1, 2);
        gradient.add_triangle(0, 2, 3);
        painter.add(egui::Shape::mesh(gradient));

        let mut pieces: Vec<String> = Vec::new();
        if let Some(folder) = entry.id.path.parent().and_then(|p| p.file_name()) {
            pieces.push(folder.to_string_lossy().into_owned());
        }
        pieces.push(info::file_size(entry.id.size));
        match state {
            LoadState::Loading => pieces.push("loading\u{2026}".to_owned()),
            LoadState::Failed(_) => pieces.push("cannot open".to_owned()),
            LoadState::Ready(loaded) => {
                pieces.push(format!(
                    "{}\u{d7}{}",
                    loaded.original_size[0], loaded.original_size[1]
                ));
                pieces.extend(
                    info::source_label(loaded.source, loaded.original_size).map(str::to_owned),
                );
                pieces.extend(info::shot_pieces(&loaded.shot));
                pieces.push(format!("{:.0}%", self.zoom_percent()));
                pieces.push(format!("{:.0} ms", loaded.load_ms));
            }
        }
        pieces.push(format!("{} / {}", self.index + 1, self.images.len()));

        let mut job = LayoutJob::default();
        let font = theme::mono(12.0);
        job.append(
            &entry.name,
            0.0,
            TextFormat::simple(font.clone(), theme::INK_BRIGHT),
        );
        for piece in pieces {
            job.append(
                &format!(" \u{b7} {piece}"),
                0.0,
                TextFormat::simple(font.clone(), Color32::from_gray(0xdd)),
            );
        }
        job.wrap = TextWrapping {
            max_width: rect.width() - 32.0,
            max_rows: 1,
            break_anywhere: true,
            overflow_character: Some('\u{2026}'),
        };
        let galley = painter.layout_job(job);
        let position = egui::pos2(rect.left() + 16.0, rect.bottom() - 12.0 - galley.size().y);
        painter.galley(position, galley, theme::INK);
    }

    /// Previous, next and close buttons, shown while the pointer is near an edge.
    fn edge_buttons(&self, ui: &Ui, rect: Rect, pointer: Option<Pos2>) -> Vec<Command> {
        let mut commands = Vec::new();
        let painter = ui.painter_at(rect);
        let near_left = pointer.is_some_and(|p| p.x < rect.left() + EDGE_ZONE);
        let near_right = pointer.is_some_and(|p| p.x > rect.right() - EDGE_ZONE);
        let near_top = pointer.is_some_and(|p| p.y < rect.top() + EDGE_ZONE);

        let buttons = [
            (
                near_left && self.index > 0,
                egui::pos2(rect.left() + 35.0, rect.center().y),
                Command::Prev,
                "prev",
            ),
            (
                near_right && self.index + 1 < self.images.len(),
                egui::pos2(rect.right() - 35.0, rect.center().y),
                Command::Next,
                "next",
            ),
            (
                near_top && near_right,
                egui::pos2(rect.right() - 31.0, rect.top() + 31.0),
                Command::Escape,
                "close",
            ),
        ];
        for (visible, center, command, name) in buttons {
            if !visible {
                continue;
            }
            let area = Rect::from_center_size(center, Vec2::splat(42.0));
            let response = ui.interact(area, ui.scope_id().with(name), Sense::click());
            let fill = if response.hovered() {
                Color32::from_black_alpha(220)
            } else {
                theme::OVERLAY
            };
            painter.circle(center, 21.0, fill, Stroke::new(1.0, theme::OVERLAY_BORDER));
            let stroke = Stroke::new(1.6, theme::INK);
            let arm = 6.0;
            match command {
                Command::Prev => {
                    painter
                        .line_segment([center + vec2(3.0, -arm), center + vec2(-3.0, 0.0)], stroke);
                    painter
                        .line_segment([center + vec2(-3.0, 0.0), center + vec2(3.0, arm)], stroke);
                }
                Command::Next => {
                    painter
                        .line_segment([center + vec2(-3.0, -arm), center + vec2(3.0, 0.0)], stroke);
                    painter
                        .line_segment([center + vec2(3.0, 0.0), center + vec2(-3.0, arm)], stroke);
                }
                _ => {
                    painter
                        .line_segment([center + vec2(-5.0, -5.0), center + vec2(5.0, 5.0)], stroke);
                    painter
                        .line_segment([center + vec2(-5.0, 5.0), center + vec2(5.0, -5.0)], stroke);
                }
            }
            if response.clicked() {
                commands.push(command);
            }
        }
        commands
    }
}

/// Sizes of the camera previews `loader` holds.
fn preview_size_in(loader: &FullLoader) -> impl Fn(&LoadKey) -> Option<[u32; 2]> + '_ {
    |key| match loader.state(key) {
        LoadState::Ready(loaded) if loaded.source == Source::RawPreview => {
            Some(loaded.original_size)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::listing::FileId;
    use std::path::PathBuf;
    use std::time::SystemTime;

    fn entries(count: usize) -> Arc<[ImageEntry]> {
        (0..count)
            .map(|i| ImageEntry {
                id: FileId {
                    path: PathBuf::from(format!("{i}.jpg")),
                    modified_nanos: 0,
                    size: 1,
                },
                name: format!("{i}.jpg"),
                kind: Kind::Jpeg,
                modified: SystemTime::UNIX_EPOCH,
            })
            .collect()
    }

    fn paths(keys: &[LoadKey]) -> Vec<String> {
        keys.iter()
            .map(|key| key.id.path.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn stepping_clamps_at_both_ends() {
        let mut viewer = Viewer::new(entries(3), 0, None, RawMode::default());
        viewer.step(-1);
        assert_eq!(viewer.index, 0);
        viewer.step(5);
        assert_eq!(viewer.index, 2);
        viewer.step(1);
        assert_eq!(viewer.index, 2);
    }

    #[test]
    fn wanted_prefers_current_then_travel_direction() {
        let mut viewer = Viewer::new(entries(10), 4, None, RawMode::default());
        assert_eq!(
            paths(&viewer.wanted_given(&|_| None)),
            ["4.jpg", "5.jpg", "3.jpg", "6.jpg"]
        );
        viewer.step(-1);
        assert_eq!(
            paths(&viewer.wanted_given(&|_| None)),
            ["3.jpg", "2.jpg", "4.jpg", "1.jpg"]
        );
        let single = Viewer::new(entries(1), 0, None, RawMode::default());
        assert_eq!(paths(&single.wanted_given(&|_| None)), ["0.jpg"]);
    }

    fn raw_entries(count: usize) -> Arc<[ImageEntry]> {
        entries(count)
            .iter()
            .cloned()
            .map(|entry| ImageEntry {
                kind: Kind::Raw,
                ..entry
            })
            .collect()
    }

    fn renditions(keys: &[LoadKey]) -> Vec<(String, Rendition)> {
        paths(keys)
            .into_iter()
            .zip(keys.iter().map(|key| key.rendition))
            .collect()
    }

    #[test]
    fn automatic_raw_mode_develops_once_the_preview_turns_out_small() {
        use Rendition::{Developed, Preview};
        let name = |index: usize| format!("{index}.jpg");
        let viewer = Viewer::new(raw_entries(2), 0, None, RawMode::Auto);
        assert_eq!(
            renditions(&viewer.wanted_given(&|_| None)),
            [(name(0), Preview), (name(1), Preview)]
        );
        let small_first = |key: &LoadKey| (key.id.path == *"0.jpg").then_some([1616, 1080]);
        assert_eq!(
            renditions(&viewer.wanted_given(&small_first)),
            [(name(0), Preview), (name(0), Developed), (name(1), Preview)]
        );
        let large = |_: &LoadKey| Some([4240, 2384]);
        assert_eq!(
            renditions(&viewer.wanted_given(&large)),
            [(name(0), Preview), (name(1), Preview)]
        );
    }

    #[test]
    fn fixed_raw_modes_do_not_depend_on_the_preview() {
        use Rendition::{Developed, Preview};
        let name = |index: usize| format!("{index}.jpg");
        let preview_only = Viewer::new(raw_entries(2), 0, None, RawMode::Embedded);
        assert_eq!(
            renditions(&preview_only.wanted_given(&|_| Some([1616, 1080]))),
            [(name(0), Preview), (name(1), Preview)]
        );
        // The current image gets its preview first, to show while the sensor data develops.
        let developed = Viewer::new(raw_entries(2), 0, None, RawMode::Developed);
        assert_eq!(
            renditions(&developed.wanted_given(&|_| None)),
            [
                (name(0), Preview),
                (name(0), Developed),
                (name(1), Developed)
            ]
        );
    }

    #[test]
    fn zoom_keeps_anchor_point_fixed_and_resets_on_step() {
        let mut viewer = Viewer::new(entries(3), 0, None, RawMode::default());
        viewer.shown_factor = 0.5;
        let anchor = vec2(100.0, 40.0);
        viewer.zoom_to(1.0, anchor);
        assert_eq!(viewer.zoom, Zoom::Factor(1.0));
        // The point under the anchor was at (anchor - pan) / factor before and after.
        let before = anchor / 0.5;
        let after = (anchor - viewer.pan) / 1.0;
        assert!((before - after).length() < 1e-4);
        viewer.step(1);
        assert_eq!(viewer.zoom, Zoom::Fit);
        assert_eq!(viewer.pan, Vec2::ZERO);
    }

    #[test]
    fn adopt_keeps_the_current_image() {
        let all = entries(5);
        let single: Arc<[ImageEntry]> = Arc::from(vec![all[3].clone()]);
        let mut viewer = Viewer::new(single, 0, None, RawMode::default());
        viewer.awaits_folder = true;
        viewer.adopt(all, 0);
        assert_eq!(viewer.index, 3);
        assert_eq!(viewer.images.len(), 5);
        assert!(!viewer.awaits_folder);
    }
}

//! The application: owns all state, turns input into commands and runs them.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use egui::{
    Align, Align2, Color32, Layout, Rect, RichText, Sense, Stroke, Ui, ViewportCommand, vec2,
};

use crate::command::{self, Command, Direction, Mode};
use crate::config::{self, Config, WheelAction, WindowGeometry};
use crate::filework::{Done, FileWorker, Job, Outcome};
use crate::gallery::{Gallery, GalleryContext, layout};
use crate::library::lister::{ListResult, Lister};
use crate::library::listing::ImageEntry;
use crate::media::developed::DevelopedCache;
use crate::media::pixels::{self, Channels};
use crate::media::rate::RateOutcome;
use crate::media::remove::Removal;
use crate::media::rotate::RotateOutcome;
use crate::media::xmp::{REJECTED, Rating};
use crate::orientations::Orientations;
use crate::ratings::Ratings;
use crate::repaint::Repaint;
use crate::script::{Action, Script};
use crate::theme;
use crate::thumbs::service::ThumbService;
use crate::thumbs::store::ThumbStore;
use crate::viewer::loader::{FullLoader, LoadState};
use crate::viewer::{Viewer, ViewerContext};

const HEADER_HEIGHT: f32 = 44.0;
const TOAST_SECONDS: f64 = 2.2;
const MAX_BREADCRUMBS: usize = 5;
const DRAG_PICTURE_LONG_EDGE: u32 = 160;

pub struct Startup {
    /// Folder or file given on the command line.
    pub open: Option<PathBuf>,
    pub fullscreen: bool,
    pub script: Option<Script>,
    /// Print start-up timings to stderr.
    pub timing: bool,
    pub process_start: Instant,
}

struct Toast {
    text: String,
    /// Shown until this time; set when the toast is first drawn.
    until: Option<f64>,
}

struct Timing {
    process_start: Instant,
    first_frame_reported: bool,
    content_reported: bool,
}

pub struct App {
    config: Config,
    repaint: Repaint,
    thumbs: ThumbService,
    lister: Lister,
    loader: FullLoader,
    developed: DevelopedCache,
    files: FileWorker,
    /// For the picture that follows the pointer while an image is dragged out.
    thumb_store: ThumbStore,
    /// The image to drag out of the window once this frame's commands have run.
    drag_out: Option<ImageEntry>,
    /// What was last given to Explorer, and which file.
    handed_over: Option<(String, PathBuf)>,
    /// The file last dragged out. Dropping it back onto this window does nothing.
    dragged_out: Option<PathBuf>,
    /// Where the pointer was when the system took the mouse for a drag. The release of
    /// the button never reaches the interface and is made up for from here.
    pointer_taken: Option<egui::Pos2>,
    orientations: Orientations,
    ratings: Ratings,
    gallery: Gallery,
    viewer: Option<Viewer>,
    fullscreen: bool,
    help: bool,
    settings: bool,
    /// The image the user is asked about deleting for good.
    confirm_remove: Option<ImageEntry>,
    toast: Option<Toast>,
    script: Option<Script>,
    timing: Option<Timing>,
    window: Option<WindowGeometry>,
    /// Window title last sent to the system.
    title: String,
}

impl App {
    /// Builds the app and starts loading before the window exists.
    pub fn new(startup: Startup) -> Self {
        let config = Config::load();
        let repaint = Repaint::default();
        let store = ThumbStore::open(&config::thumb_store_file());
        let developed = DevelopedCache::open(
            &config::developed_dir(),
            config::raw_cache_bytes(config.raw_cache_gb),
        );
        let mut app = Self {
            thumbs: ThumbService::new(store.clone(), repaint.clone()),
            lister: Lister::new(repaint.clone()),
            loader: FullLoader::new(repaint.clone(), developed.clone()),
            developed,
            files: FileWorker::new(store.clone(), repaint.clone()),
            thumb_store: store,
            drag_out: None,
            handed_over: None,
            dragged_out: None,
            pointer_taken: None,
            orientations: Orientations::default(),
            ratings: Ratings::default(),
            gallery: Gallery::new(config.sort, config.thumb_scale, config.nested),
            viewer: None,
            fullscreen: startup.fullscreen,
            help: false,
            settings: false,
            confirm_remove: None,
            toast: None,
            script: startup.script,
            timing: startup.timing.then_some(Timing {
                process_start: startup.process_start,
                first_frame_reported: false,
                content_reported: false,
            }),
            window: config.window,
            title: String::new(),
            repaint,
            config,
        };
        let initial = startup.open.or_else(|| app.config.last_folder.clone());
        if let Some(path) = initial {
            app.open(&path);
            app.publish_wanted();
        }
        app
    }

    pub fn window_geometry(&self) -> Option<WindowGeometry> {
        self.window
    }

    pub fn starts_fullscreen(&self) -> bool {
        self.fullscreen
    }

    /// Connects background threads to the window so they can wake it.
    pub fn attach(&self, ctx: &egui::Context) {
        theme::apply(ctx);
        self.repaint.attach(ctx);
    }

    fn mode(&self) -> Mode {
        if self.confirm_remove.is_some() {
            Mode::Confirm
        } else if self.viewer.is_some() {
            Mode::Viewer
        } else {
            Mode::Gallery
        }
    }

    fn is_idle(&self) -> bool {
        self.lister.is_idle()
            && self.thumbs.is_idle()
            && self.loader.is_idle()
            && self.developed.is_idle()
            && self.thumb_store.is_idle()
            && self.files.is_idle()
    }

    fn say(&mut self, text: impl Into<String>) {
        self.toast = Some(Toast {
            text: text.into(),
            until: None,
        });
    }

    fn open(&mut self, path: &Path) {
        if path.is_dir() {
            self.viewer = None;
            self.gallery.open(path, &mut self.lister);
            self.config.last_folder = Some(path.to_path_buf());
            return;
        }
        let entry = match ImageEntry::from_path(path) {
            Ok(entry) => entry,
            Err(error) => {
                self.say(format!("Cannot open {}: {error}", path.display()));
                return;
            }
        };
        let Some(folder) = path.parent() else { return };
        if self.gallery.folder.as_deref() == Some(folder)
            && let Some(images) = self.gallery.images_of(0)
            && let Some(index) = images.iter().position(|e| e.id.path == entry.id.path)
        {
            self.viewer = Some(Viewer::new(images, index, Some(0), self.config.raw));
            return;
        }
        self.gallery.open(folder, &mut self.lister);
        self.config.last_folder = Some(folder.to_path_buf());
        let mut viewer = Viewer::new(Arc::from(vec![entry]), 0, None, self.config.raw);
        viewer.awaits_folder = true;
        self.viewer = Some(viewer);
    }

    fn on_listing(&mut self, result: ListResult) {
        let Some(listed) = self.gallery.on_listing(result, &mut self.lister) else {
            return;
        };
        if let Some(viewer) = &mut self.viewer
            && let Some(roll) = &mut viewer.roll
            && *roll > listed.roll
        {
            *roll += listed.inserted;
        }
        if listed.roll == 0
            && let Some(viewer) = self.viewer.as_mut().filter(|v| v.awaits_folder)
            && let Some(images) = self.gallery.images_of(0)
        {
            viewer.adopt(images, 0);
        }
    }

    fn close_viewer(&mut self, ctx: &egui::Context) {
        if let Some(viewer) = self.viewer.take()
            && let Some(roll) = viewer.roll
        {
            self.gallery.select(roll, viewer.index);
        }
        self.set_fullscreen(ctx, false);
    }

    fn set_fullscreen(&mut self, ctx: &egui::Context, fullscreen: bool) {
        if self.fullscreen == fullscreen {
            return;
        }
        self.fullscreen = fullscreen;
        ctx.send_viewport_cmd(ViewportCommand::Fullscreen(fullscreen));
        // The system puts a window that was snapped to a screen edge back where it was
        // before the snap, so the place it had when it went fullscreen is set again.
        if !fullscreen && let Some(window) = self.window.filter(|window| !window.maximized) {
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(window.size.into()));
            if let Some(position) = window.position {
                ctx.send_viewport_cmd(ViewportCommand::OuterPosition(position.into()));
            }
        }
    }

    /// Rotates the viewed or selected image clockwise by `quarter_turns`.
    fn rotate(&mut self, quarter_turns: u8) {
        let Some(entry) = self.target_entry() else {
            self.say("Select an image to rotate");
            return;
        };
        let from_loaded = self
            .viewer
            .as_ref()
            .and_then(|viewer| viewer.shown(&self.loader))
            .map(|(loaded, _)| loaded.orientation);
        let from_file =
            from_loaded.or_else(|| self.thumbs.peek(&entry.id).map(|thumb| thumb.orientation));
        let Some(from_file) = from_file else {
            self.say("Image is still loading");
            return;
        };
        let rotated = self
            .orientations
            .effective(&entry.id.path, from_file)
            .rotated_cw(quarter_turns);
        self.orientations.set(entry.id.path.clone(), rotated);
        self.files.submit(entry, Job::Rotate(rotated));
    }

    /// Hands `entry`'s file to the system as a drag in progress, so it can be dropped into
    /// another program. Returns when the user has dropped it or given up.
    fn start_drag_out(&mut self, frame: &eframe::Frame, ctx: &egui::Context, entry: &ImageEntry) {
        self.dragged_out = Some(entry.id.path.clone());
        // A scripted run has no mouse button held: the system would drop the file at once,
        // wherever the real pointer happens to be.
        if self.script.is_some() {
            return;
        }
        let pointer = ctx.pointer_latest_pos().unwrap_or_default();
        let picture = self
            .thumb_store
            .get(&entry.id.cache_key())
            .and_then(|stored| drag_picture(&stored.jpeg))
            .unwrap_or_default();
        let started = drag::start_drag(
            frame,
            drag::DragItem::Files(vec![entry.id.path.clone()]),
            drag::Image::Raw(picture),
            |_, _| {},
            drag::Options::default(),
        );
        self.pointer_taken = Some(pointer);
        if let Err(error) = started {
            self.say(format!("Cannot drag {}: {error}", entry.name));
        }
    }

    /// Starts Explorer with `options` and the viewed or selected image's path. Without
    /// options Explorer opens the file in the program set for its type.
    fn hand_to_explorer(&mut self, done: &str, options: &[&str]) {
        let Some(entry) = self.target_entry() else {
            self.say("Select an image first");
            return;
        };
        self.handed_over = Some((done.to_owned(), entry.id.path.clone()));
        // A scripted run must not open windows on the screen of whoever sits at the computer.
        if self.script.is_some() {
            return;
        }
        let started = std::process::Command::new("explorer.exe")
            .args(options)
            .arg(&entry.id.path)
            .spawn();
        match started {
            Ok(_) => self.say(format!("{} \u{b7} {}", entry.name, done.to_lowercase())),
            Err(error) => self.say(format!("Cannot start Explorer: {error}")),
        }
    }

    /// The image a command about "this image" means: the viewed one, else the selected one.
    fn target_entry(&self) -> Option<ImageEntry> {
        match &self.viewer {
            Some(viewer) => viewer.current().cloned(),
            None => self.gallery.selected_entry().cloned(),
        }
    }

    /// The rating to show for `entry`: the one given in this session, else the file's.
    fn rating_of(&self, entry: &ImageEntry) -> Rating {
        let from_viewer = self
            .viewer
            .as_ref()
            .filter(|viewer| {
                viewer
                    .current()
                    .is_some_and(|current| current.id == entry.id)
            })
            .and_then(|viewer| viewer.shown(&self.loader))
            .map(|(loaded, _)| loaded.rating);
        let from_file = from_viewer
            .or_else(|| self.thumbs.peek(&entry.id).map(|thumb| thumb.rating))
            .unwrap_or(0);
        self.ratings.effective(&entry.id.path, from_file)
    }

    /// Rates the viewed or selected image. The mark shows at once; the file follows.
    fn rate(&mut self, rating: Rating) {
        let Some(entry) = self.target_entry() else {
            self.say("Select an image to rate");
            return;
        };
        self.ratings.set(entry.id.path.clone(), rating);
        self.files.submit(entry, Job::Rate(rating));
    }

    fn toggle_reject(&mut self) {
        let rejected = self
            .target_entry()
            .is_some_and(|entry| self.rating_of(&entry) == REJECTED);
        self.rate(if rejected { 0 } else { REJECTED });
    }

    fn remove(&mut self) {
        match self.target_entry() {
            Some(entry) => self.files.submit(entry, Job::Remove { permanently: false }),
            None => self.say("Select an image to delete"),
        }
    }

    fn on_file_done(&mut self, done: Done, ctx: &egui::Context) {
        let name = &done.entry.name;
        match (done.job, done.result) {
            (_, Ok(Outcome::Rotated(RotateOutcome::Saved))) => {
                self.say("Rotated \u{b7} saved in file");
            }
            (_, Ok(Outcome::Rotated(RotateOutcome::ViewOnly))) => {
                self.say("Rotated for this session \u{b7} file has no EXIF orientation");
            }
            (_, Ok(Outcome::Rated(RateOutcome::Saved))) => {}
            (_, Ok(Outcome::Rated(RateOutcome::Unsupported))) => {
                self.ratings.forget(&done.entry.id.path);
                self.say(format!(
                    "{name} \u{b7} this kind of file cannot hold a rating"
                ));
            }
            (_, Ok(Outcome::Removed(Removal::NeedsConsent))) => {
                self.confirm_remove = Some(done.entry);
            }
            (_, Ok(Outcome::Removed(removal))) => {
                self.say(match removal {
                    Removal::Recycled => format!("{name} \u{b7} moved to the recycle bin"),
                    _ => format!("{name} \u{b7} deleted"),
                });
                self.forget(&done.entry, ctx);
            }
            (Job::Rotate(_), Err(error)) => self.say(format!("Rotation not saved: {error}")),
            (Job::Rate(_), Err(error)) => {
                self.ratings.forget(&done.entry.id.path);
                self.say(format!("Rating not saved: {error}"));
            }
            (Job::Remove { .. }, Err(error)) => self.say(format!("{name} not deleted: {error}")),
        }
    }

    /// Takes a removed image out of the gallery and the viewer.
    fn forget(&mut self, entry: &ImageEntry, ctx: &egui::Context) {
        let removed = self.gallery.remove_image(&entry.id.path);
        let Some(viewer) = &mut self.viewer else {
            return;
        };
        let remaining = removed.and_then(|(roll, index)| {
            let images = self.gallery.images_of(roll)?;
            (viewer.roll == Some(roll) && !images.is_empty()).then_some((images, index))
        });
        let shows_removed = viewer
            .current()
            .is_some_and(|current| current.id.path == entry.id.path);
        match remaining {
            Some((images, index)) => viewer.after_removal(images, index),
            None if shows_removed || removed.is_some_and(|(roll, _)| viewer.roll == Some(roll)) => {
                self.close_viewer(ctx);
            }
            None => {}
        }
    }

    fn run(&mut self, command: Command, ctx: &egui::Context) {
        match command {
            Command::Open(path) => self.open(&path),
            Command::OpenFolderDialog => {
                let mut dialog = rfd::FileDialog::new();
                if let Some(folder) = &self.gallery.folder {
                    dialog = dialog.set_directory(folder);
                }
                if let Some(folder) = dialog.pick_folder() {
                    self.open(&folder);
                }
            }
            Command::GoParent => {
                let parent = self
                    .gallery
                    .folder
                    .as_deref()
                    .and_then(Path::parent)
                    .map(Path::to_path_buf);
                if let Some(parent) = parent {
                    self.open(&parent);
                }
            }
            Command::Refresh => self.gallery.refresh(&mut self.lister),
            Command::Select(direction) => self.gallery.move_selection(direction, 1),
            Command::SelectPage(direction) => {
                self.gallery.move_selection(direction, layout::PAGE_ROWS);
            }
            Command::SelectFirst => self.gallery.select_end(false),
            Command::SelectLast => self.gallery.select_end(true),
            Command::OpenSelected => {
                if let Some((roll, index)) = self.gallery.selection {
                    self.run(Command::OpenImage { roll, index }, ctx);
                }
            }
            Command::OpenImage { roll, index } => {
                if let Some(images) = self.gallery.images_of(roll) {
                    self.gallery.select(roll, index);
                    self.viewer = Some(Viewer::new(images, index, Some(roll), self.config.raw));
                }
            }
            Command::ToggleRoll(roll) => self.gallery.toggle_roll(roll),
            Command::SelectImage { roll, index } => self.gallery.select(roll, index),
            Command::OpenExternally => self.hand_to_explorer("Opened in the default app", &[]),
            Command::ShowInFolder => self.hand_to_explorer("Shown in its folder", &["/select,"]),
            Command::DragOut { roll, index } => {
                if let Some(entry) = self
                    .gallery
                    .images_of(roll)
                    .and_then(|images| images.get(index).cloned())
                {
                    self.gallery.select(roll, index);
                    self.drag_out = Some(entry);
                }
            }
            Command::ToggleAllRolls => self.gallery.toggle_all(),
            Command::ToggleNested => {
                let nested = !self.gallery.nested;
                self.gallery.set_nested(nested, &mut self.lister);
                self.config.nested = nested;
                self.say(if nested {
                    "All nested subfolders"
                } else {
                    "Direct subfolders only"
                });
            }
            Command::ScaleThumbs(factor) => self.set_thumb_scale(self.gallery.thumb_scale * factor),
            Command::CycleThumbSize => {
                self.set_thumb_scale(layout::next_thumb_size(self.gallery.thumb_scale));
            }
            Command::CycleSort => {
                let sort = self.gallery.sort.next();
                self.gallery.set_sort(sort);
                self.config.sort = sort;
            }
            Command::Next => self.step_viewer(1),
            Command::Prev => self.step_viewer(-1),
            Command::First => self.with_viewer(|viewer| viewer.go_to(0)),
            Command::Last => self.with_viewer(|viewer| viewer.go_to(usize::MAX)),
            Command::Pan(direction) => {
                let moved = self
                    .viewer
                    .as_mut()
                    .is_some_and(|viewer| viewer.pan_step(direction));
                match direction {
                    _ if moved => {}
                    Direction::Left => self.step_viewer(-1),
                    Direction::Right => self.step_viewer(1),
                    Direction::Up | Direction::Down => {}
                }
            }
            Command::ZoomIn => self.with_viewer(|viewer| viewer.zoom_step(true)),
            Command::ZoomOut => self.with_viewer(|viewer| viewer.zoom_step(false)),
            Command::ZoomFit => self.with_viewer(Viewer::zoom_fit),
            Command::ZoomActual => self.with_viewer(Viewer::zoom_actual),
            Command::CycleRawMode => {
                self.config.raw = self.config.raw.next();
                let raw = self.config.raw;
                self.with_viewer(|viewer| viewer.raw_mode = raw);
                self.say(format!("RAW \u{b7} {}", raw.label()));
            }
            Command::CycleRawCache => {
                let size = config::next_raw_cache_size(self.config.raw_cache_gb);
                self.config.raw_cache_gb = size;
                self.developed.set_budget(config::raw_cache_bytes(size));
            }
            Command::ClearCaches => {
                self.thumb_store.clear();
                self.developed.clear();
                self.say("Thumbnail and RAW caches cleared");
            }
            Command::ToggleInfo => self.config.show_info = !self.config.show_info,
            Command::ToggleWheelAction => {
                self.config.wheel = self.config.wheel.toggled();
                self.say(match self.config.wheel {
                    WheelAction::Zoom => "Wheel zooms \u{b7} Ctrl+Wheel steps",
                    WheelAction::Step => "Wheel steps \u{b7} Ctrl+Wheel zooms",
                });
            }
            Command::ToggleHelp => {
                self.help = !self.help;
                self.settings = false;
            }
            Command::ToggleSettings => {
                self.settings = !self.settings;
                self.help = false;
            }
            Command::RotateCw => self.rotate(1),
            Command::RotateCcw => self.rotate(3),
            Command::Rotate180 => self.rotate(2),
            Command::ToggleFullscreen => self.set_fullscreen(ctx, !self.fullscreen),
            Command::Rate(stars) => self.rate(stars),
            Command::ToggleReject => self.toggle_reject(),
            Command::Remove => self.remove(),
            Command::ConfirmRemove => {
                if let Some(entry) = self.confirm_remove.take() {
                    self.files.submit(entry, Job::Remove { permanently: true });
                }
            }
            Command::Escape => {
                if self.confirm_remove.is_some() {
                    self.confirm_remove = None;
                } else if self.help || self.settings {
                    self.help = false;
                    self.settings = false;
                } else if self.viewer.is_some() {
                    self.close_viewer(ctx);
                } else {
                    self.set_fullscreen(ctx, false);
                }
            }
            Command::Quit => ctx.send_viewport_cmd(ViewportCommand::Close),
        }
    }

    /// Moves to the neighbouring image. With nested folders shown, the end of a roll
    /// leads into the next roll that has images.
    fn step_viewer(&mut self, delta: isize) {
        let Some(viewer) = &mut self.viewer else {
            return;
        };
        let beyond = viewer
            .roll
            .filter(|_| self.gallery.nested && viewer.at_edge(delta))
            .and_then(|roll| self.gallery.neighbour_roll(roll, delta))
            .and_then(|roll| Some((roll, self.gallery.images_of(roll)?)));
        match beyond {
            Some((roll, images)) => viewer.enter_roll(images, roll, delta),
            None => viewer.step(delta),
        }
    }

    fn with_viewer(&mut self, action: impl FnOnce(&mut Viewer)) {
        if let Some(viewer) = &mut self.viewer {
            action(viewer);
        }
    }

    fn set_thumb_scale(&mut self, scale: f32) {
        let scale = scale.clamp(config::MIN_THUMB_SCALE, config::MAX_THUMB_SCALE);
        self.gallery.thumb_scale = scale;
        self.config.thumb_scale = scale;
    }

    /// Tells the loader which full-size images the current view needs.
    fn publish_wanted(&mut self) {
        let wanted = self
            .viewer
            .as_ref()
            .map(|viewer| viewer.wanted(&self.loader))
            .unwrap_or_default();
        self.loader.want(&wanted);
    }

    fn track_window(&mut self, ctx: &egui::Context) {
        if self.fullscreen {
            return;
        }
        let geometry = ctx.input(|input| {
            let viewport = input.viewport();
            let special =
                viewport.fullscreen.unwrap_or(false) || viewport.minimized.unwrap_or(false);
            let maximized = viewport.maximized.unwrap_or(false);
            match (special, viewport.inner_rect, viewport.outer_rect) {
                (false, Some(inner), outer) => {
                    Some((inner.size(), outer.map(|o| o.min), maximized))
                }
                _ => None,
            }
        });
        if let Some((size, position, maximized)) = geometry {
            let previous = self.window;
            self.window = Some(if maximized {
                WindowGeometry {
                    maximized: true,
                    ..previous.unwrap_or(WindowGeometry {
                        size: [size.x, size.y],
                        position: None,
                        maximized,
                    })
                }
            } else {
                WindowGeometry {
                    size: [size.x, size.y],
                    position: position.map(|p| [p.x, p.y]),
                    maximized: false,
                }
            });
        }
    }

    /// Names the viewed image or open folder in the window title.
    fn update_title(&mut self, ctx: &egui::Context) {
        let subject = match &self.viewer {
            Some(viewer) => viewer.current().map(|entry| entry.name.clone()),
            None => self
                .gallery
                .folder
                .as_ref()
                .map(|folder| folder.to_string_lossy().into_owned()),
        };
        let title =
            subject.map_or_else(|| "rawv".to_owned(), |subject| format!("rawv - {subject}"));
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    fn report_timing(&mut self) {
        let content_ready = match &self.viewer {
            Some(viewer) => matches!(viewer.final_state(&self.loader), LoadState::Ready(_)),
            None => self.gallery.folder.is_some() && self.lister.is_idle() && self.thumbs.is_idle(),
        };
        let Some(timing) = &mut self.timing else {
            return;
        };
        let elapsed = timing.process_start.elapsed().as_secs_f64() * 1000.0;
        if !timing.first_frame_reported {
            timing.first_frame_reported = true;
            eprintln!("timing first-frame {elapsed:.0} ms");
        } else if !timing.content_reported && content_ready {
            timing.content_reported = true;
            eprintln!("timing content-ready {elapsed:.0} ms");
        }
    }

    /// Appends the observable app state as one JSON line. Used by scripted runs.
    fn dump_state(&self, path: &Path) {
        use std::io::Write;
        let viewer = self.viewer.as_ref().map(|viewer| {
            let entry = viewer.current();
            let loaded = match viewer.final_state(&self.loader) {
                LoadState::Ready(loaded) => Some(loaded),
                _ => None,
            };
            let orientation = entry.zip(loaded).map(|(entry, loaded)| {
                self.orientations
                    .effective(&entry.id.path, loaded.orientation)
                    .to_exif()
            });
            serde_json::json!({
                "index": viewer.index,
                "roll": viewer.roll,
                "total": viewer.images.len(),
                "name": entry.map(|entry| entry.name.clone()),
                "raw_mode": format!("{:?}", viewer.raw_mode),
                "zoom_percent": viewer.zoom_percent(),
                "pan": viewer.pan_offset(),
                "loaded": loaded.is_some(),
                "source": loaded.map(|loaded| format!("{:?}", loaded.source)),
                "cached": loaded.map(|loaded| loaded.cached),
                "size": loaded.map(|loaded| loaded.original_size),
                "load_ms": loaded.map(|loaded| loaded.load_ms),
                "orientation": orientation,
                "rating": entry.map(|entry| self.rating_of(entry)),
            })
        });
        let rolls: Vec<serde_json::Value> = self
            .gallery
            .rolls
            .iter()
            .map(|roll| {
                serde_json::json!({
                    "title": roll.title,
                    "trail": roll.trail,
                    "expanded": roll.expanded,
                    "images": roll.images().len(),
                    "first": roll.images().first().map(|entry| entry.name.clone()),
                })
            })
            .collect();
        let selected_orientation = self.gallery.selected_entry().and_then(|entry| {
            let thumb = self.thumbs.peek(&entry.id)?;
            Some(
                self.orientations
                    .effective(&entry.id.path, thumb.orientation)
                    .to_exif(),
            )
        });
        let state = serde_json::json!({
            "mode": format!("{:?}", self.mode()),
            "folder": self.gallery.folder,
            "fullscreen": self.fullscreen,
            "window": self.window.map(|window| (window.position, window.size, window.maximized)),
            "help": self.help,
            "settings": self.settings,
            "confirm_remove": self.confirm_remove.as_ref().map(|entry| entry.name.clone()),
            "dragged_out": self.dragged_out,
            "handed_over": self.handed_over,
            "show_info": self.config.show_info,
            "toast": self.toast.as_ref().map(|toast| toast.text.clone()),
            "selection": self.gallery.selection,
            "selected_orientation": selected_orientation,
            "selected_rating": self.gallery.selected_entry().map(|entry| self.rating_of(entry)),
            "thumb_scale": self.gallery.thumb_scale,
            "sort": format!("{:?}", self.gallery.sort),
            "nested": self.gallery.nested,
            "wheel": format!("{:?}", self.config.wheel),
            "raw": format!("{:?}", self.config.raw),
            "rolls": rolls,
            "viewer": viewer,
        });
        let written = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut file| writeln!(file, "{state}"));
        if let Err(error) = written {
            eprintln!("dump {} failed: {error}", path.display());
        }
    }

    fn header(&self, ui: &mut Ui) -> Vec<Command> {
        let mut commands = Vec::new();
        let frame = egui::Frame::new()
            .fill(theme::HEADER)
            .inner_margin(egui::Margin::symmetric(16, 0));
        egui::Panel::top("header")
            .exact_size(HEADER_HEIGHT)
            .frame(frame)
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if header_button(ui, "+", theme::INK, "Open folder (O)") {
                        commands.push(Command::OpenFolderDialog);
                    }
                    ui.add_space(4.0);
                    if let Some(folder) = &self.gallery.folder {
                        commands.extend(breadcrumbs(ui, folder));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if header_button(ui, "Settings", theme::INK, "Settings (,)") {
                            commands.push(Command::ToggleSettings);
                        }
                        let all = "Expand / collapse all rolls (A)";
                        if header_button(ui, "All", theme::INK, all) {
                            commands.push(Command::ToggleAllRolls);
                        }
                        let nested_color = if self.gallery.nested {
                            theme::ACCENT
                        } else {
                            theme::INK
                        };
                        let nested = "Rolls for all nested subfolders, not only direct ones (N)";
                        if header_button(ui, "Nested", nested_color, nested) {
                            commands.push(Command::ToggleNested);
                        }
                        let size = format!(
                            "Size {}",
                            layout::thumb_size_label(self.gallery.thumb_scale)
                        );
                        if header_button(ui, &size, theme::INK, "Thumbnail size (T)") {
                            commands.push(Command::CycleThumbSize);
                        }
                        let sort = "Change sort order (S)";
                        if header_button(ui, self.gallery.sort.label(), theme::INK, sort) {
                            commands.push(Command::CycleSort);
                        }
                    });
                });
                let rect = ui.max_rect().expand2(vec2(16.0, 0.0));
                ui.painter().hline(
                    rect.x_range(),
                    rect.bottom() - 0.5,
                    Stroke::new(1.0, theme::BORDER),
                );
            });
        commands
    }

    /// The settings as rows of name, current value and key. Clicking a value changes it.
    fn settings_panel(&self, ctx: &egui::Context) -> Vec<Command> {
        let on_off = |on: bool| if on { "On" } else { "Off" };
        let nested = if self.gallery.nested {
            "All nested"
        } else {
            "Direct only"
        };
        let size = layout::thumb_size_label(self.gallery.thumb_scale);
        let raw_cache = config::raw_cache_label(self.config.raw_cache_gb);
        let everywhere = [
            (
                "Mouse wheel in viewer",
                self.config.wheel.label(),
                "W",
                Command::ToggleWheelAction,
            ),
            (
                "RAW files",
                self.config.raw.label(),
                "P",
                Command::CycleRawMode,
            ),
            (
                "Developed RAW cache",
                raw_cache.as_str(),
                "",
                Command::CycleRawCache,
            ),
            (
                "Thumbnail and RAW caches",
                "Clear",
                "",
                Command::ClearCaches,
            ),
            (
                "Info line in viewer",
                on_off(self.config.show_info),
                "I",
                Command::ToggleInfo,
            ),
        ];
        let gallery_only = [
            ("Subfolders as rolls", nested, "N", Command::ToggleNested),
            ("Thumbnail size", size, "T", Command::CycleThumbSize),
            (
                "Sort order",
                self.gallery.sort.label(),
                "S",
                Command::CycleSort,
            ),
        ];
        let rows = everywhere
            .into_iter()
            .chain(gallery_only.into_iter().filter(|_| self.viewer.is_none()));

        let mut commands = Vec::new();
        let frame = egui::Frame::new()
            .fill(theme::HEADER)
            .stroke(Stroke::new(1.0, theme::BORDER))
            .corner_radius(8.0)
            .inner_margin(20.0);
        let modal = egui::Modal::new(egui::Id::unique("settings"))
            .frame(frame)
            .backdrop_color(Color32::from_black_alpha(140))
            .show(ctx, |ui| {
                ui.label(
                    RichText::new("Settings")
                        .font(theme::strong(13.0))
                        .color(theme::INK),
                );
                ui.add_space(10.0);
                egui::Grid::new("settings-rows")
                    .num_columns(3)
                    .spacing([16.0, 8.0])
                    .show(ui, |ui| {
                        for (name, value, key, command) in rows {
                            ui.label(
                                RichText::new(name)
                                    .font(theme::body(12.0))
                                    .color(theme::INK),
                            );
                            let text = RichText::new(value)
                                .font(theme::body(12.0))
                                .color(theme::ACCENT);
                            let button = egui::Button::new(text)
                                .fill(theme::BUTTON)
                                .stroke(Stroke::new(1.0, theme::BUTTON_BORDER))
                                .corner_radius(8.0)
                                .min_size(vec2(220.0, 26.0));
                            if ui.add(button).clicked() {
                                commands.push(command);
                            }
                            ui.label(
                                RichText::new(key)
                                    .font(theme::mono(12.0))
                                    .color(theme::MUTED),
                            );
                            ui.end_row();
                        }
                    });
                ui.add_space(10.0);
                ui.label(
                    RichText::new("Click a value to change it.")
                        .font(theme::body(11.0))
                        .color(theme::FAINT),
                );
            });
        if modal.should_close() {
            commands.push(Command::Escape);
        }
        commands
    }

    /// Asks before deleting `entry` for good.
    fn confirm_remove_panel(&self, ctx: &egui::Context, entry: &ImageEntry) -> Vec<Command> {
        let mut commands = Vec::new();
        let frame = egui::Frame::new()
            .fill(theme::HEADER)
            .stroke(Stroke::new(1.0, theme::BORDER))
            .corner_radius(8.0)
            .inner_margin(20.0);
        let modal = egui::Modal::new(egui::Id::unique("confirm-remove"))
            .frame(frame)
            .backdrop_color(Color32::from_black_alpha(140))
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(format!("Delete {} for good?", entry.name))
                        .font(theme::strong(13.0))
                        .color(theme::INK),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new(
                        "This drive has no recycle bin. The file cannot be brought back.",
                    )
                    .font(theme::body(12.0))
                    .color(theme::MUTED),
                );
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if header_button(ui, "Delete (Enter)", theme::ACCENT, "Delete the file") {
                        commands.push(Command::ConfirmRemove);
                    }
                    if header_button(ui, "Keep (Esc)", theme::INK, "Leave the file alone") {
                        commands.push(Command::Escape);
                    }
                });
            });
        if modal.should_close() {
            commands.push(Command::Escape);
        }
        commands
    }

    fn paint_help(&self, ui: &Ui) {
        let lines = command::help_lines(self.mode());
        let screen = ui.ctx().content_rect();
        let size = vec2(460.0, lines.len() as f32 * 20.0 + 64.0);
        let panel = Rect::from_center_size(screen.center(), size);
        let painter = ui.ctx().layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::unique("help"),
        ));
        painter.rect_filled(screen, 0.0, Color32::from_black_alpha(140));
        painter.rect_filled(panel, 8.0, theme::HEADER);
        painter.rect_stroke(
            panel,
            8.0,
            Stroke::new(1.0, theme::BORDER),
            egui::StrokeKind::Inside,
        );
        painter.text(
            panel.left_top() + vec2(20.0, 16.0),
            Align2::LEFT_TOP,
            "Keys",
            theme::strong(13.0),
            theme::INK,
        );
        for (row, (key, description)) in lines.iter().enumerate() {
            let y = panel.top() + 46.0 + row as f32 * 20.0;
            painter.text(
                egui::pos2(panel.left() + 20.0, y),
                Align2::LEFT_TOP,
                *key,
                theme::mono(12.0),
                theme::ACCENT,
            );
            painter.text(
                egui::pos2(panel.left() + 200.0, y),
                Align2::LEFT_TOP,
                *description,
                theme::body(12.0),
                theme::INK,
            );
        }
    }

    fn paint_toast(&mut self, ui: &Ui) {
        let now = ui.input(|input| input.time);
        let Some(toast) = &mut self.toast else { return };
        let until = *toast.until.get_or_insert(now + TOAST_SECONDS);
        if now >= until {
            self.toast = None;
            return;
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(until - now));
        let screen = ui.ctx().content_rect();
        let painter = ui.ctx().layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::unique("toast"),
        ));
        let galley =
            painter.layout_no_wrap(toast.text.clone(), theme::strong(13.0), theme::ON_ACCENT);
        let pill = Rect::from_center_size(
            egui::pos2(screen.center().x, screen.top() + 76.0),
            galley.size() + vec2(28.0, 16.0),
        );
        painter.rect_filled(pill, 8.0, theme::ACCENT);
        painter.galley(
            pill.center() - galley.size() / 2.0,
            galley,
            theme::ON_ACCENT,
        );
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.track_window(&ctx);

        self.thumbs.begin_frame(&ctx);
        self.loader.poll(&ctx);
        for result in self.lister.poll() {
            self.on_listing(result);
        }
        for done in self.files.poll() {
            self.on_file_done(done, &ctx);
        }

        let mut commands = command::from_keyboard(&ctx, self.mode());
        let dropped: Vec<PathBuf> = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .collect()
        });
        let pressed_anew = ctx.input(|input| input.pointer.any_pressed());
        if pressed_anew {
            self.dragged_out = None;
        }
        let dropped = dropped
            .into_iter()
            .find(|path| self.dragged_out.as_ref() != Some(path));
        commands.extend(dropped.map(Command::Open));
        let idle = self.is_idle();
        match self
            .script
            .as_mut()
            .and_then(|script| script.tick(idle, &ctx))
        {
            Some(Action::Run(command)) => commands.push(command),
            Some(Action::Dump(path)) => self.dump_state(&path),
            None => {}
        }

        if let Some(viewer) = &mut self.viewer {
            let cx = ViewerContext {
                loader: &self.loader,
                thumbs: &self.thumbs,
                orientations: &self.orientations,
                ratings: &self.ratings,
                show_info: self.config.show_info,
                fullscreen: self.fullscreen,
                wheel: self.config.wheel,
            };
            commands.extend(viewer.ui(ui, &cx));
        } else {
            commands.extend(self.header(ui));
            let frame = egui::Frame::new().fill(theme::BACKGROUND);
            egui::CentralPanel::default().frame(frame).show(ui, |ui| {
                let mut cx = GalleryContext {
                    thumbs: &mut self.thumbs,
                    lister: &mut self.lister,
                    orientations: &self.orientations,
                    ratings: &self.ratings,
                };
                commands.extend(self.gallery.ui(ui, &mut cx));
            });
        }
        if self.help {
            self.paint_help(ui);
        }
        if self.settings {
            commands.extend(self.settings_panel(&ctx));
        }
        if let Some(entry) = &self.confirm_remove {
            commands.extend(self.confirm_remove_panel(&ctx, entry));
        }
        self.paint_toast(ui);

        if !commands.is_empty() {
            ctx.request_repaint();
        }
        for command in commands {
            self.run(command, &ctx);
        }
        self.publish_wanted();
        self.update_title(&ctx);
        self.thumbs.end_frame();
        if let Some(entry) = self.drag_out.take() {
            self.start_drag_out(frame, &ctx, &entry);
        }
        self.report_timing();
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if let Some(script) = &mut self.script {
            script.inject(raw_input);
        }
        if let Some(pos) = self.pointer_taken.take() {
            raw_input.events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            });
            raw_input.events.push(egui::Event::PointerGone);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.config.window = self.window;
        self.config.save();
    }
}

/// Clickable path segments, deepest folder last.
fn breadcrumbs(ui: &mut Ui, folder: &Path) -> Vec<Command> {
    let mut commands = Vec::new();
    let mut ancestors: Vec<&Path> = folder.ancestors().collect();
    ancestors.reverse();
    let skipped = ancestors.len().saturating_sub(MAX_BREADCRUMBS);
    if skipped > 0 {
        ui.label(RichText::new("\u{2026}").size(12.0).color(theme::FAINT));
    }
    let last = ancestors.len() - 1;
    for (index, ancestor) in ancestors.iter().enumerate().skip(skipped) {
        let name = ancestor.file_name().map_or_else(
            || {
                ancestor
                    .to_string_lossy()
                    .trim_end_matches(['\\', '/'])
                    .to_owned()
            },
            |name| name.to_string_lossy().into_owned(),
        );
        if name.is_empty() {
            continue;
        }
        let color = if index == last {
            theme::INK
        } else {
            theme::MUTED
        };
        let label =
            egui::Label::new(RichText::new(name).size(12.0).color(color)).sense(Sense::click());
        let response = ui.add(label);
        if response.hovered() && index != last {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            ui.painter().hline(
                response.rect.x_range(),
                response.rect.bottom(),
                Stroke::new(1.0, theme::ACCENT),
            );
        }
        if response.clicked() && index != last {
            commands.push(Command::Open(ancestor.to_path_buf()));
        }
        if index != last {
            ui.label(RichText::new("/").size(12.0).color(theme::FAINT));
        }
    }
    commands
}

/// A small JPEG of a thumbnail, to follow the pointer during a drag.
fn drag_picture(thumbnail_jpeg: &[u8]) -> Option<Vec<u8>> {
    let pixels = pixels::decode_jpeg(thumbnail_jpeg, Channels::Rgb).ok()?;
    let small = pixels.fit_within(DRAG_PICTURE_LONG_EDGE).ok()?;
    pixels::encode_jpeg(&small, 85).ok()
}

fn header_button(ui: &mut Ui, text: &str, color: Color32, tooltip: &str) -> bool {
    let button = egui::Button::new(RichText::new(text).size(12.0).color(color))
        .fill(theme::BUTTON)
        .stroke(Stroke::new(1.0, theme::BUTTON_BORDER))
        .corner_radius(8.0)
        .min_size(vec2(30.0, 28.0));
    let response = ui.add(button).on_hover_text(tooltip);
    if response.hovered() {
        ui.painter()
            .rect_filled(response.rect, 8.0, theme::BUTTON_HOVER.gamma_multiply(0.3));
    }
    response.clicked()
}

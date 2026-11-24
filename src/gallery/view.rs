//! Drawing the gallery.

use egui::{
    Align2, Color32, Rect, ScrollArea, Sense, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2,
};

use super::layout::{self, Metrics};
use super::{Gallery, Roll, RollState};
use crate::command::Command;
use crate::draw;
use crate::library::lister::Lister;
use crate::library::listing::ImageEntry;
use crate::media::formats::Kind;
use crate::orientations::Orientations;
use crate::theme;
use crate::thumbs::service::{ThumbService, ThumbState};

const SPROCKET_PITCH: f32 = 14.0;
const SPROCKET_RADIUS: f32 = 2.2;
const LABEL_HEIGHT: f32 = 14.0;

pub struct GalleryContext<'a> {
    pub thumbs: &'a mut ThumbService,
    pub lister: &'a mut Lister,
    pub orientations: &'a Orientations,
}

struct RollView<'a> {
    index: usize,
    roll: &'a Roll,
    metrics: Metrics,
    columns: usize,
    selected: Option<usize>,
    reveal: bool,
}

impl Gallery {
    pub fn ui(&mut self, ui: &mut Ui, cx: &mut GalleryContext<'_>) -> Vec<Command> {
        let mut commands = Vec::new();
        if self.folder.is_none() {
            return empty_state(ui);
        }
        let zoom = ui.input(|input| input.zoom_delta());
        if zoom != 1.0 {
            commands.push(Command::ScaleThumbs(zoom));
        }

        let metrics = Metrics::new(self.thumb_scale);
        let reveal_selection = std::mem::take(&mut self.reveal_selection);
        let selection = self.selection;
        let generation = self.generation;
        let nested = self.nested;
        let mut columns = self.columns;

        ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            ui.add_space(8.0);
            let width = ui.available_width();
            let strip_inner = width
                - 2.0 * layout::PAGE_MARGIN
                - layout::META_WIDTH
                - layout::META_GAP
                - 2.0 * layout::STRIP_PAD_X;
            columns = metrics.columns(strip_inner);

            for (index, roll) in self.rolls.iter_mut().enumerate() {
                let count = roll.images().len();
                let is_ready = matches!(roll.state, RollState::Ready { .. });
                // A folder without images is represented by the rolls of its subfolders.
                let shown_through_subfolders = roll.is_root || (nested && roll.folder_count() > 0);
                if is_ready && count == 0 && shown_through_subfolders {
                    continue;
                }
                let rows = if roll.expanded {
                    Metrics::rows(count, columns)
                } else {
                    1
                };
                let height = if is_ready && count == 0 {
                    layout::EMPTY_ROLL_HEIGHT + 2.0 * layout::ROLL_PADDING + 1.0
                } else {
                    metrics.roll_height(rows)
                };
                let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
                let selected = selection.filter(|s| s.0 == index).map(|s| s.1);
                let reveal = reveal_selection && selected.is_some();
                if !ui.is_rect_visible(rect) && !reveal {
                    continue;
                }
                if matches!(roll.state, RollState::Unrequested) {
                    cx.lister.request(generation, roll.path.clone());
                    roll.state = RollState::Loading;
                }
                let view = RollView {
                    index,
                    roll,
                    metrics,
                    columns,
                    selected,
                    reveal,
                };
                view.paint(ui, rect, cx, &mut commands);
            }
            ui.add_space(24.0);
        });
        self.columns = columns;
        commands
    }
}

impl RollView<'_> {
    fn paint(
        &self,
        ui: &mut Ui,
        rect: Rect,
        cx: &mut GalleryContext<'_>,
        commands: &mut Vec<Command>,
    ) {
        let painter = ui.painter().clone();
        painter.hline(
            rect.x_range().shrink(4.0),
            rect.bottom() - 0.5,
            Stroke::new(1.0, theme::ROLL_BORDER),
        );
        let meta = Rect::from_min_size(
            rect.left_top() + vec2(layout::PAGE_MARGIN, layout::ROLL_PADDING),
            vec2(
                layout::META_WIDTH,
                rect.height() - 2.0 * layout::ROLL_PADDING,
            ),
        );
        self.paint_meta(ui, meta, commands);

        let images = self.roll.images();
        let is_ready = matches!(self.roll.state, RollState::Ready { .. });
        if is_ready && images.is_empty() {
            return;
        }
        let rows = if self.roll.expanded {
            Metrics::rows(images.len(), self.columns)
        } else {
            1
        };
        let strip = Rect::from_min_max(
            pos2(meta.right() + layout::META_GAP, meta.top()),
            pos2(
                rect.right() - layout::PAGE_MARGIN,
                meta.top() + self.metrics.strip_height(rows),
            ),
        );
        painter.rect_filled(strip, 6.0, theme::STRIP);
        paint_sprockets(&painter, strip);

        let frames = Rect::from_min_max(
            strip.left_top() + vec2(layout::STRIP_PAD_X, layout::STRIP_PAD_TOP),
            strip.right_bottom() - vec2(layout::STRIP_PAD_X, 2.0),
        );
        match &self.roll.state {
            RollState::Unrequested | RollState::Loading => {
                strip_note(&painter, frames, self.metrics, "loading\u{2026}");
            }
            RollState::Failed(message) => strip_note(&painter, frames, self.metrics, message),
            RollState::Ready { .. } if self.roll.expanded => {
                self.paint_wrapped(ui, frames, images, cx, commands);
            }
            RollState::Ready { .. } => self.paint_single_row(ui, frames, images, cx, commands),
        }
    }

    fn paint_meta(&self, ui: &mut Ui, meta: Rect, commands: &mut Vec<Command>) {
        let painter = ui.painter().clone();
        let chevron_area = Rect::from_min_size(meta.left_top(), vec2(16.0, 20.0));
        let chevron = ui.interact(
            chevron_area,
            ui.scope_id().with(("chevron", self.index)),
            Sense::click(),
        );
        let title_galley = {
            let mut job = egui::text::LayoutJob::simple_singleline(
                self.title_text(&painter, meta.width() - 18.0),
                theme::strong(13.0),
                theme::INK,
            );
            job.wrap = egui::text::TextWrapping {
                max_width: meta.width() - 18.0,
                max_rows: 1,
                break_anywhere: true,
                overflow_character: Some('\u{2026}'),
            };
            painter.layout_job(job)
        };
        let title_area = Rect::from_min_size(
            meta.left_top() + vec2(16.0, 0.0),
            vec2(title_galley.size().x, 20.0),
        );
        let mut title = ui.interact(
            title_area,
            ui.scope_id().with(("title", self.index)),
            Sense::click(),
        );
        if !self.roll.trail.is_empty() {
            title = title.on_hover_text(self.path_text(0));
        }
        let hovered = chevron.hovered() || title.hovered();
        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let chevron_color = if self.roll.expanded || chevron.hovered() {
            theme::ACCENT
        } else {
            theme::FAINT
        };
        draw::chevron(
            &painter,
            chevron_area.left_center() + vec2(5.0, 0.0),
            self.roll.expanded,
            chevron_color,
        );
        let title_color = if title.hovered() {
            theme::ACCENT
        } else {
            theme::INK
        };
        painter.galley_with_override_text_color(
            title_area.left_center() - vec2(0.0, title_galley.size().y / 2.0),
            title_galley,
            title_color,
        );
        if chevron.clicked() || (title.clicked() && self.roll.is_root) {
            commands.push(Command::ToggleRoll(self.index));
        } else if title.clicked() {
            commands.push(Command::Open(self.roll.path.clone()));
        }

        let (date, caption) = match &self.roll.state {
            RollState::Ready {
                images,
                folder_count,
                date,
            } => {
                let photos = match images.len() {
                    0 => None,
                    1 => Some("1 photo".to_owned()),
                    n => Some(format!("{n} photos")),
                };
                let folders = match (*folder_count, self.roll.is_root) {
                    (0, _) | (_, true) => None,
                    (1, false) => Some("1 folder".to_owned()),
                    (n, false) => Some(format!("{n} folders")),
                };
                let caption = match (photos, folders) {
                    (Some(photos), Some(folders)) => format!("{photos} \u{b7} {folders}"),
                    (Some(text), None) | (None, Some(text)) => text,
                    (None, None) => "empty".to_owned(),
                };
                (date.clone(), caption)
            }
            RollState::Failed(_) => (None, "cannot read folder".to_owned()),
            RollState::Unrequested | RollState::Loading => (None, String::new()),
        };
        let mut y = meta.top() + 24.0;
        if let Some(date) = date {
            painter.text(
                pos2(meta.left(), y),
                Align2::LEFT_TOP,
                date,
                theme::mono(12.0),
                Color32::from_gray(0xdd),
            );
            y += 20.0;
        }
        painter.text(
            pos2(meta.left(), y),
            Align2::LEFT_TOP,
            caption,
            theme::body(12.0),
            theme::MUTED,
        );
        if !self.roll.images().is_empty() {
            painter.hline(
                meta.left()..=meta.right() - 8.0,
                y + 22.0,
                Stroke::new(2.0, theme::ACCENT),
            );
        }
    }

    /// The roll's title with as many parent folders in front of it as fit `max_width`.
    fn title_text(&self, painter: &egui::Painter, max_width: f32) -> String {
        let fits = |text: &String| {
            let galley = painter.layout_no_wrap(text.clone(), theme::strong(13.0), theme::INK);
            galley.size().x <= max_width
        };
        (0..=self.roll.trail.len())
            .map(|skipped| self.path_text(skipped))
            .find(fits)
            .unwrap_or_else(|| self.roll.title.clone())
    }

    /// The path from the open folder to this roll, without its first `skipped` folders.
    fn path_text(&self, skipped: usize) -> String {
        let trail = &self.roll.trail;
        let mut names: Vec<&str> = Vec::new();
        if skipped > 0 && skipped <= trail.len() {
            names.push("\u{2026}");
        }
        names.extend(trail.iter().skip(skipped).map(String::as_str));
        names.push(&self.roll.title);
        names.join(" / ")
    }

    fn paint_wrapped(
        &self,
        ui: &mut Ui,
        frames: Rect,
        images: &[ImageEntry],
        cx: &mut GalleryContext<'_>,
        commands: &mut Vec<Command>,
    ) {
        let clip = ui.clip_rect();
        let rows = Metrics::rows(images.len(), self.columns);
        let visible_rows = self.metrics.visible_rows(
            clip.top() - frames.top(),
            clip.bottom() - frames.top(),
            rows,
        );
        let first = visible_rows.start * self.columns;
        let count = visible_rows.len() * self.columns;
        for (index, entry) in images.iter().enumerate().skip(first).take(count) {
            let position = frames.left_top() + self.metrics.frame_offset(index, self.columns);
            let frame = Rect::from_min_size(position, self.metrics.frame);
            commands.extend(self.paint_frame(ui, frame, entry, index, cx));
        }
        if let (true, Some(index)) = (self.reveal, self.selected) {
            let position = frames.left_top() + self.metrics.frame_offset(index, self.columns);
            let frame = Rect::from_min_size(position, self.metrics.frame);
            ui.scroll_to_rect(frame.expand2(vec2(0.0, 40.0)), None);
        }
    }

    fn paint_single_row(
        &self,
        ui: &mut Ui,
        frames: Rect,
        images: &[ImageEntry],
        cx: &mut GalleryContext<'_>,
        commands: &mut Vec<Command>,
    ) {
        if let (true, Some(_)) = (self.reveal, self.selected) {
            ui.scroll_to_rect(frames.expand2(vec2(0.0, 40.0)), None);
        }
        let mut child = ui.new_child(UiBuilder::new().max_rect(frames));
        ScrollArea::horizontal()
            .id_salt(("roll", &self.roll.path))
            .auto_shrink(false)
            .show_viewport(&mut child, |ui, viewport| {
                let size = vec2(
                    self.metrics.row_width(images.len()),
                    self.metrics.frame.y + LABEL_HEIGHT,
                );
                let (content, _) = ui.allocate_exact_size(size, Sense::hover());
                let visible =
                    self.metrics
                        .visible_in_row(viewport.left(), viewport.right(), images.len());
                for index in visible {
                    let position =
                        content.left_top() + self.metrics.frame_offset(index, usize::MAX);
                    let frame = Rect::from_min_size(position, self.metrics.frame);
                    commands.extend(self.paint_frame(ui, frame, &images[index], index, cx));
                }
                if let (true, Some(index)) = (self.reveal, self.selected) {
                    let position =
                        content.left_top() + self.metrics.frame_offset(index, usize::MAX);
                    let frame = Rect::from_min_size(position, self.metrics.frame);
                    ui.scroll_to_rect(frame.expand2(vec2(40.0, 0.0)), None);
                }
            });
    }

    fn paint_frame(
        &self,
        ui: &mut Ui,
        frame: Rect,
        entry: &ImageEntry,
        index: usize,
        cx: &mut GalleryContext<'_>,
    ) -> Option<Command> {
        let response = ui
            .interact(
                frame,
                ui.scope_id().with(("frame", self.index, index)),
                Sense::click(),
            )
            .on_hover_text(&entry.name);
        let painter = ui.painter();
        let shown_pixels = frame.width() * ui.ctx().pixels_per_point();
        match cx.thumbs.lookup(entry, shown_pixels) {
            ThumbState::Ready(thumb) => {
                let orientation = cx.orientations.effective(&entry.id.path, thumb.orientation);
                let size = draw::shown_size(thumb.size, orientation, thumb.content);
                let crop = draw::cover_crop(size[0] / size[1], frame.width() / frame.height());
                painter.rect_filled(frame, 0.0, theme::FRAME);
                draw::image(
                    painter,
                    thumb.texture.id(),
                    frame,
                    orientation,
                    thumb.content,
                    crop,
                );
            }
            ThumbState::Pending => {
                painter.rect_filled(frame, 0.0, theme::FRAME_EMPTY);
            }
            ThumbState::Failed => {
                painter.rect_filled(frame, 0.0, theme::FRAME_EMPTY);
                painter.text(
                    frame.center(),
                    Align2::CENTER_CENTER,
                    "no preview",
                    theme::body(10.0),
                    theme::FAINT,
                );
            }
        }
        if entry.kind == Kind::Raw {
            let tag = Rect::from_min_size(frame.right_top() + vec2(-29.0, 3.0), vec2(26.0, 13.0));
            painter.rect_filled(tag, 2.0, Color32::from_black_alpha(170));
            painter.text(
                tag.center(),
                Align2::CENTER_CENTER,
                "RAW",
                theme::mono(9.0),
                theme::ACCENT,
            );
        }
        if self.selected == Some(index) {
            painter.rect_stroke(
                frame,
                1.0,
                Stroke::new(2.0, theme::ACCENT),
                StrokeKind::Outside,
            );
        } else if response.hovered() {
            painter.rect_stroke(
                frame,
                1.0,
                Stroke::new(1.0, theme::FRAME_HOVER),
                StrokeKind::Outside,
            );
        }
        painter.text(
            frame.center_bottom() + vec2(0.0, 3.0),
            Align2::CENTER_TOP,
            format!("{:02}", index + 1),
            theme::mono(10.0),
            theme::MUTED,
        );
        response.clicked().then_some(Command::OpenImage {
            roll: self.index,
            index,
        })
    }
}

fn paint_sprockets(painter: &egui::Painter, strip: Rect) {
    let count = (strip.width() / SPROCKET_PITCH).floor() as usize;
    for i in 0..count {
        let x = strip.left() + SPROCKET_PITCH / 2.0 + i as f32 * SPROCKET_PITCH;
        painter.circle_filled(pos2(x, strip.top() + 5.0), SPROCKET_RADIUS, theme::SPROCKET);
        painter.circle_filled(
            pos2(x, strip.bottom() - 5.0),
            SPROCKET_RADIUS,
            theme::SPROCKET,
        );
    }
}

fn strip_note(painter: &egui::Painter, frames: Rect, metrics: Metrics, text: &str) {
    painter.text(
        frames.left_top() + vec2(4.0, metrics.frame.y / 2.0),
        Align2::LEFT_CENTER,
        text,
        theme::body(12.0),
        theme::FAINT,
    );
}

fn empty_state(ui: &mut Ui) -> Vec<Command> {
    let mut commands = Vec::new();
    let rect = ui.available_rect_before_wrap();
    let painter = ui.painter().clone();
    let center = rect.center();
    painter.text(
        center - vec2(0.0, 44.0),
        Align2::CENTER_CENTER,
        "Pick a folder of images. Subfolders become rolls.",
        theme::body(13.0),
        theme::MUTED,
    );
    let button = Rect::from_center_size(center, vec2(132.0, 36.0));
    let response = ui.interact(button, ui.scope_id().with("choose-folder"), Sense::click());
    let fill = if response.hovered() {
        theme::ACCENT.gamma_multiply(0.9)
    } else {
        theme::ACCENT
    };
    painter.rect_filled(button, 8.0, fill);
    painter.text(
        button.center(),
        Align2::CENTER_CENTER,
        "Choose folder",
        theme::strong(13.0),
        theme::ON_ACCENT,
    );
    painter.text(
        center + vec2(0.0, 42.0),
        Align2::CENTER_CENTER,
        "O open folder \u{b7} drop a folder or file here \u{b7} H help",
        theme::body(11.0),
        theme::FAINT,
    );
    if response.clicked() {
        commands.push(Command::OpenFolderDialog);
    }
    commands
}

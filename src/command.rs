//! Everything the app can be told to do. Keys, mouse, buttons and scripts all go through here.

use std::path::PathBuf;

use egui::{Key, Modifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Opens a folder in the gallery or a file in the viewer.
    Open(PathBuf),
    OpenFolderDialog,
    GoParent,
    Refresh,
    Select(Direction),
    OpenSelected,
    /// Opens image `index` of gallery roll `roll` in the viewer.
    OpenImage {
        roll: usize,
        index: usize,
    },
    ToggleRoll(usize),
    ToggleAllRolls,
    /// Switches between rolls for direct subfolders and rolls for every nested subfolder.
    ToggleNested,
    /// Multiplies the thumbnail size.
    ScaleThumbs(f32),
    /// Switches to the next thumbnail size preset.
    CycleThumbSize,
    CycleSort,
    Next,
    Prev,
    First,
    Last,
    ZoomIn,
    ZoomOut,
    ZoomFit,
    ZoomActual,
    /// Switches how RAW files are shown: camera preview, automatic, developed.
    CycleRawMode,
    /// Switches the size limit of the developed RAW cache.
    CycleRawCache,
    ToggleInfo,
    /// Swaps what the wheel and Ctrl+wheel do in the viewer: zoom or step.
    ToggleWheelAction,
    ToggleHelp,
    ToggleSettings,
    RotateCw,
    RotateCcw,
    Rotate180,
    ToggleFullscreen,
    /// Leaves the innermost thing: help or settings, then viewer, then fullscreen.
    Escape,
    Quit,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Gallery,
    Viewer,
}

struct Binding {
    modifiers: Modifiers,
    key: Key,
    mode: Option<Mode>,
    command: fn() -> Command,
    /// Key label and description for the help overlay; `None` for alternate keys.
    help: Option<(&'static str, &'static str)>,
}

const fn bind(
    modifiers: Modifiers,
    key: Key,
    mode: Option<Mode>,
    command: fn() -> Command,
    help: Option<(&'static str, &'static str)>,
) -> Binding {
    Binding {
        modifiers,
        key,
        mode,
        command,
        help,
    }
}

const NONE: Modifiers = Modifiers::NONE;
const SHIFT: Modifiers = Modifiers::SHIFT;
const GALLERY: Option<Mode> = Some(Mode::Gallery);
const VIEWER: Option<Mode> = Some(Mode::Viewer);

#[rustfmt::skip]
const BINDINGS: &[Binding] = &[
    bind(NONE, Key::ArrowRight, VIEWER, || Command::Next, Some(("Right / Space", "Next image"))),
    bind(NONE, Key::Space, VIEWER, || Command::Next, None),
    bind(NONE, Key::PageDown, VIEWER, || Command::Next, None),
    bind(NONE, Key::ArrowLeft, VIEWER, || Command::Prev, Some(("Left / Backspace", "Previous image"))),
    bind(NONE, Key::Backspace, VIEWER, || Command::Prev, None),
    bind(NONE, Key::PageUp, VIEWER, || Command::Prev, None),
    bind(NONE, Key::Home, VIEWER, || Command::First, Some(("Home", "First image"))),
    bind(NONE, Key::End, VIEWER, || Command::Last, Some(("End", "Last image"))),
    bind(NONE, Key::Plus, VIEWER, || Command::ZoomIn, Some(("+", "Zoom in"))),
    bind(NONE, Key::Equals, VIEWER, || Command::ZoomIn, None),
    bind(NONE, Key::Minus, VIEWER, || Command::ZoomOut, Some(("-", "Zoom out"))),
    bind(NONE, Key::Slash, VIEWER, || Command::ZoomActual, Some(("/", "100 %"))),
    bind(NONE, Key::I, VIEWER, || Command::ToggleInfo, Some(("I", "Info line"))),
    bind(NONE, Key::Enter, VIEWER, || Command::Escape, Some(("Enter", "Back to folder"))),
    bind(NONE, Key::ArrowRight, GALLERY, || Command::Select(Direction::Right), Some(("Arrows", "Move selection"))),
    bind(NONE, Key::ArrowLeft, GALLERY, || Command::Select(Direction::Left), None),
    bind(NONE, Key::ArrowUp, GALLERY, || Command::Select(Direction::Up), None),
    bind(NONE, Key::ArrowDown, GALLERY, || Command::Select(Direction::Down), None),
    bind(NONE, Key::Enter, GALLERY, || Command::OpenSelected, Some(("Enter / Click", "Open image"))),
    bind(NONE, Key::Backspace, GALLERY, || Command::GoParent, Some(("Backspace", "Parent folder"))),
    bind(NONE, Key::A, GALLERY, || Command::ToggleAllRolls, Some(("A", "Expand / collapse all"))),
    bind(NONE, Key::N, GALLERY, || Command::ToggleNested, Some(("N", "All nested subfolders as rolls"))),
    bind(NONE, Key::S, GALLERY, || Command::CycleSort, Some(("S", "Change sort order"))),
    bind(NONE, Key::Plus, GALLERY, || Command::ScaleThumbs(1.15), Some(("+ / Ctrl+Wheel", "Larger thumbnails"))),
    bind(NONE, Key::Equals, GALLERY, || Command::ScaleThumbs(1.15), None),
    bind(NONE, Key::Minus, GALLERY, || Command::ScaleThumbs(1.0 / 1.15), Some(("-", "Smaller thumbnails"))),
    bind(NONE, Key::T, GALLERY, || Command::CycleThumbSize, Some(("T", "Thumbnail size S / M / L"))),
    bind(NONE, Key::F5, GALLERY, || Command::Refresh, Some(("F5", "Reload folder"))),
    bind(NONE, Key::R, None, || Command::RotateCw, Some(("R", "Rotate 90\u{b0} clockwise"))),
    bind(SHIFT, Key::R, None, || Command::RotateCcw, Some(("L / Shift+R", "Rotate 90\u{b0} counter-clockwise"))),
    bind(NONE, Key::L, None, || Command::RotateCcw, None),
    bind(NONE, Key::U, None, || Command::Rotate180, Some(("U", "Rotate 180\u{b0}"))),
    bind(NONE, Key::W, None, || Command::ToggleWheelAction, Some(("W", "Wheel zooms or steps (Ctrl+Wheel: the other)"))),
    bind(NONE, Key::P, None, || Command::CycleRawMode, Some(("P", "RAW: camera preview / automatic / full size"))),
    bind(NONE, Key::F, None, || Command::ToggleFullscreen, Some(("F / F11 / Double click", "Fullscreen"))),
    bind(NONE, Key::F11, None, || Command::ToggleFullscreen, None),
    bind(NONE, Key::O, None, || Command::OpenFolderDialog, Some(("O", "Open folder"))),
    bind(NONE, Key::Comma, None, || Command::ToggleSettings, Some((",", "Settings"))),
    bind(NONE, Key::H, None, || Command::ToggleHelp, Some(("H / F1", "This help"))),
    bind(NONE, Key::F1, None, || Command::ToggleHelp, None),
    bind(NONE, Key::Escape, None, || Command::Escape, Some(("Esc", "Close / back"))),
    bind(NONE, Key::Q, None, || Command::Quit, Some(("Q", "Quit"))),
];

/// A key that egui reports only as typed text, such as `*` on the number pad.
struct TextBinding {
    text: &'static str,
    mode: Option<Mode>,
    command: fn() -> Command,
    help: (&'static str, &'static str),
}

const TEXT_BINDINGS: &[TextBinding] = &[TextBinding {
    text: "*",
    mode: VIEWER,
    command: || Command::ZoomFit,
    help: ("* / Middle click", "Fit to window"),
}];

/// Commands for the keys pressed this frame.
pub fn from_keyboard(ctx: &egui::Context, mode: Mode) -> Vec<Command> {
    ctx.input_mut(|input| {
        // Bindings with modifiers are checked first so Shift+R does not also trigger R.
        let mut ordered: Vec<&Binding> = BINDINGS.iter().collect();
        ordered.sort_by_key(|binding| binding.modifiers == NONE);
        let mut commands: Vec<Command> = ordered
            .into_iter()
            .filter(|binding| binding.mode.is_none_or(|m| m == mode))
            .filter(|binding| input.consume_key(binding.modifiers, binding.key))
            .map(|binding| (binding.command)())
            .collect();
        for event in &input.events {
            let egui::Event::Text(typed) = event else {
                continue;
            };
            let typed_bindings = TEXT_BINDINGS
                .iter()
                .filter(|binding| binding.mode.is_none_or(|m| m == mode))
                .filter(|binding| binding.text == typed);
            commands.extend(typed_bindings.map(|binding| (binding.command)()));
        }
        commands
    })
}

/// Key and description pairs for the help overlay.
pub fn help_lines(mode: Mode) -> Vec<(&'static str, &'static str)> {
    let shown = |binding_mode: Option<Mode>| binding_mode.is_none_or(|m| m == mode);
    let keys = BINDINGS
        .iter()
        .filter(|binding| shown(binding.mode))
        .filter_map(|binding| binding.help);
    let typed = TEXT_BINDINGS
        .iter()
        .filter(|binding| shown(binding.mode))
        .map(|binding| binding.help);
    keys.chain(typed).collect()
}

/// Parses a script word such as `next`, `rotate-cw` or `open:C:\photos`.
pub fn parse(text: &str) -> Option<Command> {
    let (name, argument) = match text.split_once(':') {
        Some((name, argument)) => (name, Some(argument)),
        None => (text, None),
    };
    let command = match (name.trim(), argument) {
        ("open", Some(path)) => Command::Open(PathBuf::from(path.trim())),
        ("parent", None) => Command::GoParent,
        ("refresh", None) => Command::Refresh,
        ("left", None) => Command::Select(Direction::Left),
        ("right", None) => Command::Select(Direction::Right),
        ("up", None) => Command::Select(Direction::Up),
        ("down", None) => Command::Select(Direction::Down),
        ("open-selected", None) => Command::OpenSelected,
        ("toggle-all", None) => Command::ToggleAllRolls,
        ("nested", None) => Command::ToggleNested,
        ("thumbs-larger", None) => Command::ScaleThumbs(1.15),
        ("thumbs-smaller", None) => Command::ScaleThumbs(1.0 / 1.15),
        ("thumb-size", None) => Command::CycleThumbSize,
        ("sort", None) => Command::CycleSort,
        ("next", None) => Command::Next,
        ("prev", None) => Command::Prev,
        ("first", None) => Command::First,
        ("last", None) => Command::Last,
        ("zoom-in", None) => Command::ZoomIn,
        ("zoom-out", None) => Command::ZoomOut,
        ("zoom-fit", None) => Command::ZoomFit,
        ("zoom-actual", None) => Command::ZoomActual,
        ("raw-mode", None) => Command::CycleRawMode,
        ("raw-cache", None) => Command::CycleRawCache,
        ("info", None) => Command::ToggleInfo,
        ("wheel-action", None) => Command::ToggleWheelAction,
        ("help", None) => Command::ToggleHelp,
        ("settings", None) => Command::ToggleSettings,
        ("rotate-cw", None) => Command::RotateCw,
        ("rotate-ccw", None) => Command::RotateCcw,
        ("rotate-180", None) => Command::Rotate180,
        ("fullscreen", None) => Command::ToggleFullscreen,
        ("escape", None) => Command::Escape,
        ("quit", None) => Command::Quit,
        _ => return None,
    };
    Some(command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_script_words() {
        assert_eq!(parse("next"), Some(Command::Next));
        assert_eq!(parse(" rotate-cw "), Some(Command::RotateCw));
        assert_eq!(
            parse(r"open:C:\photos\roll 1"),
            Some(Command::Open(PathBuf::from(r"C:\photos\roll 1")))
        );
        assert_eq!(parse("nonsense"), None);
        assert_eq!(parse("next:extra"), None);
    }

    #[test]
    fn every_mode_lists_help_without_duplicates() {
        for mode in [Mode::Gallery, Mode::Viewer] {
            let lines = help_lines(mode);
            assert!(lines.len() > 8);
            let mut descriptions: Vec<&str> = lines.iter().map(|(_, text)| *text).collect();
            descriptions.sort_unstable();
            let before = descriptions.len();
            descriptions.dedup();
            assert_eq!(before, descriptions.len());
        }
    }
}

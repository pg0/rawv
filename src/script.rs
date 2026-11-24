//! Scripted runs: `--script "next;rotate-cw;shot:out.png;quit"`.
//!
//! Each step runs once the app has settled (nothing loading), so a script exercises the
//! real interface deterministically. Steps are command words (see `command::parse`),
//! `shot:FILE.png`, and pointer input in window points: `click:X,Y`, `double-click:X,Y`,
//! `middle-click:X,Y`, `move:X,Y`, `drag:X1,Y1,X2,Y2`, `wheel:LINES`, `ctrl-wheel:LINES`.
//! `drop:PATH` drops a file or folder onto the window. `type:TEXT` types text.
//! `dump:FILE` appends the app state as one JSON line, for assertions. `pause:MS` lets
//! time pass, as a person would between two separate double clicks.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use egui::{Event, Modifiers, MouseWheelUnit, PointerButton, Pos2, pos2, vec2};

use crate::command::{self, Command};

/// Frames the app must stay idle before the next step, so the result of the previous
/// step has been drawn.
const SETTLE_FRAMES: u32 = 3;

#[derive(Debug, PartialEq)]
enum Step {
    Run(Command),
    Shot(PathBuf),
    Dump(PathBuf),
    Pause(f64),
    Drop(PathBuf),
    /// Pointer events, one inner list per frame.
    Input(Vec<Vec<Event>>),
}

/// What a script step asks the app to do.
pub enum Action {
    Run(Command),
    /// Append the app state to this file.
    Dump(PathBuf),
}

pub struct Script {
    steps: VecDeque<Step>,
    input_frames: VecDeque<Vec<Event>>,
    drops: Vec<PathBuf>,
    settled: u32,
    /// Input time before which no further step runs.
    paused_until: f64,
    shots_pending: Arc<AtomicUsize>,
}

impl Script {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut steps = VecDeque::new();
        for word in text.split(';').map(str::trim).filter(|w| !w.is_empty()) {
            steps.push_back(parse_step(word)?);
        }
        Ok(Self {
            steps,
            input_frames: VecDeque::new(),
            drops: Vec::new(),
            settled: 0,
            paused_until: 0.0,
            shots_pending: Arc::new(AtomicUsize::new(0)),
        })
    }

    /// Adds this frame's scripted pointer events and file drops to the real input.
    pub fn inject(&mut self, raw_input: &mut egui::RawInput) {
        for path in self.drops.drain(..) {
            raw_input.dropped_files.push(Arc::new(ScriptedDrop(path)));
        }
        if let Some(events) = self.input_frames.pop_front() {
            raw_input.events.extend(events);
        }
    }

    /// Advances the script. Returns what the app should do this frame, if anything.
    pub fn tick(&mut self, app_idle: bool, ctx: &egui::Context) -> Option<Action> {
        let shots_pending = self.shots_pending.load(Ordering::Relaxed) > 0;
        let input_pending = !self.input_frames.is_empty() || !self.drops.is_empty();
        if self.steps.is_empty() && !input_pending && !shots_pending {
            return None;
        }
        ctx.request_repaint();
        let (now, wheel_in_motion) = ctx.input(|input| {
            let moving = input.zoom_delta() != 1.0 || input.smooth_scroll_delta != egui::Vec2::ZERO;
            (input.time, moving)
        });
        let busy = !app_idle || shots_pending || input_pending || wheel_in_motion;
        if busy || now < self.paused_until {
            self.settled = 0;
            return None;
        }
        self.settled += 1;
        if self.settled < SETTLE_FRAMES {
            return None;
        }
        self.settled = 0;
        match self.steps.pop_front()? {
            Step::Run(command) => Some(Action::Run(command)),
            Step::Dump(path) => Some(Action::Dump(path)),
            Step::Drop(path) => {
                self.drops.push(path);
                None
            }
            Step::Pause(seconds) => {
                self.paused_until = now + seconds;
                None
            }
            Step::Input(frames) => {
                self.input_frames.extend(frames);
                None
            }
            Step::Shot(path) => {
                self.shots_pending.fetch_add(1, Ordering::Relaxed);
                let pending = Arc::clone(&self.shots_pending);
                let repaint = ctx.clone();
                ctx.request_screenshot(move |image| {
                    let saved = image::save_buffer(
                        &path,
                        image.as_raw(),
                        image.width() as u32,
                        image.height() as u32,
                        image::ExtendedColorType::Rgba8,
                    );
                    if let Err(error) = saved {
                        eprintln!("screenshot {} failed: {error}", path.display());
                    }
                    pending.fetch_sub(1, Ordering::Relaxed);
                    repaint.request_repaint();
                });
                None
            }
        }
    }
}

fn parse_step(word: &str) -> Result<Step, String> {
    let (name, argument) = word.split_once(':').unwrap_or((word, ""));
    let numbers = || -> Result<Vec<f32>, String> {
        argument
            .split(',')
            .map(|n| {
                n.trim()
                    .parse::<f32>()
                    .map_err(|_| format!("bad number in: {word}"))
            })
            .collect()
    };
    let point = || -> Result<Pos2, String> {
        match numbers()?.as_slice() {
            [x, y] => Ok(pos2(*x, *y)),
            _ => Err(format!("expected X,Y in: {word}")),
        }
    };
    let step = match name {
        "shot" => Step::Shot(PathBuf::from(argument.trim())),
        "dump" => Step::Dump(PathBuf::from(argument.trim())),
        "drop" => Step::Drop(PathBuf::from(argument.trim())),
        "pause" => match numbers()?.as_slice() {
            [milliseconds] => Step::Pause(f64::from(*milliseconds) / 1000.0),
            _ => return Err(format!("expected MS in: {word}")),
        },
        "type" => Step::Input(vec![vec![Event::Text(argument.trim().to_owned())]]),
        "move" => Step::Input(vec![vec![Event::PointerMoved(point()?)]]),
        "click" => Step::Input(click(point()?, PointerButton::Primary)),
        "middle-click" => Step::Input(click(point()?, PointerButton::Middle)),
        "double-click" => {
            let position = point()?;
            let mut frames = click(position, PointerButton::Primary);
            frames.extend(click(position, PointerButton::Primary));
            Step::Input(frames)
        }
        "drag" => match numbers()?.as_slice() {
            [x1, y1, x2, y2] => {
                let (from, to) = (pos2(*x1, *y1), pos2(*x2, *y2));
                Step::Input(vec![
                    vec![
                        Event::PointerMoved(from),
                        button(from, PointerButton::Primary, true),
                    ],
                    vec![Event::PointerMoved(from.lerp(to, 0.5))],
                    vec![Event::PointerMoved(to)],
                    vec![button(to, PointerButton::Primary, false)],
                ])
            }
            _ => return Err(format!("expected X1,Y1,X2,Y2 in: {word}")),
        },
        "wheel" | "ctrl-wheel" => match numbers()?.as_slice() {
            [lines] => {
                let modifiers = if name == "ctrl-wheel" {
                    Modifiers::COMMAND | Modifiers::CTRL
                } else {
                    Modifiers::NONE
                };
                Step::Input(vec![vec![Event::MouseWheel {
                    unit: MouseWheelUnit::Line,
                    delta: vec2(0.0, *lines),
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Unknown,
                    modifiers,
                }]])
            }
            _ => return Err(format!("expected LINES in: {word}")),
        },
        _ => Step::Run(command::parse(word).ok_or_else(|| format!("unknown script step: {word}"))?),
    };
    Ok(step)
}

/// A file or folder dropped onto the window by a script.
#[derive(Debug)]
struct ScriptedDrop(PathBuf);

impl egui::DroppedFile for ScriptedDrop {
    fn path(&self) -> &std::path::Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|error| error.to_string())
    }
}

fn button(position: Pos2, button: PointerButton, pressed: bool) -> Event {
    Event::PointerButton {
        pos: position,
        button,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

/// Press in one frame, release in the next.
fn click(position: Pos2, pointer_button: PointerButton) -> Vec<Vec<Event>> {
    vec![
        vec![
            Event::PointerMoved(position),
            button(position, pointer_button, true),
        ],
        vec![button(position, pointer_button, false)],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_steps_and_rejects_unknown_words() {
        let script = Script::parse("next; shot:C:\\out\\a.png ;quit;").expect("parse");
        assert_eq!(
            script.steps,
            [
                Step::Run(Command::Next),
                Step::Shot(PathBuf::from("C:\\out\\a.png")),
                Step::Run(Command::Quit),
            ]
        );
        assert!(Script::parse("next;bogus").is_err());
    }

    #[test]
    fn parses_pointer_steps() {
        let script = Script::parse("click:10,20;wheel:-1;drag:0,0,10,10").expect("parse");
        let frame_counts: Vec<usize> = script
            .steps
            .iter()
            .map(|step| match step {
                Step::Input(frames) => frames.len(),
                _ => 0,
            })
            .collect();
        assert_eq!(frame_counts, [2, 1, 4]);
        assert!(Script::parse("click:10").is_err());
        assert!(Script::parse("wheel:up").is_err());
        assert!(Script::parse(r"open:C:\photos").is_ok());
    }

    #[test]
    fn injects_one_frame_of_events_at_a_time() {
        let mut script = Script::parse("click:5,5").expect("parse");
        let Some(Step::Input(frames)) = script.steps.pop_front() else {
            panic!("expected input step");
        };
        script.input_frames.extend(frames);
        let mut raw = egui::RawInput::default();
        script.inject(&mut raw);
        assert_eq!(raw.events.len(), 2);
        script.inject(&mut raw);
        assert_eq!(raw.events.len(), 3);
        script.inject(&mut raw);
        assert_eq!(raw.events.len(), 3);
    }
}

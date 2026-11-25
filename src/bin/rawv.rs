//! The viewer window.
//!
//! `rawv [FOLDER-OR-FILE] [--fullscreen] [--timing] [--script "step;step"] [--offscreen]`
//!
//! `--offscreen` opens the window outside the screen and without taking the keyboard, so a
//! scripted run neither disturbs the person at the computer nor receives their input.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use rawv::app::{App, Startup};
use rawv::script::Script;

/// Left of every monitor of a usual desk.
const OFFSCREEN_POSITION: [f32; 2] = [-8000.0, 100.0];

fn main() -> ExitCode {
    let process_start = Instant::now();
    let (startup, offscreen) = match parse_arguments(process_start) {
        Ok(arguments) => arguments,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    };
    let app = App::new(startup);

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("rawv")
        .with_app_id("rawv")
        .with_min_inner_size([480.0, 320.0])
        .with_fullscreen(app.starts_fullscreen())
        .with_drag_and_drop(true);
    if let Some(icon) = rawv::theme::window_icon() {
        viewport = viewport.with_icon(icon);
    }
    viewport = match app.window_geometry() {
        Some(geometry) => {
            let sized = viewport
                .with_inner_size(geometry.size)
                .with_maximized(geometry.maximized);
            match geometry.position {
                Some(position) => sized.with_position(position),
                None => sized,
            }
        }
        None => viewport.with_inner_size([1500.0, 950.0]),
    };
    if offscreen {
        viewport = viewport
            .with_position(OFFSCREEN_POSITION)
            .with_active(false)
            .with_taskbar(false);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    let result = eframe::run_native(
        "rawv",
        options,
        Box::new(move |creation| {
            app.attach(&creation.egui_ctx);
            Ok(Box::new(app))
        }),
    );
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// The startup options and whether the window opens off screen.
fn parse_arguments(process_start: Instant) -> Result<(Startup, bool), String> {
    let mut startup = Startup {
        open: None,
        fullscreen: false,
        script: None,
        timing: false,
        process_start,
    };
    let mut offscreen = false;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--fullscreen" | "--fs" => startup.fullscreen = true,
            "--timing" => startup.timing = true,
            "--offscreen" => offscreen = true,
            "--script" => {
                let text = arguments.next().ok_or("--script needs a value")?;
                startup.script = Some(Script::parse(&text)?);
            }
            option if option.starts_with("--") => return Err(format!("unknown option: {option}")),
            path => startup.open = Some(PathBuf::from(path)),
        }
    }
    Ok((startup, offscreen))
}

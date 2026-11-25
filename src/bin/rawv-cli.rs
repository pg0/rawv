//! Console companion to the viewer: checks the media pipeline without opening a window.

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [command, path] if command == "probe" => rawv::bench::probe_file(Path::new(path)),
        [command, path] if command == "bench" => rawv::bench::bench(Path::new(path)),
        [command, path, stars] if command == "rate" => rate(Path::new(path), stars),
        [command, path] if command == "pixels" => rawv::bench::pixel_sums(Path::new(path)),
        [command, path] if command == "remove" => remove(Path::new(path), false),
        [command, path, flag] if command == "remove" && flag == "--permanently" => {
            remove(Path::new(path), true)
        }
        [command] if command == "register" => register(None),
        [command, viewer] if command == "register" => register(Some(Path::new(viewer))),
        [command] if command == "unregister" => {
            rawv::default_app::unregister().map_err(|e| e.to_string())
        }
        _ => Err(
            "usage: rawv-cli probe <file> | bench <file-or-folder> | pixels <file> | rate <file> <-1..5> | remove <file> [--permanently] | register [<rawv.exe>] | unregister"
                .to_owned(),
        ),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

/// Registers the viewer for its file types, by default the `rawv.exe` next to this program,
/// and opens the Settings page where the user makes it the default.
fn register(viewer: Option<&Path>) -> Result<(), String> {
    let viewer = match viewer {
        Some(viewer) => std::path::absolute(viewer).map_err(|e| e.to_string())?,
        None => std::env::current_exe()
            .map_err(|e| e.to_string())?
            .with_file_name("rawv.exe"),
    };
    let registration = rawv::default_app::register(&viewer).map_err(|e| e.to_string())?;
    println!(
        "registered {} for {} file types",
        viewer.display(),
        registration.extensions
    );
    if registration.real {
        println!(
            "to make rawv the default: Settings > Apps > Default apps > rawv, pick it per file type"
        );
        rawv::default_app::open_settings().map_err(|e| format!("cannot open Settings: {e}"))?;
    }
    Ok(())
}

/// Removes a file the way the Delete key does and prints what happened.
fn remove(path: &Path, permanently: bool) -> Result<(), String> {
    let removal = rawv::media::remove::remove(path, permanently).map_err(|e| e.to_string())?;
    println!("{removal:?}");
    Ok(())
}

/// Writes a rating into a file the way the number keys do and prints what happened.
fn rate(path: &Path, stars: &str) -> Result<(), String> {
    let rating: i8 = stars
        .parse()
        .map_err(|_| format!("not a rating: {stars}"))?;
    if !(rawv::media::xmp::REJECTED..=rawv::media::xmp::MAX_RATING).contains(&rating) {
        return Err(format!("rating out of range: {rating}"));
    }
    let kind = rawv::media::formats::kind_of(path).ok_or("unsupported file type")?;
    let outcome = rawv::media::rate::persist(path, kind, rating).map_err(|e| e.to_string())?;
    println!("{outcome:?}");
    Ok(())
}

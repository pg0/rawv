//! Console companion to the viewer: checks the media pipeline without opening a window.

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [command, path] if command == "probe" => rawv::bench::probe_file(Path::new(path)),
        [command, path] if command == "bench" => rawv::bench::bench(Path::new(path)),
        [command, path] if command == "remove" => remove(Path::new(path), false),
        [command, path, flag] if command == "remove" && flag == "--permanently" => {
            remove(Path::new(path), true)
        }
        _ => Err(
            "usage: rawv-cli probe <file> | bench <file-or-folder> | remove <file> [--permanently]"
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

/// Removes a file the way the Delete key does and prints what happened.
fn remove(path: &Path, permanently: bool) -> Result<(), String> {
    let removal = rawv::media::remove::remove(path, permanently).map_err(|e| e.to_string())?;
    println!("{removal:?}");
    Ok(())
}

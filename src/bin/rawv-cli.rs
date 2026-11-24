//! Console companion to the viewer: checks the media pipeline without opening a window.

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [command, path] if command == "probe" => rawv::bench::probe_file(Path::new(path)),
        [command, path] if command == "bench" => rawv::bench::bench(Path::new(path)),
        _ => Err("usage: rawv-cli probe <file> | rawv-cli bench <file-or-folder>".to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

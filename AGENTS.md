# rawv

Fast, minimal photo and RAW viewer for Windows. Pure Rust, egui on OpenGL. Viewer only: the single
edit is lossless rotation.

## Commands

```
cargo build --release                      # target/release/rawv.exe (window) and rawv-cli.exe (console)
cargo test --release                       # unit tests
cargo install --path . --locked            # puts rawv and rawv-cli on the PATH (~/.cargo/bin)
cargo clippy --release --all-targets -- -D warnings
cargo fmt
python tools/verify.py <photo-tree>        # drives the real window (off screen), checks state and file bytes
python tools/make_test_photos.py <out> <raw-file> <seed-jpeg>   # builds <photo-tree>; needs pillow, piexif
target/release/rawv-cli bench <file-or-folder>                 # pipeline timings
target/release/rawv-cli probe <file>                           # what the metadata probe finds
target/release/rawv-cli remove <file> [--permanently]          # what the Delete key does to a file
target/release/rawv-cli rate <file> <-1..5>                    # writes a rating like the number keys
target/release/rawv-cli pixels <file>                          # checksum of the decoded picture
target/release/rawv-cli register [<rawv.exe>] | unregister    # file type registration for the current user
python tools/make_icon.py                                      # redraws assets/rawv.ico; needs pillow
```

A change is done when `verify.py` passes; add a check there for new behaviour. Its script
language is in `src/script.rs` (`--script "next;dump:state.jsonl;shot:a.png;quit"`).
Set `RAWV_DATA_DIR` to keep a run's settings and caches out of the user's profile, and
`RAWV_REGISTRY_ROOT` (a key below HKCU) to keep `register` out of the user's file type settings.
Run scripted windows with `--offscreen`: they then neither cover the screen nor receive the
input of the person at the computer.

## Release

The version stays 0.1.1 (`Cargo.toml`, tag `v0.1.1`) whatever changes, until the owner asks
for a new release; then both change together and a new GitHub release is made. A release is `dist/rawv-windows-x64.zip` with both executables and the
README, attached to the GitHub release:

```
cargo build --release && python tools/package.py      # writes dist/rawv-windows-x64.zip
gh release upload v0.1.1 dist/rawv-windows-x64.zip --clobber
```

The executables link the C runtime statically (`.cargo/config.toml`) and need nothing installed.

## Layout

| Folder | Holds |
|---|---|
| `src/media/` | File reading: `probe` (TIFF/EXIF directories, one open and one head read per file), `load` (full image), `thumb`, `rotate`, `raw` (sensor data through rawler), `render` (camera colours to display pixels), `developed` (cache of developed RAW images), `pixels` |
| `src/library/` | Folder listing, natural sort, background lister |
| `src/thumbs/` | Thumbnail store (redb, one local file) and worker service |
| `src/gallery/` | Folder view: rolls, layout maths, selection |
| `src/viewer/` | Single-image view, full-image loader, info line |
| `src/config.rs` | Settings kept between sessions and where data is stored |
| `src/filework.rs` | The one thread that changes photo files: rotate, rate, remove |
| `src/workers.rs` | `WantedPool`: workers pull from a "wanted" list the interface republishes each frame |
| `src/command.rs` | Every action as a `Command`; key bindings and script words map to it |
| `src/app.rs` | Owns all state, runs commands |
| `src/theme.rs` | All colours and fonts, the window icon |
| `src/default_app.rs` | Registers rawv for its file types (HKCU), opens the Default apps page |
| `assets/` | `rawv.ico`, the only source of the icon: `build.rs` puts it into both executables |
| `tools/` | Verification driver and test photo generator |

## Rules

- Actions are `Command`s. Keys, clicks, buttons and scripts produce one and `App::run` is the
  only place that acts on it. The exceptions are the viewer's pointer gestures (drag to pan,
  Ctrl+wheel and middle click to zoom), which act inside `Viewer::ui`.
- Pixels stay in stored orientation. Orientation is applied when drawing (`draw::image`).
- Background work goes through `WantedPool`: publish what is needed now, most urgent first.
  Do not queue work; a queue keeps doing what the user scrolled away from.
- Folder listings, thumbnails and images are read on worker threads. The interface thread
  only stats a path when it is opened.
- Photo files are changed only through `filework.rs`, in order and off the interface thread:
  - Rotation rewrites the two-byte EXIF orientation value in place and keeps the modification
    time (`media/rotate.rs`).
  - A rating is written as XMP metadata inside the photo file, never as a separate file
    (`media/rate.rs`, `media/xmp.rs`): in place where the file has an XMP packet, as a new
    segment in a JPEG, as an appended packet and first directory in Sony RAW and DNG files.
    Picture data and file times stay as they are. -1 marks a rejected picture.
  - Removal moves a file to the recycle bin. Where the volume has none (network shares), it
    deletes for good only after the user agreed (`media/remove.rs`). Whether there is a bin is
    checked up front: told to recycle on a network share, Windows deletes without saying so.
- Colours, fonts and sizes come from `theme.rs` and `gallery/layout.rs`.
- A setting is a field of `Config` changed by one `Command`. Its key and its row in the
  settings panel (`App::settings_panel`) both send that command.
- A RAW file has two renditions (`media::load::Rendition`): the camera's embedded JPEG and
  the developed sensor data. `Viewer::renditions` decides which to load; the preview stands
  in until the developed one is ready. Developed images take brightness and contrast from
  the embedded JPEG (`media/render.rs`).
- No `unsafe` (forbidden in `Cargo.toml`), no `unwrap`/`expect` outside tests (clippy denies them).

## Known limits

- HEIC, AVIF and JPEG XL are not decoded. No colour management. Animated GIF and WebP files play in the viewer
  (`media/animation.rs`); one whose frames need more than 256 MB shows its first frame, and
  gallery thumbnails always show the first frame.
- Files without an EXIF orientation value (PNG, plain JPEG) and RAW containers that are not
  TIFF-based (CR3, RAF) rotate for the session only.
- Dragging an image out of the window is checked by `verify.py` only up to the point where
  the file is handed to the system; a scripted run does not start the system's drag.
- `verify.py` leaves one small PNG per run in the Windows recycle bin.
- Ratings cannot be stored in PNG and other bitmap files, nor in RAW files that are neither
  Sony nor DNG and bring no XMP packet. The EXIF rating tag Windows uses (0x4746) is not
  written. Appending to a Sony RAW file without an XMP packet has unit tests and was checked
  by hand on a real file, but has no case in `verify.py`.
- The thumbnail store is never pruned, only emptied by Clear in the settings. A second running instance cannot open it and works
  without a thumbnail cache.
- Developed RAW images get no lens corrections (distortion, vignetting) and, without an
  embedded JPEG to follow, only the plain sRGB tone curve.
- With nested folders shown, every folder below the open one is listed at once.

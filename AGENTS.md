# rawv

Fast, minimal photo and RAW viewer for Windows. Pure Rust, egui on OpenGL. Viewer only: the single
edit is lossless rotation.

## Commands

```
cargo build --release                      # target/release/rawv.exe (window) and rawv-cli.exe (console)
cargo test --release                       # unit tests
cargo clippy --release --all-targets -- -D warnings
cargo fmt
python tools/verify.py <photo-tree>        # drives the real window (off screen), checks state and file bytes
python tools/make_test_photos.py <out> <raw-file> <seed-jpeg>   # builds <photo-tree>; needs pillow, piexif
target/release/rawv-cli bench <file-or-folder>                 # pipeline timings
target/release/rawv-cli probe <file>                           # what the metadata probe finds
```

A change is done when `verify.py` passes; add a check there for new behaviour. Its script
language is in `src/script.rs` (`--script "next;dump:state.jsonl;shot:a.png;quit"`).
Set `RAWV_DATA_DIR` to keep a run's settings and caches out of the user's profile.
Run scripted windows with `--offscreen`: they then neither cover the screen nor receive the
input of the person at the computer.

## Layout

| Folder | Holds |
|---|---|
| `src/media/` | File reading: `probe` (TIFF/EXIF directories, one open and one head read per file), `load` (full image), `thumb`, `rotate`, `raw` (sensor data through rawler), `render` (camera colours to display pixels), `developed` (cache of developed RAW images), `pixels` |
| `src/library/` | Folder listing, natural sort, background lister |
| `src/thumbs/` | Thumbnail store (redb, one local file) and worker service |
| `src/gallery/` | Folder view: rolls, layout maths, selection |
| `src/viewer/` | Single-image view, full-image loader, info line |
| `src/config.rs` | Settings kept between sessions and where data is stored |
| `src/workers.rs` | `WantedPool`: workers pull from a "wanted" list the interface republishes each frame |
| `src/command.rs` | Every action as a `Command`; key bindings and script words map to it |
| `src/app.rs` | Owns all state, runs commands |
| `src/theme.rs` | All colours and fonts |
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
- Rotation rewrites the two-byte EXIF orientation value in place and keeps the modification
  time (`media/rotate.rs`). Nothing else may write to a photo.
- Colours, fonts and sizes come from `theme.rs` and `gallery/layout.rs`.
- A setting is a field of `Config` changed by one `Command`. Its key and its row in the
  settings panel (`App::settings_panel`) both send that command.
- A RAW file has two renditions (`media::load::Rendition`): the camera's embedded JPEG and
  the developed sensor data. `Viewer::renditions` decides which to load; the preview stands
  in until the developed one is ready. Developed images take brightness and contrast from
  the embedded JPEG (`media/render.rs`).
- No `unsafe` (forbidden in `Cargo.toml`), no `unwrap`/`expect` outside tests (clippy denies them).

## Known limits

- HEIC, AVIF and JPEG XL are not decoded. No colour management. Animated GIFs show the first frame.
- Files without an EXIF orientation value (PNG, plain JPEG) and RAW containers that are not
  TIFF-based (CR3, RAF) rotate for the session only.
- The thumbnail store is never pruned. A second running instance cannot open it and works
  without a thumbnail cache.
- Developed RAW images get no lens corrections (distortion, vignetting) and, without an
  embedded JPEG to follow, only the plain sRGB tone curve.
- With nested folders shown, every folder below the open one is listed at once.

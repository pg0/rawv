# rawv

A fast, minimal photo and RAW viewer for Windows. One executable, no installer.

- **Gallery**: a folder and its subfolders as film rolls; `N` shows all nested folders.
- **Viewer**: instant stepping with preloading, zoom at the pointer, fullscreen.
- **RAW**: shows the camera's embedded preview at once and, where that is small, the
  developed sensor data in full size. Developed images are cached on the computer.
- **Drag out**: drag an image from the gallery into another program or the browser.
- **Rate, reject, rotate, delete**: stars and the reject mark are saved inside the photo file,
  rotation rewrites the EXIF orientation only, Delete moves the file to the recycle bin.

Reads JPEG, PNG, WebP, GIF, BMP, TIFF and the RAW formats `rawler` decodes (ARW, DNG, CR2,
NEF, RAF, RW2, ORF and more).

## Install

Download `rawv-windows-x64.zip` from the releases, unpack it anywhere, run `rawv.exe`.
Put the folder on the `PATH` to start it from a terminal.

### Make rawv the default viewer

Open a terminal in the unpacked folder and run:

```
rawv-cli register
```

This registers `rawv.exe` for JPEG, PNG, WebP, GIF, BMP, TIFF and the RAW formats, for your
user only (no admin rights), and opens **Settings > Apps > Default apps > rawv**. Windows lets
only you change the default: pick rawv there for each file type, or right-click a photo,
**Open with > rawv > Always**.

Moved the folder? Run `rawv-cli register` again. `rawv-cli unregister` removes the entries.

```
rawv                    reopen the last folder
rawv C:\photos          open a folder
rawv C:\photos\a.arw    open a file; its folder becomes the image list
rawv --fullscreen ...
```

## Keys

| Key | Does |
|---|---|
| Space, Page Up / Down, Ctrl+Wheel | next / previous image |
| Arrows | move a zoomed-in image; when it fits, Left / Right step |
| Wheel, `+` `-` | zoom (`W` swaps wheel and Ctrl+wheel) |
| `*` / `/` | fit to window / 100 % |
| `F`, double click | fullscreen |
| `1`-`5`, `0`, `X` | rate, remove the rating, reject |
| `R`, `L`, `U` | rotate right, left, 180° |
| `Delete` | move to the recycle bin |
| `P` | RAW: camera preview / automatic / always full size |
| `N`, `T`, `S`, `A` | gallery: nested folders, thumbnail size, sort order, expand all |
| `O`, `,`, `H` | open folder, settings, all keys |
| `Esc`, `Q` | back, quit |

## Good to know

- Settings live in `%APPDATA%\rawv`, the thumbnail and RAW caches in `%LOCALAPPDATA%\rawv`.
  **Settings > Thumbnail and RAW caches > Clear** empties both.
- Ratings are XMP metadata written into the photo file itself; no sidecar files. A Sony RAW
  file without a rating field grows by about 3 KB the first time it is rated.
- A network share has no recycle bin: there, Delete asks first and then deletes for good.
- Not supported: HEIC, AVIF, JPEG XL, colour management, lens corrections for developed RAW.

## Build

```
cargo build --release          # target/release/rawv.exe and rawv-cli.exe
cargo test --release
python tools/verify.py <photo-tree>    # drives the real window; see AGENTS.md
```

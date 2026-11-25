# Feature map

User-facing feature, where it lives, and the script word that triggers it
(`rawv --script "word;word"`, see `src/script.rs`).

## Start

| Feature | Entry | Trigger |
|---|---|---|
| Open a folder in the gallery | `App::open` | `rawv <folder>`, `open:<folder>`, drop, `O`, `+` button |
| Open a file straight in the viewer | `App::open`, `Viewer::adopt` | `rawv <file>`, `open:<file>`, drop |
| Reopen the last folder | `Config::last_folder` | `rawv` |
| Start fullscreen | `Startup::fullscreen` | `rawv --fullscreen` |
| Window opens where it was, also after fullscreen | `App::track_window`, `App::set_fullscreen` | automatic |
| Register as a program for photo file types | `default_app::register`, `default_app::unregister` | `rawv-cli register`, `rawv-cli unregister` |

## Gallery (`src/gallery/`)

| Feature | Entry | Trigger |
|---|---|---|
| Folder and subfolders as rolls | `Gallery::on_listing` | automatic |
| All nested subfolders as rolls | `Command::ToggleNested`, `Gallery::set_nested` | `nested`, `N`, subfolders button in the header |
| Camera thumbnail first, sharper one after | `ThumbService::lookup` | automatic |
| Expand or collapse a roll | `Command::ToggleRoll` | chevron, title of the open folder |
| Expand or collapse all | `Command::ToggleAllRolls` | `toggle-all`, `A`, expand button in the header |
| Enter a subfolder | `Command::Open` | click roll title, breadcrumb |
| Parent folder | `Command::GoParent` | `parent`, `Backspace` |
| Move selection | `Gallery::move_selection` | `left` `right` `up` `down`, arrow keys |
| Move selection by pages, to first or last image | `Gallery::move_selection`, `Gallery::select_end` | `page-up` `page-down` `select-first` `select-last`, Page Up, Page Down, Home, End |
| Open image | `Command::OpenImage` | click frame, `open-selected`, `Enter` |
| Right-click menu: open in the default app, show in folder | `Command::OpenExternally`, `Command::ShowInFolder`, `App::hand_to_explorer` | right click on a frame, `open-externally`, `show-in-folder` |
| Drag an image's file into another program | `Command::DragOut`, `App::start_drag_out` | drag a frame out of the window |
| Thumbnail size | `Command::ScaleThumbs` | `thumbs-larger`, `thumbs-smaller`, `+` `-`, Ctrl+wheel |
| Thumbnail size S / M / L | `Command::CycleThumbSize`, `layout::THUMB_SIZES` | `thumb-size`, `T`, size button in the header |
| Sort order | `Command::CycleSort` | `sort`, `S`, sort button in the header |
| Reload | `Command::Refresh` | `refresh`, `F5` |

## Viewer (`src/viewer/`)

| Feature | Entry | Trigger |
|---|---|---|
| Next / previous | `App::step_viewer`, `Viewer::step` | `next` `prev`, Space, Page Up / Down, Backspace, Ctrl+wheel, edge buttons |
| Continue into the next folder (nested view) | `App::step_viewer`, `Viewer::enter_roll` | same as next / previous |
| First / last | `Viewer::go_to` | `first` `last`, Home, End |
| Zoom at pointer | `Viewer::zoom_to` | `zoom-in` `zoom-out`, `+` `-`, wheel |
| Fit / 100 % | `Viewer::zoom_fit`, `Viewer::zoom_actual` | `zoom-fit` `zoom-actual`, `*`, `/`, middle click switches |
| Wheel zooms or steps | `Command::ToggleWheelAction`, `Viewer::handle_wheel` | `wheel-action`, `W` |
| Pan | `Viewer::ui` | drag |
| Pan with the keyboard | `Command::Arrow`, `Viewer::pan_step` | `arrow-left` `arrow-right` `arrow-up` `arrow-down`, `key:ArrowRight`, arrow keys |
| Arrow keys move or always step | `Command::ToggleArrowAction`, `Config::arrows` | `arrow-action`, settings panel |
| RAW: camera preview, automatic or developed | `Command::CycleRawMode`, `Viewer::renditions` | `raw-mode`, `P` |
| Camera preview shown until the developed image is ready | `Viewer::shown` | automatic |
| Developed RAW images cached on this computer | `media::developed`, `Command::CycleRawCache` | `raw-cache`, settings panel |
| Animated GIF and WebP play | `media::animation`, `Loaded::frame_at` | automatic |
| Info line | `Viewer::paint_info` | `info`, `I` |
| Neighbour preloading | `Viewer::wanted`, `FullLoader` | automatic |
| Back to gallery | `App::close_viewer` | `escape`, Esc, Enter, close button |

## Everywhere

| Feature | Entry | Trigger |
|---|---|---|
| Fullscreen | `App::set_fullscreen` | `fullscreen`, `F`, F11, double click in viewer |
| Clear the thumbnail and RAW caches | `Command::ClearCaches`, `ThumbStore::clear`, `DevelopedCache::clear` | `clear-caches`, Clear in the settings panel |
| Rotate 90° / 180°, saved in the file | `App::rotate`, `media::rotate::persist` | `rotate-cw` `rotate-ccw` `rotate-180`, `R`, `L`, `U` |
| Rate with 1 to 5 stars, saved in the file | `App::rate`, `media::rate::persist` | `rate:N`, `1` to `5`, `0` removes |
| Reject mark, saved in the file | `App::toggle_reject` | `reject`, `X` |
| Rating shown bottom right, not in fullscreen | `draw::rating` | automatic |
| Delete to the recycle bin; asks before deleting for good where there is none | `App::remove`, `media::remove` | `remove`, `confirm-remove`, Delete, Enter in the question |
| Help | `App::paint_help` | `help`, `H`, F1 |
| Settings panel | `App::settings_panel` | `settings`, `,`, `Settings` button |
| Quit | `Command::Quit` | `quit`, `Q` |

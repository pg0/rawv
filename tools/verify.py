"""Drives the real rawv window through scripted sessions and checks the resulting state.

Usage: python tools/verify.py PHOTO_TREE [--keep] [--only NAME]

PHOTO_TREE is a folder made by tools/make_test_photos.py. Each session works on a fresh
copy of it with its own settings folder, so nothing outside the temp folder is touched.
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import winreg
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
RAWV = REPO / "target" / "release" / "rawv.exe"
CLI = REPO / "target" / "release" / "rawv-cli.exe"
ICON = REPO / "assets" / "rawv.ico"

# Window points for a fresh 1500x950 window at thumbnail scale 1.
FIRST_FRAME = "246,115"
SECOND_FRAME = "372,115"
SECOND_ROLL_TITLE = "60,299"
CENTER = "750,475"
NEXT_BUTTON = "1465,475"
CLOSE_BUTTON = "1469,31"
CLEAR_CACHES_BUTTON = "803,477"
ARROW_KEYS_LIST = "803,389"
NEXT_PREVIOUS_CHOICE = "747,440"

failures = []


def check(name, condition, detail=""):
    print(f"  {'PASS' if condition else 'FAIL'}  {name}" + (f"  ({detail})" if detail and not condition else ""))
    if not condition:
        failures.append(name)


class Session:
    """One run of the app on a fresh copy of the photo tree."""

    def __init__(self, source, work):
        self.work = Path(work)
        self.photos = self.work / "shoot"
        shutil.copytree(Path(source) / "shoot", self.photos)
        self.data = self.work / "data"
        self.data.mkdir()
        self.dump_file = self.work / "state.jsonl"

    def run(self, target, steps, extra=("--offscreen",)):
        """Runs the app off screen; every `dump` step yields one state dict."""
        if self.dump_file.exists():
            self.dump_file.unlink()
        script = ";".join(step.replace("dump", f"dump:{self.dump_file}") if step == "dump" else step for step in steps)
        env = dict(os.environ, RAWV_DATA_DIR=str(self.data))
        result = subprocess.run(
            [str(RAWV), str(target), "--timing", "--script", script, *extra],
            env=env, capture_output=True, text=True, timeout=180,
        )
        self.stderr = result.stderr
        if not self.dump_file.exists():
            return []
        return [json.loads(line) for line in self.dump_file.read_text(encoding="utf-8").splitlines()]

    def timing(self, label):
        for line in self.stderr.splitlines():
            if line.startswith(f"timing {label} "):
                return float(line.split()[2])
        return None


def cli(*arguments):
    return subprocess.run([str(CLI), *map(str, arguments)], capture_output=True, text=True, timeout=120).stdout


def probe(path, field="orientation"):
    for line in cli("probe", path).splitlines():
        if line.startswith(field + " "):
            return int(line.split()[1])
    return None


def gallery(session):
    states = session.run(session.photos, [
        "dump",
        "right", "right", "down", "dump",
        "select-first", "dump", f"drag:{SECOND_FRAME},700,600", "dump", f"right-click:{SECOND_FRAME}", "show-in-folder", "dump", "escape", "page-down", "dump", "select-last", "dump",
        "select-first", "right", "right", "down",
        "toggle-all", "dump", "toggle-all", "toggle-all", "dump",
        "sort", "dump", "sort", "sort", "sort",
        "thumb-size", "dump", "thumb-size", "thumb-size",
        "thumbs-larger", "dump", "thumbs-smaller",
        f"move:{CENTER}", "ctrl-wheel:1", "dump", "ctrl-wheel:-1",
        f"click:{SECOND_ROLL_TITLE}", "dump",
        "parent", "dump",
        "quit",
    ])
    check("gallery: fifteen state dumps", len(states) == 15, f"got {len(states)}")
    if len(states) != 15:
        return
    first, selected, home, dragged, shown, paged, end, expanded, collapsed, sorted_desc, medium, larger, wheel, entered, parent = states
    titles = [roll["title"] for roll in first["rolls"]]
    counts = [roll["images"] for roll in first["rolls"]]
    check("gallery: folder and subfolders become rolls",
          titles == ["shoot", "2026-05 Berlin", "2026-06 Wedding", "archive", "empty", "odd files", "raw only", "small preview"], str(titles))
    check("gallery: image counts per roll", counts == [15, 40, 14, 0, 0, 1, 6, 1], str(counts))
    check("gallery: natural name order", first["rolls"][0]["first"] == "DSC00000.ARW", first["rolls"][0]["first"])
    check("gallery: only the open folder starts expanded",
          [roll["expanded"] for roll in first["rolls"]] == [True] + [False] * 7)
    check("gallery: arrow keys move the selection by column and row", selected["selection"] == [0, 11], str(selected["selection"]))
    check("gallery: Home selects the first image", home["selection"] == [0, 0], str(home["selection"]))
    check("gallery: Page Down moves the selection several rows", paged["selection"] not in ([0, 0], home["selection"]) and paged["selection"][0] >= 0,
          str(paged["selection"]))
    last_roll, last_index = end["selection"]
    check("gallery: End selects the last image of the last roll",
          last_roll > 0 and last_index == end["rolls"][last_roll]["images"] - 1, str(end["selection"]))
    check("gallery: dragging a frame offers its file to other programs",
          dragged["dragged_out"] is not None and dragged["dragged_out"].endswith("DSC00001.ARW") and dragged["mode"] == "Gallery",
          str(dragged["dragged_out"]))
    check("gallery: the right-click menu's actions go to the selected image",
          shown["handed_over"] is not None and shown["handed_over"][0] == "Shown in its folder"
          and shown["handed_over"][1].endswith("DSC00001.ARW") and shown["selection"] == [0, 1], str(shown["handed_over"]))
    check("gallery: toggle-all expands every roll", all(roll["expanded"] for roll in expanded["rolls"]))
    check("gallery: toggle-all twice returns to all expanded", all(roll["expanded"] for roll in collapsed["rolls"]))
    check("gallery: sort cycles to name descending",
          sorted_desc["sort"] == "NameDescending" and sorted_desc["rolls"][0]["first"] == "portrait_noexif.jpg",
          f'{sorted_desc["sort"]} {sorted_desc["rolls"][0]["first"]}')
    check("gallery: the size switch goes from S to M", abs(medium["thumb_scale"] - 1.6) < 0.001, str(medium["thumb_scale"]))
    check("gallery: thumbnails grow on command", abs(larger["thumb_scale"] - 1.15) < 0.01, str(larger["thumb_scale"]))
    check("gallery: ctrl+wheel scales thumbnails", wheel["thumb_scale"] > 1.0, str(wheel["thumb_scale"]))
    check("gallery: clicking a roll title opens that folder",
          entered["folder"].endswith("2026-05 Berlin") and entered["rolls"][0]["images"] == 40, entered["folder"])
    check("gallery: Backspace returns to the parent folder", parent["folder"].endswith("shoot"), parent["folder"])
    config = json.loads((session.data / "config.json").read_text(encoding="utf-8"))
    check("gallery: last folder and sort are saved on exit",
          config["last_folder"].endswith("shoot") and config["sort"] == "NameAscending", str(config))
    check("gallery: thumbnail cache file exists", (session.data / "thumbs.redb").stat().st_size > 0)

    reopened = session.run("--timing", ["dump", "quit"])
    check("gallery: reopens the last folder without a path argument",
          len(reopened) == 1 and reopened[0]["folder"].endswith("shoot"))


def viewer(session):
    states = session.run(session.photos, [
        f"click:{FIRST_FRAME}", "dump",
        f"move:{CENTER}", "ctrl-wheel:-1", "dump", "ctrl-wheel:1", "dump",
        "next", "next", "next", "next", "dump", "next", "dump",
        "wheel:2", "dump", "zoom-fit", "zoom-actual", "zoom-actual", "dump",
        "drag:700,500,600,450", "dump", f"middle-click:{CENTER}", "dump",
        "zoom-actual", "type:*", "dump",
        "wheel-action", "wheel:-1", "dump", "wheel:1", "wheel-action",
        f"double-click:{CENTER}", "dump", "pause:800", f"double-click:{CENTER}", "dump",
        "info", "dump", "info",
        "help", "dump", "escape", "dump",
        "last", "dump", "next", "dump", "first", "prev", "dump",
        "escape", "dump",
        "quit",
    ])
    check("viewer: all state dumps arrive", len(states) == 20, f"got {len(states)}")
    if len(states) != 20:
        return
    (opened, wheel_next, wheel_prev, png, jpeg, zoomed, actual, dragged, refit, star_fit, wheel_steps,
     full_on, full_off, info_off, help_on, help_off, last, past_last, before_first, closed) = states
    view = opened["viewer"]
    check("viewer: clicking a frame opens it", opened["mode"] == "Viewer" and view["name"] == "DSC00000.ARW", str(view))
    check("viewer: RAW shows the full-size camera preview",
          view["loaded"] and view["source"] == "RawPreview" and view["size"] == [4240, 2384], str(view))
    check("viewer: ctrl+wheel down goes to the next image", wheel_next["viewer"]["index"] == 1)
    check("viewer: ctrl+wheel up goes back", wheel_prev["viewer"]["index"] == 0)
    check("viewer: PNG loads", png["viewer"]["name"] == "graphic.png" and png["viewer"]["source"] == "Bitmap", str(png["viewer"]))
    check("viewer: 24 MP JPEG loads at full size",
          jpeg["viewer"]["name"] == "IMG_0001.JPG" and jpeg["viewer"]["size"] == [6000, 4000], str(jpeg["viewer"]))
    fit = jpeg["viewer"]["zoom_percent"]
    check("viewer: starts fitted to the window", 20 < fit < 60, str(fit))
    check("viewer: the wheel zooms in by default",
          zoomed["wheel"] == "Zoom" and abs(zoomed["viewer"]["zoom_percent"] - fit * 1.25 ** 2) < 0.5 and zoomed["viewer"]["index"] == 5,
          str(zoomed["viewer"]))
    check("viewer: 100 % zoom stays at 100 % when asked twice",
          abs(actual["viewer"]["zoom_percent"] - 100) < 0.01, str(actual["viewer"]["zoom_percent"]))
    pan_before, pan_after = actual["viewer"]["pan"], dragged["viewer"]["pan"]
    check("viewer: dragging pans by the drag distance",
          abs(pan_after[0] - pan_before[0] + 100) < 1 and abs(pan_after[1] - pan_before[1] + 50) < 1,
          f"{pan_before} -> {pan_after}")
    check("viewer: middle click returns to fit", abs(refit["viewer"]["zoom_percent"] - fit) < 0.5 and refit["viewer"]["pan"] == [0, 0],
          str(refit["viewer"]))
    check("viewer: * fits the image to the window",
          abs(star_fit["viewer"]["zoom_percent"] - fit) < 0.5 and star_fit["viewer"]["pan"] == [0, 0], str(star_fit["viewer"]))
    check("viewer: after switching, the wheel steps through images",
          wheel_steps["wheel"] == "Step" and wheel_steps["viewer"]["index"] == 6 and abs(wheel_steps["viewer"]["zoom_percent"] - fit) < 5,
          str(wheel_steps["viewer"]))
    check("viewer: double click enters fullscreen", full_on["fullscreen"] is True)
    check("viewer: double click leaves fullscreen", full_off["fullscreen"] is False)
    check("viewer: info line toggles", info_off["show_info"] is False)
    check("viewer: help opens and Escape closes only the help",
          help_on["help"] is True and help_off["help"] is False and help_off["mode"] == "Viewer")
    check("viewer: End jumps to the last image", last["viewer"]["index"] == 14 and last["viewer"]["name"] == "portrait_noexif.jpg")
    check("viewer: stepping past the end stays on the last image", past_last["viewer"]["index"] == 14)
    check("viewer: stepping before the start stays on the first image", before_first["viewer"]["index"] == 0)
    check("viewer: Escape returns to the gallery", closed["mode"] == "Gallery" and closed["viewer"] is None)


def viewer_close(session):
    states = session.run(session.photos, [
        f"click:{FIRST_FRAME}", "next", "next", "fullscreen", "dump", "escape", "dump", "quit",
    ])
    check("viewer close: two state dumps", len(states) == 2, f"got {len(states)}")
    if len(states) != 2:
        return
    check("viewer close: Escape returns to the gallery and leaves fullscreen",
          states[0]["fullscreen"] is True and states[1]["mode"] == "Gallery" and states[1]["fullscreen"] is False)
    check("viewer close: the last viewed image is selected", states[1]["selection"] == [0, 2], str(states[1]["selection"]))

    # The round buttons at the window's edges, shown while the pointer is near them.
    states = session.run(session.photos, [
        f"click:{FIRST_FRAME}", f"move:{NEXT_BUTTON}", f"click:{NEXT_BUTTON}", "dump",
        f"move:{CLOSE_BUTTON}", f"click:{CLOSE_BUTTON}", "dump", "quit",
    ])
    check("viewer close: the next button at the right edge steps forward",
          len(states) == 2 and states[0]["mode"] == "Viewer" and states[0]["viewer"]["index"] == 1, str(states and states[0]["viewer"]))
    check("viewer close: the close button at the top right returns to the gallery",
          len(states) == 2 and states[1]["mode"] == "Gallery", str(states and states[-1]["mode"]))


def rotation(session):
    jpeg = session.photos / "IMG_0001.JPG"
    raw = session.photos / "DSC00000.ARW"
    plain = session.photos / "portrait_noexif.jpg"
    before = {path: (path.stat().st_size, path.stat().st_mtime_ns, path.read_bytes()) for path in (jpeg, raw, plain)}
    check("rotation: files start upright", probe(jpeg) == 1 and probe(raw) == 1)

    states = session.run(jpeg, ["rotate-cw", "dump", "quit"])
    check("rotation: R shows the image rotated", len(states) == 1 and states[0]["viewer"]["orientation"] == 6)
    check("rotation: saved message", len(states) == 1 and "saved in file" in (states[0]["toast"] or ""), str(states and states[0]["toast"]))
    check("rotation: JPEG orientation written to the file", probe(jpeg) == 6, str(probe(jpeg)))
    after = jpeg.read_bytes()
    differing = sum(1 for a, b in zip(before[jpeg][2], after) if a != b)
    check("rotation: exactly one byte of the JPEG changed", len(after) == before[jpeg][0] and differing == 1, f"{differing} bytes")
    check("rotation: JPEG modification time kept", jpeg.stat().st_mtime_ns == before[jpeg][1])

    states = session.run(jpeg, ["dump", "rotate-ccw", "dump", "quit"])
    check("rotation: reopened image is shown rotated", len(states) == 2 and states[0]["viewer"]["orientation"] == 6)
    check("rotation: counter-clockwise restores the original file", jpeg.read_bytes() == before[jpeg][2])

    states = session.run(raw, ["rotate-180", "dump", "quit"])
    check("rotation: RAW orientation written to the file", probe(raw) == 3, str(probe(raw)))
    after = raw.read_bytes()
    differing = sum(1 for a, b in zip(before[raw][2], after) if a != b)
    check("rotation: exactly one byte of the RAW changed", len(after) == before[raw][0] and differing == 1, f"{differing} bytes")

    states = session.run(plain, ["rotate-cw", "dump", "quit"])
    check("rotation: file without EXIF rotates in view only",
          len(states) == 1 and states[0]["viewer"]["orientation"] == 6 and "this session" in (states[0]["toast"] or ""),
          str(states and states[0]["toast"]))
    check("rotation: file without EXIF is untouched", plain.read_bytes() == before[plain][2])

    states = session.run(session.photos, ["right", "dump", "rotate-cw", "dump", "quit"])
    check("rotation: gallery shows the orientation saved earlier",
          len(states) == 2 and states[0]["selected_orientation"] == 3, str(states and states[0]["selected_orientation"]))
    check("rotation: R rotates the selected thumbnail",
          len(states) == 2 and states[1]["selected_orientation"] == 8, str(states and states[-1]["selected_orientation"]))
    check("rotation: gallery rotation written to the file", probe(raw) == 8, str(probe(raw)))
    states = session.run(session.photos, ["right", "dump", "quit"])
    check("rotation: cached thumbnail keeps the rotation after a restart",
          len(states) == 1 and states[0]["selected_orientation"] == 8, str(states and states[0]["selected_orientation"]))

    states = session.run(session.photos / "IMG_0003.JPG", ["dump", "quit"])
    check("rotation: camera-rotated JPEG is shown upright", len(states) == 1 and states[0]["viewer"]["orientation"] == 6)


def raw(session):
    small = session.photos / "small preview" / "DSC20000.ARW"
    cache = session.data / "developed"
    states = session.run(small, ["dump", "quit"])
    view = states[0]["viewer"] if states else None
    check("raw: automatic mode develops a file whose camera preview is small",
          bool(view) and view["source"] == "RawDeveloped" and view["size"] == [4240, 2832] and view["cached"] is False, str(view))
    stored = list(cache.glob("*.developed"))
    check("raw: the developed image is kept in the local cache", len(stored) == 1 and not list(cache.glob("*.part")), str(stored))
    states = session.run(small, ["dump", "raw-cache", "raw-cache", "raw-cache", "dump", "quit"])
    view = states[0]["viewer"] if states else None
    check("raw: the next start takes it from the cache",
          bool(view) and view["source"] == "RawDeveloped" and view["size"] == [4240, 2832] and view["cached"] is True, str(view))
    check("raw: switching the cache off empties it", len(states) == 2 and not list(cache.glob("*.developed")), str(list(cache.iterdir())))
    (session.data / "config.json").unlink()

    states = session.run(session.photos / "DSC00001.ARW", [
        "dump", "raw-mode", "dump", "raw-mode", "dump", "raw-mode", "raw-mode", "quit",
    ])
    check("raw: three state dumps", len(states) == 3, f"got {len(states)}")
    if len(states) != 3:
        return
    check("raw: automatic mode keeps a full-size camera preview",
          states[0]["raw"] == "Auto" and states[0]["viewer"]["source"] == "RawPreview" and states[0]["viewer"]["size"] == [4240, 2384],
          str(states[0]["viewer"]))
    check("raw: P switches to developing the sensor data",
          states[1]["raw"] == "Developed" and states[1]["viewer"]["source"] == "RawDeveloped"
          and states[1]["viewer"]["size"] == [4240, 2832], str(states[1]["viewer"]))
    check("raw: P again shows the camera preview",
          states[2]["raw"] == "Embedded" and states[2]["viewer"]["source"] == "RawPreview", str(states[2]["viewer"]))
    states = session.run(session.photos / "DSC00001.ARW", ["dump", "quit"])
    check("raw: the chosen mode is kept for the next start",
          len(states) == 1 and states[0]["raw"] == "Developed" and states[0]["viewer"]["source"] == "RawDeveloped",
          str(states and states[0]["viewer"]))


def startup(session):
    states = session.run(session.photos / "IMG_0005.JPG", ["dump", "prev", "dump", "quit"])
    check("startup: two state dumps", len(states) == 2, f"got {len(states)}")
    if len(states) == 2:
        view = states[0]["viewer"]
        check("startup: a file argument opens straight in the viewer", states[0]["mode"] == "Viewer" and view["name"] == "IMG_0005.JPG")
        check("startup: the file's folder becomes the image list", view["total"] == 15 and view["index"] == 9, str(view))
        check("startup: stepping reaches the neighbouring file", states[1]["viewer"]["name"] == "IMG_0004.JPG")
    first_frame, content = session.timing("first-frame"), session.timing("content-ready")
    print(f"        first frame {first_frame} ms, image shown {content} ms after process start")
    check("startup: image shown within one second", content is not None and content < 1000, str(content))

    berlin = session.photos / "2026-05 Berlin"
    states = session.run(session.photos, [f"drop:{berlin / 'IMG_0102.JPG'}", "dump", f"drop:{session.photos}", "dump", "quit"])
    check("startup: dropping a file opens it in the viewer",
          len(states) == 2 and states[0]["mode"] == "Viewer" and states[0]["viewer"]["name"] == "IMG_0102.JPG"
          and states[0]["viewer"]["total"] == 40, str(states and states[0]["viewer"]))
    check("startup: dropping a folder opens it in the gallery",
          len(states) == 2 and states[1]["mode"] == "Gallery" and states[1]["folder"].endswith("shoot"))

    odd = session.photos / "odd files"
    states = session.run(odd / "png_named.jpg", ["dump", "escape", "dump", "quit"])
    view = states[0]["viewer"] if states else None
    check("startup: a file named .jpg that holds a PNG opens",
          bool(view) and view["loaded"] and view["source"] == "Bitmap" and view["size"] == [800, 600], str(view))
    check("startup: and gets a thumbnail", len(states) == 2 and states[1]["rolls"][0]["images"] == 1 and states[1]["toast"] is None,
          str(states and states[-1]["toast"]))

    states = session.run(session.photos, ["dump", "quit"], extra=["--offscreen", "--fullscreen"])
    check("startup: --fullscreen starts fullscreen", len(states) == 1 and states[0]["fullscreen"] is True)


def nested(session):
    states = session.run(session.photos, [
        "nested", "dump",
        f"click:{FIRST_FRAME}", "last", "next", "dump", "prev", "dump",
        "next", "last", "next", "last", "next", "dump", "prev", "dump",
        "escape", "dump",
        "quit",
    ])
    check("nested: six state dumps", len(states) == 6, f"got {len(states)}")
    if len(states) != 6:
        return
    listed, into_next, back, past_container, back_over_container, closed = states
    titles = [roll["title"] for roll in listed["rolls"]]
    counts = [roll["images"] for roll in listed["rolls"]]
    check("nested: every nested folder becomes a roll after its parent",
          titles == ["shoot", "2026-05 Berlin", "2026-06 Wedding", "archive", "2019", "empty", "odd files", "raw only", "small preview"], str(titles))
    check("nested: all rolls are listed without scrolling to them", counts == [15, 40, 14, 0, 3, 0, 1, 6, 1], str(counts))
    check("nested: a deeper roll knows its parent folders", listed["rolls"][4]["trail"] == ["archive"], str(listed["rolls"][4]))
    view = into_next["viewer"]
    check("nested: stepping past the last image enters the next roll",
          view["roll"] == 1 and view["index"] == 0 and view["total"] == 40, str(view))
    view = back["viewer"]
    check("nested: stepping back returns to the last image of the roll before",
          view["roll"] == 0 and view["index"] == 14 and view["name"] == "portrait_noexif.jpg", str(view))
    view = past_container["viewer"]
    check("nested: a folder without images is skipped", view["roll"] == 4 and view["index"] == 0 and view["total"] == 3, str(view))
    view = back_over_container["viewer"]
    check("nested: and skipped on the way back", view["roll"] == 2 and view["index"] == 13, str(view))
    check("nested: closing selects the viewed image in its roll",
          closed["mode"] == "Gallery" and closed["selection"] == [2, 13], str(closed["selection"]))

    states = session.run("--timing", ["dump", "nested", "dump", f"click:{FIRST_FRAME}", "last", "next", "dump", "quit"])
    check("nested: the choice is kept for the next start",
          len(states) == 3 and states[0]["nested"] is True and len(states[0]["rolls"]) == 9)
    check("nested: switching it off shows direct subfolders only",
          len(states) == 3 and states[1]["nested"] is False and len(states[1]["rolls"]) == 8)
    check("nested: without it, stepping stays inside the roll",
          len(states) == 3 and states[2]["viewer"]["roll"] == 0 and states[2]["viewer"]["index"] == 14)


def settings(session):
    states = session.run(session.photos, [
        "settings", "dump", f"shot:{session.work / 'settings.png'}", "escape", "dump",
        "help", "settings", "dump", "help", "dump", "escape",
        "quit",
    ])
    check("settings: four state dumps", len(states) == 4, f"got {len(states)}")
    if len(states) != 4:
        return
    opened, closed, over_help, help_again = states
    check("settings: the panel opens", opened["settings"] is True and opened["mode"] == "Gallery")
    check("settings: Escape closes only the panel", closed["settings"] is False and closed["mode"] == "Gallery")
    check("settings: settings and help replace each other",
          over_help["settings"] is True and over_help["help"] is False
          and help_again["settings"] is False and help_again["help"] is True)
    states = session.run(session.photos, ["settings", f"click:{ARROW_KEYS_LIST}", f"click:{NEXT_PREVIOUS_CHOICE}", "dump", "quit"])
    chosen = states[0]["arrows"] if states else None
    check("settings: a value is picked from a list", chosen == "Step", str(chosen))


def window(session):
    geometry = {"size": [1200, 800], "position": [150, 90], "maximized": False}
    (session.data / "config.json").write_text(json.dumps({"window": geometry}), encoding="utf-8")
    states = session.run(session.photos, ["dump", "fullscreen", "dump", "fullscreen", "dump", "quit"], extra=())
    check("window: three state dumps", len(states) == 3, f"got {len(states)}")
    if len(states) != 3:
        return
    check("window: opens where it was last time", states[0]["window"] == [[150, 90], [1200, 800], False], str(states[0]["window"]))
    check("window: leaving fullscreen puts it back in the same place",
          states[1]["fullscreen"] is True and states[2]["fullscreen"] is False and states[2]["window"] == states[0]["window"],
          str(states[2]["window"]))


def delete(session):
    png = session.photos / "graphic.png"
    states = session.run(session.photos, [
        f"click:{FIRST_FRAME}", "next", "next", "next", "next", "dump", "remove", "dump", "escape", "dump", "quit",
    ])
    check("delete: three state dumps", len(states) == 3, f"got {len(states)}")
    if len(states) != 3:
        return
    before, after, closed = states
    check("delete: the image to delete is shown", before["viewer"]["name"] == "graphic.png" and before["viewer"]["total"] == 15)
    check("delete: Delete moves the file to the recycle bin",
          not png.exists() and "recycle bin" in (after["toast"] or ""), str(after["toast"]))
    check("delete: the viewer shows the image that followed",
          after["viewer"]["name"] == "IMG_0001.JPG" and after["viewer"]["index"] == 4 and after["viewer"]["total"] == 14,
          str(after["viewer"]))
    check("delete: the gallery no longer lists the file",
          closed["rolls"][0]["images"] == 14 and closed["selection"] == [0, 4], str(closed["selection"]))

    # A network share has no recycle bin; this computer's own share stands in for one.
    share = Path("//localhost/" + str(session.photos).replace(":", "$", 1))
    if not share.is_dir():
        print("        skipped: no local share to test deleting without a recycle bin")
        return
    raw = session.photos / "DSC00000.ARW"
    states = session.run(share, ["right", "remove", "dump", "escape", "dump", "quit"])
    check("delete: two state dumps on a share", len(states) == 2, f"got {len(states)}")
    if len(states) != 2:
        return
    asked, kept = states
    check("delete: without a recycle bin it asks first", asked["confirm_remove"] == "DSC00000.ARW", str(asked["confirm_remove"]))
    check("delete: Escape keeps the file",
          kept["confirm_remove"] is None and raw.exists() and kept["rolls"][0]["images"] == 14, str(kept["rolls"][0]))
    states = session.run(share, ["right", "remove", "confirm-remove", "dump", "quit"])
    deleted = states[0] if states else {"rolls": [{}], "selection": None, "toast": None}
    check("delete: after agreeing the file is deleted",
          not raw.exists() and deleted["rolls"][0].get("images") == 13 and deleted["selection"] == [0, 0]
          and "deleted" in (deleted["toast"] or ""), str(deleted["toast"]))


def rating(session):
    raw = session.photos / "DSC00000.ARW"
    original = raw.read_bytes()
    states = session.run(session.photos, ["right", "rate:4", "dump", "reject", "dump", "reject", "dump", "quit"])
    check("rating: three state dumps", len(states) == 3, f"got {len(states)}")
    if len(states) != 3:
        return
    check("rating: a number key rates the selected image", states[0]["selected_rating"] == 4, str(states[0]["selected_rating"]))
    check("rating: X rejects it", states[1]["selected_rating"] == -1, str(states[1]["selected_rating"]))
    check("rating: X again takes the mark back", states[2]["selected_rating"] == 0, str(states[2]["selected_rating"]))
    check("rating: a RAW file with a rating field is changed in place and restored exactly", raw.read_bytes() == original)

    states = session.run(session.photos, ["right", "rate:2", "dump", "quit"])
    changed = raw.read_bytes()
    differing = sum(1 for a, b in zip(original, changed) if a != b)
    check("rating: two stars change one byte of the RAW file",
          len(changed) == len(original) and differing == 1 and probe(raw, "rating") == 2, f"{differing} bytes")

    jpeg = session.photos / "IMG_0002.JPG"
    size, modified, pixels = jpeg.stat().st_size, jpeg.stat().st_mtime_ns, cli("pixels", jpeg)
    states = session.run(jpeg, ["rate:5", "dump", "quit"])
    check("rating: the viewer shows the new rating at once", len(states) == 1 and states[0]["viewer"]["rating"] == 5)
    check("rating: a JPEG without a rating field gets one; picture and date stay",
          probe(jpeg, "rating") == 5 and jpeg.stat().st_size > size and jpeg.stat().st_mtime_ns == modified
          and cli("pixels", jpeg) == pixels and pixels != "", cli("pixels", jpeg))
    check("rating: no helper file is left behind", not list(session.photos.glob("*.rawv-part")) and not list(session.photos.glob("*.xmp")))
    states = session.run(jpeg, ["dump", "escape", "dump", "quit"])
    check("rating: the rating is read back from the file",
          len(states) == 2 and states[0]["viewer"]["rating"] == 5 and states[1]["selected_rating"] == 5, str(states and states[0]["viewer"]))

    png = session.photos / "graphic.png"
    before = png.read_bytes()
    states = session.run(png, ["rate:3", "dump", "quit"])
    check("rating: a file that cannot hold a rating says so and stays untouched",
          len(states) == 1 and "cannot hold a rating" in (states[0]["toast"] or "") and states[0]["viewer"]["rating"] == 0
          and png.read_bytes() == before, str(states and states[0]["toast"]))


def keyboard(session):
    states = session.run(session.photos, [
        f"click:{FIRST_FRAME}", "next", "next", "next", "next", "next", "zoom-actual", "dump",
        "key:ArrowRight", "dump", "key:ArrowDown", "dump", "key:ArrowLeft", "key:ArrowLeft", "dump",
        *["key:ArrowRight"] * 20, "dump",
        "zoom-fit", "key:ArrowRight", "dump", "key:ArrowLeft", "dump", "key:ArrowDown", "dump",
        "key:PageDown", "dump", "key:PageUp", "dump", "key:Space", "dump",
        "arrow-action", "zoom-actual", "key:ArrowDown", "dump", "key:ArrowUp", "dump",
        "quit",
    ])
    check("keyboard: all state dumps arrive", len(states) == 13, f"got {len(states)}")
    if len(states) != 13:
        return
    (actual, right, down, left, edge, fit_right, fit_left, fit_down, page_down, page_up, space,
     step_down, step_up) = states
    view = actual["viewer"]
    check("keyboard: a 24 MP JPEG at 100 %", view["name"] == "IMG_0001.JPG" and abs(view["zoom_percent"] - 100) < 0.01, str(view))
    start = view["pan"]
    moved = right["viewer"]["pan"]
    check("keyboard: Right moves a zoomed-in image to show more on the right",
          moved[0] < start[0] - 50 and moved[1] == start[1] and right["viewer"]["index"] == 5, f"{start} -> {moved}")
    check("keyboard: Down shows more below", down["viewer"]["pan"][1] < moved[1] - 50, str(down["viewer"]["pan"]))
    check("keyboard: Left moves back", left["viewer"]["pan"][0] > down["viewer"]["pan"][0] + 50, str(left["viewer"]["pan"]))
    check("keyboard: at the right edge Right stays on the image",
          edge["viewer"]["index"] == 5 and edge["viewer"]["pan"][0] < left["viewer"]["pan"][0] - 500, str(edge["viewer"]))
    check("keyboard: fitted, arrows never step to another image",
          [fit_right["viewer"]["index"], fit_left["viewer"]["index"], fit_down["viewer"]["index"]] == [5, 5, 5]
          and fit_down["viewer"]["pan"] == [0, 0],
          f'{fit_right["viewer"]["index"]} {fit_left["viewer"]["index"]} {fit_down["viewer"]}')
    check("keyboard: Page Down, Page Up and Space step",
          page_down["viewer"]["index"] == 6 and page_up["viewer"]["index"] == 5 and space["viewer"]["index"] == 6,
          f'{page_down["viewer"]["index"]} {page_up["viewer"]["index"]} {space["viewer"]["index"]}')
    check("keyboard: set to step, arrows step even when zoomed in",
          step_down["arrows"] == "Step" and step_down["viewer"]["index"] == 7 and step_up["viewer"]["index"] == 6,
          f'{step_down["arrows"]} {step_down["viewer"]["index"]} {step_up["viewer"]["index"]}')


def caches(session):
    thumbs = session.data / "thumbs.redb"
    developed = session.data / "developed"
    session.run(session.photos, [f"click:{FIRST_FRAME}", "raw-mode", "escape", "quit"])
    filled = thumbs.stat().st_size if thumbs.exists() else 0
    developed_before = len(list(developed.glob("*.developed")))
    check("caches: viewing fills both caches", filled > 500_000 and developed_before > 0,
          f"thumbs {filled} bytes, {developed_before} developed")
    states = session.run(session.photos / "empty", ["settings", f"click:{CLEAR_CACHES_BUTTON}", "dump", "quit"])
    check("caches: one state dump", len(states) == 1, f"got {len(states)}")
    if len(states) != 1:
        return
    cleared = thumbs.stat().st_size
    check("caches: Clear in the settings empties the thumbnail store", cleared < filled / 2, f"{filled} -> {cleared} bytes")
    check("caches: Clear deletes the developed RAW images",
          not list(developed.glob("*.developed")), str(list(developed.iterdir())[:3]))
    check("caches: a message says so", "cleared" in (states[0]["toast"] or ""), str(states[0]["toast"]))


def registry_key(root, key):
    return winreg.OpenKey(winreg.HKEY_CURRENT_USER, root + "\\" + key if key else root)


def registry_value(root, key, name=""):
    try:
        with registry_key(root, key) as opened:
            return winreg.QueryValueEx(opened, name)[0]
    except OSError:
        return None


def registry_names(root, key):
    try:
        with registry_key(root, key) as opened:
            return {winreg.EnumValue(opened, i)[0] for i in range(winreg.QueryInfoKey(opened)[1])}
    except OSError:
        return set()


def registry_keys_with_values(root, key=""):
    """Every key below `key` that still holds a value."""
    try:
        with registry_key(root, key) as opened:
            subkeys, values, _ = winreg.QueryInfoKey(opened)
            children = [winreg.EnumKey(opened, i) for i in range(subkeys)]
    except OSError:
        return []
    found = [key] if values else []
    for child in children:
        found += registry_keys_with_values(root, key + "\\" + child if key else child)
    return found


def delete_registry_tree(root, key=""):
    try:
        with registry_key(root, key) as opened:
            children = [winreg.EnumKey(opened, i) for i in range(winreg.QueryInfoKey(opened)[0])]
    except OSError:
        return
    for child in children:
        delete_registry_tree(root, key + "\\" + child if key else child)
    winreg.DeleteKey(winreg.HKEY_CURRENT_USER, root + "\\" + key if key else root)


def register(session):
    # Everything goes below a throwaway key instead of the user's real file type settings.
    root = rf"Software\rawv-verify-{os.getpid()}"
    env = dict(os.environ, RAWV_REGISTRY_ROOT=root)
    try:
        result = subprocess.run([str(CLI), "register", str(RAWV)], env=env, capture_output=True, text=True, timeout=60)
        check("register: succeeds", result.returncode == 0, result.stderr.strip())
        command = registry_value(root, r"Software\Classes\rawv.image\shell\open\command")
        check("register: opening a photo runs rawv with its path", command == f'"{RAWV}" "%1"', str(command))
        check("register: the file type icon comes from rawv.exe",
              registry_value(root, r"Software\Classes\rawv.image\DefaultIcon") == f'"{RAWV}",0')
        check("register: listed among the programs Settings offers",
              registry_value(root, r"Software\RegisteredApplications", "rawv") == r"Software\rawv\Capabilities")
        associations = registry_names(root, r"Software\rawv\Capabilities\FileAssociations")
        check("register: offered for JPEG, PNG and RAW files",
              {".jpg", ".jpeg", ".png", ".arw", ".cr2", ".dng", ".nef"} <= associations and len(associations) >= 30,
              str(sorted(associations)))
        check("register: in Open with for .cr2",
              "rawv.image" in registry_names(root, r"Software\Classes\.cr2\OpenWithProgids"))

        result = subprocess.run([str(CLI), "unregister"], env=env, capture_output=True, text=True, timeout=60)
        check("unregister: succeeds", result.returncode == 0, result.stderr.strip())
        leftovers = registry_keys_with_values(root)
        check("unregister: no value of rawv is left", not leftovers, str(leftovers[:5]))
    finally:
        delete_registry_tree(root)

    # The icon Explorer shows for rawv.exe is the one in assets/rawv.ico.
    shown = session.work / "exe-icon.png"
    subprocess.run(["powershell.exe", "-NoProfile", "-Command",
                    "Add-Type -AssemblyName System.Drawing; "
                    f"[System.Drawing.Icon]::ExtractAssociatedIcon('{RAWV}').ToBitmap().Save('{shown}')"],
                   capture_output=True, timeout=60)
    try:
        from PIL import Image, ImageChops
        expected = Image.open(ICON)
        expected.size = (32, 32)
        expected = expected.convert("RGBA")
        actual = Image.open(shown).convert("RGBA")
        difference = max(high for _, high in ImageChops.difference(expected, actual).getextrema())
        check("icon: rawv.exe carries the rawv icon", difference <= 8, f"largest difference {difference}")
    except (ImportError, OSError) as error:
        check("icon: rawv.exe carries the rawv icon", False, str(error))


def animation(session):
    from PIL import Image
    folder = session.work / "gifs"
    folder.mkdir()
    colors = [(220, 40, 40), (40, 200, 60), (40, 80, 230), (240, 220, 40), (200, 40, 200), (40, 220, 220)]
    frames = [Image.new("RGB", (640, 400), color) for color in colors]
    frames[0].save(folder / "a_moving.gif", save_all=True, append_images=frames[1:], duration=150, loop=0)
    frames[0].save(folder / "b_still.gif")
    frames[2].save(folder / "c_still.webp")
    shot = session.work / "frame.png"
    states = session.run(folder / "a_moving.gif", [
        "dump", "pause:400", "dump", "pause:400", "dump", f"shot:{shot}", "next", "dump", "pause:400", "dump",
        "next", "dump", "quit",
    ])
    check("animation: six state dumps", len(states) == 6, f"got {len(states)}")
    if len(states) != 6:
        return
    first, later, latest, still, still_later, webp = (state["viewer"] for state in states)
    check("animation: all frames of an animated GIF load", first["name"] == "a_moving.gif" and first["frames"] == 6, str(first))
    shown = [first["frame"], later["frame"], latest["frame"]]
    check("animation: the frames play one after the other", len(set(shown)) == 3, str(shown))
    if shot.exists():
        image = Image.open(shot).convert("RGB")
        center = image.getpixel((image.width // 2, image.height // 2))
        check("animation: the window shows a frame of the GIF",
              any(sum(abs(a - b) for a, b in zip(center, color)) < 30 for color in colors), str(center))
    else:
        check("animation: the window shows a frame of the GIF", False, "no screenshot")
    check("animation: a still WebP opens as a picture",
          webp["name"] == "c_still.webp" and webp["loaded"] and webp["frames"] == 1 and webp["size"] == [640, 400],
          str(webp))
    check("animation: a single-frame GIF stays still",
          still["name"] == "b_still.gif" and still["frames"] == 1 and still["frame"] == 0 and still_later["frame"] == 0,
          str(still))


def heic(session):
    try:
        import pillow_heif
        from PIL import Image, ImageDraw
    except ImportError:
        print("        skipped: needs pillow and pillow-heif to write a HEIC file")
        return
    pillow_heif.register_heif_opener()
    folder = session.work / "heic"
    folder.mkdir()
    colors = {"top left": (220, 40, 40), "top right": (40, 200, 60), "bottom left": (40, 80, 230), "bottom right": (240, 220, 40)}
    picture = Image.new("RGB", (1200, 900))
    pen = ImageDraw.Draw(picture)
    for (x, y), color in zip([(0, 0), (600, 0), (0, 450), (600, 450)], colors.values()):
        pen.rectangle([x, y, x + 599, y + 449], fill=color)
    picture.save(folder / "a_quadrants.heic", quality=95)
    shutil.copy(session.photos / "IMG_0001.JPG", folder / "b_really_a_jpeg.heic")
    shot = session.work / "heic.png"
    states = session.run(folder / "a_quadrants.heic", ["dump", f"shot:{shot}", "next", "dump", "escape", "dump", "quit"])
    check("heic: three state dumps", len(states) == 3, f"got {len(states)}")
    if len(states) != 3:
        return
    quadrants, jpeg, gallery = states
    check("heic: a HEIC file opens at full size",
          quadrants["viewer"]["source"] == "Heic" and quadrants["viewer"]["size"] == [1200, 900], str(quadrants["viewer"]))
    if shot.exists():
        image = Image.open(shot).convert("RGB")
        middle_x, middle_y = image.width // 2, image.height // 2
        offsets = {"top left": (-1, -1), "top right": (1, -1), "bottom left": (-1, 1), "bottom right": (1, 1)}
        wrong = []
        for name, color in colors.items():
            dx, dy = offsets[name]
            shown = image.getpixel((middle_x + dx * image.width // 8, middle_y + dy * image.height // 8))
            if sum(abs(a - b) for a, b in zip(shown, color)) > 40:
                wrong.append(f"{name} {shown}")
        check("heic: shown upright in its own colours", not wrong, ", ".join(wrong))
    else:
        check("heic: shown upright in its own colours", False, "no screenshot")
    check("heic: a .heic file holding a JPEG opens as one",
          jpeg["viewer"]["name"] == "b_really_a_jpeg.heic" and jpeg["viewer"]["source"] == "Jpeg"
          and jpeg["viewer"]["size"] == [6000, 4000], str(jpeg["viewer"]))
    check("heic: both are listed in the gallery", gallery["rolls"][0]["images"] == 2, str(gallery["rolls"][0]))


SESSIONS = {
    "gallery": gallery,
    "viewer": viewer,
    "viewer-close": viewer_close,
    "rotation": rotation,
    "raw": raw,
    "startup": startup,
    "nested": nested,
    "settings": settings,
    "window": window,
    "delete": delete,
    "rating": rating,
    "keyboard": keyboard,
    "caches": caches,
    "register": register,
    "animation": animation,
    "heic": heic,
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("photos", help="folder made by tools/make_test_photos.py")
    parser.add_argument("--only", choices=SESSIONS, help="run a single session")
    parser.add_argument("--keep", action="store_true", help="keep the temp folders")
    args = parser.parse_args()
    for binary in (RAWV, CLI):
        if not binary.exists():
            sys.exit(f"missing {binary}: run cargo build --release")
    for name, session_check in SESSIONS.items():
        if args.only and args.only != name:
            continue
        print(name)
        work = tempfile.mkdtemp(prefix=f"rawv-verify-{name}-")
        try:
            session_check(Session(args.photos, work))
        finally:
            if not args.keep:
                shutil.rmtree(work, ignore_errors=True)
    print(f"\n{len(failures)} failed" if failures else "\nall checks passed")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()

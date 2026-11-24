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
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
RAWV = REPO / "target" / "release" / "rawv.exe"
CLI = REPO / "target" / "release" / "rawv-cli.exe"

# Window points for a fresh 1500x950 window at thumbnail scale 1.
FIRST_FRAME = "246,115"
SECOND_ROLL_TITLE = "60,299"
CENTER = "750,475"

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


def probe(path):
    out = subprocess.run([str(CLI), "probe", str(path)], capture_output=True, text=True, timeout=60).stdout
    for line in out.splitlines():
        if line.startswith("orientation "):
            return int(line.split()[1])
    return None


def gallery(session):
    states = session.run(session.photos, [
        "dump",
        "right", "right", "down", "dump",
        "toggle-all", "dump", "toggle-all", "toggle-all", "dump",
        "sort", "dump", "sort", "sort", "sort",
        "thumb-size", "dump", "thumb-size", "thumb-size",
        "thumbs-larger", "dump", "thumbs-smaller",
        f"move:{CENTER}", "ctrl-wheel:1", "dump", "ctrl-wheel:-1",
        f"click:{SECOND_ROLL_TITLE}", "dump",
        "parent", "dump",
        "quit",
    ])
    check("gallery: ten state dumps", len(states) == 10, f"got {len(states)}")
    if len(states) != 10:
        return
    first, selected, expanded, collapsed, sorted_desc, medium, larger, wheel, entered, parent = states
    titles = [roll["title"] for roll in first["rolls"]]
    counts = [roll["images"] for roll in first["rolls"]]
    check("gallery: folder and subfolders become rolls",
          titles == ["shoot", "2026-05 Berlin", "2026-06 Wedding", "archive", "empty", "raw only", "small preview"], str(titles))
    check("gallery: image counts per roll", counts == [15, 40, 14, 0, 0, 6, 1], str(counts))
    check("gallery: natural name order", first["rolls"][0]["first"] == "DSC00000.ARW", first["rolls"][0]["first"])
    check("gallery: only the open folder starts expanded",
          [roll["expanded"] for roll in first["rolls"]] == [True] + [False] * 6)
    check("gallery: arrow keys move the selection by column and row", selected["selection"] == [0, 11], str(selected["selection"]))
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
          titles == ["shoot", "2026-05 Berlin", "2026-06 Wedding", "archive", "2019", "empty", "raw only", "small preview"], str(titles))
    check("nested: all rolls are listed without scrolling to them", counts == [15, 40, 14, 0, 3, 0, 6, 1], str(counts))
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
          len(states) == 3 and states[0]["nested"] is True and len(states[0]["rolls"]) == 8)
    check("nested: switching it off shows direct subfolders only",
          len(states) == 3 and states[1]["nested"] is False and len(states[1]["rolls"]) == 7)
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
